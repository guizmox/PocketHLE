//! CE Messenger loads three embedded MSFT libraries before creating its UI.
//! Type metadata has a real ITypeLib/ITypeInfo ABI, references and descriptors;
//! a successful HRESULT must always supply the requested interface.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{
    automation::{Descriptor, Kind, Object},
    registry::RegistryValue,
    DispatchOutcome, KernelError,
};
use pocket_pe::typelib::{Function, Library, TypeDesc};
use std::sync::Arc;

const INVALID: u32 = 0x8007_0057;
const POINTER: u32 = 0x8000_4003;
const NO_MEMORY: u32 = 0x8007_000e;
const MISSING: u32 = 0x8002_802b;
const LIBRARY_MISSING: u32 = 0x8002_801d;
const LOAD_ERROR: u32 = 0x8002_9c4a;
const LIB_METHODS: [&str; 13] = [
    "automation_query",
    "automation_add_ref",
    "automation_release",
    "type_lib_count",
    "type_lib_info",
    "type_lib_kind",
    "type_lib_guid",
    "type_lib_attr",
    "automation_notimpl",
    "type_lib_documentation",
    "type_lib_is_name",
    "automation_notimpl",
    "automation_release_descriptor",
];
const INFO_METHODS: [&str; 22] = [
    "automation_query",
    "automation_add_ref",
    "automation_release",
    "type_info_attr",
    "automation_notimpl",
    "type_info_func",
    "automation_notimpl",
    "type_info_names",
    "type_info_ref",
    "type_info_ref_flags",
    "type_info_ids",
    "automation_notimpl",
    "type_info_documentation",
    "automation_notimpl",
    "type_info_ref_info",
    "automation_notimpl",
    "automation_notimpl",
    "type_info_mops",
    "type_info_library",
    "automation_release_descriptor",
    "automation_release_descriptor",
    "automation_release_descriptor",
];

pub(crate) fn register(d: &mut WinCeDispatcher) {
    for dll in ["oleaut32.dll", "coredll.dll"] {
        d.register_handler(dll, "LoadTypeLib", load);
        d.register_handler(dll, "LoadTypeLibEx", load_ex);
        d.register_handler(dll, "LoadRegTypeLib", load_registered);
        d.register_handler(dll, "RegisterTypeLib", register_library);
        d.register_handler(dll, "DispGetIDsOfNames", disp_ids);
    }
    for name in LIB_METHODS.into_iter().chain(INFO_METHODS) {
        d.register_handler("ole32.dll", name, dispatch);
    }
}
fn ret(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn access(ctx: &CallCtx<'_>, p: u32, n: u32, prot: Prot) -> bool {
    p != 0 && ctx.cpu.check_guest_access(p, n, prot).is_ok()
}
fn text(ctx: &mut CallCtx<'_>, p: u32) -> Option<String> {
    if p == 0 {
        return None;
    }
    crate::coredll::read_wstr(ctx, p, 0x10000)
        .ok()
        .map(|u| String::from_utf16_lossy(&u))
}
fn output(ctx: &mut CallCtx<'_>, p: u32) -> Result<bool, KernelError> {
    if !access(ctx, p, 4, Prot::WRITE) {
        return Ok(false);
    }
    ctx.cpu.write_mem(p, &0u32.to_le_bytes())?;
    Ok(true)
}
fn new_object(
    ctx: &mut CallCtx<'_>,
    key: String,
    metadata: Arc<Library>,
    kind: Kind,
) -> Result<u32, KernelError> {
    if let Some((this, _)) = ctx
        .kernel
        .ce
        .com
        .automation
        .objects
        .iter()
        .find(|(_, o)| o.key == key && o.kind == kind)
    {
        let this = *this;
        ctx.kernel
            .ce
            .com
            .automation
            .objects
            .get_mut(&this)
            .unwrap()
            .refs += 1;
        return Ok(this);
    }
    let methods = if kind == Kind::Library {
        &LIB_METHODS[..]
    } else {
        &INFO_METHODS[..]
    };
    let Some(exports) = ctx
        .kernel
        .dynamic_exports
        .get(&pocket_kernel::OLE32_MODULE_HANDLE)
    else {
        return Ok(0);
    };
    let Some(addresses) = methods
        .iter()
        .map(|name| exports.get(*name).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(0);
    };
    if addresses.contains(&0) {
        return Ok(0);
    }
    let Some(vtable) = ctx.kernel.heap.alloc((addresses.len() * 4) as u32) else {
        return Ok(0);
    };
    let Some(this) = ctx.kernel.heap.alloc(4) else {
        ctx.kernel.heap.free(vtable);
        return Ok(0);
    };
    if let Err(error) = ctx
        .cpu
        .write_mem(
            vtable,
            &addresses
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .and_then(|_| ctx.cpu.write_mem(this, &vtable.to_le_bytes()))
    {
        ctx.kernel.heap.free(vtable);
        ctx.kernel.heap.free(this);
        return Err(error.into());
    }
    if let Kind::TypeInfo { parent, .. } = kind {
        ctx.kernel
            .ce
            .com
            .automation
            .objects
            .get_mut(&parent)
            .unwrap()
            .refs += 1;
    }
    ctx.kernel.ce.com.automation.objects.insert(
        this,
        Object {
            vtable,
            refs: 1,
            key,
            metadata,
            kind,
        },
    );
    Ok(this)
}
fn release(ctx: &mut CallCtx<'_>, this: u32) -> u32 {
    let Some(object) = ctx.kernel.ce.com.automation.objects.get_mut(&this) else {
        return 0;
    };
    object.refs -= 1;
    if object.refs != 0 {
        return object.refs;
    }
    let object = ctx.kernel.ce.com.automation.objects.remove(&this).unwrap();
    let descriptors: Vec<_> = ctx
        .kernel
        .ce
        .com
        .automation
        .descriptors
        .iter()
        .filter_map(|(p, d)| (d.owner == this).then_some(*p))
        .collect();
    for p in descriptors {
        if let Some(d) = ctx.kernel.ce.com.automation.descriptors.remove(&p) {
            for block in d.allocations {
                ctx.kernel.heap.free(block);
            }
        }
    }
    ctx.kernel.heap.free(object.vtable);
    ctx.kernel.heap.free(this);
    if let Kind::TypeInfo { parent, .. } = object.kind {
        release(ctx, parent);
    }
    0
}
fn file_metadata(ctx: &mut CallCtx<'_>, path: &str) -> Option<(String, Arc<Library>)> {
    let mut resource = 1u32;
    let mut path = path.trim_matches('"').replace('/', "\\");
    if let Some((file, suffix)) = path.rsplit_once('\\') {
        if let Ok(id) = suffix.parse::<u32>() {
            resource = id;
            path = file.to_owned();
        }
    }
    let source = ctx
        .kernel
        .vfs
        .resolve(&path)
        .or_else(|| ctx.kernel.vfs.resolve(&format!("\\Windows\\{path}")))?;
    let key = format!("{}#{resource}", source.to_string_lossy().to_lowercase());
    if let Some(metadata) = ctx.kernel.ce.com.automation.parsed.get(&key) {
        return Some((key, metadata.clone()));
    }
    let bytes = std::fs::read(source).ok()?;
    let raw = if bytes.starts_with(b"MSFT") {
        &bytes[..]
    } else {
        pocket_pe::file_resource(
            &bytes,
            &pocket_pe::ResourceKey::Name("TYPELIB".into()),
            &pocket_pe::ResourceKey::Id(resource),
        )
        .ok()??
    };
    let metadata = match pocket_pe::typelib::parse(raw) {
        Ok(value) => Arc::new(value),
        Err(error) => {
            log::warn!("LoadTypeLib({path:?}, resource={resource}): {error}");
            return None;
        }
    };
    log::info!(
        "Automation: {} ({} types) from {path:?}#{resource}",
        metadata.name,
        metadata.types.len()
    );
    ctx.kernel
        .ce
        .com
        .automation
        .parsed
        .insert(key.clone(), metadata.clone());
    Some((key, metadata))
}
fn load_impl(ctx: &mut CallCtx<'_>, ex: bool) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(if ex { 2 } else { 1 })?;
    if !output(ctx, out)? {
        return ret(POINTER);
    }
    if ex && ctx.arg_u32(1)? > 2 {
        return ret(INVALID);
    }
    let pointer = ctx.arg_u32(0)?;
    let Some(path) = text(ctx, pointer) else {
        return ret(INVALID);
    };
    let Some((key, metadata)) = file_metadata(ctx, &path) else {
        return ret(LOAD_ERROR);
    };
    if ex && ctx.arg_u32(1)? == 1 || !ex && !path.contains(['\\', '/']) {
        let result = write_registration(ctx, &metadata, &path);
        if result != 0 {
            return ret(result);
        }
    }
    let object = new_object(ctx, key, metadata, Kind::Library)?;
    if object == 0 {
        return ret(NO_MEMORY);
    }
    ctx.cpu.write_mem(out, &object.to_le_bytes())?;
    ret(0)
}
fn load(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    load_impl(ctx, false)
}
fn load_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    load_impl(ctx, true)
}
fn write_registration(ctx: &mut CallCtx<'_>, metadata: &Library, path: &str) -> u32 {
    let base = format!(
        "HKCR\\TypeLib\\{}\\{:x}.{:x}",
        crate::ole32::guid_string(&metadata.guid),
        metadata.version.0,
        metadata.version.1
    );
    let baseline = ctx.kernel.registry.isolated_copy();
    let mut staged = baseline.isolated_copy();
    for (key, value) in [
        (base.clone(), metadata.name.clone()),
        (
            format!("{base}\\{:x}\\win32", metadata.lcid),
            path.to_owned(),
        ),
    ] {
        if !staged.set_value(&key, "default", RegistryValue::Sz(value)) {
            return 0x8002_801c;
        }
    }
    if !ctx.kernel.registry.commit_isolated(&baseline, &staged) {
        return 0x8002_801c;
    }
    0
}
fn register_library(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    let path_p = ctx.arg_u32(1)?;
    let Some(path) = text(ctx, path_p) else {
        return ret(INVALID);
    };
    let Some(object) = ctx.kernel.ce.com.automation.objects.get(&this).cloned() else {
        return ret(INVALID);
    };
    if object.kind != Kind::Library {
        return ret(INVALID);
    }
    ret(write_registration(ctx, &object.metadata, &path))
}
fn load_registered(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (iid, major, minor, lcid, out) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
    );
    if !output(ctx, out)? || !access(ctx, iid, 16, Prot::READ) {
        return ret(POINTER);
    }
    let wanted: [u8; 16] = ctx.cpu.read_mem(iid, 16)?.try_into().unwrap();
    let selected = {
        let key = format!(
            "HKCR\\TypeLib\\{}\\{:x}.{:x}",
            crate::ole32::guid_string(&wanted),
            major,
            minor
        );
        let mut selected = None;
        for locale in [format!("{lcid:x}"), "0".into(), "409".into()] {
            let key = format!("{key}\\{locale}\\win32");
            let value = ctx
                .kernel
                .registry
                .value(&key, "")
                .or_else(|| ctx.kernel.registry.value(&key, "default"));
            if let Some(RegistryValue::Sz(path)) = value {
                if let Some((key, lib)) = file_metadata(ctx, &path) {
                    if lib.guid == wanted && lib.version == (major as u16, minor as u16) {
                        selected = Some((key, lib));
                        break;
                    }
                }
            }
        }
        selected
    };
    let Some((key, metadata)) = selected else {
        return ret(LIBRARY_MISSING);
    };
    let this = new_object(ctx, key, metadata, Kind::Library)?;
    if this == 0 {
        return ret(NO_MEMORY);
    }
    ctx.cpu.write_mem(out, &this.to_le_bytes())?;
    ret(0)
}
fn type_info(
    ctx: &mut CallCtx<'_>,
    parent: u32,
    object: &Object,
    index: usize,
    out: u32,
) -> Result<DispatchOutcome, KernelError> {
    if !output(ctx, out)? {
        return ret(POINTER);
    }
    if index >= object.metadata.types.len() {
        return ret(MISSING);
    }
    let this = new_object(
        ctx,
        object.key.clone(),
        object.metadata.clone(),
        Kind::TypeInfo { parent, index },
    )?;
    if this == 0 {
        return ret(NO_MEMORY);
    }
    ctx.cpu.write_mem(out, &this.to_le_bytes())?;
    ret(0)
}
fn alloc(ctx: &mut CallCtx<'_>, n: u32, allocations: &mut Vec<u32>) -> Result<u32, KernelError> {
    let Some(p) = ctx.kernel.heap.alloc(n) else {
        return Ok(0);
    };
    allocations.push(p);
    ctx.cpu.write_mem(p, &vec![0; n as usize])?;
    Ok(p)
}
fn type_desc(
    ctx: &mut CallCtx<'_>,
    desc: &TypeDesc,
    p: u32,
    allocations: &mut Vec<u32>,
) -> Result<bool, KernelError> {
    let value = if let Some(child) = &desc.child {
        let child_p = alloc(ctx, 8, allocations)?;
        if child_p == 0 || !type_desc(ctx, child, child_p, allocations)? {
            return Ok(false);
        }
        child_p
    } else {
        desc.value
    };
    ctx.cpu.write_mem(p, &value.to_le_bytes())?;
    ctx.cpu.write_mem(p + 4, &desc.vt.to_le_bytes())?;
    Ok(true)
}
fn finish_descriptor(
    ctx: &mut CallCtx<'_>,
    this: u32,
    out: u32,
    p: u32,
    allocations: Vec<u32>,
    ok: bool,
) -> Result<DispatchOutcome, KernelError> {
    if p == 0 || !ok {
        for block in allocations {
            ctx.kernel.heap.free(block);
        }
        return ret(NO_MEMORY);
    }
    ctx.kernel.ce.com.automation.descriptors.insert(
        p,
        Descriptor {
            owner: this,
            allocations,
        },
    );
    ctx.cpu.write_mem(out, &p.to_le_bytes())?;
    ret(0)
}
fn info_function(
    lib: &Library,
    index: usize,
    id: Option<i32>,
    name: Option<&str>,
    depth: usize,
) -> Option<Function> {
    if depth > 32 {
        return None;
    }
    let info = lib.types.get(index)?;
    if let Some(function) = info
        .functions
        .iter()
        .find(|f| id == Some(f.id) || name.is_some_and(|name| f.name.eq_ignore_ascii_case(name)))
    {
        return Some(function.clone());
    }
    for (reference, _) in &info.implementations {
        if let Some(parent) = lib
            .type_offsets
            .iter()
            .position(|offset| offset == reference)
        {
            if let Some(found) = info_function(lib, parent, id, name, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}
fn ids(
    ctx: &mut CallCtx<'_>,
    object: &Object,
    pointers: u32,
    count: u32,
    out: u32,
) -> Result<DispatchOutcome, KernelError> {
    let Kind::TypeInfo { index, .. } = object.kind else {
        return ret(INVALID);
    };
    if count == 0
        || count > 1024
        || !access(ctx, pointers, count * 4, Prot::READ)
        || !access(ctx, out, count * 4, Prot::WRITE)
    {
        return ret(INVALID);
    }
    let mut names = Vec::new();
    for i in 0..count {
        let p = ctx.cpu.read_u32_le(pointers + i * 4)?;
        let Some(name) = text(ctx, p) else {
            return ret(POINTER);
        };
        names.push(name);
    }
    let function = info_function(&object.metadata, index, None, Some(&names[0]), 0);
    let mut unknown = false;
    for (i, name) in names.iter().enumerate() {
        let id = function.as_ref().and_then(|f| {
            if i == 0 {
                Some(f.id)
            } else {
                f.parameters
                    .iter()
                    .position(|p| p.name.eq_ignore_ascii_case(name))
                    .map(|index| index as i32)
            }
        });
        unknown |= id.is_none();
        ctx.cpu
            .write_mem(out + i as u32 * 4, &id.unwrap_or(-1).to_le_bytes())?;
    }
    ret(if unknown { 0x8002_0006 } else { 0 })
}
fn disp_ids(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (this, pointers, count, out) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    );
    let Some(object) = ctx.kernel.ce.com.automation.objects.get(&this).cloned() else {
        return ret(INVALID);
    };
    ids(ctx, &object, pointers, count, out)
}
fn document(ctx: &mut CallCtx<'_>, name: &str) -> Result<DispatchOutcome, KernelError> {
    let (name_out, doc_out, context_out, file_out) = (
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
        ctx.arg_u32(5)?,
    );
    for p in [name_out, doc_out, context_out, file_out] {
        if p != 0 && !output(ctx, p)? {
            return ret(POINTER);
        }
    }
    if name_out != 0 {
        let bstr = crate::msxml::alloc_bstr(ctx, name)?;
        if bstr == 0 {
            return ret(NO_MEMORY);
        }
        ctx.cpu.write_mem(name_out, &bstr.to_le_bytes())?;
    }
    ret(0)
}
fn dispatch(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    let Some(object) = ctx.kernel.ce.com.automation.objects.get(&this).cloned() else {
        return ret(INVALID);
    };
    let metadata = &object.metadata;
    let info = match object.kind {
        Kind::TypeInfo { index, .. } => metadata.types.get(index),
        Kind::Library => None,
    };
    match ctx.thunk.friendly_name.as_deref().unwrap_or("") {
        "automation_notimpl" => ret(0x8000_4001),
        "automation_query" => {
            let iid = ctx.arg_u32(1)?;
            let out = ctx.arg_u32(2)?;
            if !output(ctx, out)? || !access(ctx, iid, 16, Prot::READ) {
                return ret(POINTER);
            }
            let bytes = ctx.cpu.read_mem(iid, 16)?;
            let unknown = [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
            let mut supported = unknown;
            supported[0] = if object.kind == Kind::Library { 2 } else { 1 };
            supported[1] = 4;
            supported[2] = 2;
            if bytes != unknown && bytes != supported {
                return ret(0x80004002);
            }
            ctx.kernel
                .ce
                .com
                .automation
                .objects
                .get_mut(&this)
                .unwrap()
                .refs += 1;
            ctx.cpu.write_mem(out, &this.to_le_bytes())?;
            ret(0)
        }
        "automation_add_ref" => {
            let o = ctx.kernel.ce.com.automation.objects.get_mut(&this).unwrap();
            o.refs = o.refs.saturating_add(1);
            ret(o.refs)
        }
        "automation_release" => ret(release(ctx, this)),
        "automation_release_descriptor" => {
            let p = ctx.arg_u32(1)?;
            if ctx
                .kernel
                .ce
                .com
                .automation
                .descriptors
                .get(&p)
                .is_some_and(|d| d.owner == this)
            {
                let d = ctx.kernel.ce.com.automation.descriptors.remove(&p).unwrap();
                for block in d.allocations {
                    ctx.kernel.heap.free(block);
                }
            }
            ret(0)
        }
        "type_lib_count" => ret(metadata.types.len() as u32),
        "type_lib_info" => {
            let index = ctx.arg_u32(1)? as usize;
            let out = ctx.arg_u32(2)?;
            type_info(ctx, this, &object, index, out)
        }
        "type_lib_kind" => {
            let index = ctx.arg_u32(1)? as usize;
            let out = ctx.arg_u32(2)?;
            if !output(ctx, out)? {
                return ret(POINTER);
            }
            let Some(info) = metadata.types.get(index) else {
                return ret(MISSING);
            };
            ctx.cpu.write_mem(out, &info.kind.to_le_bytes())?;
            ret(0)
        }
        "type_lib_guid" => {
            let iid = ctx.arg_u32(1)?;
            let out = ctx.arg_u32(2)?;
            if !output(ctx, out)? || !access(ctx, iid, 16, Prot::READ) {
                return ret(POINTER);
            }
            let iid = ctx.cpu.read_mem(iid, 16)?;
            let Some(index) = metadata.types.iter().position(|info| info.guid == iid[..]) else {
                return ret(MISSING);
            };
            type_info(ctx, this, &object, index, out)
        }
        "type_lib_attr" | "type_info_attr" => {
            let out = ctx.arg_u32(1)?;
            if !output(ctx, out)? {
                return ret(POINTER);
            }
            let mut allocations = Vec::new();
            let p = alloc(ctx, if info.is_some() { 76 } else { 32 }, &mut allocations)?;
            if p == 0 {
                return ret(NO_MEMORY);
            }
            let mut ok = true;
            if let Some(info) = info {
                let mut bytes = [0; 76];
                bytes[..16].copy_from_slice(&info.guid);
                bytes[16..20].copy_from_slice(&metadata.lcid.to_le_bytes());
                bytes[24..32].fill(0xff);
                bytes[36..40].copy_from_slice(&info.size.to_le_bytes());
                bytes[40..44].copy_from_slice(&info.kind.to_le_bytes());
                for (at, value) in [
                    (44, info.functions.len() as u16),
                    (46, info.variables),
                    (48, info.implementations.len() as u16),
                    (50, info.vtable_size),
                    (52, info.alignment),
                    (54, info.flags),
                    (56, info.version.0),
                    (58, info.version.1),
                ] {
                    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
                }
                ctx.cpu.write_mem(p, &bytes)?;
                if let Some(alias) = &info.alias {
                    ok = type_desc(ctx, alias, p + 60, &mut allocations)?;
                }
            } else {
                let mut bytes = [0; 32];
                bytes[..16].copy_from_slice(&metadata.guid);
                bytes[16..20].copy_from_slice(&metadata.lcid.to_le_bytes());
                bytes[20..24].copy_from_slice(&metadata.syskind.to_le_bytes());
                bytes[24..26].copy_from_slice(&metadata.version.0.to_le_bytes());
                bytes[26..28].copy_from_slice(&metadata.version.1.to_le_bytes());
                bytes[28..30].copy_from_slice(&metadata.flags.to_le_bytes());
                ctx.cpu.write_mem(p, &bytes)?;
            }
            finish_descriptor(ctx, this, out, p, allocations, ok)
        }
        "type_info_func" => {
            let index = ctx.arg_u32(1)? as usize;
            let out = ctx.arg_u32(2)?;
            if !output(ctx, out)? {
                return ret(POINTER);
            }
            let Some(function) = info.and_then(|info| info.functions.get(index)) else {
                return ret(MISSING);
            };
            let mut allocations = Vec::new();
            let p = alloc(ctx, 52, &mut allocations)?;
            let params = if function.parameters.is_empty() {
                0
            } else {
                alloc(ctx, function.parameters.len() as u32 * 16, &mut allocations)?
            };
            if p == 0 || !function.parameters.is_empty() && params == 0 {
                return finish_descriptor(ctx, this, out, p, allocations, false);
            }
            let mut bytes = [0; 52];
            bytes[..4].copy_from_slice(&function.id.to_le_bytes());
            bytes[8..12].copy_from_slice(&params.to_le_bytes());
            bytes[12..16].copy_from_slice(&function.kind.to_le_bytes());
            bytes[16..20].copy_from_slice(&function.invoke.to_le_bytes());
            bytes[20..24].copy_from_slice(&function.callconv.to_le_bytes());
            bytes[24..26].copy_from_slice(&(function.parameters.len() as u16).to_le_bytes());
            bytes[26..28].copy_from_slice(&function.optional.to_le_bytes());
            bytes[28..30].copy_from_slice(&function.vtable_offset.to_le_bytes());
            bytes[48..50].copy_from_slice(&function.flags.to_le_bytes());
            ctx.cpu.write_mem(p, &bytes)?;
            let mut ok = type_desc(ctx, &function.result, p + 32, &mut allocations)?;
            for (index, param) in function.parameters.iter().enumerate() {
                ok &= type_desc(ctx, &param.ty, params + index as u32 * 16, &mut allocations)?;
                ctx.cpu
                    .write_mem(params + index as u32 * 16 + 12, &param.flags.to_le_bytes())?;
            }
            finish_descriptor(ctx, this, out, p, allocations, ok)
        }
        "type_info_ids" => {
            let (p, n, out) = (ctx.arg_u32(1)?, ctx.arg_u32(2)?, ctx.arg_u32(3)?);
            ids(ctx, &object, p, n, out)
        }
        "type_info_names" => {
            let (id, array, count, out) = (
                ctx.arg_u32(1)? as i32,
                ctx.arg_u32(2)?,
                ctx.arg_u32(3)?,
                ctx.arg_u32(4)?,
            );
            if !output(ctx, out)?
                || count > 1024
                || count != 0 && !access(ctx, array, count * 4, Prot::WRITE)
            {
                return ret(POINTER);
            }
            let Kind::TypeInfo { index, .. } = object.kind else {
                return ret(INVALID);
            };
            let Some(function) = info_function(metadata, index, Some(id), None, 0) else {
                return ret(MISSING);
            };
            let names = std::iter::once(function.name)
                .chain(function.parameters.into_iter().map(|p| p.name));
            let mut written = 0u32;
            for name in names.take(count as usize) {
                if name.is_empty() {
                    break;
                }
                let bstr = crate::msxml::alloc_bstr(ctx, &name)?;
                if bstr == 0 {
                    ctx.cpu.write_mem(out, &written.to_le_bytes())?;
                    return ret(NO_MEMORY);
                }
                ctx.cpu
                    .write_mem(array + written * 4, &bstr.to_le_bytes())?;
                written += 1;
            }
            ctx.cpu.write_mem(out, &written.to_le_bytes())?;
            ret(0)
        }
        "type_info_ref" | "type_info_ref_flags" => {
            let index = ctx.arg_u32(1)? as usize;
            let out = ctx.arg_u32(2)?;
            if !output(ctx, out)? {
                return ret(POINTER);
            }
            let Some((reference, flags)) = info.and_then(|info| info.implementations.get(index))
            else {
                return ret(MISSING);
            };
            let value = if ctx.thunk.friendly_name.as_deref() == Some("type_info_ref") {
                *reference
            } else {
                *flags
            };
            ctx.cpu.write_mem(out, &value.to_le_bytes())?;
            ret(0)
        }
        "type_info_ref_info" => {
            let reference = ctx.arg_u32(1)?;
            let out = ctx.arg_u32(2)?;
            if !output(ctx, out)? {
                return ret(POINTER);
            }
            let Kind::TypeInfo { parent, .. } = object.kind else {
                return ret(INVALID);
            };
            let Some(index) = metadata
                .type_offsets
                .iter()
                .position(|offset| *offset == reference)
            else {
                return ret(MISSING);
            };
            type_info(ctx, parent, &object, index, out)
        }
        "type_info_library" => {
            let out = ctx.arg_u32(1)?;
            let index_out = ctx.arg_u32(2)?;
            if !output(ctx, out)? || !output(ctx, index_out)? {
                return ret(POINTER);
            }
            let Kind::TypeInfo { parent, index } = object.kind else {
                return ret(INVALID);
            };
            ctx.kernel
                .ce
                .com
                .automation
                .objects
                .get_mut(&parent)
                .unwrap()
                .refs += 1;
            ctx.cpu.write_mem(out, &parent.to_le_bytes())?;
            ctx.cpu
                .write_mem(index_out, &(index as u32).to_le_bytes())?;
            ret(0)
        }
        "type_lib_documentation" => {
            let index = ctx.arg_u32(1)? as i32;
            let name = if index == -1 {
                metadata.name.clone()
            } else {
                let Some(info) = metadata.types.get(index as usize) else {
                    return ret(MISSING);
                };
                info.name.clone()
            };
            document(ctx, &name)
        }
        "type_info_documentation" => {
            let id = ctx.arg_u32(1)? as i32;
            let Kind::TypeInfo { index, .. } = object.kind else {
                return ret(INVALID);
            };
            let name = if id == -1 {
                metadata.types[index].name.clone()
            } else {
                let Some(function) = info_function(metadata, index, Some(id), None, 0) else {
                    return ret(MISSING);
                };
                function.name
            };
            document(ctx, &name)
        }
        "type_lib_is_name" => {
            let p = ctx.arg_u32(1)?;
            let out = ctx.arg_u32(3)?;
            if !output(ctx, out)? {
                return ret(POINTER);
            }
            let Some(name) = text(ctx, p) else {
                return ret(POINTER);
            };
            let found = metadata.types.iter().any(|t| {
                t.name.eq_ignore_ascii_case(&name)
                    || t.functions
                        .iter()
                        .any(|f| f.name.eq_ignore_ascii_case(&name))
            });
            ctx.cpu.write_mem(out, &u32::from(found).to_le_bytes())?;
            ret(0)
        }
        "type_info_mops" => {
            let out = ctx.arg_u32(2)?;
            ret(if output(ctx, out)? { 0 } else { POINTER })
        }
        _ => ret(0x80004001),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{stub::StubCpu, Cpu};
    use pocket_pe::typelib::{Parameter, TypeInfo};

    fn fixture() -> Arc<Library> {
        Arc::new(Library {
            guid: [0x11; 16],
            lcid: 0,
            syskind: 1,
            flags: 0,
            version: (1, 0),
            name: "Fixture".into(),
            type_offsets: vec![0],
            types: vec![TypeInfo {
                guid: [0x22; 16],
                name: "IFixture".into(),
                kind: 4,
                flags: 0x140,
                version: (1, 0),
                size: 4,
                alignment: 4,
                vtable_size: 32,
                variables: 0,
                implementations: vec![],
                alias: None,
                functions: vec![Function {
                    id: 42,
                    name: "Server".into(),
                    result: TypeDesc {
                        vt: 25,
                        value: 0,
                        child: None,
                    },
                    kind: 4,
                    invoke: 2,
                    callconv: 4,
                    flags: 0,
                    vtable_offset: 28,
                    optional: 0,
                    parameters: vec![Parameter {
                        name: "value".into(),
                        flags: 0x0a,
                        ty: TypeDesc {
                            vt: 26,
                            value: 0,
                            child: Some(Box::new(TypeDesc {
                                vt: 8,
                                value: 0,
                                child: None,
                            })),
                        },
                    }],
                }],
            }],
        })
    }
    fn prepare() -> (StubCpu, pocket_kernel::KernelState, WinCeDispatcher, u32) {
        let (mut cpu, mut kernel, dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        kernel.heap = pocket_kernel::Heap::new(0x50000000, 0x10000);
        kernel.dynamic_exports.insert(
            pocket_kernel::OLE32_MODULE_HANDLE,
            LIB_METHODS
                .iter()
                .chain(INFO_METHODS.iter())
                .enumerate()
                .map(|(index, name)| (name.to_string(), 0x6000 + index as u32 * 32))
                .collect(),
        );
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x70000000,
            iat_va: 0,
            dll: "ole32.dll".into(),
            binding: pocket_pe::ImportBinding::Name("LoadTypeLib".into()),
            friendly_name: Some("LoadTypeLib".into()),
        };
        let library = new_object(
            &mut CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            },
            "fixture".into(),
            fixture(),
            Kind::Library,
        )
        .unwrap();
        (cpu, kernel, dispatcher, library)
    }

    #[test]
    fn type_info_owns_its_library_and_descriptors_preserve_the_32_bit_com_layout() {
        let (mut cpu, mut kernel, mut dispatcher, library) = prepare();
        cpu.write_mem(0x1100, &[0x22; 16]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "type_lib_guid",
                &[library, 0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let info = cpu.read_u32_le(0x1200).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "automation_release",
                &[library]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "type_info_attr",
                &[info, 0x1204]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let attr = cpu.read_u32_le(0x1204).unwrap();
        assert_eq!(cpu.read_mem(attr, 16).unwrap(), [0x22; 16]);
        assert_eq!(
            cpu.read_mem(attr + 44, 16).unwrap(),
            [1, 0, 0, 0, 0, 0, 32, 0, 4, 0, 0x40, 1, 1, 0, 0, 0]
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "type_info_func",
                &[info, 0, 0x1208]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let function = cpu.read_u32_le(0x1208).unwrap();
        assert_eq!(cpu.read_u32_le(function).unwrap(), 42);
        assert_eq!(
            cpu.read_mem(function + 24, 8).unwrap(),
            [1, 0, 0, 0, 28, 0, 0, 0]
        );
        let parameter = cpu.read_u32_le(function + 8).unwrap();
        let nested = cpu.read_u32_le(parameter).unwrap();
        assert_eq!(cpu.read_mem(parameter + 4, 2).unwrap(), 26u16.to_le_bytes());
        assert_eq!(
            cpu.read_mem(parameter + 12, 2).unwrap(),
            10u16.to_le_bytes()
        );
        assert_eq!(cpu.read_mem(nested + 4, 2).unwrap(), 8u16.to_le_bytes());
        for pointer in [attr, function] {
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "automation_release_descriptor",
                &[info, pointer],
            );
            assert_eq!(kernel.heap.msize(pointer), None);
        }
        assert_eq!(kernel.heap.msize(nested), None);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "automation_release",
                &[info]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(kernel.ce.com.automation.objects.is_empty());
        assert!(kernel.ce.com.automation.descriptors.is_empty());
        assert_eq!(kernel.heap.free_bytes(), 0x10000);
    }

    #[test]
    fn unknown_types_clear_outputs_and_library_registration_writes_only_private_metadata() {
        let (mut cpu, mut kernel, mut dispatcher, library) = prepare();
        cpu.write_mem(0x1100, &[0x33; 16]).unwrap();
        cpu.write_mem(0x1200, &0xa5a5a5a5u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "type_lib_guid",
                &[library, 0x1100, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(MISSING)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "type_lib_info",
                &[library, 1000, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(MISSING)
        );
        let path: Vec<_> = r"\Windows\fixture.exe\2"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1400, &path).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "oleaut32.dll",
                "RegisterTypeLib",
                &[library, 0x1400, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            kernel.registry.value(
                r"HKCR\TypeLib\{11111111-1111-1111-1111-111111111111}\1.0\0\win32",
                "default"
            ),
            Some(RegistryValue::Sz(r"\Windows\fixture.exe\2".into()))
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "oleaut32.dll",
                "LoadTypeLibEx",
                &[0x1400, 99, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(INVALID)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 0);
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "ole32.dll",
            "automation_release",
            &[library],
        );
    }
}
