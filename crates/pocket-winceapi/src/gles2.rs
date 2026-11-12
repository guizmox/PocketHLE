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
const EGL_BAD_MATCH: u32 = 0x3009;
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
    initialized: bool,
    window: bool,
    owner: Option<usize>,
    error: u32,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            renderer: None,
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
                        let supported = match key {
                            0x3020 => 16,
                            0x3021 => 0,
                            0x3022 | 0x3024 => 5,
                            0x3023 => 6,
                            0x3025 => 16,
                            0x3026 => 8,
                            0x3031 | 0x3032 => 0,
                            0x3033 => 4,
                            0x3040 => 4,
                            _ => {
                                return Ok(DispatchOutcome::ReturnedR0(
                                    self.fail(EGL_BAD_ATTRIBUTE),
                                ))
                            }
                        };
                        matches &= if matches!(key, 0x3033 | 0x3040) {
                            supported & value == value
                        } else {
                            supported >= value
                        };
                    }
                }
                if count == 0 || capacity < 0 {
                    self.fail(0x300c)
                } else {
                    let n = u32::from(matches && (config == 0 || capacity > 0));
                    ctx.cpu.write_mem(count, &n.to_le_bytes())?;
                    if n != 0 && config != 0 {
                        ctx.cpu.write_mem(config, &CONFIG.to_le_bytes())?;
                    }
                    1
                }
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
                    match Renderer::new(width, height) {
                        Ok(renderer) => {
                            self.renderer = Some(renderer);
                            log::info!("Adreno 200 GLES2 context ready at {width}x{height}");
                            CONTEXT
                        }
                        Err(error) => {
                            log::error!("Adreno 200 GLES2 context creation failed: {error}");
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
                log::warn!("Unsupported Adreno 200 EGL call {name}");
                self.fail(EGL_BAD_MATCH)
            }
        };
        Ok(DispatchOutcome::ReturnedR0(value))
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
