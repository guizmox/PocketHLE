//! Programmable WinCE EGL/GLES path. The fixed-function renderer owns a
//! separate context and never sees the guest's programmable objects.
use pocket_kernel::{DispatchOutcome, KernelError};
use std::cell::RefCell;

use crate::{CallCtx, Handler};

mod renderer;
use renderer::Renderer;

const DISPLAY: u32 = 1;
const CONFIG: u32 = 2;
const WINDOW: u32 = 3;
const CONTEXT: u32 = 4;
const EGL_NONE: u32 = 0x3038;
const EGL_SUCCESS: u32 = 0x3000;
const EGL_BAD_ATTRIBUTE: u32 = 0x3004;
const EGL_BAD_CONFIG: u32 = 0x3005;
const EGL_BAD_MATCH: u32 = 0x3009;
const EGL_BAD_PARAMETER: u32 = 0x300c;
const EGL_BAD_SURFACE: u32 = 0x300d;
const EGL_OPENGL_ES_API: u32 = 0x30a0;

fn config_attribute(key: u32) -> Option<i32> {
    Some(match key {
        0x3020 | 0x3025 => 16,              // buffer and depth sizes
        0x3022 | 0x3024 => 5,               // blue and red sizes
        0x3023 => 6,                        // green size
        0x3026 => 8,                        // stencil size
        0x3027 | 0x3034 => EGL_NONE as i32, // caveat, transparent type
        0x3028 => CONFIG as i32,            // config ID
        0x3033 => 4,                        // window surface bit
        0x3040 | 0x3042 => 4,               // renderable and conformant ES2 bits
        0x303b => 0,                        // minimum swap interval
        0x303c => 1,                        // maximum swap interval
        0x303f => 0x308e,                   // RGB color buffer
        0x3021 | 0x3029..=0x302f | 0x3031 | 0x3032 | 0x3035..=0x3037 | 0x3039 | 0x303a => 0,
        _ => return None,
    })
}
type Result<T> = std::result::Result<T, KernelError>;

fn error(message: impl Into<String>) -> KernelError {
    KernelError::Dispatch(message.into())
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

pub(crate) fn take_session() -> Option<Session> {
    SESSION.with(RefCell::take)
}

pub(crate) fn restore_session(session: Option<Session>) {
    SESSION.with(|slot| *slot.borrow_mut() = session);
}

pub fn set_enabled(enabled: bool) {
    restore_session(enabled.then(Session::default));
}

pub fn set_internal_resolution(policy: u8) {
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow_mut().as_mut() {
            session.resolution_policy = policy;
        }
    });
}

pub(crate) fn handler(thunk: &pocket_kernel::Thunk) -> Option<Handler> {
    if !(thunk.dll.eq_ignore_ascii_case("libglesv2.dll")
        || thunk.dll.eq_ignore_ascii_case("libegl.dll"))
        || !SESSION.with(|session| session.borrow().is_some())
    {
        return None;
    }
    Some(dispatch)
}

fn dispatch(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome> {
    let name = crate::import_name(ctx.thunk);
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let session = slot.as_mut().expect("enabled GLES2 session");
        if ctx.thunk.dll.eq_ignore_ascii_case("libegl.dll") {
            session.egl(&name, ctx)
        } else {
            let Some(renderer) = session
                .renderer
                .as_mut()
                .filter(|_| session.owner == Some(ctx.kernel.current_thread))
            else {
                return Ok(DispatchOutcome::ReturnedR0(0));
            };
            renderer.gl(&name, ctx).map(DispatchOutcome::ReturnedR0)
        }
    })
}

pub(crate) struct Session {
    renderer: Option<Renderer>,
    resolution_policy: u8,
    initialized: bool,
    window: bool,
    owner: Option<usize>,
    error: u32,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            renderer: None,
            resolution_policy: 1,
            initialized: false,
            window: false,
            owner: None,
            error: EGL_SUCCESS,
        }
    }
}

impl Session {
    fn egl(&mut self, name: &str, ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome> {
        let value = match name {
            "eglGetDisplay" => DISPLAY,
            "eglGetError" => std::mem::replace(&mut self.error, EGL_SUCCESS),
            "eglBindAPI" if ctx.arg_u32(0)? == EGL_OPENGL_ES_API => 1,
            "eglBindAPI" => self.fail(EGL_BAD_PARAMETER),
            "eglQueryAPI" => EGL_OPENGL_ES_API,
            _ if ctx.arg_u32(0)? != DISPLAY => self.fail(0x3008),
            "eglInitialize" => {
                for (index, value) in [(1, 1u32), (2, 4u32)] {
                    let out = ctx.arg_u32(index)?;
                    if out != 0 {
                        ctx.cpu.write_mem(out, &value.to_le_bytes())?;
                    }
                }
                self.initialized = true;
                1
            }
            _ if !self.initialized => self.fail(0x3001),
            "eglChooseConfig" | "eglGetConfigs" => {
                let choose = name == "eglChooseConfig";
                let first = 1 + u8::from(choose);
                let config = ctx.arg_u32(first)?;
                let capacity = ctx.arg_u32(first + 1)? as i32;
                let count = ctx.arg_u32(first + 2)?;
                let mut matches = true;
                if choose {
                    for (key, value) in attributes(ctx, 1)? {
                        if value == -1 {
                            continue;
                        }
                        let Some(supported) = config_attribute(key) else {
                            return Ok(DispatchOutcome::ReturnedR0(self.fail(EGL_BAD_ATTRIBUTE)));
                        };
                        matches &= match key {
                            0x3033 | 0x3040 | 0x3042 => supported & value == value,
                            0x3020..=0x3026 | 0x3031 | 0x3032 => supported >= value,
                            _ => supported == value,
                        };
                    }
                }
                if count == 0 || config != 0 && capacity < 0 {
                    self.fail(EGL_BAD_PARAMETER)
                } else {
                    let n = u32::from(matches && (config == 0 || capacity > 0));
                    ctx.cpu.write_mem(count, &n.to_le_bytes())?;
                    if n != 0 && config != 0 {
                        ctx.cpu.write_mem(config, &CONFIG.to_le_bytes())?;
                    }
                    1
                }
            }
            "eglGetConfigAttrib" => {
                if ctx.arg_u32(1)? != CONFIG {
                    return Ok(DispatchOutcome::ReturnedR0(self.fail(EGL_BAD_CONFIG)));
                }
                let key = ctx.arg_u32(2)?;
                self.write_attribute(ctx, config_attribute(key))?
            }
            "eglQueryString" => {
                let text = match ctx.arg_u32(1)? {
                    0x3053 => "PocketHLE",
                    0x3054 => "1.4 PocketHLE",
                    0x3055 => "",
                    0x308d => "OpenGL_ES",
                    _ => return Ok(DispatchOutcome::ReturnedR0(self.fail(EGL_BAD_PARAMETER))),
                };
                crate::gles::intern_string(ctx, text)?
            }
            "eglQuerySurface" => {
                if !self.window || ctx.arg_u32(1)? != WINDOW {
                    return Ok(DispatchOutcome::ReturnedR0(self.fail(EGL_BAD_SURFACE)));
                }
                let value = match ctx.arg_u32(2)? {
                    0x3028 => Some(CONFIG as i32),
                    0x3056 => Some(ctx.kernel.framebuffer.height as i32),
                    0x3057 => Some(ctx.kernel.framebuffer.width as i32),
                    0x3086 => Some(0x3084), // EGL_BACK_BUFFER
                    0x3093 => Some(0x3095), // EGL_BUFFER_DESTROYED
                    _ => None,
                };
                self.write_attribute(ctx, value)?
            }
            "eglCreateWindowSurface" if ctx.arg_u32(1)? == CONFIG && ctx.arg_u32(2)? != 0 => {
                if self.window {
                    self.fail(0x3003)
                } else {
                    self.window = true;
                    WINDOW
                }
            }
            "eglCreateContext" if ctx.arg_u32(1)? == CONFIG && ctx.arg_u32(2)? == 0 => {
                if attributes(ctx, 3)?.as_slice() != [(0x3098, 2)] {
                    self.fail(EGL_BAD_MATCH)
                } else if self.renderer.is_some() {
                    self.fail(0x3003)
                } else {
                    let width = ctx.kernel.framebuffer.width;
                    let height = ctx.kernel.framebuffer.height;
                    match Renderer::new(width, height, self.resolution_policy) {
                        Ok(renderer) => {
                            let (render_width, render_height) = renderer.render_size();
                            log::info!("GLES2 context ready at {width}x{height}, rendering at {render_width}x{render_height}");
                            self.renderer = Some(renderer);
                            CONTEXT
                        }
                        Err(error) => {
                            log::error!("GLES2 context creation failed: {error}");
                            self.fail(0x3003)
                        }
                    }
                }
            }
            "eglMakeCurrent" => {
                let (draw, read, context) = (ctx.arg_u32(1)?, ctx.arg_u32(2)?, ctx.arg_u32(3)?);
                if (draw, read, context) == (0, 0, 0) {
                    if self.owner == Some(ctx.kernel.current_thread) {
                        self.owner = None;
                    }
                    1
                } else if (draw, read, context) == (WINDOW, WINDOW, CONTEXT)
                    && self.window
                    && self.renderer.is_some()
                {
                    if self
                        .owner
                        .is_some_and(|owner| owner != ctx.kernel.current_thread)
                    {
                        return Ok(DispatchOutcome::ReturnedR0(self.fail(0x3002)));
                    }
                    self.owner = Some(ctx.kernel.current_thread);
                    1
                } else {
                    self.fail(EGL_BAD_MATCH)
                }
            }
            "eglSwapInterval" if self.owner == Some(ctx.kernel.current_thread) => 1,
            "eglSwapBuffers"
                if self.owner == Some(ctx.kernel.current_thread)
                    && self.window
                    && ctx.arg_u32(1)? == WINDOW =>
            {
                if let Some(wait) = crate::ddraw::wait_display(ctx, None)? {
                    return Ok(wait);
                }
                self.renderer.as_mut().unwrap().present(ctx)?;
                1
            }
            "eglDestroySurface" if self.window && ctx.arg_u32(1)? == WINDOW => {
                self.window = false;
                self.owner = None;
                1
            }
            "eglDestroyContext" if self.renderer.is_some() && ctx.arg_u32(1)? == CONTEXT => {
                self.renderer = None;
                self.owner = None;
                1
            }
            "eglTerminate" => {
                *self = Self::default();
                1
            }
            _ => {
                log::warn!("Unsupported GLES2 EGL call {name}");
                self.fail(EGL_BAD_MATCH)
            }
        };
        Ok(DispatchOutcome::ReturnedR0(value))
    }

    fn write_attribute(&mut self, ctx: &mut CallCtx<'_>, value: Option<i32>) -> Result<u32> {
        let Some(value) = value else {
            return Ok(self.fail(EGL_BAD_ATTRIBUTE));
        };
        let out = ctx.arg_u32(3)?;
        if out == 0 {
            return Ok(self.fail(EGL_BAD_PARAMETER));
        }
        ctx.cpu.write_mem(out, &value.to_le_bytes())?;
        Ok(1)
    }

    fn fail(&mut self, error: u32) -> u32 {
        self.error = error;
        0
    }
}

fn attributes(ctx: &mut CallCtx<'_>, index: u8) -> Result<Vec<(u32, i32)>> {
    let mut address = ctx.arg_u32(index)?;
    let mut result = Vec::new();
    if address == 0 {
        return Ok(result);
    }
    for _ in 0..64 {
        let key = ctx.cpu.read_u32_le(address)?;
        if key == EGL_NONE {
            return Ok(result);
        }
        let value = ctx.cpu.read_u32_le(
            address
                .checked_add(4)
                .ok_or_else(|| error("EGL attribute address overflow"))?,
        )?;
        result.push((key, value as i32));
        address = address
            .checked_add(8)
            .ok_or_else(|| error("EGL attribute address overflow"))?;
    }
    Err(error("Unterminated EGL attribute list"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::regs::ArmReg::{R0, R1, R2, R3};
    use pocket_kernel::Thunk;
    use pocket_pe::ImportBinding;

    fn egl(session: &mut Session, ctx: &mut CallCtx<'_>, name: &str, args: [u32; 4]) -> u32 {
        for (register, value) in [R0, R1, R2, R3].into_iter().zip(args) {
            ctx.cpu.write_reg(register, value).unwrap();
        }
        match session.egl(name, ctx).unwrap() {
            DispatchOutcome::ReturnedR0(value) => value,
            _ => panic!("unexpected EGL dispatch result"),
        }
    }

    #[test]
    fn enumerated_window_config_can_be_queried_before_context_creation() {
        let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
        let thunk = Thunk {
            thunk_va: 0,
            iat_va: 0,
            dll: "libegl.dll".into(),
            binding: ImportBinding::Name("eglGetConfigs".into()),
            friendly_name: None,
        };
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            thunk: &thunk,
            kernel: &mut kernel,
        };
        let mut session = Session::default();
        assert_eq!(
            egl(
                &mut session,
                &mut ctx,
                "eglBindAPI",
                [EGL_OPENGL_ES_API, 0, 0, 0]
            ),
            1
        );
        assert_eq!(
            egl(&mut session, &mut ctx, "eglInitialize", [DISPLAY, 0, 0, 0]),
            1
        );
        assert_eq!(
            egl(
                &mut session,
                &mut ctx,
                "eglGetConfigs",
                [DISPLAY, 0x2000, 1, 0x2004]
            ),
            1
        );
        assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), CONFIG);
        assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), 1);
        for (key, expected) in [(0x3033, 4), (0x3040, 4), (0x3024, 5), (0x302f, 0)] {
            assert_eq!(
                egl(
                    &mut session,
                    &mut ctx,
                    "eglGetConfigAttrib",
                    [DISPLAY, CONFIG, key, 0x2008]
                ),
                1
            );
            assert_eq!(ctx.cpu.read_u32_le(0x2008).unwrap(), expected);
        }
        assert_eq!(
            egl(
                &mut session,
                &mut ctx,
                "eglGetConfigAttrib",
                [DISPLAY, 0, 0x3033, 0x2008]
            ),
            0
        );
        assert_eq!(
            egl(&mut session, &mut ctx, "eglGetError", [0; 4]),
            EGL_BAD_CONFIG
        );
    }
}
