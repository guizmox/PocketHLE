//! Opt-in CE system-image metadata. Guest system DLL code remains HLE.
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub mod device;

/// System CPL metadata is mapped read-only; only these HLE exports execute.
pub const CPL_MODULES: &[(&str, u32)] = &[("cplmain.cpl", 0x1000_0020)];

#[derive(Debug)]
pub struct Settings {
    pub key_repeat_delay_ms: u64,
    pub key_repeat_interval_ms: u64,
    pub(crate) audio: Option<(u32, u32)>,
    pub(crate) sip_visible: Option<bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            key_repeat_delay_ms: 400,
            key_repeat_interval_ms: 33,
            audio: None,
            sip_visible: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ControlPanel {
    pub hwnd: u32,
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub applet: usize,
    pub previous_active: u32,
    pub previous_focus: u32,
    pub fields: Vec<u32>,
    pub sound_schemes: Vec<String>,
    pub tab: u32,
    pub pages: Vec<Vec<u32>>,
    pub selected_page: usize,
    pub status: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOperation {
    Message,
    DestroyWindow,
}

#[derive(Debug)]
pub struct PendingSend {
    pub id: u64,
    pub sender: usize,
    pub thunk: u32,
    pub sp: u32,
    pub output: u32,
    pub recipient: usize,
    pub args: [u32; 4],
    pub operation: SendOperation,
    pub deadline: Instant,
    pub result: Option<Result<u32, u32>>,
    pub delivered: bool,
}

#[derive(Debug)]
pub struct SendCallback {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub pending: Option<u64>,
    pub output: u32,
    pub builtin: bool,
    pub operation: SendOperation,
}

#[derive(Debug, Default)]
pub struct SendMessages {
    pub next_id: u64,
    pub pending: Vec<PendingSend>,
    pub callbacks: Vec<SendCallback>,
}

#[derive(Debug)]
pub struct SipEnumeration {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub scratch: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum ComStage {
    Loading,
    Factory,
    Creating,
    Releasing,
}

#[derive(Debug)]
pub struct ComActivation {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub output: u32,
    pub outer: u32,
    pub scratch: u32,
    pub server: String,
    pub create: bool,
    pub stage: ComStage,
    pub factory: u32,
    pub result: u32,
    pub value: u32,
}

#[derive(Debug, Default)]
pub struct ComState {
    pub automation: crate::automation::State,
    pub streams: crate::com_stream::Streams,
    pub activations: Vec<ComActivation>,
    pub runs: Vec<ComRun>,
    pub callbacks: Vec<ComCallback>,
    pub bind_contexts: HashMap<u32, BindContext>,
    /// COM keeps loaded servers until process teardown, independently of an
    /// individual factory's reference count.
    pub servers: HashMap<String, u32>,
}

#[derive(Debug, Clone, Copy)]
pub enum ComRunStage {
    Query,
    Run,
    Release,
}

#[derive(Debug)]
pub struct ComRun {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub scratch: u32,
    pub object: u32,
    pub stage: ComRunStage,
    pub result: u32,
}

#[derive(Debug)]
pub struct BindContext {
    pub vtable: u32,
    pub refs: u32,
    pub options: [u32; 8],
    pub bound: Vec<u32>,
    pub params: HashMap<String, u32>,
}

#[derive(Debug)]
pub struct ComCallback {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub calls: Vec<(u32, u32)>,
    pub result: u32,
    pub free: Vec<u32>,
}

#[derive(Debug)]
pub struct FocusChange {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub previous: u32,
    pub messages: Vec<(u32, u32, u32)>,
}

#[derive(Debug)]
pub struct ShellFolderCall {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub scratch: u32,
}

#[derive(Debug, Default)]
pub struct ShellWindows {
    pub desktop: Option<u32>,
    pub taskbar: Option<(u32, bool)>,
    pub started: HashSet<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PropertyName {
    Atom(u16),
    Text(Vec<u16>),
}

#[derive(Debug, Default)]
pub struct WindowBatch {
    pub parent: Option<u32>,
    pub positions: Vec<[u32; 7]>,
}

#[derive(Debug, Default)]
pub struct WindowAux {
    pub properties: HashMap<(u32, PropertyName), u32>,
    pub batches: HashMap<u32, WindowBatch>,
    pub dialog_units: HashMap<u32, [i32; 2]>,
}

#[derive(Debug)]
pub struct DebugZones {
    pub address: u32,
    pub name: String,
    pub zones: [String; 16],
    pub mask: u32,
}

#[derive(Debug, Default)]
pub struct State {
    pub device: device::Context,
    pub power_calls: HashMap<(usize, u32, u32), u64>,
    pub suspend_observation: Option<(u64, usize)>,
    pub version: Option<(u32, u32, u32)>,
    pub module_versions: HashMap<u32, u32>,
    pub debug_zones: HashMap<u32, DebugZones>,
    pub com: ComState,
    pub carets: crate::caret::Carets,
    pub focus_changes: Vec<FocusChange>,
    pub shell_folder_calls: Vec<ShellFolderCall>,
    pub ordinals: HashMap<String, HashMap<u16, String>>,
    pub system_modules: HashSet<String>,
    /// Logical LoadLibrary references; HLE code/export mappings stay pinned.
    pub hle_module_refs: HashMap<u32, u32>,
    pub windows: crate::windowing::ClassicWindows,
    pub window_aux: WindowAux,
    pub window_icons: HashMap<u32, [u32; 2]>,
    pub send_messages: SendMessages,
    pub control_panel: Option<ControlPanel>,
    pub settings: Settings,
    pub sounds: crate::system_sound::SoundState,
    pub shell: ShellWindows,
    pub clipboard: crate::clipboard::Client,
    pub sip_enumerations: Vec<SipEnumeration>,
    pub api_waits: HashMap<(usize, u32, u32), (u32, Instant)>,
    pub(crate) inline_ids: Option<(u32, u32)>,
}
