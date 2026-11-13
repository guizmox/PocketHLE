//! Managed startup and service dispatch.

use crate::metadata::{aligned, dword, word, Assembly};
use crate::{forms, graphics, sensors, storage, system, vm};

struct Startup<'a> {
    assembly: Assembly<'a>,
    unresolved: Option<&'a str>,
    streams: Vec<&'a [u8]>,
    unresolved_type: Option<(&'a str, &'a str)>,
    system: system::Services,
    forms: forms::Forms,
    graphics: graphics::Graphics,
    sensors: sensors::Sensors,
    strings: Vec<String>,
    program_path: String,
    storage: storage::Storage,
    started: std::time::Instant,
}

impl vm::Environment for Startup<'_> {
    fn signature(&self, token: u32) -> Option<vm::Signature> {
        self.assembly.signature(token)
    }

    fn body(&self, token: u32) -> Option<&[u8]> {
        self.assembly.method(token).map(|method| method.il)
    }

    fn exception_clauses(&self, token: u32) -> Vec<vm::ExceptionClause> {
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
                ("System", "EventHandler") | ("System.ComponentModel", "CancelEventHandler"),
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
        let string = |value| match value {
            Value::StringToken(token) => self.assembly.user_string(token),
            Value::HostString(index) => self.strings.get(index).cloned(),
            _ => None,
        };
        let text = match (kind, member, args) {
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
            let index = self.strings.len();
            self.strings.push(text);
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
                    let id = self.streams.len();
                    self.streams.push(data);
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
                    .and_then(|data| self.graphics.texture(data, *filter != 0))
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
                | "ContainerControl" | "ListControl"),
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
    machine: vm::Machine,
}

impl<'a> Session<'a> {
    pub fn start(
        bytes: &'a [u8],
        size: (u32, u32),
        budget: usize,
    ) -> Result<Self, StartupError<'a>> {
        Self::start_in(bytes, size, budget, None, None, false)
    }

    pub(crate) fn start_in(
        bytes: &'a [u8],
        size: (u32, u32),
        budget: usize,
        save_root: Option<std::path::PathBuf>,
        guest_path: Option<String>,
        gpu_renderer: bool,
    ) -> Result<Self, StartupError<'a>> {
        if !(1..=2048).contains(&size.0) || !(1..=2048).contains(&size.1) {
            return Err(StartupError::InvalidScreen);
        }
        let assembly = Assembly::read(bytes).ok_or(StartupError::Assembly)?;
        let entry = assembly.entry_point().ok_or(StartupError::Assembly)?.token;
        let mut startup = Startup {
            assembly,
            unresolved: None,
            streams: Vec::new(),
            unresolved_type: None,
            system: system::Services::default(),
            forms: forms::Forms::default(),
            graphics: graphics::Graphics::new(size, gpu_renderer),
            sensors: sensors::Sensors::default(),
            strings: Vec::new(),
            started: std::time::Instant::now(),
            program_path: guest_path
                .unwrap_or_else(|| r"\Program Files\Application\application.exe".into()),
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
        Ok(Self { startup, machine })
    }

    pub fn set_acceleration(&mut self, value: [f32; 3]) {
        self.startup.sensors.set_acceleration(value);
    }

    pub fn click(&mut self, x: u16, y: u16, budget: usize) -> Result<(), StartupError<'a>> {
        self.startup.forms.set_pointer(x, y);
        for (sender, handler) in self.startup.forms.click_events(x, y) {
            self.machine
                .event(&mut self.startup, sender, handler, budget)
                .map_err(|fault| self.startup.error(fault))?;
        }
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
        self.startup
            .forms
            .paint_controls(frame, |token| self.startup.assembly.user_string(token));
    }

    pub fn frame(&self) -> Option<(u64, (u32, u32), &[u8])> {
        self.startup.graphics.frame()
    }

    pub fn running(&self) -> bool {
        self.startup.forms.running()
    }

    pub fn tick(&mut self, now_ms: u64, budget: usize) -> Result<(), StartupError<'a>> {
        for (sender, handler) in self.startup.forms.timer_events(now_ms) {
            self.machine
                .event(&mut self.startup, sender, handler, budget)
                .map_err(|fault| self.startup.error(fault))?;
        }
        if let Some((form, paint)) = self.startup.forms.take_paint() {
            self.machine
                .paint(&mut self.startup, form, paint, budget)
                .map_err(|fault| self.startup.error(fault))?;
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
                gpu,
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
