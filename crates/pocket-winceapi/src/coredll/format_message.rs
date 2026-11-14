//! CE's RDP Display page uses allocated FormatMessageW strings for resolution
//! labels. The result belongs to LocalFree, not to the resource or argument list.
use super::*;
use pocket_cpu::CpuError;

const ALLOCATE: u32 = 0x100;
const IGNORE_INSERTS: u32 = 0x200;
const FROM_STRING: u32 = 0x400;
const FROM_MODULE: u32 = 0x800;
const FROM_SYSTEM: u32 = 0x1000;
const ARGUMENT_ARRAY: u32 = 0x2000;
const MAX_CHARS: usize = 32768;

enum Failure {
    Api(u32),
    Cpu(CpuError),
}
type FormatResult<T> = std::result::Result<T, Failure>;

pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "FormatMessageW", wide);
    d.register_handler("coredll.dll", "FormatMessageA", ansi);
}

fn memory_error(error: CpuError) -> Failure {
    match error {
        error @ CpuError::ImageOutOfMemory { .. } => Failure::Cpu(error),
        _ => Failure::Api(87),
    }
}

fn read(ctx: &mut CallCtx<'_>, address: u32, size: u32) -> FormatResult<Vec<u8>> {
    if address == 0 || address.checked_add(size).is_none() {
        return Err(Failure::Api(87));
    }
    ctx.cpu
        .check_guest_access(address, size, Prot::READ)
        .map_err(memory_error)?;
    ctx.cpu.read_mem(address, size).map_err(memory_error)
}

fn read_text(ctx: &mut CallCtx<'_>, pointer: u32, wide: bool) -> FormatResult<String> {
    let stride = if wide { 2 } else { 1 };
    let mut bytes = Vec::new();
    for index in 0..MAX_CHARS as u32 {
        let address = pointer
            .checked_add(index * stride)
            .ok_or(Failure::Api(87))?;
        let unit = read(ctx, address, stride)?;
        if unit.iter().all(|&byte| byte == 0) {
            return Ok(if wide {
                String::from_utf16_lossy(
                    &bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect::<Vec<_>>(),
                )
            } else {
                crate::crt_ansi::decode(&bytes)
            });
        }
        bytes.extend(unit);
    }
    Err(Failure::Api(87))
}

fn table_message(bytes: &[u8], id: u32) -> FormatResult<Option<String>> {
    let word = |offset: usize| -> FormatResult<u32> {
        Ok(u32::from_le_bytes(
            bytes
                .get(offset..offset + 4)
                .ok_or(Failure::Api(13))?
                .try_into()
                .unwrap(),
        ))
    };
    let count = word(0)? as usize;
    if count > 4096
        || count
            .checked_mul(12)
            .and_then(|n| n.checked_add(4))
            .is_none_or(|n| n > bytes.len())
    {
        return Err(Failure::Api(13));
    }
    for block in 0..count {
        let offset = 4 + block * 12;
        let (low, high) = (word(offset)?, word(offset + 4)?);
        if id < low || id > high {
            continue;
        }
        let mut entry = word(offset + 8)? as usize;
        for current in low..=id {
            let header = bytes.get(entry..entry + 4).ok_or(Failure::Api(13))?;
            let length = u16::from_le_bytes([header[0], header[1]]) as usize;
            let flags = u16::from_le_bytes([header[2], header[3]]);
            if length < 4 || flags & !1 != 0 {
                return Err(Failure::Api(13));
            }
            let end = entry.checked_add(length).ok_or(Failure::Api(13))?;
            let record = bytes.get(entry + 4..end).ok_or(Failure::Api(13))?;
            if current == id {
                return Ok(Some(if flags & 1 != 0 {
                    if record.len() % 2 != 0 {
                        return Err(Failure::Api(13));
                    }
                    let units: Vec<_> = record
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .take_while(|&unit| unit != 0)
                        .collect();
                    String::from_utf16_lossy(&units)
                } else {
                    crate::crt_ansi::decode(record.split(|&b| b == 0).next().unwrap_or_default())
                }));
            }
            entry = end;
        }
    }
    Ok(None)
}

fn module_message(ctx: &mut CallCtx<'_>, module: u32, id: u32) -> FormatResult<Option<String>> {
    let (base, resources) =
        if module == 0 || module == FAKE_MODULE_HANDLE || module == ctx.kernel.image_base {
            (ctx.kernel.image_base, ctx.kernel.resources.clone())
        } else {
            let loaded = ctx
                .kernel
                .modules
                .iter()
                .find(|m| m.base == module)
                .ok_or(Failure::Api(6))?;
            (loaded.base, loaded.resources.clone())
        };
    for resource in resources.iter().filter(|r| r.ty == ResourceKey::Id(11)) {
        if resource.size > 1024 * 1024 {
            return Err(Failure::Api(13));
        }
        let address = base
            .checked_add(resource.data_rva)
            .ok_or(Failure::Api(13))?;
        if let Some(message) = table_message(&read(ctx, address, resource.size)?, id)? {
            return Ok(Some(message));
        }
    }
    Ok(None)
}

fn system_message(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "The operation completed successfully.\r\n",
        2 => "The system cannot find the file specified.\r\n",
        3 => "The system cannot find the path specified.\r\n",
        5 => "Access is denied.\r\n",
        6 => "The handle is invalid.\r\n",
        8 | 14 => "Not enough memory is available to complete this operation.\r\n",
        17 => "The system cannot move the file to a different volume.\r\n",
        29 => "The system cannot write to the specified device.\r\n",
        32 => "The file is being used by another process.\r\n",
        50 => "The request is not supported.\r\n",
        87 => "The parameter is incorrect.\r\n",
        112 => "There is not enough space on the disk.\r\n",
        122 => "The data area passed to a system call is too small.\r\n",
        123 => "The filename, directory name, or volume label syntax is incorrect.\r\n",
        183 => "Cannot create a file when that file already exists.\r\n",
        267 => "The directory name is invalid.\r\n",
        995 => "The I/O operation has been aborted.\r\n",
        997 => "Overlapped I/O operation is in progress.\r\n",
        _ => return None,
    })
}

fn insert(
    ctx: &mut CallCtx<'_>,
    arguments: u32,
    array: bool,
    index: u32,
    spec: &str,
    wide: bool,
) -> FormatResult<String> {
    let conversion = spec.chars().last().ok_or(Failure::Api(87))?;
    if !matches!(
        conversion,
        'd' | 'i' | 'u' | 'x' | 'X' | 'c' | 'C' | 's' | 'S' | 'p'
    ) || spec[..spec.len() - conversion.len_utf8()]
        .chars()
        .any(|c| !c.is_ascii_digit() && !matches!(c, '-' | '.' | 'l' | 'h' | 'w' | 'I'))
    {
        return Err(Failure::Api(87));
    }
    let base = if array {
        arguments
    } else {
        u32::from_le_bytes(read(ctx, arguments, 4)?.try_into().unwrap())
    };
    let address = base.checked_add((index - 1) * 4).ok_or(Failure::Api(87))?;
    let integer64 = spec.contains("I64") || spec.contains("ll");
    let value = read(ctx, address, if integer64 { 8 } else { 4 })?;
    if matches!(conversion, 's' | 'S') {
        let pointer = u32::from_le_bytes(value[..4].try_into().unwrap());
        let string_wide = if spec.contains('l') || spec.contains('w') {
            true
        } else if spec.contains('h') {
            false
        } else if conversion == 'S' {
            !wide
        } else {
            wide
        };
        if pointer != 0 {
            read_text(ctx, pointer, string_wide)?;
        }
    }
    match render_printf_va(ctx, &format!("%{spec}"), wide, address) {
        Ok(text) => Ok(text),
        Err(KernelError::Cpu(error)) => Err(memory_error(error)),
        Err(_) => Err(Failure::Api(87)),
    }
}

fn render(
    ctx: &mut CallCtx<'_>,
    template: &str,
    flags: u32,
    arguments: u32,
    wide: bool,
) -> FormatResult<String> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '%' {
            if flags & 0xff != 0 && matches!(character, '\r' | '\n') {
                if character == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(' ');
            } else {
                out.push(character);
            }
            continue;
        }
        let next = chars.next().ok_or(Failure::Api(87))?;
        match next {
            '0' => break,
            '%' => out.push('%'),
            'n' => out.push_str("\r\n"),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push(' '),
            '1'..='9' => {
                let mut index = next as u32 - '0' as u32;
                if chars.peek().is_some_and(char::is_ascii_digit) {
                    index = index * 10 + chars.next().unwrap() as u32 - '0' as u32;
                }
                let mut spec = None;
                if chars.peek() == Some(&'!') {
                    chars.next();
                    let mut value = String::new();
                    loop {
                        match chars.next() {
                            Some('!') => break,
                            Some(c) if value.len() < 64 => value.push(c),
                            _ => return Err(Failure::Api(87)),
                        }
                    }
                    spec = Some(value);
                }
                if flags & IGNORE_INSERTS != 0 {
                    out.push_str(&format!("%{index}"));
                    if let Some(spec) = spec {
                        out.push_str(&format!("!{spec}!"));
                    }
                } else {
                    out.push_str(&insert(
                        ctx,
                        arguments,
                        flags & ARGUMENT_ARRAY != 0,
                        index,
                        spec.as_deref().unwrap_or("s"),
                        wide,
                    )?);
                }
            }
            _ => return Err(Failure::Api(87)),
        }
        if out.encode_utf16().count() >= MAX_CHARS {
            return Err(Failure::Api(122));
        }
    }
    let width = flags & 0xff;
    if width != 0 && width != 0xff {
        let mut wrapped = String::new();
        for (index, line) in out.split("\r\n").enumerate() {
            if index != 0 {
                wrapped.push_str("\r\n");
            }
            let mut column = 0;
            for word in line.split_whitespace() {
                let length = word.encode_utf16().count() as u32;
                if column != 0 {
                    if column + length + 1 > width {
                        wrapped.push_str("\r\n");
                        column = 0;
                    } else {
                        wrapped.push(' ');
                        column += 1;
                    }
                }
                wrapped.push_str(word);
                column += length;
            }
        }
        out = wrapped;
    }
    Ok(out)
}

fn wide(ctx: &mut CallCtx<'_>) -> std::result::Result<DispatchOutcome, KernelError> {
    format_message(ctx, true)
}
fn ansi(ctx: &mut CallCtx<'_>) -> std::result::Result<DispatchOutcome, KernelError> {
    format_message(ctx, false)
}

fn format_message(
    ctx: &mut CallCtx<'_>,
    wide: bool,
) -> std::result::Result<DispatchOutcome, KernelError> {
    let (flags, source, id, _language, output, capacity, arguments) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
        ctx.arg_u32(5)?,
        ctx.arg_u32(6)?,
    );
    let result = (|| -> FormatResult<u32> {
        if flags & !0x3fff != 0
            || flags & (FROM_STRING | FROM_MODULE | FROM_SYSTEM) == 0
            || flags & FROM_STRING != 0 && flags & (FROM_MODULE | FROM_SYSTEM) != 0
            || output == 0
        {
            return Err(Failure::Api(87));
        }
        let template = if flags & FROM_STRING != 0 {
            read_text(ctx, source, wide)?
        } else {
            let from_module = if flags & FROM_MODULE != 0 {
                module_message(ctx, source, id)?
            } else {
                None
            };
            from_module
                .or_else(|| {
                    if flags & FROM_SYSTEM != 0 {
                        system_message(id).map(str::to_string)
                    } else {
                        None
                    }
                })
                .ok_or(Failure::Api(317))?
        };
        let text = render(ctx, &template, flags, arguments, wide)?;
        let mut bytes = if wide {
            text.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            crate::crt_ansi::encode(&text)
        };
        let stride = if wide { 2 } else { 1 };
        let length = bytes.len() as u32 / stride;
        if length as usize >= MAX_CHARS {
            return Err(Failure::Api(122));
        }
        bytes.extend(std::iter::repeat_n(0, stride as usize));
        if flags & ALLOCATE != 0 {
            ctx.cpu
                .check_guest_access(output, 4, Prot::WRITE)
                .map_err(memory_error)?;
            let size = capacity
                .max(length + 1)
                .checked_mul(stride)
                .filter(|&size| size <= 1024 * 1024)
                .ok_or(Failure::Api(87))?;
            let allocated = ctx.kernel.heap.alloc(size).ok_or(Failure::Api(8))?;
            let written = ctx
                .cpu
                .check_guest_access(allocated, bytes.len() as u32, Prot::WRITE)
                .and_then(|()| ctx.cpu.write_mem(allocated, &bytes))
                .and_then(|()| ctx.cpu.write_mem(output, &allocated.to_le_bytes()));
            if let Err(error) = written {
                ctx.kernel.heap.free(allocated);
                return Err(memory_error(error));
            }
        } else {
            if capacity <= length {
                return Err(Failure::Api(122));
            }
            ctx.cpu
                .check_guest_access(output, bytes.len() as u32, Prot::WRITE)
                .map_err(memory_error)?;
            ctx.cpu.write_mem(output, &bytes).map_err(memory_error)?;
        }
        Ok(length)
    })();
    match result {
        Ok(length) => Ok(DispatchOutcome::ReturnedR0(length)),
        Err(Failure::Api(error)) => {
            set_thread_error(ctx, error);
            Ok(DispatchOutcome::ReturnedR0(0))
        }
        Err(Failure::Cpu(error)) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{dummy_thunk, fresh_kernel, scanf_cpu};
    use super::*;
    use pocket_cpu::{stub::StubCpu, Cpu};
    use pocket_kernel::Dispatcher;
    use pocket_pe::ImportBinding;

    fn setup() -> (StubCpu, KernelState, WinCeDispatcher) {
        let mut cpu = scanf_cpu();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        (cpu, fresh_kernel(), WinCeDispatcher::new())
    }

    fn put(cpu: &mut StubCpu, address: u32, text: &str) {
        cpu.write_mem(
            address,
            &text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }

    fn get(cpu: &mut StubCpu, address: u32) -> String {
        let mut units = Vec::new();
        loop {
            let unit = u16::from_le_bytes(
                cpu.read_mem(address + units.len() as u32 * 2, 2)
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        String::from_utf16(&units).unwrap()
    }

    fn call(
        cpu: &mut StubCpu,
        kernel: &mut KernelState,
        dispatcher: &mut WinCeDispatcher,
        name: &str,
        args: [u32; 7],
    ) -> u32 {
        for (index, value) in args.into_iter().enumerate() {
            if index < 4 {
                cpu.write_reg(
                    [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3][index],
                    value,
                )
                .unwrap();
            } else {
                cpu.write_mem(0x9000 + (index as u32 - 4) * 4, &value.to_le_bytes())
                    .unwrap();
            }
        }
        let mut thunk = dummy_thunk();
        thunk.dll = "coredll.dll".into();
        thunk.binding = ImportBinding::Name(name.into());
        thunk.friendly_name = Some(name.into());
        let DispatchOutcome::ReturnedR0(value) = dispatcher.dispatch(cpu, &thunk, kernel).unwrap()
        else {
            panic!();
        };
        value
    }

    #[test]
    fn rdp_resolution_labels_use_a_pointer_to_va_list_and_localfree_owned_storage() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        put(&mut cpu, 0x1100, "%1!ld! by %2!ld! pixels");
        cpu.write_mem(0x2100, &0x2200u32.to_le_bytes()).unwrap();
        cpu.write_mem(
            0x2200,
            &[640u32.to_le_bytes(), 480u32.to_le_bytes()].concat(),
        )
        .unwrap();
        let free = kernel.heap.free_bytes();
        kernel.thread_last_errors.insert(0, 1234);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [ALLOCATE | FROM_STRING, 0x1100, 0, 0, 0x2000, 512, 0x2100]
            ),
            17
        );
        let pointer = cpu.read_u32_le(0x2000).unwrap();
        assert_eq!(get(&mut cpu, pointer), "640 by 480 pixels");
        assert!(kernel.heap.msize(pointer).unwrap() >= 1024);
        assert_eq!(kernel.thread_last_errors.get(&0), Some(&1234));
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "LocalFree",
                [pointer, 0, 0, 0, 0, 0, 0]
            ),
            0
        );
        assert_eq!(kernel.heap.free_bytes(), free);
        assert_eq!(kernel.framebuffer.frame_counter, 0);
    }

    #[test]
    fn indexed_inserts_reorder_unicode_and_numbers_without_modifying_the_array() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        put(&mut cpu, 0x1100, "%2!04X! %1 / %1%n%3!hs!");
        put(&mut cpu, 0x1200, "Привет");
        cpu.write_mem(0x1300, b"CE\0").unwrap();
        let array = [
            0x1200u32.to_le_bytes(),
            0x2au32.to_le_bytes(),
            0x1300u32.to_le_bytes(),
        ]
        .concat();
        cpu.write_mem(0x2100, &array).unwrap();
        let text = "002A Привет / Привет\r\nCE";
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [
                    FROM_STRING | ARGUMENT_ARRAY,
                    0x1100,
                    0,
                    0,
                    0x3000,
                    256,
                    0x2100
                ]
            ),
            text.encode_utf16().count() as u32
        );
        assert_eq!(get(&mut cpu, 0x3000), text);
        assert_eq!(cpu.read_mem(0x2100, 12).unwrap(), array);
    }

    #[test]
    fn ignored_inserts_escapes_width_and_terminator_do_not_dereference_arguments() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        put(&mut cpu, 0x1100, "one\r\ntwo %1!d!%% %nend%0ignored");
        let text = "one two %1!d!% \r\nend";
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [
                    FROM_STRING | IGNORE_INSERTS | 0xff,
                    0x1100,
                    0,
                    0,
                    0x3000,
                    256,
                    0
                ]
            ),
            text.encode_utf16().count() as u32
        );
        assert_eq!(get(&mut cpu, 0x3000), text);
        put(&mut cpu, 0x1100, "one two three%nlast");
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "FormatMessageW",
            [FROM_STRING | 7, 0x1100, 0, 0, 0x3000, 256, 0],
        );
        assert_eq!(get(&mut cpu, 0x3000), "one two\r\nthree\r\nlast");
    }

    #[test]
    fn format_failures_preserve_outputs_and_refund_unpublished_allocations() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        put(&mut cpu, 0x1100, "long message");
        cpu.write_mem(0x3000, &[0xa5; 32]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [FROM_STRING, 0x1100, 0, 0, 0x3000, 4, 0]
            ),
            0
        );
        assert_eq!(kernel.thread_last_errors.get(&0), Some(&122));
        assert_eq!(cpu.read_mem(0x3000, 32).unwrap(), [0xa5; 32]);
        put(&mut cpu, 0x1100, "%1");
        cpu.write_mem(0x2100, &0x7000u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [
                    FROM_STRING | ARGUMENT_ARRAY,
                    0x1100,
                    0,
                    0,
                    0x3000,
                    256,
                    0x2100
                ]
            ),
            0
        );
        assert_eq!(kernel.thread_last_errors.get(&0), Some(&87));
        assert_eq!(cpu.read_mem(0x3000, 32).unwrap(), [0xa5; 32]);
        put(&mut cpu, 0x1100, "allocated");
        let free = kernel.heap.free_bytes();
        cpu.unmap_region(0x50000000, 0x10000).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [FROM_STRING | ALLOCATE, 0x1100, 0, 0, 0x3000, 0, 0]
            ),
            0
        );
        assert_eq!(kernel.heap.free_bytes(), free);
        assert_eq!(cpu.read_mem(0x3000, 32).unwrap(), [0xa5; 32]);
        cpu.protect_region(0x3000, 0x1000, Prot::READ).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [FROM_STRING | ALLOCATE, 0x1100, 0, 0, 0x3000, 0, 0]
            ),
            0
        );
        assert_eq!(kernel.heap.free_bytes(), free);
    }

    #[test]
    fn message_tables_and_known_system_errors_fail_honestly_for_missing_ids() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let message: Vec<_> = "Native %1"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut table = [
            1u32.to_le_bytes(),
            42u32.to_le_bytes(),
            42u32.to_le_bytes(),
            16u32.to_le_bytes(),
        ]
        .concat();
        table.extend(((message.len() + 4) as u16).to_le_bytes());
        table.extend(1u16.to_le_bytes());
        table.extend(message);
        cpu.write_mem(0x3500, &table).unwrap();
        kernel.image_base = 0x1000;
        kernel.resources.push(ResourceEntry {
            ty: ResourceKey::Id(11),
            name: ResourceKey::Id(1),
            data_rva: 0x2500,
            native_leaves: Vec::new(),
            size: table.len() as u32,
            codepage: 0,
        });
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "FormatMessageW",
            [
                FROM_MODULE | IGNORE_INSERTS,
                FAKE_MODULE_HANDLE,
                42,
                0,
                0x3000,
                256,
                0,
            ],
        );
        assert_eq!(get(&mut cpu, 0x3000), "Native %1");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [FROM_MODULE, FAKE_MODULE_HANDLE, 43, 0, 0x3000, 256, 0]
            ),
            0
        );
        assert_eq!(kernel.thread_last_errors.get(&0), Some(&317));
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "FormatMessageA",
            [FROM_SYSTEM, 0, 5, 0, 0x3000, 256, 0],
        );
        assert_eq!(
            cpu.read_mem(0x3000, 20).unwrap(),
            b"Access is denied.\r\n\0"
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "FormatMessageW",
                [FROM_SYSTEM, 0, 0x12345678, 0, 0x3000, 256, 0]
            ),
            0
        );
        assert_eq!(kernel.thread_last_errors.get(&0), Some(&317));
    }
}
