//! Decode instructions needed by the managed startup path.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instruction {
    LoadArgument(u8),
    StoreArgument(u8),
    ArgumentAddress(u8),
    LoadLocal(u8),
    StoreLocal(u8),
    LocalAddress(u8),
    Branch(i32),
    Leave(i32),
    BranchCompare(i32, u8),
    ArrayLength,
    LoadElement,
    LoadElementAddress,
    LoadObject,
    CastClass(u32),
    EndFinally,
    Throw,
    ConvertInt,
    BranchIf(i32, bool),
    LoadInt(i32),
    LoadFloat(u32),
    LoadDouble(u64),
    ConvertDouble,
    LoadNull,
    LoadString(u32),
    LoadFunction(u32),
    LoadToken(u32),
    Duplicate,
    Pop,
    StoreElement,
    Call(u32),
    CallVirtual(u32),
    NewObject(u32),
    NewArray(u32),
    LoadField(u32),
    FieldAddress(u32),
    StoreIndirectFloat,
    StoreObject,
    ConvertFloat,
    Negate,
    Arithmetic(u8),
    LoadStaticField(u32),
    StoreStaticField(u32),
    StoreField(u32),
    Return,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated { offset: usize },
    Unsupported { offset: usize, opcode: u8 },
}

pub fn decode(code: &[u8], offset: usize) -> Result<(Instruction, usize), DecodeError> {
    let truncated = DecodeError::Truncated { offset };
    let bytes = code.get(offset..).ok_or(truncated)?;
    let (&opcode, operand) = bytes.split_first().ok_or(truncated)?;
    let token = || {
        Ok(u32::from_le_bytes(
            operand
                .get(..4)
                .ok_or(truncated)?
                .try_into()
                .map_err(|_| truncated)?,
        ))
    };
    use Instruction::*;
    let (instruction, size) = match opcode {
        0x02..=0x05 => (LoadArgument(opcode - 2), 1),
        0x10 => (StoreArgument(*operand.first().ok_or(truncated)?), 2),
        0x0f => (ArgumentAddress(*operand.first().ok_or(truncated)?), 2),
        0x0e => (LoadArgument(*operand.first().ok_or(truncated)?), 2),
        0x06..=0x09 => (LoadLocal(opcode - 6), 1),
        0x0a..=0x0d => (StoreLocal(opcode - 0x0a), 1),
        0x11 => (LoadLocal(*operand.first().ok_or(truncated)?), 2),
        0x12 => (LocalAddress(*operand.first().ok_or(truncated)?), 2),
        0x13 => (StoreLocal(*operand.first().ok_or(truncated)?), 2),
        0x2b => (
            Branch(i32::from(*operand.first().ok_or(truncated)? as i8)),
            2,
        ),
        0x2c | 0x2d => (
            BranchIf(
                i32::from(*operand.first().ok_or(truncated)? as i8),
                opcode == 0x2d,
            ),
            2,
        ),
        0x2e..=0x37 => (
            BranchCompare(
                i32::from(*operand.first().ok_or(truncated)? as i8),
                opcode - 0x2e,
            ),
            2,
        ),
        0x3b..=0x44 => (BranchCompare(token()? as i32, opcode - 0x3b), 5),
        0x58..=0x5e => (Arithmetic(opcode), 1),
        0x56 => (StoreIndirectFloat, 1),
        0x65 => (Negate, 1),
        0x6b => (ConvertFloat, 1),
        0x7c => (FieldAddress(token()?), 5),
        0x69 | 0xd3 | 0xe0 => (ConvertInt, 1),
        0x8e => (ArrayLength, 1),
        0x8f => (LoadElementAddress, {
            token()?;
            5
        }),
        0x81 => (StoreObject, {
            token()?;
            5
        }),
        0x71 => (LoadObject, {
            token()?;
            5
        }),
        0x74 => (CastClass(token()?), 5),
        0xdc => (EndFinally, 1),
        0x7a => (Throw, 1),
        0x90..=0x9a => (LoadElement, 1),
        0xde => (
            Leave(i32::from(*operand.first().ok_or(truncated)? as i8)),
            2,
        ),
        0xdd => (Leave(token()? as i32), 5),
        0x38 => (Branch(token()? as i32), 5),
        0x39 | 0x3a => (BranchIf(token()? as i32, opcode == 0x3a), 5),
        0x14 => (LoadNull, 1),
        0x15..=0x1e => (LoadInt(i32::from(opcode) - 0x16), 1),
        0x1f => (
            LoadInt(i32::from(*operand.first().ok_or(truncated)? as i8)),
            2,
        ),
        0x23 => (
            LoadDouble(u64::from_le_bytes(
                operand
                    .get(..8)
                    .ok_or(truncated)?
                    .try_into()
                    .map_err(|_| truncated)?,
            )),
            9,
        ),
        0x6c => (ConvertDouble, 1),
        0x22 => (LoadFloat(token()?), 5),
        0x20 => (LoadInt(token()? as i32), 5),
        0xd0 => (LoadToken(token()?), 5),
        0x72 => (LoadString(token()?), 5),
        0x25 => (Duplicate, 1),
        0x26 => (Pop, 1),
        0x9b..=0xa2 => (StoreElement, 1),
        0x28 => (Call(token()?), 5),
        0x2a => (Return, 1),
        0x6f => (CallVirtual(token()?), 5),
        0x73 => (NewObject(token()?), 5),
        0x7b => (LoadField(token()?), 5),
        0x7e => (LoadStaticField(token()?), 5),
        0x80 => (StoreStaticField(token()?), 5),
        0x7d => (StoreField(token()?), 5),
        0x8d => (NewArray(token()?), 5),
        0xfe if operand.first() == Some(&6) => (
            LoadFunction(u32::from_le_bytes(
                operand
                    .get(1..5)
                    .ok_or(truncated)?
                    .try_into()
                    .map_err(|_| truncated)?,
            )),
            6,
        ),
        _ => return Err(DecodeError::Unsupported { offset, opcode }),
    };
    Ok((instruction, offset + size))
}
