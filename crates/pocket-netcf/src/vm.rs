//! Stack execution for the subset of IL used during managed startup.

use crate::il::{self, Instruction};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i32),
    Float(u32),
    Double(u64),
    SizeF(u32, u32),
    Vector3([u32; 3]),
    Reference(usize),
    LocalAddress(usize, u8),
    ArgumentAddress(usize, u8),
    ArrayAddress(usize, usize),
    FieldAddress(usize, u32),
    Rectangle(i32, i32, i32, i32),
    StringToken(u32),
    Assembly,
    AssemblyName,
    HostString(usize),
    ResourceStream(usize),
    EventArgs,
    MouseEvent(i32, i32),
    Culture,
    DefaultEncoding,
    DataSlot(usize),
    DrawingSurface(usize),
    DateTime(u64),
    Font,
    Image(usize),
    Sensor,
    Acceleration([u32; 3]),
    Function(u32),
    MetadataToken(u32),
    ManagedType(u32),
    Delegate { target: Option<usize>, method: u32 },
    ControlCollection(usize),
    ItemCollection(usize),
    Point(i32, i32),
    Size(i32, i32),
    Null,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    Decode(il::DecodeError),
    Stack,
    Argument,
    Reference,
    ArrayLength,
    Budget,
    UnsupportedCall(u32),
    MissingMethod(u32),
    InvalidSignature,
    InvalidBranch,
    ManagedException,
    MissingFile,
}

#[derive(Clone, Copy)]
pub struct ExceptionClause {
    pub flags: u32,
    pub try_start: usize,
    pub try_end: usize,
    pub handler_start: usize,
    pub handler_end: usize,
    pub exception: u32,
}

pub struct Signature {
    pub parameters: usize,
    pub instance: bool,
    pub returns_value: bool,
}

/// Services outside the interpreter, including method resolution and the CF API.
pub trait Environment<'image> {
    fn signature(&self, token: u32) -> Option<Signature>;
    fn body(&self, token: u32) -> Option<&'image [u8]>;
    fn exception_clauses(&self, _token: u32) -> Vec<ExceptionClause> {
        Vec::new()
    }
    fn catches(&self, _exception: u32, _fault: &Fault) -> bool {
        false
    }
    fn value_field(&self, _token: u32, _value: Value) -> Option<Value> {
        None
    }
    fn store_value_field(&self, _token: u32, _object: Value, _value: Value) -> Option<Value> {
        None
    }
    fn unbox_any(&self, _machine: &Machine, _method: u32, _ty: u32, _value: Value) -> bool {
        false
    }
    fn resolve_type(&self, _method: u32, token: u32) -> u32 {
        token
    }
    fn resolve_field(&self, token: u32) -> u32 {
        token
    }
    fn field_default(&self, _token: u32) -> Value {
        Value::Null
    }
    fn static_initializer(&self, _token: u32) -> Option<u32> {
        None
    }
    fn static_field(&mut self, token: u32) -> Result<Value, Fault>;
    fn construct_value(&self, _token: u32, _args: &[Value]) -> Option<Value> {
        None
    }
    fn call(
        &mut self,
        machine: &mut Machine,
        depth: usize,
        token: u32,
        args: &[Value],
    ) -> Result<Option<Value>, Fault>;
}

enum Object {
    Instance {
        constructor: u32,
        fields: BTreeMap<u32, Value>,
    },
    Array(Vec<Value>),
}

pub struct Machine {
    objects: Vec<Option<Object>>,
    free_objects: Vec<usize>,
    remaining: usize,
    statics: BTreeMap<u32, Value>,
    initialized: BTreeSet<u32>,
    locals: Vec<[Value; 256]>,
    arguments: Vec<Vec<Value>>,
    trace: bool,
}

impl Machine {
    #[cfg(test)]
    pub fn object_count(&self) -> usize {
        self.objects.len() - self.free_objects.len()
    }

    pub fn new(instruction_budget: usize) -> Self {
        Self {
            objects: Vec::new(),
            free_objects: Vec::new(),
            remaining: instruction_budget,
            statics: BTreeMap::new(),
            initialized: BTreeSet::new(),
            locals: Vec::new(),
            arguments: Vec::new(),
            trace: std::env::var_os("POCKETHLE_NETCF_TRACE").is_some(),
        }
    }

    pub fn run<'image>(
        &mut self,
        environment: &mut impl Environment<'image>,
        entry: u32,
    ) -> Result<Option<Value>, Fault> {
        self.invoke(environment, entry, &[], 0)
    }

    pub fn event<'image>(
        &mut self,
        environment: &mut impl Environment<'image>,
        sender: usize,
        handler: Value,
        budget: usize,
    ) -> Result<(), Fault> {
        self.event_with_args(environment, sender, handler, Value::EventArgs, budget)
    }

    pub fn event_with_args<'image>(
        &mut self,
        environment: &mut impl Environment<'image>,
        sender: usize,
        handler: Value,
        event: Value,
        budget: usize,
    ) -> Result<(), Fault> {
        let Value::Delegate { target, method } = handler else {
            return Err(Fault::Reference);
        };
        let mut args = Vec::with_capacity(3);
        if let Some(target) = target {
            args.push(Value::Reference(target));
        }
        args.extend([Value::Reference(sender), event]);
        self.remaining = budget;
        self.invoke(environment, method, &args, 0)?;
        Ok(())
    }

    pub fn constructor(&self, reference: Value) -> Option<u32> {
        let Value::Reference(id) = reference else {
            return None;
        };
        match self.objects.get(id)?.as_ref()? {
            Object::Instance { constructor, .. } => Some(*constructor),
            Object::Array(_) => None,
        }
    }

    pub fn paint<'image>(
        &mut self,
        environment: &mut impl Environment<'image>,
        form: usize,
        method: u32,
        budget: usize,
    ) -> Result<(), Fault> {
        self.remaining = budget;
        self.invoke(
            environment,
            method,
            &[Value::Reference(form), Value::EventArgs],
            0,
        )?;
        Ok(())
    }

    pub fn byte_array(&mut self, bytes: &[u8]) -> Result<Value, Fault> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or(Fault::Budget)?;
        self.allocate(Object::Array(
            bytes
                .iter()
                .map(|&byte| Value::Int(i32::from(byte)))
                .collect(),
        ))
    }

    pub fn array(&self, reference: Value) -> Option<&[Value]> {
        let Value::Reference(index) = reference else {
            return None;
        };
        match self.objects.get(index)?.as_ref()? {
            Object::Array(elements) => Some(elements),
            Object::Instance { .. } => None,
        }
    }

    pub fn array_mut(&mut self, reference: Value) -> Option<&mut [Value]> {
        let Value::Reference(index) = reference else {
            return None;
        };
        match self.objects.get_mut(index)?.as_mut()? {
            Object::Array(elements) => Some(elements),
            Object::Instance { .. } => None,
        }
    }

    pub fn read_address(&mut self, address: Value) -> Option<Value> {
        match address {
            Value::ArgumentAddress(frame, index) => {
                self.arguments.get(frame)?.get(usize::from(index)).copied()
            }
            Value::LocalAddress(frame, index) => {
                self.locals.get(frame)?.get(usize::from(index)).copied()
            }
            Value::FieldAddress(object, field) => Some(
                self.fields(Value::Reference(object))
                    .ok()?
                    .get(&field)
                    .copied()
                    .unwrap_or(Value::Null),
            ),
            Value::ArrayAddress(array, index) => {
                self.array(Value::Reference(array))?.get(index).copied()
            }
            _ => None,
        }
    }

    pub fn write_address(&mut self, address: Value, value: Value) -> Result<(), Fault> {
        match address {
            Value::ArgumentAddress(frame, index) => {
                *self
                    .arguments
                    .get_mut(frame)
                    .and_then(|args| args.get_mut(usize::from(index)))
                    .ok_or(Fault::Argument)? = value;
            }
            Value::LocalAddress(frame, index) => {
                self.locals.get_mut(frame).ok_or(Fault::Reference)?[usize::from(index)] = value;
            }
            Value::FieldAddress(object, field) => {
                self.fields(Value::Reference(object))?.insert(field, value);
            }
            Value::ArrayAddress(array, index) => {
                *self
                    .array_mut(Value::Reference(array))
                    .and_then(|items| items.get_mut(index))
                    .ok_or(Fault::Reference)? = value;
            }
            _ => return Err(Fault::Reference),
        }
        Ok(())
    }

    pub fn field(&mut self, object: Value, token: u32) -> Result<Option<Value>, Fault> {
        Ok(self.fields(object)?.get(&token).copied())
    }

    pub fn construct<'image>(
        &mut self,
        environment: &mut impl Environment<'image>,
        token: u32,
        depth: usize,
    ) -> Result<Value, Fault> {
        let object = self.allocate(Object::Instance {
            constructor: token,
            fields: BTreeMap::new(),
        })?;
        self.invoke(environment, token, &[object], depth)?;
        Ok(object)
    }

    /// Collect between guest callbacks, when no interpreter frames are active.
    pub fn collect(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
        mut trace: impl FnMut(Value, &mut Vec<Value>),
    ) -> Vec<bool> {
        debug_assert!(self.locals.is_empty() && self.arguments.is_empty());
        let mut pending: Vec<_> = roots
            .into_iter()
            .chain(self.statics.values().copied())
            .collect();
        let mut live = vec![false; self.objects.len()];
        while let Some(value) = pending.pop() {
            let id = match value {
                Value::Reference(id)
                | Value::DrawingSurface(id)
                | Value::FieldAddress(id, _)
                | Value::ArrayAddress(id, _)
                | Value::ControlCollection(id)
                | Value::ItemCollection(id)
                | Value::Delegate {
                    target: Some(id), ..
                } => id,
                _ => {
                    trace(value, &mut pending);
                    continue;
                }
            };
            let Some(mark) = live.get_mut(id) else {
                continue;
            };
            if std::mem::replace(mark, true) {
                continue;
            }
            trace(Value::Reference(id), &mut pending);
            match self.objects[id].as_ref() {
                Some(Object::Instance { fields, .. }) => pending.extend(fields.values().copied()),
                Some(Object::Array(values)) => pending.extend(values.iter().copied()),
                None => {}
            }
        }
        for (id, (object, &live)) in self.objects.iter_mut().zip(&live).enumerate() {
            if !live && object.take().is_some() {
                self.free_objects.push(id);
            }
        }
        live
    }

    fn allocate(&mut self, object: Object) -> Result<Value, Fault> {
        if let Some(index) = self.free_objects.pop() {
            self.objects[index] = Some(object);
            return Ok(Value::Reference(index));
        }
        self.objects.try_reserve(1).map_err(|_| Fault::Budget)?;
        let index = self.objects.len();
        self.objects.push(Some(object));
        Ok(Value::Reference(index))
    }

    fn fields(&mut self, value: Value) -> Result<&mut BTreeMap<u32, Value>, Fault> {
        let Value::Reference(index) = value else {
            return Err(Fault::Reference);
        };
        match self.objects.get_mut(index).and_then(Option::as_mut) {
            Some(Object::Instance { fields, .. }) => Ok(fields),
            _ => Err(Fault::Reference),
        }
    }

    fn invoke<'image>(
        &mut self,
        environment: &mut impl Environment<'image>,
        token: u32,
        args: &[Value],
        depth: usize,
    ) -> Result<Option<Value>, Fault> {
        if depth >= 64 {
            return Err(Fault::Budget);
        }
        let signature = environment
            .signature(token)
            .ok_or(Fault::MissingMethod(token))?;
        if signature
            .parameters
            .checked_add(usize::from(signature.instance))
            != Some(args.len())
        {
            return Err(Fault::InvalidSignature);
        }
        let Some(body) = environment.body(token) else {
            let result = environment.call(self, depth, token, args)?;
            return if result.is_some() == signature.returns_value {
                Ok(result)
            } else {
                Err(Fault::InvalidSignature)
            };
        };
        let frame = self.locals.len();
        self.locals.push([Value::Null; 256]);
        self.arguments.push(args.to_vec());
        let result = self.execute(
            environment,
            token,
            body,
            depth,
            signature.returns_value,
            frame,
        );
        self.locals.pop();
        self.arguments.pop();
        result
    }

    fn execute<'image>(
        &mut self,
        environment: &mut impl Environment<'image>,
        token: u32,
        body: &[u8],
        depth: usize,
        returns_value: bool,
        frame: usize,
    ) -> Result<Option<Value>, Fault> {
        let mut stack = Vec::new();
        let mut offset = 0;
        let clauses = environment.exception_clauses(token);
        let mut finally_resume: Option<usize> = None;
        loop {
            self.remaining = self.remaining.checked_sub(1).ok_or(Fault::Budget)?;
            let instruction_offset = offset;
            let (instruction, next) = il::decode(body, offset).map_err(Fault::Decode)?;
            offset = next;
            if self.trace {
                eprintln!("{token:08x}:{instruction_offset:04x} {instruction:?} {stack:?}");
            }
            match instruction {
                Instruction::LoadLocal(index) => stack.push(self.locals[frame][usize::from(index)]),
                Instruction::StoreLocal(index) => {
                    self.locals[frame][usize::from(index)] = stack.pop().ok_or(Fault::Stack)?;
                }
                Instruction::LocalAddress(index) => stack.push(Value::LocalAddress(frame, index)),
                Instruction::Leave(delta) => {
                    stack.clear();
                    let destination = offset
                        .checked_add_signed(delta as isize)
                        .ok_or(Fault::InvalidBranch)?;
                    let finally = clauses.iter().find(|clause| {
                        clause.flags == 2
                            && instruction_offset >= clause.try_start
                            && instruction_offset < clause.try_end
                            && !(clause.try_start..clause.try_end).contains(&destination)
                    });
                    if let Some(clause) = finally {
                        finally_resume = Some(destination);
                        offset = clause.handler_start;
                    } else {
                        offset = destination;
                    }
                }
                Instruction::EndFinally => {
                    offset = finally_resume.take().ok_or(Fault::InvalidBranch)?;
                }
                Instruction::CastClass(_) => match stack.last() {
                    Some(Value::Reference(_) | Value::Image(_) | Value::Null) => {}
                    _ => return Err(Fault::ManagedException),
                },
                Instruction::Branch(delta) => {
                    offset = offset
                        .checked_add_signed(delta as isize)
                        .ok_or(Fault::InvalidBranch)?;
                }
                Instruction::Switch { table, count } => {
                    let Value::Int(index) = stack.pop().ok_or(Fault::Stack)? else {
                        return Err(Fault::Stack);
                    };
                    if (index as usize) < count {
                        let entry = table + index as usize * 4;
                        let delta = i32::from_le_bytes(body[entry..entry + 4].try_into().unwrap());
                        offset = offset
                            .checked_add_signed(delta as isize)
                            .ok_or(Fault::InvalidBranch)?;
                    }
                }
                Instruction::BranchIf(delta, expected) => {
                    let value = stack.pop().ok_or(Fault::Stack)?;
                    let truth = match value {
                        Value::Int(value) => value != 0,
                        Value::Null => false,
                        Value::Reference(_)
                        | Value::ArrayAddress(..)
                        | Value::Sensor
                        | Value::HostString(_)
                        | Value::StringToken(_) => true,
                        _ => return Err(Fault::Stack),
                    };
                    if truth == expected {
                        offset = offset
                            .checked_add_signed(delta as isize)
                            .ok_or(Fault::InvalidBranch)?;
                    }
                }
                Instruction::Compare(op) => {
                    let right = stack.pop().ok_or(Fault::Stack)?;
                    let left = stack.pop().ok_or(Fault::Stack)?;
                    let float = |v| match v {
                        Value::Float(bits) => Some(f64::from(f32::from_bits(bits))),
                        Value::Double(bits) => Some(f64::from_bits(bits)),
                        _ => None,
                    };
                    let result = match (left, right) {
                        (Value::Int(a), Value::Int(b)) => match op {
                            1 => a == b,
                            2 => a > b,
                            3 => (a as u32) > (b as u32),
                            4 => a < b,
                            _ => (a as u32) < (b as u32),
                        },
                        _ if float(left).zip(float(right)).is_some() => {
                            let (a, b) = float(left).zip(float(right)).unwrap();
                            match op {
                                1 => a == b,
                                2 => a > b,
                                3 => a > b || a.is_nan() || b.is_nan(),
                                4 => a < b,
                                _ => a < b || a.is_nan() || b.is_nan(),
                            }
                        }
                        _ if op == 1 => left == right,
                        _ if op == 3 && right == Value::Null => left != Value::Null,
                        _ => return Err(Fault::Stack),
                    };
                    stack.push(Value::Int(i32::from(result)));
                }
                Instruction::BranchCompare(delta, operation) => {
                    let right = stack.pop().ok_or(Fault::Stack)?;
                    let left = stack.pop().ok_or(Fault::Stack)?;
                    let float = |value| match value {
                        Value::Float(bits) => Some(f64::from(f32::from_bits(bits))),
                        Value::Double(bits) => Some(f64::from_bits(bits)),
                        _ => None,
                    };
                    let jump = match (left, right) {
                        (Value::Int(left), Value::Int(right)) => match operation {
                            0 => left == right,
                            1 => left >= right,
                            2 => left > right,
                            3 => left <= right,
                            4 => left < right,
                            5 => left != right,
                            6 => (left as u32) >= (right as u32),
                            7 => (left as u32) > (right as u32),
                            8 => (left as u32) <= (right as u32),
                            9 => (left as u32) < (right as u32),
                            _ => return Err(Fault::InvalidBranch),
                        },
                        _ => {
                            let (left, right) =
                                float(left).zip(float(right)).ok_or(Fault::Stack)?;
                            let unordered = left.is_nan() || right.is_nan();
                            match operation {
                                0 => left == right,
                                1 => left >= right,
                                2 => left > right,
                                3 => left <= right,
                                4 => left < right,
                                5 => left != right,
                                6 => unordered || left >= right,
                                7 => unordered || left > right,
                                8 => unordered || left <= right,
                                9 => unordered || left < right,
                                _ => return Err(Fault::InvalidBranch),
                            }
                        }
                    };
                    if jump {
                        offset = offset
                            .checked_add_signed(delta as isize)
                            .ok_or(Fault::InvalidBranch)?;
                    }
                }
                Instruction::ArrayLength => {
                    let array = stack.pop().ok_or(Fault::Stack)?;
                    let length = self.array(array).ok_or(Fault::Reference)?.len();
                    stack.push(Value::Int(
                        i32::try_from(length).map_err(|_| Fault::ArrayLength)?,
                    ));
                }
                Instruction::LoadElement => {
                    let Some(Value::Int(index)) = stack.pop() else {
                        return Err(Fault::Stack);
                    };
                    let array = stack.pop().ok_or(Fault::Stack)?;
                    let value = self
                        .array(array)
                        .ok_or(Fault::Reference)?
                        .get(index as usize)
                        .ok_or(Fault::ArrayLength)?;
                    stack.push(*value);
                }
                Instruction::LoadElementAddress => {
                    let Some(Value::Int(index)) = stack.pop() else {
                        return Err(Fault::Stack);
                    };
                    let Some(Value::Reference(array)) = stack.pop() else {
                        return Err(Fault::Reference);
                    };
                    self.array(Value::Reference(array))
                        .and_then(|items| items.get(index as usize))
                        .ok_or(Fault::ArrayLength)?;
                    stack.push(Value::ArrayAddress(array, index as usize));
                }
                Instruction::LoadObject => {
                    let value = match stack.pop().ok_or(Fault::Stack)? {
                        Value::LocalAddress(frame, index) => self
                            .locals
                            .get(frame)
                            .and_then(|locals| locals.get(usize::from(index))),
                        Value::ArrayAddress(array, index) => self
                            .array(Value::Reference(array))
                            .and_then(|items| items.get(index)),
                        _ => None,
                    }
                    .ok_or(Fault::Reference)?;
                    stack.push(*value);
                }
                Instruction::Throw => return Err(Fault::ManagedException),
                Instruction::FieldAddress(field) => {
                    let field = environment.resolve_field(field);
                    let Some(Value::Reference(object)) = stack.pop() else {
                        return Err(Fault::Reference);
                    };
                    self.fields(Value::Reference(object))?;
                    stack.push(Value::FieldAddress(object, field));
                }
                Instruction::StoreIndirectFloat | Instruction::StoreObject => {
                    let value = stack.pop().ok_or(Fault::Stack)?;
                    let address = stack.pop().ok_or(Fault::Stack)?;
                    if instruction == Instruction::StoreIndirectFloat
                        && !matches!(value, Value::Float(_))
                    {
                        return Err(Fault::Stack);
                    }
                    self.write_address(address, value)?;
                }
                Instruction::Negate => {
                    let value = match stack.pop().ok_or(Fault::Stack)? {
                        Value::Int(value) => Value::Int(value.wrapping_neg()),
                        Value::Float(value) => Value::Float(value ^ (1 << 31)),
                        _ => return Err(Fault::Stack),
                    };
                    stack.push(value);
                }
                Instruction::ConvertDouble => {
                    let value = match stack.pop().ok_or(Fault::Stack)? {
                        Value::Int(value) => f64::from(value).to_bits(),
                        Value::Float(value) => f64::from(f32::from_bits(value)).to_bits(),
                        Value::Double(value) => value,
                        _ => return Err(Fault::Stack),
                    };
                    stack.push(Value::Double(value));
                }
                Instruction::ConvertFloat => {
                    let value = match stack.pop().ok_or(Fault::Stack)? {
                        Value::Int(value) => (value as f32).to_bits(),
                        Value::Float(value) => value,
                        Value::Double(value) => (f64::from_bits(value) as f32).to_bits(),
                        _ => return Err(Fault::Stack),
                    };
                    stack.push(Value::Float(value));
                }
                Instruction::Arithmetic(operation) => {
                    let right = stack.pop().ok_or(Fault::Stack)?;
                    let left = stack.pop().ok_or(Fault::Stack)?;
                    let result = match (left, right) {
                        (Value::Int(l), Value::Int(r)) => Value::Int(match operation {
                            0x58 => l.wrapping_add(r),
                            0x59 => l.wrapping_sub(r),
                            0x5a => l.wrapping_mul(r),
                            0x5b => l.checked_div(r).ok_or(Fault::ManagedException)?,
                            0x5c => (l as u32)
                                .checked_div(r as u32)
                                .ok_or(Fault::ManagedException)?
                                as i32,
                            0x5d => l.checked_rem(r).ok_or(Fault::ManagedException)?,
                            0xd6 => l.checked_add(r).ok_or(Fault::ManagedException)?,
                            0xd8 => l.checked_mul(r).ok_or(Fault::ManagedException)?,
                            0xda => l.checked_sub(r).ok_or(Fault::ManagedException)?,
                            0x5f => l & r,
                            0x60 => l | r,
                            0x61 => l ^ r,
                            0x5e => (l as u32)
                                .checked_rem(r as u32)
                                .ok_or(Fault::ManagedException)?
                                as i32,
                            _ => return Err(Fault::Stack),
                        }),
                        (Value::Float(l), Value::Float(r)) => {
                            let (l, r) = (f32::from_bits(l), f32::from_bits(r));
                            Value::Float(
                                match operation {
                                    0x58 => l + r,
                                    0x59 => l - r,
                                    0x5a => l * r,
                                    0x5b => l / r,
                                    0x5d => l % r,
                                    _ => return Err(Fault::Stack),
                                }
                                .to_bits(),
                            )
                        }
                        (Value::Double(l), Value::Double(r)) => {
                            let (l, r) = (f64::from_bits(l), f64::from_bits(r));
                            Value::Double(
                                match operation {
                                    0x58 => l + r,
                                    0x59 => l - r,
                                    0x5a => l * r,
                                    0x5b => l / r,
                                    0x5d => l % r,
                                    _ => return Err(Fault::Stack),
                                }
                                .to_bits(),
                            )
                        }
                        _ => return Err(Fault::Stack),
                    };
                    stack.push(result);
                }
                Instruction::ConvertInt => {
                    let value = match stack.pop().ok_or(Fault::Stack)? {
                        Value::Int(value) => value,
                        Value::ArrayAddress(array, index) => {
                            stack.push(Value::ArrayAddress(array, index));
                            continue;
                        }
                        Value::Float(value) => f32::from_bits(value) as i32,
                        Value::Double(value) => f64::from_bits(value) as i32,
                        _ => return Err(Fault::Stack),
                    };
                    stack.push(Value::Int(value));
                }
                Instruction::LoadArgument(index) => {
                    stack.push(
                        *self.arguments[frame]
                            .get(usize::from(index))
                            .ok_or(Fault::Argument)?,
                    );
                }
                Instruction::StoreArgument(index) => {
                    let value = stack.pop().ok_or(Fault::Stack)?;
                    self.write_address(Value::ArgumentAddress(frame, index), value)?;
                }
                Instruction::ArgumentAddress(index) => {
                    self.arguments[frame]
                        .get(usize::from(index))
                        .ok_or(Fault::Argument)?;
                    stack.push(Value::ArgumentAddress(frame, index));
                }
                Instruction::LoadDouble(value) => stack.push(Value::Double(value)),
                Instruction::LoadFloat(value) => stack.push(Value::Float(value)),
                Instruction::LoadInt(value) => stack.push(Value::Int(value)),
                Instruction::LoadNull => stack.push(Value::Null),
                Instruction::LoadString(token) => stack.push(Value::StringToken(token)),
                Instruction::LoadFunction(token) => stack.push(Value::Function(token)),
                Instruction::LoadVirtualFunction(token) => {
                    if !matches!(stack.pop(), Some(Value::Reference(_))) {
                        return Err(Fault::Reference);
                    }
                    stack.push(Value::Function(token));
                }
                // The interpreter already represents boxed primitives as tagged values.
                Instruction::UnboxAny(ty) => {
                    let value = *stack.last().ok_or(Fault::Stack)?;
                    if !environment.unbox_any(self, token, ty, value) {
                        return Err(Fault::ManagedException);
                    }
                }
                Instruction::ConvertChecked(op) => {
                    let value = match stack.pop().ok_or(Fault::Stack)? {
                        Value::Int(value) if op == 0x86 => f64::from(value as u32),
                        Value::Int(value) => f64::from(value),
                        Value::Float(bits) => f64::from(f32::from_bits(bits)).trunc(),
                        Value::Double(bits) => f64::from_bits(bits).trunc(),
                        _ => return Err(Fault::Stack),
                    };
                    let (min, max) = match op {
                        0x86 | 0xb4 => (0.0, f64::from(u8::MAX)),
                        0xb5 => (f64::from(i16::MIN), f64::from(i16::MAX)),
                        0xb7 => (f64::from(i32::MIN), f64::from(i32::MAX)),
                        _ => return Err(Fault::Stack),
                    };
                    if !(min..=max).contains(&value) {
                        return Err(Fault::ManagedException);
                    }
                    stack.push(Value::Int(value as i32));
                }
                Instruction::BoxValue => {
                    stack.last().ok_or(Fault::Stack)?;
                }
                Instruction::LoadToken(operand) => stack.push(Value::MetadataToken(
                    environment.resolve_type(token, operand),
                )),
                Instruction::LoadField(field) => {
                    let field = environment.resolve_field(field);
                    let object = stack.pop().ok_or(Fault::Stack)?;
                    let object = self.read_address(object).unwrap_or(object);
                    let value = if field >> 24 == 4 {
                        self.fields(object)?
                            .get(&field)
                            .copied()
                            .unwrap_or_else(|| environment.field_default(field))
                    } else {
                        environment
                            .value_field(field, object)
                            .ok_or(Fault::Reference)?
                    };
                    stack.push(value);
                }
                Instruction::LoadStaticField(field) => {
                    let field = environment.resolve_field(field);
                    if !self.statics.contains_key(&field) {
                        if let Some(initializer) = environment.static_initializer(field) {
                            if self.initialized.insert(initializer) {
                                self.invoke(environment, initializer, &[], depth + 1)?;
                            }
                        }
                    }
                    let value = match self.statics.get(&field) {
                        Some(value) => *value,
                        None if field >> 24 == 4 => environment.field_default(field),
                        None => environment.static_field(field)?,
                    };
                    stack.push(value);
                }
                Instruction::StoreStaticField(field) => {
                    let field = environment.resolve_field(field);
                    let value = stack.pop().ok_or(Fault::Stack)?;
                    self.statics.insert(field, value);
                }
                Instruction::StoreField(field) => {
                    let field = environment.resolve_field(field);
                    let value = stack.pop().ok_or(Fault::Stack)?;
                    let object = stack.pop().ok_or(Fault::Stack)?;
                    if field >> 24 == 4 {
                        self.fields(object)?.insert(field, value);
                    } else {
                        let current = self.read_address(object).ok_or(Fault::Reference)?;
                        let updated = environment
                            .store_value_field(field, current, value)
                            .ok_or(Fault::Reference)?;
                        self.write_address(object, updated)?;
                    }
                }
                Instruction::Duplicate => {
                    stack.push(*stack.last().ok_or(Fault::Stack)?);
                }
                Instruction::Pop => {
                    stack.pop().ok_or(Fault::Stack)?;
                }
                Instruction::StoreElement => {
                    let value = stack.pop().ok_or(Fault::Stack)?;
                    let Some(Value::Int(index)) = stack.pop() else {
                        return Err(Fault::Stack);
                    };
                    let Some(Value::Reference(array)) = stack.pop() else {
                        return Err(Fault::Reference);
                    };
                    let Some(Object::Array(elements)) =
                        self.objects.get_mut(array).and_then(Option::as_mut)
                    else {
                        return Err(Fault::Reference);
                    };
                    *elements.get_mut(index as usize).ok_or(Fault::ArrayLength)? = value;
                }
                Instruction::NewArray(_) => {
                    let Some(Value::Int(length)) = stack.pop() else {
                        return Err(Fault::Stack);
                    };
                    let length = usize::try_from(length).map_err(|_| Fault::ArrayLength)?;
                    self.remaining = self.remaining.checked_sub(length).ok_or(Fault::Budget)?;
                    let mut elements = Vec::new();
                    elements
                        .try_reserve_exact(length)
                        .map_err(|_| Fault::Budget)?;
                    elements.resize(length, Value::Null);
                    stack.push(self.allocate(Object::Array(elements))?);
                }
                Instruction::Call(target)
                | Instruction::CallVirtual(target)
                | Instruction::NewObject(target) => {
                    let method = environment
                        .signature(target)
                        .ok_or(Fault::MissingMethod(target))?;
                    let constructor = matches!(instruction, Instruction::NewObject(_));
                    if constructor && (!method.instance || method.returns_value) {
                        return Err(Fault::InvalidSignature);
                    }
                    let count = method
                        .parameters
                        .checked_add(usize::from(method.instance && !constructor))
                        .ok_or(Fault::InvalidSignature)?;
                    let start = stack.len().checked_sub(count).ok_or(Fault::Stack)?;
                    let mut operands = stack.split_off(start);
                    if constructor {
                        if let Some(value) = environment.construct_value(target, &operands) {
                            stack.push(value);
                            continue;
                        }
                    }
                    let object = if constructor {
                        let object = self.allocate(Object::Instance {
                            constructor: target,
                            fields: BTreeMap::new(),
                        })?;
                        operands.insert(0, object);
                        Some(object)
                    } else {
                        None
                    };
                    if matches!(instruction, Instruction::CallVirtual(_))
                        && operands.first() == Some(&Value::Null)
                    {
                        return Err(Fault::Reference);
                    }
                    match self.invoke(environment, target, &operands, depth + 1) {
                        Ok(result) => {
                            if let Some(value) = result.or(object) {
                                stack.push(value);
                            }
                        }
                        Err(error) => {
                            let handler = clauses
                                .iter()
                                .filter(|clause| {
                                    clause.flags == 0
                                        && (clause.try_start..clause.try_end)
                                            .contains(&instruction_offset)
                                        && environment.catches(clause.exception, &error)
                                })
                                .min_by_key(|clause| clause.try_end - clause.try_start);
                            let Some(handler) = handler else {
                                return Err(error);
                            };
                            stack.clear();
                            stack.push(self.allocate(Object::Instance {
                                constructor: 0,
                                fields: BTreeMap::new(),
                            })?);
                            offset = handler.handler_start;
                        }
                    }
                }
                Instruction::Return => {
                    if stack.len() != usize::from(returns_value) {
                        return Err(Fault::Stack);
                    }
                    return Ok(stack.pop());
                }
            }
        }
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;

    #[test]
    fn collector_follows_host_edges_and_reuses_detached_slots() {
        let mut machine = Machine::new(100);
        let mut allocate = || machine.allocate(Object::Array(Vec::new())).unwrap();
        let root = allocate();
        let child = allocate();
        let detached = allocate();
        machine.collect([root], |value, pending| {
            if value == root {
                pending.push(child);
            }
        });
        assert!(machine.array(child).is_some());
        assert!(machine.array(detached).is_none());
        assert_eq!(
            machine.allocate(Object::Array(Vec::new())).unwrap(),
            detached
        );
        machine.collect([root], |_, _| {});
        assert!(machine.array(child).is_none());
    }
}
