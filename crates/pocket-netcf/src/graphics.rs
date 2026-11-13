//! Managed EGL/GLES entry points backed by PocketHLE's renderer.

use crate::vm::{Fault, Machine, Value};
use pocket_gles::context::{Context, GuestMemory};
use std::collections::BTreeMap;

pub struct Graphics {
    size: (u32, u32),
    gpu_renderer: bool,
    initialized: bool,
    window: bool,
    current: bool,
    context: Option<Context>,
    bindings: BTreeMap<u32, (usize, usize)>,
    frame: Vec<u8>,
    generation: u64,
}

impl Graphics {
    pub fn roots(&self, roots: &mut Vec<Value>) {
        roots.extend(self.bindings.values().map(|&(id, _)| Value::Reference(id)));
    }

    pub fn new(size: (u32, u32), gpu_renderer: bool) -> Self {
        Self {
            size,
            gpu_renderer,
            initialized: false,
            window: false,
            current: false,
            context: None,
            bindings: BTreeMap::new(),
            frame: Vec::new(),
            generation: 0,
        }
    }

    pub fn egl(
        &mut self,
        machine: &mut Machine,
        name: &str,
        args: &[Value],
    ) -> Option<Result<Option<Value>, Fault>> {
        use Value::{Int, Null};
        let result = match (name, args) {
            ("GetDisplay", [Int(0)]) => 1,
            ("Initialize", [Int(1), major, minor]) => {
                let written = machine
                    .write_address(*major, Int(1))
                    .and_then(|()| machine.write_address(*minor, Int(1)));
                if let Err(error) = written {
                    return Some(Err(error));
                }
                self.initialized = true;
                1
            }
            ("ChooseConfig", [Int(1), attrs, configs, Int(capacity), count])
                if self.initialized && *capacity > 0 =>
            {
                let attrs = machine.array(*attrs)?;
                let end = attrs.iter().position(|value| *value == Int(0x3038))?;
                if end % 2 != 0 {
                    return None;
                }
                let supported = attrs[..end].chunks_exact(2).all(|pair| match pair {
                    [Int(0x3024), Int(n)] => *n <= 5,
                    [Int(0x3023), Int(n)] => *n <= 6,
                    [Int(0x3022), Int(n)] => *n <= 5,
                    [Int(0x3021), Int(n)] => *n <= 0,
                    [Int(0x3025), Int(n)] => *n <= 16,
                    [Int(0x3026), Int(n)] => *n <= 0,
                    [Int(0x3033), Int(mask)] => mask & !4 == 0,
                    [Int(0x3040), Int(mask)] => mask & !1 == 0,
                    [Int(0x3020), Int(n)] => *n <= 16,
                    _ => false,
                });
                let number = i32::from(supported);
                if supported {
                    *machine.array_mut(*configs)?.first_mut()? = Int(1);
                }
                if let Err(error) = machine.write_address(*count, Int(number)) {
                    return Some(Err(error));
                }
                1
            }
            ("CreateWindowSurface", [Int(1), Int(1), Int(window), Null])
                if self.initialized && *window > 0 =>
            {
                self.window = true;
                1
            }
            ("CreateContext", [Int(1), Int(1), Int(0), Null]) if self.initialized => {
                let mut context = Context::new_rgb565(self.size.0, self.size.1);
                if self.gpu_renderer {
                    match context.enable_gpu() {
                        Ok(()) => log::info!("Compact Framework GLES renderer: GPU"),
                        Err(error) => {
                            context.disable_gpu();
                            log::warn!(
                                "Compact Framework GLES renderer: software fallback ({error})"
                            );
                        }
                    }
                }
                self.context = Some(context);
                1
            }
            ("MakeCurrent", [Int(1), Int(1), Int(1), Int(1)])
                if self.window && self.context.is_some() =>
            {
                self.current = true;
                1
            }
            ("DestroySurface", [Int(1), Int(1)]) if self.window => {
                self.window = false;
                self.current = false;
                1
            }
            ("DestroyContext", [Int(1), Int(1)]) if self.context.is_some() => {
                self.context = None;
                self.current = false;
                self.bindings.clear();
                1
            }
            ("Terminate", [Int(1)]) if self.initialized => {
                self.context = None;
                self.initialized = false;
                self.window = false;
                self.current = false;
                self.bindings.clear();
                1
            }
            ("SwapBuffers", [Int(1), Int(1)]) if self.current => {
                let context = self.context.as_mut()?;
                let length = (self.size.0 as usize)
                    .checked_mul(self.size.1 as usize)?
                    .checked_mul(2)?;
                self.frame.resize(length, 0);
                context.present_rgb565(&mut self.frame);
                self.generation = self.generation.wrapping_add(1);
                1
            }
            _ => return None,
        };
        Some(Ok(Some(Int(result))))
    }

    #[cfg(test)]
    pub fn gpu_active(&self) -> bool {
        self.context.as_ref().is_some_and(Context::gpu_active)
    }

    pub fn frame(&self) -> Option<(u64, (u32, u32), &[u8])> {
        (!self.frame.is_empty()).then_some((self.generation, self.size, &self.frame))
    }

    pub fn texture_size(&self, name: u32) -> Option<(u32, u32)> {
        let texture = self.context.as_ref()?.textures.get(&name)?;
        Some((texture.width, texture.height))
    }

    pub fn texture(&mut self, data: &[u8], linear: bool) -> Option<u32> {
        use std::io::Cursor;
        let context = self.context.as_mut().filter(|_| self.current)?;
        let mut reader = image::io::Reader::new(Cursor::new(data))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::io::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().ok()?.to_rgba8();
        let name = *context.gen_textures(1).first()?;
        context.bind_texture(0x0de1, name);
        context.tex_image_2d(
            0x0de1,
            0,
            image.width(),
            image.height(),
            0x1908,
            0x1401,
            image.as_raw(),
        );
        let filter = if linear { 0x2601 } else { 0x2600 };
        context.tex_parameter(0x0de1, 0x2801, filter);
        context.tex_parameter(0x0de1, 0x2800, filter);
        (context.take_error() == 0).then_some(name)
    }

    pub fn gl(&mut self, machine: &Machine, name: &str, args: &[Value]) -> Option<Option<Value>> {
        use Value::{Float, Int};
        let context = self.context.as_mut().filter(|_| self.current)?;
        match (name, args) {
            ("Color4f", [Float(r), Float(g), Float(b), Float(a)]) => {
                context.current_color = [*r, *g, *b, *a].map(f32::from_bits);
            }
            ("Translatef", [Float(x), Float(y), Float(z)]) => {
                context.translate(f32::from_bits(*x), f32::from_bits(*y), f32::from_bits(*z));
            }
            ("Rotatef", [Float(a), Float(x), Float(y), Float(z)]) => {
                context.rotate(
                    f32::from_bits(*a),
                    f32::from_bits(*x),
                    f32::from_bits(*y),
                    f32::from_bits(*z),
                );
            }
            ("Scalef", [Float(x), Float(y), Float(z)]) => {
                context.scale(f32::from_bits(*x), f32::from_bits(*y), f32::from_bits(*z));
            }
            ("PushMatrix", []) => context.push_matrix(),
            ("PopMatrix", []) => context.pop_matrix(),
            ("Flush", []) => context.flush(),
            ("BindTexture", [Int(target), Int(texture)]) => {
                context.bind_texture(*target as u32, *texture as u32);
            }
            ("TexParameteri", [Int(target), Int(name), Int(value)]) => {
                context.tex_parameter(*target as u32, *name as u32, *value as u32);
            }
            ("EnableClientState" | "DisableClientState", [Int(array)]) => {
                context.set_client_state(*array as u32, name == "EnableClientState");
            }
            (
                "VertexPointer" | "TexCoordPointer" | "ColorPointer",
                [Int(size), Int(kind), Int(stride), Value::ArrayAddress(array, offset)],
            ) => {
                let address = match name {
                    "VertexPointer" => 0x1000_0000,
                    "TexCoordPointer" => 0x2000_0000,
                    _ => 0x3000_0000,
                };
                self.bindings.insert(address, (*array, *offset));
                match name {
                    "VertexPointer" => context.set_vertex_pointer(
                        *size as u32,
                        *kind as u32,
                        *stride as u32,
                        address,
                    ),
                    "TexCoordPointer" => context.set_texcoord_pointer(
                        *size as u32,
                        *kind as u32,
                        *stride as u32,
                        address,
                    ),
                    _ => context.set_color_pointer(
                        *size as u32,
                        *kind as u32,
                        *stride as u32,
                        address,
                    ),
                }
            }
            ("DrawArrays", [Int(mode), Int(first), Int(count)]) if *first >= 0 && *count >= 0 => {
                let mut memory = ArrayMemory {
                    machine,
                    bindings: &self.bindings,
                };
                context.draw_arrays(&mut memory, *mode as u32, *first as u32, *count as u32);
            }
            ("Lightfv", [Int(light), Int(parameter), Value::ArrayAddress(array, offset)]) => {
                use pocket_gles::lighting::{parameter_count, Target};
                let target = Target::Light(*light as u32);
                let parameter = *parameter as u32;
                match parameter_count(target, parameter, false) {
                    Ok(count) => {
                        let end = offset.checked_add(count)?;
                        let items = machine.array(Value::Reference(*array))?.get(*offset..end)?;
                        let values = items
                            .iter()
                            .map(|item| match item {
                                Int(bits) => Some(f32::from_bits(*bits as u32)),
                                Float(bits) => Some(f32::from_bits(*bits)),
                                _ => None,
                            })
                            .collect::<Option<Vec<_>>>()?;
                        if let Err(error) = context.lighting.set(
                            target,
                            parameter,
                            &values,
                            context.modelview.current(),
                        ) {
                            context.set_error(error);
                        }
                    }
                    Err(error) => context.set_error(error),
                }
            }
            ("Orthof", [Float(l), Float(r), Float(b), Float(t), Float(n), Float(f)]) => {
                let [l, r, b, t, n, f] = [*l, *r, *b, *t, *n, *f].map(f32::from_bits);
                context.ortho(l, r, b, t, n, f);
            }
            ("ClearColor", [Float(r), Float(g), Float(b), Float(a)]) => {
                context.clear_color = [*r, *g, *b, *a].map(f32::from_bits);
            }
            ("ClearDepthf", [Float(depth)]) => {
                context.clear_depth = f32::from_bits(*depth).clamp(0.0, 1.0);
            }
            ("ShadeModel", [Int(mode)]) => context.set_shade_model(*mode as u32),
            ("Enable" | "Disable", [Int(cap)]) => {
                context.set_capability(*cap as u32, name == "Enable");
            }
            ("BlendFunc", [Int(src), Int(dst)]) => context.set_blend_func(*src as u32, *dst as u32),
            ("MatrixMode", [Int(mode)]) => context.set_matrix_mode(*mode as u32),
            ("LoadIdentity", []) => context.load_identity(),
            ("Viewport", [Int(x), Int(y), Int(w), Int(h)]) => context.set_viewport(*x, *y, *w, *h),
            ("Clear", [Int(mask)]) => context.clear(*mask as u32),
            ("GetError", []) => return Some(Some(Int(context.take_error() as i32))),
            _ => return None,
        }
        Some(None)
    }
}

struct ArrayMemory<'a> {
    machine: &'a Machine,
    bindings: &'a BTreeMap<u32, (usize, usize)>,
}

impl GuestMemory for ArrayMemory<'_> {
    fn read(&mut self, addr: u32, len: usize, out: &mut Vec<u8>) -> bool {
        let base = addr & 0xf000_0000;
        let Some(&(array, offset)) = self.bindings.get(&base) else {
            return false;
        };
        let Some(elements) = self.machine.array(Value::Reference(array)) else {
            return false;
        };
        let start = offset
            .checked_mul(4)
            .and_then(|offset| offset.checked_add((addr - base) as usize));
        let Some(start) = start else {
            return false;
        };
        let Some(end) = start.checked_add(len) else {
            return false;
        };
        if end > elements.len().saturating_mul(4) {
            return false;
        }
        out.clear();
        for index in start..end {
            let bits = match elements[index / 4] {
                Value::Float(bits) => bits,
                Value::Int(bits) => bits as u32,
                _ => return false,
            };
            out.push(bits.to_le_bytes()[index % 4]);
        }
        true
    }
}
