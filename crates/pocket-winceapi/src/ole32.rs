//! Minimal OLE32 and Windows Media Player compatibility layer.

use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};

use crate::{CallCtx, WinCeDispatcher};

const WMP_METHODS: [&str; 38] = [
    "wmp_query_interface",
    "wmp_add_ref",
    "wmp_release",
    "wmp_close",
    "wmp_get_url",
    "wmp_put_url",
    "wmp_get_open_state",
    "wmp_get_play_state",
    "wmp_get_controls",
    "wmp_get_settings",
    "wmp_get_current_media",
    "wmp_put_current_media",
    "wmp_get_media_collection",
    "wmp_get_playlist_collection",
    "wmp_get_version_info",
    "wmp_launch_url",
    "wmp_get_network",
    "wmp_get_current_playlist",
    "wmp_put_current_playlist",
    "wmp_get_cdrom_collection",
    "wmp_get_closed_caption",
    "wmp_get_is_online",
    "wmp_get_error",
    "wmp_get_status",
    "wmp_get_enabled",
    "wmp_put_enabled",
    "wmp_get_full_screen",
    "wmp_put_full_screen",
    "wmp_get_enable_context_menu",
    "wmp_put_enable_context_menu",
    "wmp_get_ui_mode",
    "wmp_put_ui_mode",
    "wmp_get_stretch_to_fit",
    "wmp_put_stretch_to_fit",
    "wmp_get_windowless_video",
    "wmp_put_windowless_video",
    "wmp_get_is_remote",
    "wmp_get_player_application",
];

const WMP_CHILD_METHOD: &str = "wmp_child_method";
const WMP_CHILD_SLOTS: usize = 64;

pub fn register(d: &mut WinCeDispatcher) {
    let dll = "ole32.dll";
    d.register_handler(dll, "CoTaskMemAlloc", co_task_mem_alloc);
    d.register_handler(dll, "CoTaskMemFree", co_task_mem_free);
    d.register_handler(dll, "CoTaskMemRealloc", co_task_mem_realloc);
    d.register_handler(dll, "CoInitialize", s_ok);
    d.register_handler(dll, "CoInitializeEx", s_ok);
    d.register_handler(dll, "CoUninitialize", void_returning);
    d.register_handler(dll, "CoCreateGuid", co_create_guid);
    d.register_handler(dll, "StringFromCLSID", string_from_clsid);
    d.register_handler(dll, "StringFromIID", string_from_clsid);
    d.register_handler(dll, "StringFromGUID2", string_from_guid2);
    d.register_handler(dll, "CLSIDFromString", clsid_from_string);
    d.register_handler(dll, "IIDFromString", clsid_from_string);
    d.register_handler(dll, "CoGetClassObject", co_get_class_object);
    d.register_handler(dll, "OleInitialize", s_ok);
    d.register_handler(dll, "OleUninitialize", void_returning);
    d.register_handler(dll, "OleRun", crate::ole_run::run);
    d.register_handler(dll, "CoCreateInstance", co_create_instance);
    for &name in &WMP_METHODS {
        d.register_handler(dll, name, wmp_method);
    }
    d.register_handler(dll, WMP_CHILD_METHOD, wmp_child_method);
    crate::directshow::register(d);
    crate::ole_bind::register(d);
    crate::ole_stream::register(d);
    crate::ole_automation::register(d);
    d.register_constant(dll, "CoGetMalloc", 0, zero_returning);
}

fn s_ok(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn void_returning(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn zero_returning(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn wmp_export_address(ctx: &CallCtx<'_>, name: &str) -> u32 {
    ctx.kernel
        .dynamic_exports
        .get(&pocket_kernel::OLE32_MODULE_HANDLE)
        .and_then(|exports| exports.get(name).copied())
        .unwrap_or(0)
}

fn alloc_child(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let vtable = ctx
        .kernel
        .heap
        .alloc((WMP_CHILD_SLOTS * 4) as u32)
        .unwrap_or(0);
    let object = ctx.kernel.heap.alloc(4).unwrap_or(0);
    if vtable == 0 || object == 0 {
        return Ok(0);
    }
    let address = wmp_export_address(ctx, WMP_CHILD_METHOD);
    for index in 0..WMP_CHILD_SLOTS {
        ctx.cpu
            .write_mem(vtable + (index as u32) * 4, &address.to_le_bytes())?;
    }
    ctx.cpu.write_mem(object, &vtable.to_le_bytes())?;
    Ok(object)
}

fn co_create_instance(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if let Some(outcome) = crate::native_com::resume(ctx)? {
        return Ok(outcome);
    }
    const CLSID_WMP: [u8; 16] = [
        0x52, 0x2a, 0xf5, 0x6b, 0x4a, 0x39, 0xd3, 0x11, 0xb1, 0x53, 0x00, 0xc0, 0x4f, 0x79, 0xfa,
        0xa6,
    ];
    const IID_WMP_PLAYER: [u8; 16] = [
        0x4f, 0x2a, 0xf5, 0x6b, 0x4a, 0x39, 0xd3, 0x11, 0xb1, 0x53, 0x00, 0xc0, 0x4f, 0x79, 0xfa,
        0xa6,
    ];
    let clsid = ctx.arg_u32(0)?;
    let outer = ctx.arg_u32(1)?;
    let iid = ctx.arg_u32(3)?;
    let out = ctx.arg_u32(4)?;
    if outer != 0 || clsid == 0 || iid == 0 || out == 0 {
        if out != 0 {
            ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
        }
        return Ok(DispatchOutcome::ReturnedR0(0x8000_4002));
    }
    let clsid_bytes = ctx.cpu.read_mem(clsid, 16)?;
    let iid_bytes = ctx.cpu.read_mem(iid, 16)?;
    if clsid_bytes == crate::directshow::CLASS {
        return crate::directshow::create(ctx, &iid_bytes, out);
    }
    if let Some(outcome) = crate::msxml::try_co_create(ctx, &clsid_bytes, &iid_bytes, out)? {
        return Ok(outcome);
    }
    if clsid_bytes != CLSID_WMP || iid_bytes != IID_WMP_PLAYER {
        if let Some(outcome) = crate::native_com::activate(ctx, true)? {
            return Ok(outcome);
        }
        ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
        return Ok(DispatchOutcome::ReturnedR0(0x8000_4002));
    }
    let vtable_slots = WMP_CHILD_SLOTS.max(WMP_METHODS.len());
    let vtable = ctx
        .kernel
        .heap
        .alloc((vtable_slots * 4) as u32)
        .unwrap_or(0);
    let object = ctx.kernel.heap.alloc(4).unwrap_or(0);
    let child = alloc_child(ctx)?;
    if vtable == 0 || object == 0 || child == 0 {
        ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
        return Ok(DispatchOutcome::ReturnedR0(0x8000_4005));
    }
    for index in 0..vtable_slots {
        let name = WMP_METHODS.get(index).copied().unwrap_or(WMP_CHILD_METHOD);
        let address = wmp_export_address(ctx, name);
        ctx.cpu
            .write_mem(vtable + (index as u32) * 4, &address.to_le_bytes())?;
    }
    ctx.cpu.write_mem(object, &vtable.to_le_bytes())?;
    ctx.cpu.write_mem(out, &object.to_le_bytes())?;
    log::info!(
        "CoCreateInstance: created Windows Media Player compatibility object 0x{object:08x}"
    );
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn wmp_method(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let method = ctx.thunk.friendly_name.as_deref().unwrap_or_default();
    match method {
        "wmp_query_interface" => {
            let out = ctx.arg_u32(2)?;
            if out != 0 {
                let this = ctx.arg_u32(0)?;
                ctx.cpu.write_mem(out, &this.to_le_bytes())?;
            }
            Ok(DispatchOutcome::ReturnedR0(0))
        }
        "wmp_add_ref" | "wmp_release" => Ok(DispatchOutcome::ReturnedR0(1)),
        "wmp_get_controls"
        | "wmp_get_settings"
        | "wmp_get_current_media"
        | "wmp_get_media_collection"
        | "wmp_get_playlist_collection"
        | "wmp_get_network"
        | "wmp_get_current_playlist"
        | "wmp_get_cdrom_collection"
        | "wmp_get_closed_caption"
        | "wmp_get_error"
        | "wmp_get_player_application" => {
            let out = ctx.arg_u32(1)?;
            if out != 0 {
                let child = alloc_child(ctx)?;
                ctx.cpu.write_mem(out, &child.to_le_bytes())?;
            }
            Ok(DispatchOutcome::ReturnedR0(0))
        }
        "wmp_get_open_state"
        | "wmp_get_play_state"
        | "wmp_get_is_online"
        | "wmp_get_enabled"
        | "wmp_get_full_screen"
        | "wmp_get_enable_context_menu"
        | "wmp_get_stretch_to_fit"
        | "wmp_get_windowless_video"
        | "wmp_get_is_remote" => {
            let out = ctx.arg_u32(1)?;
            if out != 0 {
                ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
            }
            Ok(DispatchOutcome::ReturnedR0(0))
        }
        "wmp_get_url" | "wmp_get_version_info" | "wmp_get_status" | "wmp_get_ui_mode" => {
            let out = ctx.arg_u32(1)?;
            if out != 0 {
                ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
            }
            Ok(DispatchOutcome::ReturnedR0(0))
        }
        _ => Ok(DispatchOutcome::ReturnedR0(0)),
    }
}

fn wmp_child_method(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn co_task_mem_alloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let size = ctx.arg_u32(0)?;
    let user_ptr = ctx.kernel.heap.alloc(size).unwrap_or(0);
    Ok(DispatchOutcome::ReturnedR0(user_ptr))
}

fn co_task_mem_free(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    if p != 0 {
        ctx.kernel.heap.free(p);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn co_task_mem_realloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let size = ctx.arg_u32(1)?;
    if p == 0 {
        let v = ctx.kernel.heap.alloc(size).unwrap_or(0);
        return Ok(DispatchOutcome::ReturnedR0(v));
    }
    if size == 0 {
        ctx.kernel.heap.free(p);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let old_size = ctx.kernel.heap.msize(p).unwrap_or(0);
    let new_p = match ctx.kernel.heap.alloc(size) {
        Some(np) => np,
        None => return Ok(DispatchOutcome::ReturnedR0(0)),
    };
    let to_copy = old_size.min(size);
    if to_copy > 0 {
        let bytes = ctx.cpu.read_mem(p, to_copy)?;
        ctx.cpu.write_mem(new_p, &bytes)?;
    }
    ctx.kernel.heap.free(p);
    Ok(DispatchOutcome::ReturnedR0(new_p))
}

fn co_create_guid(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(0xFEED_BABE);
    let p = ctx.arg_u32(0)?;
    if p == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0x8007_0057));
    }
    let mut buf = [0u8; 16];
    for chunk in buf.chunks_mut(4) {
        let v = SEED.fetch_add(0x9E37_79B9, Ordering::Relaxed);
        chunk.copy_from_slice(&v.to_le_bytes());
    }
    ctx.cpu.write_mem(p, &buf)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

pub(crate) fn guid_string(bytes: &[u8]) -> String {
    let data1 = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let data2 = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    let data3 = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    format!(
        "{{{data1:08X}-{data2:04X}-{data3:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn read_guid_string(ctx: &mut CallCtx<'_>, guid: u32) -> Result<Option<Vec<u8>>, KernelError> {
    if guid == 0 || ctx.cpu.check_guest_access(guid, 16, Prot::READ).is_err() {
        return Ok(None);
    }
    let bytes = ctx.cpu.read_mem(guid, 16)?;
    Ok(Some(
        guid_string(&bytes)
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect(),
    ))
}

// Portable Explorer builds shell namespace paths from CLSIDs. Its native
// idlist code copies this allocated string and releases it with CoTaskMemFree.
fn string_from_clsid(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    const INVALID: u32 = 0x8007_0057;
    const OUT_OF_MEMORY: u32 = 0x8007_000e;
    let guid = ctx.arg_u32(0)?;
    let output = ctx.arg_u32(1)?;
    if output == 0 || ctx.cpu.check_guest_access(output, 4, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    ctx.cpu.write_mem(output, &0u32.to_le_bytes())?;
    let Some(bytes) = read_guid_string(ctx, guid)? else {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    };
    let Some(pointer) = ctx.kernel.heap.alloc(bytes.len() as u32) else {
        return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY));
    };
    let write = ctx
        .cpu
        .write_mem(pointer, &bytes)
        .and_then(|()| ctx.cpu.write_mem(output, &pointer.to_le_bytes()));
    if let Err(error) = write {
        ctx.kernel.heap.free(pointer);
        return Err(error.into());
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn string_from_guid2(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let guid = ctx.arg_u32(0)?;
    let output = ctx.arg_u32(1)?;
    let capacity = ctx.arg_u32(2)?;
    if output == 0
        || (capacity as i32) < 39
        || ctx.cpu.check_guest_access(output, 78, Prot::WRITE).is_err()
    {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let Some(bytes) = read_guid_string(ctx, guid)? else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    ctx.cpu.write_mem(output, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(39))
}

fn parse_guid(text: &[u16]) -> Option<[u8; 16]> {
    if text.len() != 38
        || text[0] != b'{' as u16
        || text[37] != b'}' as u16
        || [9, 14, 19, 24].iter().any(|&i| text[i] != b'-' as u16)
    {
        return None;
    }
    let mut digits = Vec::with_capacity(32);
    for (i, &unit) in text.iter().enumerate().take(37).skip(1) {
        if [9, 14, 19, 24].contains(&i) {
            continue;
        }
        digits.push(char::from_u32(u32::from(unit))?.to_digit(16)? as u8);
    }
    let mut bytes = [0; 16];
    for (byte, pair) in bytes.iter_mut().zip(digits.chunks_exact(2)) {
        *byte = pair[0] * 16 + pair[1];
    }
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    Some(bytes)
}

fn clsid_from_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let input = ctx.arg_u32(0)?;
    let output = ctx.arg_u32(1)?;
    if output == 0 || ctx.cpu.check_guest_access(output, 16, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(0x8007_0057));
    }
    let guid = if input == 0 {
        Some([0; 16])
    } else {
        let mut text = Vec::new();
        let mut terminated = false;
        for i in 0..39 {
            let Some(address) = input.checked_add(i * 2) else {
                break;
            };
            if ctx.cpu.check_guest_access(address, 2, Prot::READ).is_err() {
                break;
            }
            let bytes = ctx.cpu.read_mem(address, 2)?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                terminated = true;
                break;
            }
            text.push(unit);
        }
        terminated.then(|| parse_guid(&text)).flatten()
    };
    let Some(guid) = guid else {
        return Ok(DispatchOutcome::ReturnedR0(0x8004_01f3));
    };
    ctx.cpu.write_mem(output, &guid)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn co_get_class_object(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if let Some(outcome) = crate::native_com::resume(ctx)? {
        return Ok(outcome);
    }
    let output = ctx.arg_u32(4)?;
    if output == 0 || ctx.cpu.check_guest_access(output, 4, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(0x8007_0057));
    }
    ctx.cpu.write_mem(output, &0u32.to_le_bytes())?;
    if let Some(outcome) = crate::native_com::activate(ctx, false)? {
        return Ok(outcome);
    }
    // Private shell classes stay in native pceshell. Unavailable host COM
    // classes must fail; S_OK with a null factory crashes the caller.
    Ok(DispatchOutcome::ReturnedR0(0x8004_0111))
}

#[cfg(test)]
mod guid_tests {
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    use pocket_kernel::DispatchOutcome;

    #[test]
    fn shell_clsid_string_uses_com_allocation_and_mixed_endian_guid_layout() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        kernel.heap = pocket_kernel::Heap::new(0x5000_0000, 0x10000);
        cpu.map_region(
            0x5000_0000,
            0x10000,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .unwrap();
        cpu.write_mem(
            0x1100,
            &[
                0x52, 0x2a, 0xf5, 0x6b, 0x4a, 0x39, 0xd3, 0x11, 0xb1, 0x53, 0, 0xc0, 0x4f, 0x79,
                0xfa, 0xa6,
            ],
        )
        .unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "StringFromCLSID",
                &[0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let pointer = cpu.read_u32_le(0x1200).unwrap();
        assert!(kernel.heap.msize(pointer).unwrap() >= 78);
        let expected: Vec<u8> = "{6BF52A52-394A-11D3-B153-00C04F79FAA6}\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(cpu.read_mem(pointer, 78).unwrap(), expected);
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "ole32.dll",
            "CoTaskMemFree",
            &[pointer],
        );
        assert!(kernel.heap.msize(pointer).is_none());
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "StringFromGUID2",
                &[0x1100, 0x1300, 38]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "StringFromGUID2",
                &[0x1100, 0x1300, 39]
            ),
            DispatchOutcome::ReturnedR0(39)
        );
        assert_eq!(cpu.read_mem(0x1300, 78).unwrap(), expected);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "CLSIDFromString",
                &[0x1300, 0x1400]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            cpu.read_mem(0x1400, 16).unwrap(),
            cpu.read_mem(0x1100, 16).unwrap()
        );
        cpu.write_mem(0x1302, &(b'Z' as u16).to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "CLSIDFromString",
                &[0x1300, 0x1400]
            ),
            DispatchOutcome::ReturnedR0(0x8004_01f3)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "StringFromCLSID",
                &[0, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0x8007_0057)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 0);
    }
}
