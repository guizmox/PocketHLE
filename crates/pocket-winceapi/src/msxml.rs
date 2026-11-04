//! MSXML3 `IXMLDOMDocument` HLE. `CoCreateInstance(CLSID_DOMDocument, ...)` in
//! `ole32` hands back a synthetic COM object whose vtable slots are PocketHLE
//! dynamic-export thunks; the handlers here drive the owned DOM arena in
//! [`pocket_kernel::msxml`].
//!
//! The vtable offsets match the MSXML3 ABI (`msxml.h`) and were confirmed
//! against the Gizmondo GTShell disassembly (`FUN_000ba408`): the guest walks
//! the tree with `get_attributes@0x44`, `get_childNodes@0x30`,
//! `get_nodeName@0x1c`, `get_nodeType@0x28`, `get_text@0x68`, and reads
//! attributes through a NamedNodeMap with `get_length@0x2c` / `get_item@0x28`.

use pocket_cpu::Prot;
use pocket_kernel::msxml::{ComKind, ComObject, Document, MAX_XML_BYTES};
use pocket_kernel::{DispatchOutcome, KernelError, OLE32_MODULE_HANDLE};

use crate::coredll::read_wstr;
use crate::{CallCtx, WinCeDispatcher};

const DOM_CLASSES: [[u8; 16]; 3] = [
    // DOMDocument, DOMDocument.3.0 and the MSXML3 version-independent class.
    [
        0x90, 0xbf, 0x33, 0x29, 0x36, 0x7b, 0xd2, 0x11, 0xb2, 0x0e, 0x00, 0xc0, 0x4f, 0x98, 0x3e,
        0x60,
    ],
    [
        0x32, 0x8f, 0x07, 0xf5, 0x51, 0xc5, 0xd3, 0x11, 0x89, 0xb9, 0x00, 0x00, 0xf8, 0x1f, 0xe2,
        0x21,
    ],
    [
        0x11, 0x0f, 0xd9, 0xf6, 0x73, 0x9c, 0xd3, 0x11, 0xb3, 0x2e, 0x00, 0xc0, 0x4f, 0x99, 0x0b,
        0xb4,
    ],
];
const IID_UNKNOWN: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
const IID_DISPATCH: [u8; 16] = [0, 4, 2, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
const IID_NODE: [u8; 16] = [
    0x80, 0xbf, 0x33, 0x29, 0x36, 0x7b, 0xd2, 0x11, 0xb2, 0x0e, 0, 0xc0, 0x4f, 0x98, 0x3e, 0x60,
];
const IID_DOCUMENT: [u8; 16] = [
    0x81, 0xbf, 0x33, 0x29, 0x36, 0x7b, 0xd2, 0x11, 0xb2, 0x0e, 0, 0xc0, 0x4f, 0x98, 0x3e, 0x60,
];
const IID_LIST: [u8; 16] = [
    0x82, 0xbf, 0x33, 0x29, 0x36, 0x7b, 0xd2, 0x11, 0xb2, 0x0e, 0, 0xc0, 0x4f, 0x98, 0x3e, 0x60,
];
const IID_MAP: [u8; 16] = [
    0x83, 0xbf, 0x33, 0x29, 0x36, 0x7b, 0xd2, 0x11, 0xb2, 0x0e, 0, 0xc0, 0x4f, 0x98, 0x3e, 0x60,
];

const DEFAULT: &str = "msxml_default";

/// `(vtable_offset, handler_name)` for the four DOM vtables. Any slot not
/// listed reports E_NOTIMPL. GTShell uses the read-only DOM methods below.
const NODE_SLOTS: &[(u32, &str)] = &[
    (0x00, "msxml_node_qi"),
    (0x04, "msxml_addref"),
    (0x08, "msxml_release"),
    (0x1c, "msxml_node_name"),
    (0x20, "msxml_node_value"),
    (0x28, "msxml_node_type"),
    (0x2c, "msxml_node_parent"),
    (0x30, "msxml_node_children"),
    (0x34, "msxml_node_first"),
    (0x38, "msxml_node_last"),
    (0x3c, "msxml_node_previous"),
    (0x40, "msxml_node_next"),
    (0x44, "msxml_node_attrs"),
    (0x58, "msxml_node_haskids"),
    (0x68, "msxml_node_text"),
];

const DOC_EXTRA_SLOTS: &[(u32, &str)] = &[
    (0x00, "msxml_doc_qi"),
    (0xb4, "msxml_doc_docelem"),
    (0xe8, "msxml_doc_load"),
    (0xfc, "msxml_doc_putasync"),
    (0x120, "msxml_doc_putpws"),
];

const LIST_SLOTS: &[(u32, &str)] = &[
    (0x00, "msxml_node_qi"),
    (0x04, "msxml_addref"),
    (0x08, "msxml_release"),
    (0x1c, "msxml_list_item"),
    (0x20, "msxml_list_length"),
    (0x24, "msxml_list_next"),
    (0x28, "msxml_list_reset"),
];

const NNM_SLOTS: &[(u32, &str)] = &[
    (0x00, "msxml_node_qi"),
    (0x04, "msxml_addref"),
    (0x08, "msxml_release"),
    (0x1c, "msxml_nnm_named"),
    (0x28, "msxml_nnm_item"),
    (0x2c, "msxml_nnm_length"),
    (0x38, "msxml_nnm_next"),
    (0x3c, "msxml_nnm_reset"),
];

const DOC_VTABLE_SLOTS: u32 = 0x140 / 4;
const NODE_VTABLE_SLOTS: u32 = 43;
const LIST_VTABLE_SLOTS: u32 = 12;
const MAP_VTABLE_SLOTS: u32 = 17;

pub fn register(d: &mut WinCeDispatcher) {
    // DOM vtable methods live under ole32 so the loader assigns each a dynamic
    // export address we can write into a vtable slot.
    for name in METHOD_NAMES {
        d.register_handler("ole32.dll", name, msxml_method);
    }
    // BSTR primitives GTShell calls around `load`; stubbed to 0 otherwise.
    for dll in ["oleaut32.dll", "coredll.dll"] {
        d.register_handler(dll, "SysAllocString", sys_alloc_string);
        d.register_handler(dll, "SysFreeString", sys_free_string);
        d.register_handler(dll, "SysStringLen", sys_string_len);
        d.register_handler(dll, "VariantInit", variant_init);
        d.register_handler(dll, "VariantClear", variant_clear);
    }
}

/// Every handler name registered under ole32 (vtable thunks + default).
const METHOD_NAMES: &[&str] = &[
    DEFAULT,
    "msxml_addref",
    "msxml_release",
    "msxml_node_qi",
    "msxml_doc_qi",
    "msxml_doc_docelem",
    "msxml_doc_load",
    "msxml_doc_putasync",
    "msxml_doc_putpws",
    "msxml_node_name",
    "msxml_node_value",
    "msxml_node_type",
    "msxml_node_parent",
    "msxml_node_children",
    "msxml_node_first",
    "msxml_node_last",
    "msxml_node_previous",
    "msxml_node_next",
    "msxml_node_attrs",
    "msxml_node_haskids",
    "msxml_node_text",
    "msxml_list_item",
    "msxml_list_length",
    "msxml_list_next",
    "msxml_list_reset",
    "msxml_nnm_named",
    "msxml_nnm_item",
    "msxml_nnm_length",
    "msxml_nnm_next",
    "msxml_nnm_reset",
];

fn export_addr(ctx: &CallCtx<'_>, name: &str) -> u32 {
    ctx.kernel
        .dynamic_exports
        .get(&OLE32_MODULE_HANDLE)
        .and_then(|m| m.get(name).copied())
        .unwrap_or(0)
}

/// Build one vtable on the guest heap: fill every slot with the `DEFAULT`
/// thunk, then override the listed offsets.
fn build_vtable(
    ctx: &mut CallCtx<'_>,
    slots: u32,
    entries: &[(u32, &str)],
) -> Result<u32, KernelError> {
    let default = export_addr(ctx, DEFAULT);
    if default == 0 || entries.iter().any(|(_, name)| export_addr(ctx, name) == 0) {
        return Err(KernelError::Loader(
            "MSXML method exports are missing".into(),
        ));
    }
    let base = allocate(ctx, slots * 4)?;
    if base == 0 {
        return Ok(0);
    }
    let writes = (|| -> Result<(), KernelError> {
        for i in 0..slots {
            ctx.cpu.write_mem(base + i * 4, &default.to_le_bytes())?;
        }
        for (off, name) in entries {
            let a = export_addr(ctx, name);
            ctx.cpu.write_mem(base + off, &a.to_le_bytes())?;
        }
        Ok(())
    })();
    if let Err(error) = writes {
        ctx.kernel.heap.free(base);
        return Err(error);
    }
    Ok(base)
}

/// Build the four vtables once and cache their addresses in kernel state.
fn ensure_vtables(ctx: &mut CallCtx<'_>) -> Result<bool, KernelError> {
    if ctx.kernel.msxml.doc_vtable != 0 {
        return Ok(true);
    }
    let mut doc_entries: Vec<(u32, &str)> = NODE_SLOTS.to_vec();
    doc_entries.extend_from_slice(DOC_EXTRA_SLOTS);
    let mut pointers = Vec::new();
    for (slots, entries) in [
        (DOC_VTABLE_SLOTS, doc_entries.as_slice()),
        (NODE_VTABLE_SLOTS, NODE_SLOTS),
        (LIST_VTABLE_SLOTS, LIST_SLOTS),
        (MAP_VTABLE_SLOTS, NNM_SLOTS),
    ] {
        match build_vtable(ctx, slots, entries) {
            Ok(pointer) if pointer != 0 => pointers.push(pointer),
            result => {
                for pointer in pointers {
                    ctx.kernel.heap.free(pointer);
                }
                return result.map(|_| false);
            }
        }
    }
    ctx.kernel.msxml.doc_vtable = pointers[0];
    ctx.kernel.msxml.node_vtable = pointers[1];
    ctx.kernel.msxml.list_vtable = pointers[2];
    ctx.kernel.msxml.nnm_vtable = pointers[3];
    Ok(true)
}

/// Allocate a guest COM object (`this` = heap(4) holding the vtable ptr) and
/// record what it wraps.
fn new_object(
    ctx: &mut CallCtx<'_>,
    vtable: u32,
    doc: u32,
    kind: ComKind,
) -> Result<u32, KernelError> {
    if matches!(kind, ComKind::Document | ComKind::Node(_)) {
        if let Some((&pointer, object)) = ctx
            .kernel
            .msxml
            .objects
            .iter_mut()
            .find(|(_, object)| object.doc == doc && object.kind == kind)
        {
            object.refs = object.refs.saturating_add(1);
            return Ok(pointer);
        }
    }
    let obj = allocate(ctx, 4)?;
    if obj == 0 {
        return Ok(0);
    }
    if let Err(error) = ctx.cpu.write_mem(obj, &vtable.to_le_bytes()) {
        ctx.kernel.heap.free(obj);
        return Err(error.into());
    }
    ctx.kernel.msxml.objects.insert(
        obj,
        ComObject {
            doc,
            kind,
            iter_pos: 0,
            refs: 1,
            preserve_whitespace: false,
        },
    );
    Ok(obj)
}

/// Wrap a node id as a fresh IXMLDOMNode COM object.
fn wrap_node(ctx: &mut CallCtx<'_>, doc: u32, id: usize) -> Result<u32, KernelError> {
    if id == 0 {
        return new_object(ctx, ctx.kernel.msxml.doc_vtable, doc, ComKind::Document);
    }
    let vt = ctx.kernel.msxml.node_vtable;
    new_object(ctx, vt, doc, ComKind::Node(id))
}

/// Allocate a BSTR (4-byte byte-length prefix, UTF-16, null-terminated) and
/// return the pointer to its first character, as `SysAllocString` would.
fn alloc_bstr(ctx: &mut CallCtx<'_>, s: &str) -> Result<u32, KernelError> {
    let u: Vec<u16> = s.encode_utf16().collect();
    alloc_bstr_units(ctx, &u)
}

fn alloc_bstr_units(ctx: &mut CallCtx<'_>, u: &[u16]) -> Result<u32, KernelError> {
    let byte_len = (u.len() * 2) as u32;
    let p = allocate(ctx, byte_len + 6)?;
    if p == 0 {
        return Ok(0);
    }
    ctx.cpu.write_mem(p, &byte_len.to_le_bytes())?;
    let mut buf = Vec::with_capacity(u.len() * 2 + 2);
    for c in u {
        buf.extend_from_slice(&c.to_le_bytes());
    }
    buf.extend_from_slice(&[0, 0]);
    ctx.cpu.write_mem(p + 4, &buf)?;
    Ok(p + 4)
}

fn write_u32(ctx: &mut CallCtx<'_>, ptr: u32, v: u32) -> Result<(), KernelError> {
    if ptr != 0 {
        ctx.cpu.write_mem(ptr, &v.to_le_bytes())?;
    }
    Ok(())
}

fn allocated_output(
    ctx: &mut CallCtx<'_>,
    out: u32,
    pointer: u32,
) -> Result<DispatchOutcome, KernelError> {
    write_u32(ctx, out, pointer)?;
    Ok(DispatchOutcome::ReturnedR0(if pointer == 0 {
        E_OUTOFMEMORY
    } else {
        S_OK
    }))
}

fn sys_alloc_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let psz = ctx.arg_u32(0)?;
    if psz == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let chars = read_wstr(ctx, psz, 0x10000)?;
    let bstr = alloc_bstr_units(ctx, &chars)?;
    Ok(DispatchOutcome::ReturnedR0(bstr))
}

fn sys_free_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let bstr = ctx.arg_u32(0)?;
    if bstr >= 4 {
        ctx.kernel.heap.free(bstr - 4);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// Only known DOM classes are handled; unrelated COM classes retain OLE's path.
pub fn try_co_create(
    ctx: &mut CallCtx<'_>,
    clsid: &[u8],
    iid: &[u8],
    out: u32,
) -> Result<Option<DispatchOutcome>, KernelError> {
    if !DOM_CLASSES.iter().any(|class| clsid == class) {
        return Ok(None);
    }
    if out == 0 || ctx.cpu.check_guest_access(out, 4, Prot::WRITE).is_err() {
        return Ok(Some(DispatchOutcome::ReturnedR0(E_POINTER)));
    }
    if !supports_interface(ComKind::Document, iid) {
        write_u32(ctx, out, 0)?;
        return Ok(Some(DispatchOutcome::ReturnedR0(E_NOINTERFACE)));
    }
    write_u32(ctx, out, 0)?;
    if !ensure_vtables(ctx)? {
        return Ok(Some(DispatchOutcome::ReturnedR0(E_OUTOFMEMORY)));
    }
    let vt = ctx.kernel.msxml.doc_vtable;
    let Some(doc) = ctx.kernel.msxml.insert_document(Document::empty()) else {
        clear_unused_vtables(ctx);
        return Ok(Some(DispatchOutcome::ReturnedR0(E_OUTOFMEMORY)));
    };
    let obj = new_object(ctx, vt, doc, ComKind::Document)?;
    if obj == 0 {
        write_u32(ctx, out, 0)?;
        ctx.kernel.msxml.collect_document(doc);
        clear_unused_vtables(ctx);
        return Ok(Some(DispatchOutcome::ReturnedR0(E_OUTOFMEMORY)));
    }
    write_u32(ctx, out, obj)?;
    log::info!("CoCreateInstance: created msxml3 DOMDocument object 0x{obj:08x}");
    Ok(Some(DispatchOutcome::ReturnedR0(0)))
}

const S_OK: u32 = 0;
const S_FALSE: u32 = 1;
const E_NOINTERFACE: u32 = 0x8000_4002;
const E_POINTER: u32 = 0x8000_4003;
const E_OUTOFMEMORY: u32 = 0x8007_000e;
const E_NOTIMPL: u32 = 0x8000_4001;

fn allocate(ctx: &mut CallCtx<'_>, size: u32) -> Result<u32, KernelError> {
    match crate::coredll::do_alloc(ctx, size)? {
        DispatchOutcome::ReturnedR0(pointer) => Ok(pointer),
        _ => unreachable!(),
    }
}

fn supports_interface(kind: ComKind, iid: &[u8]) -> bool {
    iid == IID_UNKNOWN
        || iid == IID_DISPATCH
        || match kind {
            ComKind::Document => iid == IID_NODE || iid == IID_DOCUMENT,
            ComKind::Node(_) => iid == IID_NODE,
            ComKind::ChildList(_) => iid == IID_LIST,
            ComKind::AttrList(_) => iid == IID_MAP,
        }
}

fn clear_unused_vtables(ctx: &mut CallCtx<'_>) {
    if !ctx.kernel.msxml.objects.is_empty() {
        return;
    }
    for pointer in [
        ctx.kernel.msxml.doc_vtable,
        ctx.kernel.msxml.node_vtable,
        ctx.kernel.msxml.list_vtable,
        ctx.kernel.msxml.nnm_vtable,
    ] {
        ctx.kernel.heap.free(pointer);
    }
    ctx.kernel.msxml.doc_vtable = 0;
    ctx.kernel.msxml.node_vtable = 0;
    ctx.kernel.msxml.list_vtable = 0;
    ctx.kernel.msxml.nnm_vtable = 0;
    ctx.kernel.msxml.documents.clear();
}

fn sys_string_len(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let length = if pointer == 0 {
        0
    } else {
        ctx.cpu.read_u32_le(pointer - 4)? / 2
    };
    Ok(DispatchOutcome::ReturnedR0(length))
}

fn variant_init(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    if pointer != 0 {
        ctx.cpu.write_mem(pointer, &[0; 16])?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn variant_clear(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    if pointer == 0
        || ctx
            .cpu
            .check_guest_access(pointer, 16, Prot::WRITE)
            .is_err()
    {
        return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
    }
    let kind = ctx.cpu.read_u16_le(pointer)?;
    if kind == 8 {
        let string = ctx.cpu.read_u32_le(pointer + 8)?;
        if string >= 4 {
            ctx.kernel.heap.free(string - 4);
        }
    } else if kind != 0 && kind != 1 {
        return Ok(DispatchOutcome::ReturnedR0(E_NOTIMPL));
    }
    ctx.cpu.write_mem(pointer, &[0; 16])?;
    Ok(DispatchOutcome::ReturnedR0(S_OK))
}

/// Resolve the arena node id a COM object stands for (the `#document` node is
/// id 0). Lists/maps have no single node and return `None`.
fn node_of(kind: ComKind) -> Option<usize> {
    match kind {
        ComKind::Document => Some(0),
        ComKind::Node(id) => Some(id),
        _ => None,
    }
}

/// Shared entry point for every DOM vtable thunk; dispatches on the slot's
/// handler name captured in `ctx.thunk.friendly_name`.
fn msxml_method(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let name = crate::import_name(ctx.thunk).into_owned();
    let this = ctx.arg_u32(0)?;
    let co = ctx.kernel.msxml.objects.get(&this).copied();

    if name == DEFAULT {
        return Ok(DispatchOutcome::ReturnedR0(E_NOTIMPL));
    }
    if co.is_none() && name != "msxml_addref" && name != "msxml_release" {
        return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
    }
    let output = match name.as_str() {
        "msxml_doc_load" => Some((5, 2)),
        "msxml_node_qi" | "msxml_doc_qi" | "msxml_list_item" | "msxml_nnm_item"
        | "msxml_nnm_named" => Some((2, 4)),
        "msxml_node_value" => Some((1, 16)),
        "msxml_node_haskids" => Some((1, 2)),
        "msxml_doc_docelem"
        | "msxml_node_name"
        | "msxml_node_type"
        | "msxml_node_text"
        | "msxml_node_parent"
        | "msxml_node_first"
        | "msxml_node_last"
        | "msxml_node_previous"
        | "msxml_node_next"
        | "msxml_node_children"
        | "msxml_node_attrs"
        | "msxml_list_length"
        | "msxml_nnm_length"
        | "msxml_list_next"
        | "msxml_nnm_next" => Some((1, 4)),
        _ => None,
    };
    if let Some((argument, size)) = output {
        let pointer = ctx.arg_u32(argument)?;
        if pointer == 0
            || ctx
                .cpu
                .check_guest_access(pointer, size, Prot::WRITE)
                .is_err()
        {
            return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
        }
    }

    match name.as_str() {
        "msxml_addref" => {
            let count = ctx
                .kernel
                .msxml
                .objects
                .get_mut(&this)
                .map(|object| {
                    object.refs = object.refs.saturating_add(1);
                    object.refs
                })
                .unwrap_or(0);
            return Ok(DispatchOutcome::ReturnedR0(count));
        }
        "msxml_release" => {
            if let Some(c) = co {
                if c.refs > 1 {
                    ctx.kernel.msxml.objects.get_mut(&this).unwrap().refs -= 1;
                    return Ok(DispatchOutcome::ReturnedR0(c.refs - 1));
                }
                ctx.kernel.msxml.objects.remove(&this);
                ctx.kernel.heap.free(this);
                ctx.kernel.msxml.collect_document(c.doc);
                clear_unused_vtables(ctx);
            }
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        "msxml_node_qi" | "msxml_doc_qi" => {
            let ppv = ctx.arg_u32(2)?;
            let iid = ctx.arg_u32(1)?;
            if ppv == 0 || ctx.cpu.check_guest_access(ppv, 4, Prot::WRITE).is_err() {
                return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
            }
            write_u32(ctx, ppv, 0)?;
            if iid == 0 {
                return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
            }
            let iid = ctx.cpu.read_mem(iid, 16)?;
            if co.is_some_and(|object| supports_interface(object.kind, &iid)) {
                let object = ctx.kernel.msxml.objects.get_mut(&this).unwrap();
                object.refs = object.refs.saturating_add(1);
                write_u32(ctx, ppv, this)?;
                return Ok(DispatchOutcome::ReturnedR0(S_OK));
            }
            return Ok(DispatchOutcome::ReturnedR0(E_NOINTERFACE));
        }
        "msxml_doc_putasync" => {
            return Ok(DispatchOutcome::ReturnedR0(if ctx.arg_u32(1)? == 0 {
                S_OK
            } else {
                E_NOTIMPL
            }));
        }
        "msxml_doc_putpws" => {
            let preserve = ctx.arg_u32(1)? != 0;
            let Some(object) = ctx.kernel.msxml.objects.get_mut(&this) else {
                return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
            };
            object.preserve_whitespace = preserve;
            return Ok(DispatchOutcome::ReturnedR0(S_OK));
        }
        "msxml_doc_load" => return doc_load(ctx, co),
        "msxml_doc_docelem" => return doc_document_element(ctx, co),
        _ => {}
    }

    // Node / list / map getters below all need the backing object.
    let Some(co) = co else {
        return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
    };
    node_or_list_method(ctx, &name, this, co)
}

fn doc_load(ctx: &mut CallCtx<'_>, co: Option<ComObject>) -> Result<DispatchOutcome, KernelError> {
    // load(VARIANT source /*r1..r3,stack[0]*/, VARIANT_BOOL* isSuccessful).
    // The BSTR filename is VARIANT.bstrVal in r3 (arg3); the out ptr is the
    // stack argument after the by-value VARIANT (arg5).
    let this = ctx.arg_u32(0)?;
    let kind = ctx.arg_u32(1)? & 0xffff;
    let bstr = ctx.arg_u32(3)?;
    let out = ctx.arg_u32(5)?;
    let Some(co) = co else {
        return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
    };
    if out == 0 || ctx.cpu.check_guest_access(out, 2, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(E_POINTER));
    }
    ctx.cpu.write_mem(out, &0u16.to_le_bytes())?;
    if kind != 8 || co.kind != ComKind::Document {
        return Ok(DispatchOutcome::ReturnedR0(0x8002_0005));
    }
    let mut ok = false;
    if bstr != 0 {
        let path = String::from_utf16_lossy(&read_wstr(ctx, bstr, 0x10000)?);
        if let Some(bytes) = read_xml_file(&mut ctx.kernel.vfs, &path) {
            ok = decode_xml_bytes(&bytes)
                .is_some_and(|text| ctx.kernel.msxml.load_document(this, &text));
            log::info!(
                "msxml load \"{path}\" -> {}",
                if ok { "ok" } else { "parse failed" }
            );
        } else {
            log::warn!("msxml load \"{path}\": VFS read failed or file exceeds size limit");
        }
    }
    // VARIANT_BOOL: TRUE = 0xFFFF, FALSE = 0.
    if out != 0 {
        let v: u16 = if ok { 0xFFFF } else { 0 };
        ctx.cpu.write_mem(out, &v.to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(S_OK))
}

fn doc_document_element(
    ctx: &mut CallCtx<'_>,
    co: Option<ComObject>,
) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(1)?;
    let Some(co) = co else {
        write_u32(ctx, out, 0)?;
        return Ok(DispatchOutcome::ReturnedR0(S_FALSE));
    };
    let root = ctx.kernel.msxml.doc(co.doc).and_then(|d| d.root_element);
    match root {
        Some(id) => {
            let obj = wrap_node(ctx, co.doc, id)?;
            allocated_output(ctx, out, obj)
        }
        None => {
            write_u32(ctx, out, 0)?;
            Ok(DispatchOutcome::ReturnedR0(S_FALSE))
        }
    }
}

/// Read through VFS so RAM files, overlays and share-denied opens retain their
/// normal guest contracts. Always close the temporary handle, including errors.
fn read_xml_file(vfs: &mut pocket_kernel::vfs::Vfs, path: &str) -> Option<Vec<u8>> {
    let handle = vfs.open_file(path, 1, 3, 3, false, 0).ok()?.handle;
    let result = (|| {
        let length = usize::try_from(vfs.size(handle)?).ok()?;
        if length > MAX_XML_BYTES {
            return None;
        }
        let mut bytes = vec![0; length];
        let mut position = 0;
        while position < length {
            let count = vfs.read(handle, &mut bytes[position..])?;
            if count == 0 {
                return None;
            }
            position += count;
        }
        Some(bytes)
    })();
    vfs.close(handle);
    result
}

/// Decode UTF-16 with a BOM or UTF-8; invalid byte sequences fail the load.
fn decode_xml_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        if bytes.len() % 2 != 0 {
            return None;
        }
        let u: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&u).ok()
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        if bytes.len() % 2 != 0 {
            return None;
        }
        let u: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&u).ok()
    } else {
        std::str::from_utf8(bytes)
            .ok()
            .map(|text| text.trim_start_matches('\u{feff}').to_string())
    }
}

fn node_or_list_method(
    ctx: &mut CallCtx<'_>,
    name: &str,
    this: u32,
    co: ComObject,
) -> Result<DispatchOutcome, KernelError> {
    let doc = co.doc;
    let nid = node_of(co.kind);
    match name {
        "msxml_node_name" => {
            let out = ctx.arg_u32(1)?;
            let s = nid
                .and_then(|id| ctx.kernel.msxml.node(doc, id))
                .map(|n| n.name.clone())
                .unwrap_or_default();
            let b = alloc_bstr(ctx, &s)?;
            allocated_output(ctx, out, b)
        }
        "msxml_node_type" => {
            let out = ctx.arg_u32(1)?;
            let t = nid
                .and_then(|id| ctx.kernel.msxml.node(doc, id))
                .map(|n| n.node_type as u32)
                .unwrap_or(0);
            write_u32(ctx, out, t)?;
            Ok(DispatchOutcome::ReturnedR0(S_OK))
        }
        "msxml_node_text" => {
            let out = ctx.arg_u32(1)?;
            let s = nid
                .map(|id| ctx.kernel.msxml.text_of(doc, id))
                .unwrap_or_default();
            let b = alloc_bstr(ctx, &s)?;
            allocated_output(ctx, out, b)
        }
        "msxml_node_value" => {
            // get_nodeValue(VARIANT* out). Only attribute/text/CDATA carry a
            // scalar value; everything else is VT_NULL.
            let out = ctx.arg_u32(1)?;
            let val = nid
                .and_then(|id| ctx.kernel.msxml.node(doc, id))
                .and_then(|n| match n.node_type {
                    pocket_kernel::msxml::NODE_ATTRIBUTE
                    | pocket_kernel::msxml::NODE_TEXT
                    | pocket_kernel::msxml::NODE_CDATA => Some(n.value.clone()),
                    _ => None,
                });
            if out != 0 {
                ctx.cpu.write_mem(out, &[0u8; 16])?;
                if let Some(s) = val {
                    let b = alloc_bstr(ctx, &s)?;
                    if b == 0 {
                        return Ok(DispatchOutcome::ReturnedR0(E_OUTOFMEMORY));
                    }
                    ctx.cpu.write_mem(out, &8u16.to_le_bytes())?; // VT_BSTR
                    ctx.cpu.write_mem(out + 8, &b.to_le_bytes())?;
                } else {
                    ctx.cpu.write_mem(out, &1u16.to_le_bytes())?; // VT_NULL
                }
            }
            Ok(DispatchOutcome::ReturnedR0(S_OK))
        }
        "msxml_node_haskids" => {
            let out = ctx.arg_u32(1)?;
            let has = nid
                .and_then(|id| ctx.kernel.msxml.node(doc, id))
                .map(|n| !n.children.is_empty())
                .unwrap_or(false);
            if out != 0 {
                let v: u16 = if has { 0xFFFF } else { 0 };
                ctx.cpu.write_mem(out, &v.to_le_bytes())?;
            }
            Ok(DispatchOutcome::ReturnedR0(S_OK))
        }
        "msxml_node_parent"
        | "msxml_node_first"
        | "msxml_node_last"
        | "msxml_node_next"
        | "msxml_node_previous" => {
            let out = ctx.arg_u32(1)?;
            let target = relative_node(ctx, doc, nid, name);
            match target {
                Some(id) => {
                    let obj = wrap_node(ctx, doc, id)?;
                    allocated_output(ctx, out, obj)
                }
                None => {
                    write_u32(ctx, out, 0)?;
                    Ok(DispatchOutcome::ReturnedR0(S_FALSE))
                }
            }
        }
        "msxml_node_children" => {
            let out = ctx.arg_u32(1)?;
            let vt = ctx.kernel.msxml.list_vtable;
            let obj = match nid {
                Some(id) => new_object(ctx, vt, doc, ComKind::ChildList(id))?,
                None => 0,
            };
            allocated_output(ctx, out, obj)
        }
        "msxml_node_attrs" => {
            let out = ctx.arg_u32(1)?;
            let is_elem = nid
                .and_then(|id| ctx.kernel.msxml.node(doc, id))
                .map(|n| n.node_type == pocket_kernel::msxml::NODE_ELEMENT)
                .unwrap_or(false);
            if is_elem {
                let vt = ctx.kernel.msxml.nnm_vtable;
                let obj = new_object(ctx, vt, doc, ComKind::AttrList(nid.unwrap()))?;
                allocated_output(ctx, out, obj)
            } else {
                write_u32(ctx, out, 0)?;
                Ok(DispatchOutcome::ReturnedR0(S_FALSE))
            }
        }
        "msxml_list_length" | "msxml_nnm_length" => {
            let out = ctx.arg_u32(1)?;
            let len = members(ctx, co).len() as u32;
            write_u32(ctx, out, len)?;
            Ok(DispatchOutcome::ReturnedR0(S_OK))
        }
        "msxml_list_item" | "msxml_nnm_item" => {
            let index = ctx.arg_u32(1)? as usize;
            let out = ctx.arg_u32(2)?;
            let target = members(ctx, co).get(index).copied();
            match target {
                Some(id) => {
                    let obj = wrap_node(ctx, doc, id)?;
                    allocated_output(ctx, out, obj)
                }
                None => {
                    write_u32(ctx, out, 0)?;
                    Ok(DispatchOutcome::ReturnedR0(S_FALSE))
                }
            }
        }
        "msxml_list_next" | "msxml_nnm_next" => {
            let out = ctx.arg_u32(1)?;
            let pos = ctx
                .kernel
                .msxml
                .objects
                .get(&this)
                .map(|o| o.iter_pos)
                .unwrap_or(0);
            let target = members(ctx, co).get(pos).copied();
            if let Some(o) = ctx.kernel.msxml.objects.get_mut(&this) {
                o.iter_pos = pos + 1;
            }
            match target {
                Some(id) => {
                    let obj = wrap_node(ctx, doc, id)?;
                    allocated_output(ctx, out, obj)
                }
                None => {
                    write_u32(ctx, out, 0)?;
                    Ok(DispatchOutcome::ReturnedR0(S_FALSE))
                }
            }
        }
        "msxml_list_reset" | "msxml_nnm_reset" => {
            if let Some(o) = ctx.kernel.msxml.objects.get_mut(&this) {
                o.iter_pos = 0;
            }
            Ok(DispatchOutcome::ReturnedR0(S_OK))
        }
        "msxml_nnm_named" => {
            // getNamedItem(BSTR name, IXMLDOMNode** out).
            let name_ptr = ctx.arg_u32(1)?;
            let out = ctx.arg_u32(2)?;
            let wanted = if name_ptr != 0 {
                String::from_utf16_lossy(&read_wstr(ctx, name_ptr, 0x10000)?)
            } else {
                String::new()
            };
            let found = members(ctx, co).into_iter().find(|&id| {
                ctx.kernel
                    .msxml
                    .node(doc, id)
                    .map(|n| n.name == wanted)
                    .unwrap_or(false)
            });
            match found {
                Some(id) => {
                    let obj = wrap_node(ctx, doc, id)?;
                    allocated_output(ctx, out, obj)
                }
                None => {
                    write_u32(ctx, out, 0)?;
                    Ok(DispatchOutcome::ReturnedR0(S_FALSE))
                }
            }
        }
        _ => Ok(DispatchOutcome::ReturnedR0(E_NOTIMPL)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{stub::StubCpu, Cpu};
    use pocket_kernel::KernelState;

    fn setup() -> (StubCpu, KernelState, WinCeDispatcher) {
        let (mut cpu, mut kernel, dispatcher) = crate::bluetooth::tests::setup();
        cpu.map_region(0x5000_0000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        kernel.dynamic_exports.insert(
            OLE32_MODULE_HANDLE,
            METHOD_NAMES
                .iter()
                .enumerate()
                .map(|(index, &name)| (name.to_string(), 0x7001_0000 + index as u32 * 32))
                .collect(),
        );
        (cpu, kernel, dispatcher)
    }

    fn invoke(
        cpu: &mut StubCpu,
        kernel: &mut KernelState,
        dispatcher: &mut WinCeDispatcher,
        name: &str,
        args: &[u32],
    ) -> u32 {
        let DispatchOutcome::ReturnedR0(value) =
            crate::bluetooth::tests::call(cpu, kernel, dispatcher, "ole32.dll", name, args)
        else {
            panic!("unexpected continuation")
        };
        value
    }

    fn document(
        cpu: &mut StubCpu,
        kernel: &mut KernelState,
        dispatcher: &mut WinCeDispatcher,
    ) -> u32 {
        cpu.write_mem(0x1000, &DOM_CLASSES[0]).unwrap();
        cpu.write_mem(0x1020, &IID_DOCUMENT).unwrap();
        assert_eq!(
            invoke(
                cpu,
                kernel,
                dispatcher,
                "CoCreateInstance",
                &[0x1000, 0, 1, 0x1020, 0x1040]
            ),
            S_OK
        );
        cpu.read_u32_le(0x1040).unwrap()
    }

    fn string(
        cpu: &mut StubCpu,
        kernel: &mut KernelState,
        dispatcher: &mut WinCeDispatcher,
        text: &[u16],
    ) -> u32 {
        let bytes: Vec<_> = text
            .iter()
            .chain(std::iter::once(&0))
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        cpu.write_mem(0x2000, &bytes).unwrap();
        let DispatchOutcome::ReturnedR0(pointer) = crate::bluetooth::tests::call(
            cpu,
            kernel,
            dispatcher,
            "coredll.dll",
            "SysAllocString",
            &[0x2000],
        ) else {
            panic!("unexpected continuation")
        };
        pointer
    }

    #[test]
    fn arm_by_value_variant_loads_unicode_skin_without_overwriting_bool_guard() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let directory = tempfile::tempdir().unwrap();
        let mut xml = vec![0xff, 0xfe];
        xml.extend(
            "<THEME name='Русский &amp; Français'><Label>Меню 🙂</Label></THEME>"
                .encode_utf16()
                .flat_map(|unit| unit.to_le_bytes()),
        );
        std::fs::write(directory.path().join("Skin.xml"), xml).unwrap();
        kernel.vfs.mount("\\Flash Disk2\\", directory.path());
        let doc = document(&mut cpu, &mut kernel, &mut dispatcher);
        let filename = string(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            &"\\flash disk2\\skin.xml".encode_utf16().collect::<Vec<_>>(),
        );
        cpu.write_mem(0x1180, &[0xcc; 8]).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "msxml_doc_load",
                &[doc, 8, 0, filename, 0xdead_beef, 0x1180]
            ),
            S_OK
        );
        assert_eq!(
            cpu.read_mem(0x1180, 8).unwrap(),
            [0xff, 0xff, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc]
        );
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "msxml_doc_docelem",
                &[doc, 0x1040]
            ),
            S_OK
        );
        let root = cpu.read_u32_le(0x1040).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "msxml_node_text",
                &[root, 0x1040]
            ),
            S_OK
        );
        let bstr = cpu.read_u32_le(0x1040).unwrap();
        let count = cpu.read_u32_le(bstr - 4).unwrap();
        let bytes = cpu.read_mem(bstr, count).unwrap();
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "Меню 🙂");
    }

    #[test]
    fn returned_nodes_survive_document_release_and_one_hundred_cycles_refund_heap() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let before = kernel.heap.free_bytes();
        for _ in 0..100 {
            let doc = document(&mut cpu, &mut kernel, &mut dispatcher);
            assert!(kernel
                .msxml
                .load_document(doc, "<THEME title='menu'><Label>hello</Label></THEME>"));
            assert_eq!(
                invoke(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "msxml_doc_docelem",
                    &[doc, 0x1040]
                ),
                S_OK
            );
            let root = cpu.read_u32_le(0x1040).unwrap();
            assert_eq!(
                invoke(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "msxml_release",
                    &[doc]
                ),
                0
            );
            assert_eq!(
                invoke(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "msxml_node_attrs",
                    &[root, 0x1040]
                ),
                S_OK
            );
            let map = cpu.read_u32_le(0x1040).unwrap();
            let vtable = cpu.read_u32_le(map).unwrap();
            assert_eq!(
                cpu.read_u32_le(vtable + 0x38).unwrap(),
                kernel.dynamic_exports[&OLE32_MODULE_HANDLE]["msxml_nnm_next"]
            );
            assert_eq!(
                cpu.read_u32_le(vtable + 0x3c).unwrap(),
                kernel.dynamic_exports[&OLE32_MODULE_HANDLE]["msxml_nnm_reset"]
            );
            assert_eq!(
                invoke(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "msxml_nnm_next",
                    &[map, 0x1040]
                ),
                S_OK
            );
            let attribute = cpu.read_u32_le(0x1040).unwrap();
            assert_eq!(
                invoke(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "msxml_node_value",
                    &[attribute, 0x1180]
                ),
                S_OK
            );
            assert_eq!(cpu.read_u16_le(0x1180).unwrap(), 8);
            let DispatchOutcome::ReturnedR0(value) = crate::bluetooth::tests::call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "VariantClear",
                &[0x1180],
            ) else {
                panic!("unexpected continuation")
            };
            assert_eq!(value, S_OK);
            for pointer in [root, map, attribute] {
                assert_eq!(
                    invoke(
                        &mut cpu,
                        &mut kernel,
                        &mut dispatcher,
                        "msxml_release",
                        &[pointer]
                    ),
                    0
                );
            }
            assert!(kernel.msxml.objects.is_empty());
            assert!(kernel.msxml.documents.is_empty());
            assert_eq!(kernel.heap.free_bytes(), before);
        }
    }

    #[test]
    fn bstr_copies_utf16_units_and_variant_clear_releases_the_allocation() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let before = kernel.heap.free_bytes();
        let units = [0x0041, 0xd800, 0x0042]; // Preserve even an unpaired surrogate.
        let bstr = string(&mut cpu, &mut kernel, &mut dispatcher, &units);
        assert_eq!(cpu.read_u32_le(bstr - 4).unwrap(), 6);
        assert_eq!(
            cpu.read_mem(bstr, 8).unwrap(),
            [0x41, 0, 0, 0xd8, 0x42, 0, 0, 0]
        );
        cpu.write_mem(0x1180, &[0; 16]).unwrap();
        cpu.write_mem(0x1180, &8u16.to_le_bytes()).unwrap();
        cpu.write_mem(0x1188, &bstr.to_le_bytes()).unwrap();
        crate::bluetooth::tests::call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "VariantClear",
            &[0x1180],
        );
        assert_eq!(cpu.read_mem(0x1180, 16).unwrap(), [0; 16]);
        assert_eq!(kernel.heap.free_bytes(), before);
    }

    #[test]
    fn unrelated_classes_bad_iids_and_invalid_outputs_do_not_create_dom_objects() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.write_mem(0x1000, &[0x99; 16]).unwrap();
        cpu.write_mem(0x1020, &IID_DOCUMENT).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "CoCreateInstance",
                &[0x1000, 0, 1, 0x1020, 0x1040]
            ),
            E_NOINTERFACE
        );
        cpu.write_mem(0x1000, &DOM_CLASSES[0]).unwrap();
        cpu.write_mem(0x1020, &[0x99; 16]).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "CoCreateInstance",
                &[0x1000, 0, 1, 0x1020, 0x1040]
            ),
            E_NOINTERFACE
        );
        cpu.write_mem(0x1020, &IID_DOCUMENT).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "CoCreateInstance",
                &[0x1000, 0, 1, 0x1020, 0x8000]
            ),
            E_POINTER
        );
        assert!(kernel.msxml.objects.is_empty());
        let doc = document(&mut cpu, &mut kernel, &mut dispatcher);
        let before = kernel.heap.free_bytes();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "msxml_node_name",
                &[doc, 0x8000]
            ),
            E_POINTER
        );
        assert_eq!(kernel.heap.free_bytes(), before);
    }

    #[test]
    fn xml_reads_honor_share_denial_and_close_temporary_handles() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("Skin.xml"), b"<THEME/>").unwrap();
        let mut vfs = pocket_kernel::vfs::Vfs::new();
        vfs.mount("\\Flash Disk2\\", directory.path());
        let writer = vfs
            .open_file("\\Flash Disk2\\Skin.xml", 1, 0, 3, false, 0)
            .unwrap()
            .handle;
        assert!(read_xml_file(&mut vfs, "\\Flash Disk2\\Skin.xml").is_none());
        assert!(vfs.close(writer));
        assert_eq!(
            read_xml_file(&mut vfs, "\\Flash Disk2\\Skin.xml").unwrap(),
            b"<THEME/>"
        );
        let exclusive = vfs
            .open_file("\\Flash Disk2\\Skin.xml", 1, 0, 3, false, 0)
            .unwrap()
            .handle;
        assert!(vfs.close(exclusive));
    }
}

/// child/attribute members of the list/map object `co`.
fn members(ctx: &CallCtx<'_>, co: ComObject) -> Vec<usize> {
    let doc = co.doc;
    let pid = match co.kind {
        ComKind::ChildList(p) => Some((p, false)),
        ComKind::AttrList(p) => Some((p, true)),
        _ => None,
    };
    match pid.and_then(|(p, attr)| ctx.kernel.msxml.node(doc, p).map(|n| (n, attr))) {
        Some((n, true)) => n.attributes.clone(),
        Some((n, false)) => n.children.clone(),
        None => Vec::new(),
    }
}

/// Resolve parentNode / firstChild / lastChild / nextSibling for `nid`.
fn relative_node(ctx: &CallCtx<'_>, doc: u32, nid: Option<usize>, which: &str) -> Option<usize> {
    let id = nid?;
    let n = ctx.kernel.msxml.node(doc, id)?;
    match which {
        "msxml_node_parent" => n.parent,
        "msxml_node_first" => n.children.first().copied(),
        "msxml_node_last" => n.children.last().copied(),
        "msxml_node_next" => {
            let parent = n.parent?;
            let p = ctx.kernel.msxml.node(doc, parent)?;
            let pos = p.children.iter().position(|&c| c == id)?;
            p.children.get(pos + 1).copied()
        }
        "msxml_node_previous" => {
            let parent = n.parent?;
            let p = ctx.kernel.msxml.node(doc, parent)?;
            let pos = p.children.iter().position(|&c| c == id)?;
            p.children.get(pos.checked_sub(1)?).copied()
        }
        _ => None,
    }
}
