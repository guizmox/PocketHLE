//! Managed startup and service dispatch.

use crate::metadata::{aligned, dword, word, Assembly};
use crate::{forms, graphics, sensors, storage, system, vm};

struct Startup<'a> {
    assembly: Assembly<'a>,
    unresolved: Option<&'a str>,
    streams: Vec<Option<&'a [u8]>>,
    unresolved_type: Option<(&'a str, &'a str)>,
    system: system::Services,
    forms: forms::Forms,
    graphics: graphics::Graphics,
    sensors: sensors::Sensors,
    strings: Vec<std::rc::Rc<str>>,
    string_ids: std::collections::BTreeMap<std::rc::Rc<str>, usize>,
    string_bytes: usize,
    program_path: String,
    storage: storage::Storage,
    started: std::time::Instant,
    tables: std::collections::BTreeMap<usize, std::collections::BTreeSet<u32>>,
    resource_types: std::collections::BTreeMap<usize, u32>,
    pen_widths: std::collections::BTreeMap<usize, u32>,
    drawing_colors: std::collections::BTreeMap<usize, u16>,
    audio: pocket_kernel::AudioEngine,
    fmod: crate::fmod::Fmod,
    assets: pocket_kernel::vfs::Vfs,
    data_slots: Vec<(String, vm::Value)>,
}

impl<'image> vm::Environment<'image> for Startup<'image> {
    fn signature(&self, token: u32) -> Option<vm::Signature> {
        self.assembly.signature(token)
    }

    fn body(&self, token: u32) -> Option<&'image [u8]> {
        self.assembly
            .method(self.assembly.local_method(token).unwrap_or(token))
            .map(|method| method.il)
    }

    fn exception_clauses(&self, token: u32) -> Vec<vm::ExceptionClause> {
        let token = self.assembly.local_method(token).unwrap_or(token);
        let Some(method) = self.assembly.method(token) else {
            return Vec::new();
        };
        let mut sections = method.extra_sections;
        let mut clauses = Vec::new();
        while let Some(&kind) = sections.first() {
            if kind & 0x3f != 1 {
                break;
            }
            let fat = kind & 0x40 != 0;
            let size = if fat {
                dword(sections, 0).unwrap_or(0) as usize >> 8
            } else {
                sections.get(1).copied().unwrap_or(0) as usize
            };
            let Some(records) = sections.get(4..size) else {
                break;
            };
            for row in records.chunks_exact(if fat { 24 } else { 12 }) {
                let (flags, try_start, try_len, handler_start, handler_len, exception) = if fat {
                    (
                        dword(row, 0).unwrap(),
                        dword(row, 4).unwrap(),
                        dword(row, 8).unwrap(),
                        dword(row, 12).unwrap(),
                        dword(row, 16).unwrap(),
                        dword(row, 20).unwrap(),
                    )
                } else {
                    (
                        u32::from(word(row, 0).unwrap()),
                        u32::from(word(row, 2).unwrap()),
                        u32::from(row[4]),
                        u32::from(word(row, 5).unwrap()),
                        u32::from(row[7]),
                        dword(row, 8).unwrap(),
                    )
                };
                if let (Some(try_end), Some(handler_end)) = (
                    try_start.checked_add(try_len),
                    handler_start.checked_add(handler_len),
                ) {
                    if (try_end as usize) <= method.il.len()
                        && (handler_end as usize) <= method.il.len()
                    {
                        clauses.push(vm::ExceptionClause {
                            flags,
                            try_start: try_start as usize,
                            try_end: try_end as usize,
                            handler_start: handler_start as usize,
                            handler_end: handler_end as usize,
                            exception,
                        });
                    }
                }
            }
            if kind & 0x80 == 0 {
                break;
            }
            let Some(next) = aligned(size).and_then(|size| sections.get(size..)) else {
                break;
            };
            sections = next;
        }
        clauses
    }

    fn catches(&self, exception: u32, fault: &vm::Fault) -> bool {
        if exception >> 24 != 1
            || !matches!(fault, vm::Fault::MissingFile | vm::Fault::ManagedException)
        {
            return false;
        }
        matches!(
            self.assembly.type_ref((exception & 0xffffff) as usize),
            Some(
                ("System", "Object" | "Exception" | "SystemException")
                    | ("System.IO", "IOException" | "FileNotFoundException")
            )
        )
    }

    fn value_field(&self, token: u32, value: vm::Value) -> Option<vm::Value> {
        let vm::Value::Vector3(values) = value else {
            return None;
        };
        Some(vm::Value::Float(values[self.assembly.vector_axis(token)?]))
    }

    fn store_value_field(
        &self,
        token: u32,
        object: vm::Value,
        value: vm::Value,
    ) -> Option<vm::Value> {
        let (vm::Value::Vector3(mut values), vm::Value::Float(value)) = (object, value) else {
            return None;
        };
        values[self.assembly.vector_axis(token)?] = value;
        Some(vm::Value::Vector3(values))
    }

    fn unbox_any(&self, machine: &vm::Machine, method: u32, ty: u32, value: vm::Value) -> bool {
        use vm::Value;
        let ty = self.resolve_type(method, ty);
        match ty >> 24 {
            2 => {
                value == Value::Null
                    || machine
                        .constructor(value)
                        .is_some_and(|ctor| self.assembly.type_owns_method(ty, ctor))
            }
            1 => match self.assembly.type_ref((ty & 0xffffff) as usize) {
                Some(("System", "Object")) => true,
                Some(("System", "Single")) => matches!(value, Value::Float(_)),
                Some(("System", "Double")) => matches!(value, Value::Double(_)),
                Some(("System", "Int32")) => matches!(value, Value::Int(_)),
                Some(("System", "String")) => matches!(
                    value,
                    Value::Null | Value::StringToken(_) | Value::HostString(_)
                ),
                _ => false,
            },
            _ => false,
        }
    }

    fn resolve_type(&self, method: u32, token: u32) -> u32 {
        self.assembly
            .generic_context(method)
            .and_then(|(method, args)| self.assembly.generic_type(token, args, method))
            .unwrap_or(token)
    }

    fn resolve_field(&self, token: u32) -> u32 {
        self.assembly.local_field(token).unwrap_or(token)
    }

    fn field_default(&self, token: u32) -> vm::Value {
        self.assembly.field_default(token)
    }

    fn static_initializer(&self, token: u32) -> Option<u32> {
        self.assembly.static_initializer(token)
    }

    fn static_field(&mut self, token: u32) -> Result<vm::Value, vm::Fault> {
        self.unresolved = self.assembly.member_name(token);
        self.unresolved_type = self.assembly.member_type(token);
        if matches!(
            (self.unresolved_type, self.unresolved),
            (Some(("OpenGLES", "EGLNativeDisplayType")), Some("Default"))
                | (Some(("OpenGLES", "EGLContext")), Some("None"))
        ) {
            self.unresolved = None;
            self.unresolved_type = None;
            return Ok(vm::Value::Int(0));
        }
        Err(vm::Fault::UnsupportedCall(token))
    }

    fn construct_value(&self, token: u32, args: &[vm::Value]) -> Option<vm::Value> {
        use vm::Value::{Delegate, Float, Function, Int, Null, Point, Reference, Size, SizeF};
        if self.assembly.member_name(token)? != ".ctor" {
            return None;
        }
        match (self.assembly.member_type(token)?, args) {
            (
                ("System", "EventHandler")
                | ("System.ComponentModel", "CancelEventHandler")
                | ("System.Windows.Forms", "MouseEventHandler"),
                [target, Function(method)],
            ) => {
                let target = match target {
                    Reference(id) => Some(*id),
                    Null => None,
                    _ => return None,
                };
                Some(Delegate {
                    target,
                    method: *method,
                })
            }
            (("OpenGLES", "Vector3f"), [Float(x), Float(y), Float(z)]) => {
                Some(vm::Value::Vector3([*x, *y, *z]))
            }
            (("System.Drawing", "Point"), [Int(x), Int(y)]) => Some(Point(*x, *y)),
            (("System.Drawing", "SizeF"), [Float(w), Float(h)]) => Some(SizeF(*w, *h)),
            (("System.Drawing", "Size"), [Int(w), Int(h)]) => Some(Size(*w, *h)),
            _ => None,
        }
    }

    fn call(
        &mut self,
        machine: &mut vm::Machine,
        depth: usize,
        token: u32,
        args: &[vm::Value],
    ) -> Result<Option<vm::Value>, vm::Fault> {
        if let [address @ (vm::Value::LocalAddress(..)
        | vm::Value::FieldAddress(..)
        | vm::Value::ArgumentAddress(..)
        | vm::Value::ArrayAddress(..)), values @ ..] = args
        {
            if let Some(value) = self.construct_value(token, values) {
                machine.write_address(*address, value)?;
                return Ok(None);
            }
        }
        if let Some((module, name)) = self.assembly.pinvoke(token) {
            if module.eq_ignore_ascii_case("fmodce.dll") || module.eq_ignore_ascii_case("fmodce") {
                use vm::Value::{Int, Reference};
                let value = match (name, args) {
                    ("FSOUND_Init", [Int(rate), Int(channels), Int(0)]) => {
                        Some(i32::from(self.fmod.init(&self.audio, *rate, *channels)))
                    }
                    (
                        "FSOUND_Sample_Load",
                        [Int(-1), Reference(array), Int(64), Int(0), Int(0)],
                    ) => {
                        let path = machine
                            .array(Reference(*array))
                            .and_then(|values| {
                                values
                                    .iter()
                                    .take_while(|value| **value != Int(0))
                                    .map(|value| match value {
                                        Int(byte) => u8::try_from(*byte).ok(),
                                        _ => None,
                                    })
                                    .collect::<Option<Vec<_>>>()
                            })
                            .and_then(|bytes| String::from_utf8(bytes).ok());
                        let sample = path
                            .as_deref()
                            .filter(|path| path.to_ascii_lowercase().ends_with(".wav"))
                            .and_then(|path| {
                                storage::read_bounded(&mut self.assets, path, 4 * 1024 * 1024)
                            })
                            .and_then(|bytes| self.fmod.load(&bytes));
                        Some(sample.unwrap_or(0))
                    }
                    ("FSOUND_PlaySound", [Int(channel), Int(sample)]) => {
                        Some(self.fmod.play(&self.audio, *channel, *sample).unwrap_or(-1))
                    }
                    ("FSOUND_Sample_Free", [Int(sample)]) => {
                        Some(i32::from(self.fmod.free(*sample)))
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    return Ok(self
                        .assembly
                        .signature(token)
                        .filter(|signature| signature.returns_value)
                        .map(|_| Int(value)));
                }
            }
        }
        let member = self.assembly.member_name(token);
        let kind = self.assembly.member_type(token);
        if kind == Some(("System", "Environment"))
            && member == Some("get_TickCount")
            && args.is_empty()
        {
            return Ok(Some(vm::Value::Int(
                self.started.elapsed().as_millis() as i32
            )));
        }
        use vm::Value;
        if let Some((target, _)) = self.assembly.method_spec(token) {
            if self.assembly.member_type(target)
                == Some(("Microsoft.VisualBasic.CompilerServices", "Conversions"))
                && self.assembly.member_name(target) == Some("ToGenericParameter")
            {
                if let [value @ (Value::Null | Value::Reference(_))] = args {
                    return Ok(Some(*value));
                }
            }
        }
        let string = |value| match value {
            Value::StringToken(token) => self.assembly.user_string(token),
            Value::HostString(index) => self.strings.get(index).map(|text| text.to_string()),
            _ => None,
        };
        let text = match (kind, member, args) {
            (Some(("System.Text", "Encoding")), Some("get_Default"), []) => {
                return Ok(Some(Value::DefaultEncoding))
            }
            (
                Some(("System.Text", "Encoding")),
                Some("GetBytes"),
                [Value::DefaultEncoding, text],
            ) => {
                let text = string(*text).ok_or(vm::Fault::Reference)?;
                return machine.byte_array(text.as_bytes()).map(Some);
            }

            (
                Some(("System.Windows.Forms", "MouseEventArgs")),
                Some("get_X" | "get_Y"),
                [Value::MouseEvent(x, y)],
            ) => {
                return Ok(Some(Value::Int(if member == Some("get_X") {
                    *x
                } else {
                    *y
                })));
            }
            (Some(("System.Windows.Forms", "Application")), Some("Exit"), []) => {
                self.forms.request_close();
                return Ok(None);
            }
            (Some(("Microsoft.VisualBasic", "Conversion")), Some("Hex"), [Value::Float(bits)]) => {
                Some(format!(
                    "{:X}",
                    f32::from_bits(*bits).round_ties_even() as i32
                ))
            }
            (
                Some(("Microsoft.VisualBasic.CompilerServices", "Conversions")),
                Some("ToString"),
                [value],
            ) => match value {
                Value::Int(n) => Some(n.to_string()),
                Value::Float(n) => Some(f32::from_bits(*n).to_string()),
                Value::Double(n) => Some(f64::from_bits(*n).to_string()),
                Value::DateTime(n) => Some(system::date_text(*n)),
                _ => None,
            },
            (Some(("Microsoft.VisualBasic", "DateAndTime")), Some("get_Now"), []) => {
                let seconds = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                return Ok(Some(Value::DateTime(seconds)));
            }
            (Some(("System", "DateTime")), Some("get_Date"), [address]) => {
                if let Some(Value::DateTime(seconds)) = machine.read_address(*address) {
                    return Ok(Some(Value::DateTime(seconds / 86400 * 86400)));
                }
                return Err(vm::Fault::Reference);
            }
            (
                Some(("Microsoft.VisualBasic", "Strings")),
                Some("Format"),
                [Value::DateTime(seconds), pattern],
            ) if string(*pattern).as_deref() == Some("HH:mm:ss") => Some(format!(
                "{:02}:{:02}:{:02}",
                seconds / 3600 % 24,
                seconds / 60 % 60,
                seconds % 60
            )),
            (
                Some(("System.Windows.Forms", "Control")),
                Some("get_Font"),
                [Value::Reference(_)],
            ) => return Ok(Some(Value::Font)),
            (Some(("System.Drawing", "Font")), Some("get_Name"), [Value::Font]) => {
                Some("Tahoma".into())
            }
            (Some(("System.Drawing", "Font")), Some("get_Name"), [Value::Reference(id)]) => {
                return Ok(Some(
                    self.forms.font_family(*id).ok_or(vm::Fault::Reference)?,
                ));
            }
            (
                Some(("System.Drawing", "Graphics")),
                Some("DrawString"),
                [Value::DrawingSurface(target), text, Value::Reference(font), Value::Reference(brush), Value::Float(x), Value::Float(y)],
            ) => {
                let text: Vec<_> = string(*text)
                    .ok_or(vm::Fault::Reference)?
                    .encode_utf16()
                    .collect();
                let color = *self.drawing_colors.get(brush).ok_or(vm::Fault::Reference)?;
                let font = self.forms.render_font(*font).ok_or(vm::Fault::Reference)?;
                let bitmap = self.forms.bitmap_mut(*target).ok_or(vm::Fault::Reference)?;
                let bounds = [0, 0, bitmap.width as i32, bitmap.height as i32];
                pocket_kernel::text::layout(Some(&font), &text).draw(
                    &mut pocket_kernel::gdi::Surface::Screen(bitmap),
                    f32::from_bits(*x) as i32,
                    f32::from_bits(*y) as i32,
                    color,
                    bounds,
                );
                return Ok(None);
            }
            (
                Some(("System.Drawing", "Graphics")),
                Some("DrawPolygon"),
                [Value::DrawingSurface(target), Value::Reference(pen), array],
            ) => {
                let color = *self.drawing_colors.get(pen).ok_or(vm::Fault::Reference)?;
                let points = machine.array(*array).ok_or(vm::Fault::Reference)?;
                let bitmap = self.forms.bitmap_mut(*target).ok_or(vm::Fault::Reference)?;
                if let Some(last) = points.last() {
                    let mut surface = pocket_kernel::gdi::Surface::Screen(bitmap);
                    for (from, to) in std::iter::once(last).chain(points).zip(points) {
                        let (Value::Point(x, y), Value::Point(u, v)) = (*from, *to) else {
                            return Err(vm::Fault::InvalidSignature);
                        };
                        surface.line(
                            (x, y),
                            (u, v),
                            color,
                            self.pen_widths.get(pen).copied().unwrap_or(1),
                        );
                    }
                }
                return Ok(None);
            }
            (
                Some(("System.Drawing", "Graphics")),
                Some("DrawImage"),
                [Value::DrawingSurface(target), Value::Image(image), Value::Int(x), Value::Int(y)],
            ) => {
                self.forms
                    .draw_resource(*target, *image, *x, *y)
                    .ok_or(vm::Fault::Reference)?;
                return Ok(None);
            }
            (
                Some(("System.Drawing", "SolidBrush" | "Pen")),
                Some(".ctor"),
                [Value::Reference(id), Value::Int(color), width @ ..],
            ) if width.is_empty()
                || (kind == Some(("System.Drawing", "Pen"))
                    && matches!(width, [Value::Float(_)])) =>
            {
                if let [Value::Float(bits)] = width {
                    let width = f32::from_bits(*bits);
                    if !width.is_finite() || width < 0.0 {
                        return Err(vm::Fault::ManagedException);
                    }
                    self.pen_widths
                        .insert(*id, width.round().clamp(1.0, 2048.0) as u32);
                }
                let color = pocket_kernel::framebuffer::pack_rgb565(
                    (*color >> 16) as u8,
                    (*color >> 8) as u8,
                    *color as u8,
                );
                self.drawing_colors.insert(*id, color);
                return Ok(None);
            }
            (
                Some(("System.Drawing", "Graphics")),
                Some("FillEllipse" | "FillRectangle"),
                [Value::DrawingSurface(target), Value::Reference(brush), Value::Int(x), Value::Int(y), Value::Int(w), Value::Int(h)],
            ) => {
                let color = *self.drawing_colors.get(brush).ok_or(vm::Fault::Reference)?;
                let bitmap = self.forms.bitmap_mut(*target).ok_or(vm::Fault::Reference)?;
                if member == Some("FillEllipse") {
                    pocket_kernel::gdi::Surface::Screen(bitmap).ellipse(
                        [*x, *y, x.saturating_add(*w), y.saturating_add(*h)],
                        Some(color),
                        None,
                    );
                } else {
                    bitmap.fill_rect(*x, *y, *w, *h, color);
                }
                return Ok(None);
            }
            (Some(("Microsoft.VisualBasic", "Conversion")), Some("Int"), [Value::Double(bits)]) => {
                return Ok(Some(Value::Double(f64::from_bits(*bits).floor().to_bits())))
            }
            (Some(("Microsoft.VisualBasic", "Conversion")), Some("Int"), [Value::Float(bits)]) => {
                return Ok(Some(Value::Float(f32::from_bits(*bits).floor().to_bits())))
            }
            (
                Some(("System.Drawing", "Bitmap")),
                Some(".ctor"),
                [Value::Reference(id), Value::Int(w), Value::Int(h)],
            ) => {
                if !(1..=2048).contains(w) || !(1..=2048).contains(h) {
                    return Err(vm::Fault::InvalidSignature);
                }
                self.forms.set_bitmap(*id, *w as u32, *h as u32);
                return Ok(None);
            }
            (Some(("System.Drawing", "Graphics")), Some("FromImage"), [Value::Reference(id)]) => {
                return Ok(Some(Value::DrawingSurface(*id)));
            }
            (
                Some(("CompassHTC", "HTCCompass")),
                Some(".ctor" | "Start" | "Stop"),
                [Value::Reference(_)],
            ) => return Ok(None),
            (Some(("CompassHTC", "HTCCompass")), Some("get_Angle"), [Value::Reference(_)]) => {
                return Ok(Some(Value::Double(self.sensors.tilt_heading().to_bits())))
            }
            (
                Some(("System.ComponentModel", "ComponentResourceManager")),
                Some(".ctor"),
                [Value::Reference(id), Value::ManagedType(ty)],
            ) => {
                self.resource_types.insert(*id, *ty);
                return Ok(None);
            }
            (
                Some(("System.Resources", "ResourceManager")),
                Some("GetObject"),
                [Value::Reference(id), key],
            ) => {
                let key = string(*key).ok_or(vm::Fault::Reference)?;
                let ty = *self.resource_types.get(id).ok_or(vm::Fault::Reference)?;
                let name = self
                    .assembly
                    .resource_type_name(ty)
                    .ok_or(vm::Fault::InvalidSignature)?;
                let Some(image) = self
                    .assembly
                    .resource(&name)
                    .and_then(|bytes| crate::resources::image(bytes, &key))
                else {
                    return Ok(Some(Value::Null));
                };
                return Ok(Some(Value::Image(self.forms.add_image(image))));
            }
            (Some(("System.Collections", "Hashtable")), Some(".ctor"), [Value::Reference(id)]) => {
                self.tables.insert(*id, Default::default());
                return Ok(None);
            }
            (
                Some(("System.Collections", "Hashtable")),
                Some("ContainsKey"),
                [Value::Reference(id), Value::ManagedType(ty)],
            ) => {
                return Ok(Some(Value::Int(i32::from(
                    self.tables
                        .get(id)
                        .ok_or(vm::Fault::Reference)?
                        .contains(ty),
                ))));
            }
            (
                Some(("System.Collections", "Hashtable")),
                Some("Add"),
                [Value::Reference(id), Value::ManagedType(ty), Value::Null],
            ) => {
                if !self
                    .tables
                    .get_mut(id)
                    .ok_or(vm::Fault::Reference)?
                    .insert(*ty)
                {
                    return Err(vm::Fault::ManagedException);
                }
                return Ok(None);
            }
            (
                Some(("System.Collections", "Hashtable")),
                Some("Remove"),
                [Value::Reference(id), Value::ManagedType(ty)],
            ) => {
                self.tables
                    .get_mut(id)
                    .ok_or(vm::Fault::Reference)?
                    .remove(ty);
                return Ok(None);
            }
            (Some(("System", "Activator")), Some("CreateInstance"), [Value::ManagedType(ty)]) => {
                let ctor = self
                    .assembly
                    .constructor(*ty)
                    .ok_or(vm::Fault::InvalidSignature)?;
                return machine.construct(self, ctor, depth + 1).map(Some);
            }
            (
                Some(("System.Runtime.CompilerServices", "RuntimeHelpers")),
                Some("GetObjectValue"),
                [value],
            ) => return Ok(Some(*value)),
            (Some(("System.Globalization", "CultureInfo")), Some("get_CurrentCulture"), []) => {
                return Ok(Some(Value::Culture));
            }
            (Some(("System", "String")), Some("get_Length"), [value]) => {
                let text = string(*value).ok_or(vm::Fault::Reference)?;
                return Ok(Some(Value::Int(text.encode_utf16().count() as i32)));
            }
            (Some(("System", "String")), Some("Format"), [Value::Culture, format, array]) => {
                let format = string(*format).ok_or(vm::Fault::Reference)?;
                let items = machine.array(*array).ok_or(vm::Fault::Reference)?;
                if let [Value::Int(value)] = items {
                    Some(format.replace("{0}", &value.to_string()))
                } else {
                    return Err(vm::Fault::InvalidSignature);
                }
            }
            (Some(("System.Threading", "Thread")), Some("GetNamedDataSlot"), [name]) => {
                let name = string(*name).ok_or(vm::Fault::Reference)?;
                let id = self
                    .data_slots
                    .iter()
                    .position(|(key, _)| key == &name)
                    .unwrap_or_else(|| {
                        self.data_slots.push((name, Value::Null));
                        self.data_slots.len() - 1
                    });
                return Ok(Some(Value::DataSlot(id)));
            }
            (Some(("System.Threading", "Thread")), Some("GetData"), [Value::DataSlot(id)]) => {
                return Ok(Some(
                    self.data_slots.get(*id).ok_or(vm::Fault::Reference)?.1,
                ));
            }
            (
                Some(("System.Threading", "Thread")),
                Some("SetData"),
                [Value::DataSlot(id), value],
            ) => {
                self.data_slots.get_mut(*id).ok_or(vm::Fault::Reference)?.1 = *value;
                return Ok(None);
            }
            (Some(("System.Reflection", "Assembly")), Some("GetName"), [Value::Assembly]) => {
                return Ok(Some(Value::AssemblyName));
            }
            (
                Some(("System.Reflection", "AssemblyName")),
                Some("get_CodeBase"),
                [Value::AssemblyName],
            ) => Some(self.program_path.clone()),
            (Some(("System.IO", "Path")), Some("GetDirectoryName"), [path]) => string(*path)
                .and_then(|path| path.rsplit_once(['\\', '/']).map(|(dir, _)| dir.to_owned())),
            (Some(("System", "String")), Some("Concat"), [first, second, third]) => Some(
                [first, second, third]
                    .into_iter()
                    .map(|value| string(*value))
                    .collect::<Option<Vec<_>>>()
                    .ok_or(vm::Fault::InvalidSignature)?
                    .concat(),
            ),
            (Some(("System", "String")), Some("Concat"), [left, right]) => string(*left)
                .zip(string(*right))
                .map(|(left, right)| left + &right),
            (Some(("System.IO", "StreamReader")), Some(".ctor"), [Value::Reference(id), path]) => {
                let path = string(*path).ok_or(vm::Fault::InvalidSignature)?;
                self.storage.open_reader(*id, &path)?;
                return Ok(None);
            }
            (
                Some(("System.IO", "StreamWriter")),
                Some(".ctor"),
                [Value::Reference(id), path, Value::Int(0)],
            ) => {
                let path = string(*path).ok_or(vm::Fault::InvalidSignature)?;
                self.storage.open_writer(*id, &path)?;
                return Ok(None);
            }
            _ => None,
        };
        match (kind, member, args) {
            (Some(("System", "Type")), Some("GetTypeFromHandle"), [Value::MetadataToken(ty)]) => {
                return Ok(Some(Value::ManagedType(*ty)));
            }
            (
                Some(("System.Xml.Serialization", "XmlSerializer")),
                Some(".ctor"),
                [Value::Reference(serializer), Value::ManagedType(ty)],
            ) => {
                self.storage.serializer(*serializer, *ty);
                return Ok(None);
            }
            (
                Some(("System.Xml.Serialization", "XmlSerializer")),
                Some("Serialize"),
                [Value::Reference(serializer), Value::Reference(writer), object],
            ) => {
                self.storage
                    .serialize(&self.assembly, machine, *serializer, *writer, *object)?;
                return Ok(None);
            }
            (
                Some(("System.Xml.Serialization", "XmlSerializer")),
                Some("Deserialize"),
                [Value::Reference(serializer), Value::Reference(reader)],
            ) => {
                let (constructor, fields) =
                    self.storage
                        .deserialize_plan(&self.assembly, *serializer, *reader)?;
                let object = machine.construct(self, constructor, depth + 1)?;
                let Value::Reference(id) = object else {
                    return Err(vm::Fault::Reference);
                };
                for (token, value) in fields {
                    machine.write_address(Value::FieldAddress(id, token), value)?;
                }
                return Ok(Some(object));
            }
            (
                Some(("System.IO", "TextReader" | "TextWriter")),
                Some("Close"),
                [Value::Reference(id)],
            ) => {
                self.storage.close(*id);
                return Ok(None);
            }
            _ => {}
        }
        if let Some(text) = text {
            if let Some(&index) = self.string_ids.get(text.as_str()) {
                return Ok(Some(Value::HostString(index)));
            }
            let bytes = self
                .string_bytes
                .checked_add(text.len())
                .ok_or(vm::Fault::Budget)?;
            if bytes > 16 * 1024 * 1024 || self.strings.len() >= 262_144 {
                return Err(vm::Fault::Budget);
            }
            let index = self.strings.len();
            let text: std::rc::Rc<str> = text.into();
            self.string_ids.insert(text.clone(), index);
            self.strings.push(text);
            self.string_bytes = bytes;
            return Ok(Some(Value::HostString(index)));
        }
        if let (Some(("System.Windows.Forms", "Application")), Some("Run"), [form]) =
            (kind, member, args)
        {
            let owner = machine.constructor(*form).ok_or(vm::Fault::Reference)?;
            let paint = self.assembly.override_method(owner, "OnPaint", 1);
            self.forms.run(*form, paint).ok_or(vm::Fault::Reference)?;
            return Ok(None);
        }
        if kind == Some(("System", "Math")) {
            let float = |value: vm::Value| match value {
                vm::Value::Float(bits) => Some(f64::from(f32::from_bits(bits))),
                vm::Value::Double(bits) => Some(f64::from_bits(bits)),
                _ => None,
            };
            let result = match (member, args) {
                (Some("Atan2"), [y, x]) => float(*y).zip(float(*x)).map(|(y, x)| y.atan2(x)),
                (Some("Abs"), [value]) => float(*value).map(f64::abs),
                (Some("Sin"), [value]) => float(*value).map(f64::sin),
                (Some("Cos"), [value]) => float(*value).map(f64::cos),
                (Some("Sqrt"), [value]) => float(*value).map(f64::sqrt),
                (Some("Round"), [value]) => float(*value).map(f64::round_ties_even),
                _ => None,
            };
            if let Some(value) = result {
                let single = member == Some("Abs") && matches!(args, [vm::Value::Float(_)]);
                return Ok(Some(if single {
                    vm::Value::Float((value as f32).to_bits())
                } else {
                    vm::Value::Double(value.to_bits())
                }));
            }
        }
        if let (Some(("System", "IntPtr")), Some("op_Explicit"), [pointer]) = (kind, member, args) {
            if matches!(pointer, vm::Value::ArrayAddress(..) | vm::Value::Int(_)) {
                return Ok(Some(*pointer));
            }
        }
        if let (Some(("OpenGLES", "Texture")), Some(property), [vm::Value::Int(name)]) =
            (kind, member, args)
        {
            let value = match property {
                "get_Name" => Some(*name),
                "get_Width" => self
                    .graphics
                    .texture_size(*name as u32)
                    .map(|(w, _)| w as i32),
                "get_Height" => self
                    .graphics
                    .texture_size(*name as u32)
                    .map(|(_, h)| h as i32),
                _ => None,
            };
            if let Some(value) = value {
                return Ok(Some(vm::Value::Int(value)));
            }
        }
        match (kind, member, args) {
            (Some(("System", "Object")), Some(".ctor"), [Value::Reference(_)]) => {
                return Ok(None);
            }
            (Some(("System.Reflection", "Assembly")), Some("GetExecutingAssembly"), []) => {
                return Ok(Some(vm::Value::Assembly));
            }
            (
                Some(("System.Reflection", "Assembly")),
                Some("GetManifestResourceStream"),
                [vm::Value::Assembly, vm::Value::StringToken(name)],
            ) => {
                if let Some(data) = self
                    .assembly
                    .user_string(*name)
                    .and_then(|name| self.assembly.resource(&name))
                {
                    let id = if let Some(id) = self.streams.iter().position(Option::is_none) {
                        self.streams[id] = Some(data);
                        id
                    } else {
                        self.streams.push(Some(data));
                        self.streams.len() - 1
                    };
                    return Ok(Some(vm::Value::ResourceStream(id)));
                }
                return Ok(Some(vm::Value::Null));
            }
            (
                Some(("OpenGLES", "Texture")),
                Some("LoadStream"),
                [vm::Value::ResourceStream(id), vm::Value::Int(filter)],
            ) => {
                if let Some(texture) = self
                    .streams
                    .get(*id)
                    .and_then(|data| self.graphics.texture((*data)?, *filter != 0))
                {
                    return Ok(Some(vm::Value::Int(texture as i32)));
                }
            }
            _ => {}
        }
        if kind == Some(("System", "IDisposable")) && member == Some("Dispose") {
            if let Some(value) = self.sensors.call("IDisposable", "Dispose", args) {
                return Ok(value);
            }
        }
        if let (Some(("Sensors", class)), Some(name)) = (kind, member) {
            let sample = match args {
                [address] => machine.read_address(*address),
                _ => None,
            };
            let sample_args = sample.map(|sample| [sample]);
            let args = sample_args
                .as_ref()
                .map_or(args, |values| values.as_slice());
            if let Some(value) = self.sensors.call(class, name, args) {
                return Ok(value);
            }
        }
        if let (Some(("OpenGLES", "egl")), Some(name)) = (kind, member) {
            if let Some(result) = self.graphics.egl(machine, name, args) {
                return result;
            }
        }
        if kind == Some(("OpenGLES", "Vector3f")) {
            use vm::Value::{Float, Vector3};
            let value = match (member, args) {
                (Some("op_Multiply"), [Vector3(vector), Float(scale)]) => Some(
                    vector.map(|bits| (f32::from_bits(bits) * f32::from_bits(*scale)).to_bits()),
                ),
                (Some("op_Addition" | "op_Subtraction"), [Vector3(left), Vector3(right)]) => {
                    Some(std::array::from_fn(|i| {
                        let (left, right) = (f32::from_bits(left[i]), f32::from_bits(right[i]));
                        if member == Some("op_Addition") {
                            left + right
                        } else {
                            left - right
                        }
                        .to_bits()
                    }))
                }
                _ => None,
            };
            if let Some(value) = value {
                return Ok(Some(Vector3(value)));
            }
        }
        if let (Some(("OpenGLES", "gl")), Some(name)) = (kind, member) {
            if let Some(result) = self.graphics.gl(machine, name, args) {
                return Ok(result);
            }
        }
        if kind == Some(("System.Runtime.CompilerServices", "RuntimeHelpers"))
            && member == Some("InitializeArray")
        {
            if let [array, vm::Value::MetadataToken(field)] = args {
                if let Some(elements) = machine.array_mut(*array) {
                    let length = elements.len().checked_mul(4).ok_or(vm::Fault::Budget)?;
                    if let Some(data) = self.assembly.field_data(*field, length) {
                        for (element, bytes) in elements.iter_mut().zip(data.chunks_exact(4)) {
                            *element =
                                vm::Value::Int(i32::from_le_bytes(bytes.try_into().unwrap()));
                        }
                        return Ok(None);
                    }
                }
            }
        }
        if let (Some(("Microsoft.VisualBasic", "VBMath")), Some(name)) = (kind, member) {
            if let Some(result) = self.system.vb_random(name, args) {
                return Ok(result);
            }
        }
        if let (Some(("System", "Random")), Some(name)) = (kind, member) {
            if let Some(result) = self.system.random(name, args) {
                return result;
            }
        }
        if let (
            Some((
                "System.Windows.Forms",
                class @ ("Form" | "ScrollableControl" | "Panel" | "Label" | "Button" | "CheckBox"
                | "ComboBox" | "Timer" | "MenuItem" | "MainMenu" | "Control"
                | "ContainerControl" | "ListControl" | "PictureBox"),
            )),
            Some(name),
        ) = (kind, member)
        {
            if let Some(result) = self.forms.call(machine, class, name, args) {
                return Ok(result);
            }
        }
        if let (Some(("", class @ ("ControlCollection" | "ObjectCollection"))), Some(name)) =
            (kind, member)
        {
            if let Some(result) = self.forms.call(machine, class, name, args) {
                return Ok(result);
            }
        }
        if kind == Some(("System.Drawing", "Point")) {
            use vm::Value::{Int, Null, Point};
            match (member, args) {
                (Some("set_X" | "set_Y"), [address, Int(value)]) => {
                    let (x, y) = match machine.read_address(*address) {
                        Some(Point(x, y)) => (x, y),
                        Some(Null) => (0, 0),
                        _ => return Err(vm::Fault::Reference),
                    };
                    let value = if member == Some("set_X") {
                        Point(*value, y)
                    } else {
                        Point(x, *value)
                    };
                    machine.write_address(*address, value)?;
                    return Ok(None);
                }
                (Some("get_X" | "get_Y"), [address]) => {
                    if let Some(Point(x, y)) = machine.read_address(*address) {
                        return Ok(Some(Int(if member == Some("get_X") { x } else { y })));
                    }
                }
                _ => {}
            }
        }
        if let (Some(("System.Drawing", "Rectangle")), Some(member), [address]) =
            (kind, member, args)
        {
            if let Some(vm::Value::Rectangle(x, y, w, h)) = machine.read_address(*address) {
                let value = match member {
                    "get_X" | "get_Left" => Some(x),
                    "get_Y" | "get_Top" => Some(y),
                    "get_Width" => Some(w),
                    "get_Height" => Some(h),
                    _ => None,
                };
                if let Some(value) = value {
                    return Ok(Some(vm::Value::Int(value)));
                }
            }
        }
        if kind == Some(("System.Drawing", "Font"))
            && member == Some(".ctor")
            && self.forms.font(args).is_some()
        {
            return Ok(None);
        }
        if kind == Some(("System.Drawing", "Color")) && args.is_empty() {
            if let Some(color) = match member {
                Some("get_Gold") => Some(0xffff_d700u32),
                Some("get_White") => Some(0xffff_ffffu32),
                Some("get_Black") => Some(0xff00_0000u32),
                _ => None,
            } {
                return Ok(Some(vm::Value::Int(color as i32)));
            }
        }
        if let (Some(("System.Drawing", "Color")), Some("FromArgb")) = (kind, member) {
            use vm::Value::Int;
            let component = |value: i32| (0..=255).contains(&value);
            match args {
                [Int(argb)] => return Ok(Some(Int(*argb))),
                [Int(red), Int(green), Int(blue)]
                    if [*red, *green, *blue].into_iter().all(component) =>
                {
                    return Ok(Some(Int((-1 << 24) | (red << 16) | (green << 8) | blue)));
                }
                [Int(alpha), Int(red), Int(green), Int(blue)]
                    if [*alpha, *red, *green, *blue].into_iter().all(component) =>
                {
                    return Ok(Some(Int((alpha << 24) | (red << 16) | (green << 8) | blue)));
                }
                _ => {}
            }
        }
        self.unresolved = member;
        self.unresolved_type = kind;
        Err(vm::Fault::UnsupportedCall(token))
    }
}

#[derive(Debug)]
pub enum StartupError<'a> {
    Assembly,
    InvalidScreen,
    Execution {
        fault: vm::Fault,
        unresolved_member: Option<&'a str>,
        unresolved_type: Option<(&'a str, &'a str)>,
    },
}

pub struct Session<'a> {
    startup: Startup<'a>,
    forms_frame: Option<(u64, pocket_kernel::Framebuffer)>,
    forms_canvas: Option<pocket_kernel::Framebuffer>,
    next_collection: u64,
    machine: vm::Machine,
}

impl<'a> Session<'a> {
    pub fn start(
        bytes: &'a [u8],
        size: (u32, u32),
        budget: usize,
    ) -> Result<Self, StartupError<'a>> {
        Self::start_in(bytes, size, budget, None, None, (false, 1), None)
    }

    pub(crate) fn start_in(
        bytes: &'a [u8],
        size: (u32, u32),
        budget: usize,
        save_root: Option<std::path::PathBuf>,
        guest_path: Option<String>,
        gpu: (bool, u8),
        asset_root: Option<std::path::PathBuf>,
    ) -> Result<Self, StartupError<'a>> {
        if !(1..=2048).contains(&size.0) || !(1..=2048).contains(&size.1) {
            return Err(StartupError::InvalidScreen);
        }
        let assembly = Assembly::read(bytes).ok_or(StartupError::Assembly)?;
        let entry = assembly.entry_point().ok_or(StartupError::Assembly)?.token;
        let program_path =
            guest_path.unwrap_or_else(|| r"\Program Files\Application\application.exe".into());
        let mut assets = pocket_kernel::vfs::Vfs::new();
        if let Some((root, (directory, _))) = asset_root.zip(program_path.rsplit_once(['\\', '/']))
        {
            assets.mount_read_only(directory, root);
        }
        let mut startup = Startup {
            assembly,
            unresolved: None,
            streams: Vec::new(),
            unresolved_type: None,
            system: system::Services::default(),
            forms: forms::Forms::default(),
            graphics: graphics::Graphics::new(size, gpu),
            sensors: sensors::Sensors::default(),
            strings: Vec::new(),
            string_ids: Default::default(),
            string_bytes: 0,
            started: std::time::Instant::now(),
            audio: pocket_kernel::AudioEngine::new(),
            fmod: Default::default(),
            assets,
            data_slots: Vec::new(),
            drawing_colors: Default::default(),
            pen_widths: Default::default(),
            resource_types: Default::default(),
            tables: Default::default(),
            program_path,
            storage: storage::Storage::default(),
        };
        startup.storage.root = save_root;
        startup.forms.set_display_size(size);
        let mut machine = vm::Machine::new(budget);
        machine
            .run(&mut startup, entry)
            .map_err(|fault| startup.error(fault))?;
        for (sender, handler) in startup.forms.initial_events() {
            machine
                .event(&mut startup, sender, handler, budget)
                .map_err(|fault| startup.error(fault))?;
        }
        if let Some((form, paint)) = startup.forms.take_paint() {
            machine
                .paint(&mut startup, form, paint, budget)
                .map_err(|fault| startup.error(fault))?;
        }
        let mut session = Self {
            startup,
            machine,
            forms_frame: None,
            forms_canvas: None,
            next_collection: 1000,
        };
        session.refresh_forms_frame();
        Ok(session)
    }

    pub(crate) fn audio(&mut self) -> &mut pocket_kernel::AudioEngine {
        &mut self.startup.audio
    }

    pub fn set_acceleration(&mut self, value: [f32; 3]) {
        self.startup.sensors.set_acceleration(value);
    }

    pub fn click(&mut self, x: u16, y: u16, budget: usize) -> Result<(), StartupError<'a>> {
        let (x, y) = if self.startup.graphics.frame().is_none() {
            let (width, height) = self.startup.forms.display_size();
            let (w, h) = self.startup.forms.canvas_size();
            (
                (u32::from(x) * w / width).min(w - 1) as u16,
                (u32::from(y) * h / height).min(h - 1) as u16,
            )
        } else {
            (x, y)
        };
        self.startup.forms.set_pointer(x, y);
        for (sender, handler, event) in self.startup.forms.click_events(x, y) {
            self.machine
                .event_with_args(&mut self.startup, sender, handler, event, budget)
                .map_err(|fault| self.startup.error(fault))?;
        }
        self.refresh_forms_frame();
        self.finish_close(budget)
    }

    pub fn close(&mut self, budget: usize) -> Result<(), StartupError<'a>> {
        self.startup.forms.request_close();
        self.finish_close(budget)
    }

    fn finish_close(&mut self, budget: usize) -> Result<(), StartupError<'a>> {
        if let Some(events) = self.startup.forms.close_events() {
            for (sender, handler) in events {
                self.machine
                    .event(&mut self.startup, sender, handler, budget)
                    .map_err(|fault| self.startup.error(fault))?;
            }
        }
        Ok(())
    }

    pub(crate) fn paint_controls(&self, frame: &mut pocket_kernel::Framebuffer) {
        if self.startup.graphics.frame().is_some() {
            self.startup
                .forms
                .paint_controls(frame, |token| self.startup.assembly.user_string(token));
        }
    }

    fn refresh_forms_frame(&mut self) {
        if self.startup.graphics.frame().is_some() || !self.startup.forms.running() {
            return;
        }
        let (width, height) = self.startup.forms.display_size();
        let (generation, framebuffer) = self
            .forms_frame
            .get_or_insert_with(|| (0, pocket_kernel::Framebuffer::new(width, height)));
        let (w, h) = self.startup.forms.canvas_size();
        let draw = |target: &mut pocket_kernel::Framebuffer| {
            target.fill(0);
            self.startup
                .forms
                .paint_controls(target, |token| self.startup.assembly.user_string(token));
        };
        if (w, h) == (width, height) {
            draw(framebuffer);
        } else {
            if self
                .forms_canvas
                .as_ref()
                .is_none_or(|canvas| (canvas.width, canvas.height) != (w, h))
            {
                self.forms_canvas = Some(pocket_kernel::Framebuffer::new(w, h));
            }
            let canvas = self.forms_canvas.as_mut().unwrap();
            draw(canvas);
            for y in 0..height {
                for x in 0..width {
                    let src = ((y * h / height) * w + x * w / width) as usize * 2;
                    let dst = (y * width + x) as usize * 2;
                    framebuffer.pixels[dst..dst + 2].copy_from_slice(&canvas.pixels[src..src + 2]);
                }
            }
        }
        *generation = generation.wrapping_add(1);
    }

    pub fn frame(&self) -> Option<(u64, (u32, u32), &[u8])> {
        self.startup.graphics.frame().or_else(|| {
            let (generation, frame) = self.forms_frame.as_ref()?;
            Some((
                *generation,
                (frame.width, frame.height),
                frame.pixels.as_slice(),
            ))
        })
    }

    pub fn running(&self) -> bool {
        self.startup.forms.running()
    }

    fn collect_objects(&mut self) {
        let mut roots = Vec::new();
        self.startup.forms.roots(&mut roots);
        self.startup.graphics.roots(&mut roots);
        roots.extend(self.startup.data_slots.iter().map(|(_, value)| *value));
        let mut images = std::collections::BTreeSet::new();
        let mut streams = std::collections::BTreeSet::new();
        let live = self.machine.collect(roots, |value, pending| match value {
            vm::Value::Reference(id) => self.startup.forms.trace_control(id, pending),
            vm::Value::Image(id) => {
                images.insert(id);
            }
            vm::Value::ResourceStream(id) => {
                streams.insert(id);
            }
            _ => {}
        });
        self.startup.forms.sweep(&live, &images);
        for (id, stream) in self.startup.streams.iter_mut().enumerate() {
            if !streams.contains(&id) {
                *stream = None;
            }
        }
        self.startup.storage.sweep(&live);
        self.startup.system.sweep(&live);
        self.startup.tables.retain(|&id, _| live[id]);
        self.startup.resource_types.retain(|&id, _| live[id]);
        self.startup.pen_widths.retain(|&id, _| live[id]);
        self.startup.drawing_colors.retain(|&id, _| live[id]);
    }

    pub fn tick(&mut self, now_ms: u64, budget: usize) -> Result<(), StartupError<'a>> {
        let events = self.startup.forms.timer_events(now_ms);
        let changed = !events.is_empty();
        for (sender, handler) in events {
            self.machine
                .event(&mut self.startup, sender, handler, budget)
                .map_err(|fault| self.startup.error(fault))?;
        }
        if let Some((form, paint)) = self.startup.forms.take_paint() {
            self.machine
                .paint(&mut self.startup, form, paint, budget)
                .map_err(|fault| self.startup.error(fault))?;
        }
        if changed || self.startup.forms.take_invalidated() {
            self.refresh_forms_frame();
        }
        if now_ms >= self.next_collection {
            self.collect_objects();
            self.next_collection = now_ms.saturating_add(1000);
        }
        Ok(())
    }
}

impl<'a> Startup<'a> {
    fn error(&self, fault: vm::Fault) -> StartupError<'a> {
        StartupError::Execution {
            fault,
            unresolved_member: self.unresolved,
            unresolved_type: self.unresolved_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    #[test]
    #[ignore = "requires POCKETHLE_DETECTOR_VELES_EXE"]
    fn detector_veles_displays_and_runs_its_forms_timers() {
        let exe = std::env::var_os("POCKETHLE_DETECTOR_VELES_EXE").unwrap();
        let bytes = std::fs::read(exe).unwrap();
        assert!(crate::references_accelerometer(&bytes));
        let assets =
            std::env::var_os("POCKETHLE_DETECTOR_VELES_ASSETS").map(std::path::PathBuf::from);
        let verify_audio = assets.is_some();
        let mut session =
            Session::start_in(&bytes, (480, 640), 500_000, None, None, (false, 1), assets).unwrap();
        let tap = session.audio().tap();
        if verify_audio {
            let bytes = super::storage::read_bounded(
                &mut session.startup.assets,
                r"\Program Files\Application\anomaly.wav",
                4 * 1024 * 1024,
            )
            .unwrap();
            let sample = session.startup.fmod.load(&bytes).unwrap();
            assert!(session
                .startup
                .fmod
                .play(&session.startup.audio, -1, sample)
                .is_some());
            assert!(session.startup.fmod.free(sample));
        }
        let mut pcm = [0i16; 4096];
        let mut heard = false;
        session.set_acceleration([4.9, 8.487, 0.0]);
        assert!((session.startup.sensors.tilt_heading() - 30.0).abs() < 0.1);
        let initial = session.frame().unwrap().2.to_vec();
        assert!(initial.iter().any(|byte| *byte != 0));
        for now in (0..120_000).step_by(40) {
            session
                .tick(now, 500_000)
                .unwrap_or_else(|error| panic!("timer at {now}: {error:?}"));
            let samples = tap.drain_into(&mut pcm);
            heard |= pcm[..samples].iter().any(|&sample| sample != 0);
        }
        assert_ne!(initial, session.frame().unwrap().2);
        session.click(270, 360, 500_000).unwrap();
        for now in (120_000..123_000).step_by(40) {
            session
                .tick(now, 500_000)
                .unwrap_or_else(|error| panic!("after click at {now}: {error:?}"));
        }
        assert!(session.running());
        eprintln!(
            "Veles at 33s: {} objects, {} host strings",
            session.machine.object_count(),
            session.startup.strings.len()
        );
        assert!(session.machine.object_count() < 512);
        if verify_audio {
            assert!(heard, "Detector Veles produced no mixed PCM");
        }
    }

    /// Check the liquid itself, independently of moving bubbles and UI controls.
    #[test]
    #[ignore = "requires POCKETHLE_DIAMOND_BEER_EXE"]
    fn diamond_beer_renders_duvel_and_moves_the_liquid() {
        let exe = std::env::var_os("POCKETHLE_DIAMOND_BEER_EXE").unwrap();
        let bytes = std::fs::read(exe).unwrap();
        const BUDGET: usize = 500_000;
        let gpu = std::env::var_os("POCKETHLE_NETCF_TEST_GPU").is_some();
        let render = |acceleration, show_logo| {
            let save = tempfile::tempdir().unwrap();
            let mut session = Session::start_in(
                &bytes,
                (240, 320),
                BUDGET,
                Some(save.path().to_path_buf()),
                None,
                (gpu, 1),
                None,
            )
            .unwrap();
            if show_logo {
                for (x, y) in [
                    (3, 310),
                    (110, 73),
                    (110, 73),
                    (110, 73),
                    (95, 100),
                    (65, 188),
                ] {
                    session.click(x, y, BUDGET).unwrap();
                }
            }
            session.set_acceleration(acceleration);
            for now in (0..400).step_by(40) {
                session.tick(now, BUDGET).unwrap();
            }
            assert_eq!(session.startup.graphics.gpu_active(), gpu);
            let frame = session.frame().unwrap().2.to_vec();
            session.close(BUDGET).unwrap();
            assert!(save.path().join("Settings.xml").is_file());
            frame
        };
        let upright = render([0.0, 9.8, 0.0], false);
        let right = render([4.9, 8.487, 0.0], true);
        let left = render([-4.9, 8.487, 0.0], true);
        let pixel = |frame: &[u8], x: usize, y: usize| {
            let offset = (y * 240 + x) * 2;
            u16::from_le_bytes([frame[offset], frame[offset + 1]])
        };
        let red_logo_pixels = (45..175)
            .flat_map(|y| (30..210).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = pixel(&right, x, y);
                (p >> 11) > 16 && ((p >> 5) & 63) < 25 && (p & 31) < 16
            })
            .count();
        assert!(
            red_logo_pixels > 100,
            "Duvel logo missing: {red_logo_pixels}"
        );
        let bright = |frame: &[u8], x| {
            (5..100)
                .filter(|&y| {
                    let p = pixel(frame, x, y);
                    p >> 11 > 23 && (p >> 5) & 63 > 44
                })
                .count() as i32
        };
        assert!(pixel(&upright, 120, 160) > 0x8000, "liquid missing at rest");
        let left_edge = bright(&left, 55) - bright(&right, 55);
        let right_edge = bright(&left, 185) - bright(&right, 185);
        assert!(
            left_edge.abs() > 15
                && right_edge.abs() > 15
                && left_edge.signum() != right_edge.signum(),
            "the liquid surface did not reverse slope: {left_edge}, {right_edge}"
        );
    }
}
