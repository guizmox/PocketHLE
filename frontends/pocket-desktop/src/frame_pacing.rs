//! Conservative import hint for legacy ARM games using GAPI draw callbacks.
use pocket_pe::{ImportBinding, LoadedImage, LoadedSection};
use std::{collections::HashSet, fs::File, io::Read, path::Path};

pub(crate) fn may_run_fast(path: &Path) -> bool {
    const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_FILE_BYTES
    {
        return false;
    }
    pocket_pe::load_bytes_bounded(&bytes, 32 * 1024 * 1024).is_ok_and(|image| detect(&image))
}

fn words(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

fn branch_target(pc: u32, instruction: u32) -> u32 {
    let offset = ((instruction << 8) as i32 >> 6) as u32;
    pc.wrapping_add(8).wrapping_add(offset)
}

fn literal(image: &LoadedImage, section: &LoadedSection, index: usize, word: u32) -> Option<u32> {
    if word & 0xff7f_0000 != 0xe51f_0000 {
        return None;
    }
    let pc = index * 4 + 8;
    let offset = (word & 0xfff) as usize;
    let address = if word & 0x0080_0000 == 0 {
        pc.checked_sub(offset)?
    } else {
        pc + offset
    };
    let bytes = section.data.get(address..address + 4)?;
    let value = u32::from_le_bytes(bytes.try_into().unwrap());
    image
        .sections
        .iter()
        .any(|s| {
            value.wrapping_sub(image.image_base.wrapping_add(s.virtual_address)) < s.virtual_size
        })
        .then_some(value)
}

fn detect(image: &LoadedImage) -> bool {
    if image.machine != pocket_pe::machine::ARM {
        return false;
    }
    let iat = |plain: &str, decorated: &str| {
        image.imports.iter().find_map(|import| {
            (import.dll.eq_ignore_ascii_case("gx.dll")
                && matches!(&import.binding, ImportBinding::Name(name) if name == plain || name.starts_with(decorated)))
                .then_some(import.iat_va)
        })
    };
    let (Some(begin), Some(end)) = (
        iat("GXBeginDraw", "?GXBeginDraw@@"),
        iat("GXEndDraw", "?GXEndDraw@@"),
    ) else {
        return false;
    };

    for section in image.sections.iter().filter(|s| s.is_executable()) {
        let code = words(&section.data);
        let base = image.image_base.wrapping_add(section.virtual_address);
        let pc = |i: usize| base.wrapping_add(i as u32 * 4);

        // Locate routines bracketing an indirect draw with GXBeginDraw/GXEndDraw.
        let draw_import = |i: usize, slot| {
            code.get(i..i + 4).is_some_and(|seq| {
                let register = (seq[0] >> 12) & 15;
                literal(image, section, i, seq[0]) == Some(slot)
                    && seq[1] == (0xe590_0000 | register << 16 | register << 12)
                    && seq[2] == 0xe1a0_e00f
                    && seq[3] == (0xe1a0_f000 | register)
            })
        };
        let mut presenters = HashSet::new();
        for start in 0..code.len() {
            if code[start] & 0xffff_4000 != 0xe92d_4000 {
                continue;
            }
            let mut began = false;
            for i in start + 1..code.len().min(start + 64) {
                if code[i] & 0xffff_8000 == 0xe8bd_8000 {
                    break;
                }
                if draw_import(i, begin) {
                    began = true;
                }
                if began && draw_import(i, end) {
                    presenters.insert(pc(start));
                    break;
                }
            }
        }
        if presenters.is_empty() {
            continue;
        }

        // Look for vtable dispatch followed by a call to a presenter. Only
        // consider the corresponding slots in tables of executable pointers.
        let slots: HashSet<_> =
            code.windows(4)
                .enumerate()
                .filter_map(|(i, seq)| {
                    let reg = (seq[0] >> 12) & 15;
                    if seq[0] & 0xfff0_0fff != 0xe590_0000
                        || seq[1] & 0xffff_f000 != (0xe590_0000 | reg << 16 | reg << 12)
                        || seq[2] != 0xe1a0_e00f
                        || seq[3] != (0xe1a0_f000 | reg)
                    {
                        return None;
                    }
                    let offset = (seq[1] & 0xfff) as usize;
                    (offset % 4 == 0
                        && offset < 64
                        && code[i + 4..code.len().min(i + 16)].iter().enumerate().any(
                            |(j, &word)| {
                                word >> 24 == 0xeb
                                    && presenters.contains(&branch_target(pc(i + 4 + j), word))
                            },
                        ))
                    .then_some(offset / 4)
                })
                .collect();
        let executable_index = |addr: u32| {
            let offset = addr.wrapping_sub(base);
            (offset % 4 == 0 && offset / 4 < code.len() as u32).then_some(offset as usize / 4)
        };
        if slots.is_empty() {
            continue;
        }
        let table_width = 4.max(slots.iter().max().unwrap() + 1);
        let code_references: HashSet<_> = code
            .iter()
            .enumerate()
            .filter_map(|(i, &op)| literal(image, section, i, op))
            .collect();
        let mut checked = HashSet::new();
        for table in image
            .sections
            .iter()
            .filter(|s| s.is_readable() && !s.is_writable() && !s.is_executable())
        {
            let table_base = image.image_base.wrapping_add(table.virtual_address);
            for (index, row) in words(&table.data).windows(table_width).enumerate() {
                if !code_references.contains(&table_base.wrapping_add(index as u32 * 4))
                    || !row.iter().all(|&addr| executable_index(addr).is_some())
                {
                    continue;
                }
                for &slot in &slots {
                    let start = executable_index(row[slot]).unwrap();
                    if checked.insert(start) && fixed_step(image, section, &code, start) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn fixed_step(image: &LoadedImage, section: &LoadedSection, code: &[u32], start: usize) -> bool {
    if code[start] & 0xffff_4000 != 0xe92d_4000 {
        return false;
    }
    let end = (start + 1..code.len().min(start + 512))
        .find(|&i| code[i] & 0xffff_8000 == 0xe8bd_8000)
        .unwrap_or(code.len().min(start + 512));
    let body = &code[start..end];
    for (load_index, seq) in body.windows(4).enumerate() {
        // A countdown decremented once per draw gates the state update.
        let base = (seq[0] >> 16) & 15;
        let reg = (seq[0] >> 12) & 15;
        let store = 0xe580_0000 | base << 16 | reg << 12;
        if seq[0] != (store | 0x0010_0000)
            || seq[1] != (0xe250_0001 | reg << 16 | reg << 12)
            || seq[2] != store
            || seq[3] >> 24 != 0x1a
        {
            continue;
        }
        // Compilers may schedule a constant load between the literal and
        // the countdown read.
        let i = if load_index >= 2
            && body[load_index - 1] & 0xfff0_0000 == 0xe3a0_0000
            && (body[load_index - 1] >> 12) & 15 != base
        {
            load_index - 2
        } else if load_index >= 1 {
            load_index - 1
        } else {
            continue;
        };
        let setup = body[i];
        let Some(counter) = literal(image, section, start + i, setup) else {
            continue;
        };
        if (setup >> 12) & 15 != base {
            continue;
        }
        let branch = load_index + 3;
        let skip = branch_target((branch * 4) as u32, seq[3]) as usize / 4;
        let Some(update) = body.get(branch + 1..skip) else {
            continue;
        };
        if update.len() > 32
            || update.iter().any(|word| {
                word & 0x0e00_0000 == 0x0a00_0000 // B / BL
                    || word & 0x0fff_fff0 == 0x012f_ff10 // BX
                    || word & 0x0fff_fff0 == 0x012f_ff30 // BLX
                    || word & 0x0fff_fff0 == 0x01a0_f000 // MOV PC, register
            })
            || update.last() != Some(&store)
            || !update
                .iter()
                .any(|word| word & 0xffff_ff00 == (0xe3a0_0000 | reg << 12) && word & 0xff > 1)
        {
            continue;
        }

        // Require a separate state slot read before the countdown and changed
        // by an immediate add/subtract in the zero path.
        for (j, &word) in update.iter().enumerate() {
            if word & 0x0f70_0000 != 0x0500_0000 || (word >> 16) & 15 == base {
                continue;
            }
            let state_base = (word >> 16) & 15;
            let state_reg = (word >> 12) & 15;
            let delta = &update[j.saturating_sub(4)..j];
            if !delta.iter().any(|&op| {
                let operands = state_reg << 16 | state_reg << 12;
                let operation = op & 0x0fff_f000;
                (operation == (0x0280_0000 | operands) || operation == (0x0240_0000 | operands))
                    && op & 0xfff != 0
            }) {
                continue;
            }
            if body[..i].iter().enumerate().rev().any(|(k, &op)| {
                (op >> 12) & 15 == state_base
                    && literal(image, section, start + k, op).is_some_and(|addr| addr != counter)
                    && feeds_draw_argument(&body[k + 1..i], word)
            }) {
                return true;
            }
        }
    }
    false
}

// The state has to feed a call in the draw callback, rather than just being
// sampled for statistics. Follow the common load / argument move / call form.
fn feeds_draw_argument(code: &[u32], store: u32) -> bool {
    let load = (store & 0x0fff_0fff) | 0xe010_0000;
    for (i, &word) in code.iter().enumerate() {
        if word & 0xffff_0fff != load {
            continue;
        }
        let source = (word >> 12) & 15;
        let mut argument = source < 4;
        let mut previous = word;
        for &op in &code[i + 1..code.len().min(i + 9)] {
            let direct_call = op >> 24 == 0xeb;
            let indirect_call = previous == 0xe1a0_e00f && op & 0xffff_fff0 == 0xe1a0_f000;
            if direct_call || indirect_call {
                if argument {
                    return true;
                }
                break;
            }
            if op & 0xffff_cfff == (0xe1a0_0000 | source) {
                argument = true;
            }
            previous = op;
        }
    }
    false
}
