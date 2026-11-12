//! Private offscreen contexts, confined to the emulation thread.
#[cfg(unix)]
use std::ffi::CString;
use std::{
    ffi::{c_char, c_void, CStr},
    marker::PhantomData,
    rc::Rc,
};

type Ptr = *mut c_void;
pub struct Library(Ptr);
// A loader handle is process-wide and symbol lookup does not mutate it.
// GL contexts themselves remain !Send/!Sync through Host's Rc marker.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}
impl Library {
    pub fn open(name: &str) -> Result<Self, String> {
        #[cfg(unix)]
        unsafe {
            let name = CString::new(name).unwrap();
            let p = dlopen(name.as_ptr(), 2);
            if p.is_null() {
                Err(format!("cannot load {name:?}"))
            } else {
                Ok(Self(p))
            }
        }
        #[cfg(windows)]
        unsafe {
            let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let p = LoadLibraryW(name.as_ptr());
            if p.is_null() {
                Err("cannot load OpenGL driver".into())
            } else {
                Ok(Self(p))
            }
        }
    }
    pub fn symbol(&self, name: &CStr) -> Ptr {
        #[cfg(unix)]
        unsafe {
            dlsym(self.0, name.as_ptr())
        }
        #[cfg(windows)]
        unsafe {
            GetProcAddress(self.0, name.as_ptr())
        }
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            #[cfg(unix)]
            {
                dlclose(self.0);
            }
            #[cfg(windows)]
            {
                FreeLibrary(self.0);
            }
        }
    }
}
#[cfg(unix)]
#[link(name = "dl")]
extern "C" {
    fn dlopen(n: *const c_char, flags: i32) -> Ptr;
    fn dlsym(l: Ptr, n: *const c_char) -> Ptr;
    fn dlclose(l: Ptr) -> i32;
}
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(n: *const u16) -> Ptr;
    fn GetProcAddress(l: Ptr, n: *const c_char) -> Ptr;
    fn FreeLibrary(l: Ptr) -> i32;
}

#[cfg(unix)]
mod native {
    use super::*;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex, OnceLock},
    };
    type Display = Ptr;
    type GetCurrent = unsafe extern "system" fn() -> Ptr;
    type GetSurface = unsafe extern "system" fn(i32) -> Ptr;
    type MakeCurrent = unsafe extern "system" fn(Display, Ptr, Ptr, Ptr) -> u32;
    type Destroy = unsafe extern "system" fn(Display, Ptr) -> u32;
    type Terminate = unsafe extern "system" fn(Display) -> u32;
    type GetProc = unsafe extern "system" fn(*const c_char) -> Ptr;
    struct DisplayLease {
        users: usize,
        owned: bool,
    }
    fn displays() -> &'static Mutex<HashMap<usize, DisplayLease>> {
        static D: OnceLock<Mutex<HashMap<usize, DisplayLease>>> = OnceLock::new();
        D.get_or_init(Default::default)
    }
    pub struct SavedCurrent {
        _egl: Library,
        context: Ptr,
        display: Ptr,
        draw: Ptr,
        read: Ptr,
        api: u32,
        get_api: unsafe extern "system" fn() -> u32,
        bind: unsafe extern "system" fn(u32) -> u32,
        get_display: GetCurrent,
        current: MakeCurrent,
        armed: bool,
        _thread: PhantomData<Rc<()>>,
    }
    impl SavedCurrent {
        pub fn capture() -> Result<Self, String> {
            unsafe {
                let egl = Library::open(if cfg!(target_os = "android") {
                    "libEGL.so"
                } else {
                    "libEGL.so.1"
                })?;
                macro_rules! get {
                    ($name:literal,$ty:ty) => {{
                        let p = egl.symbol(
                            CStr::from_bytes_with_nul(concat!($name, "\0").as_bytes()).unwrap(),
                        );
                        if p.is_null() {
                            return Err(concat!("missing ", $name).into());
                        }
                        std::mem::transmute::<Ptr, $ty>(p)
                    }};
                }
                // EGL cannot restore a GLX binding. Refuse before changing any
                // state rather than silently detaching an unsupported UI context.
                #[cfg(not(target_os = "android"))]
                if let Ok(gl) = Library::open("libGL.so.1") {
                    let p = gl.symbol(c"glXGetCurrentContext");
                    if !p.is_null() {
                        let current: GetCurrent = std::mem::transmute(p);
                        if !current().is_null() {
                            return Err("snapshot reader cannot preserve a GLX context".into());
                        }
                    }
                }
                let context = get!("eglGetCurrentContext", GetCurrent)();
                let get_display = get!("eglGetCurrentDisplay", GetCurrent);
                let surface = get!("eglGetCurrentSurface", GetSurface);
                let get_api = get!("eglQueryAPI", unsafe extern "system" fn() -> u32);
                let bind = get!("eglBindAPI", unsafe extern "system" fn(u32) -> u32);
                let current = get!("eglMakeCurrent", MakeCurrent);
                let display = get_display();
                let draw = surface(0x3059);
                let read = surface(0x305a);
                let api = get_api();
                Ok(Self {
                    _egl: egl,
                    context,
                    display,
                    draw,
                    read,
                    api,
                    get_api,
                    bind,
                    get_display,
                    current,
                    armed: true,
                    _thread: PhantomData,
                })
            }
        }
        pub fn restore(&mut self) -> Result<(), String> {
            unsafe {
                if !self.armed {
                    return Ok(());
                }
                if (self.get_api)() != self.api {
                    let current_display = (self.get_display)();
                    if !current_display.is_null()
                        && (self.current)(
                            current_display,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        ) == 0
                    {
                        return Err("EGL reader release failed".into());
                    }
                    if (self.bind)(self.api) == 0 {
                        return Err("EGL previous API restore failed".into());
                    }
                }
                let ok = if !self.context.is_null() {
                    (self.current)(self.display, self.draw, self.read, self.context) != 0
                } else {
                    let display = (self.get_display)();
                    display.is_null()
                        || (self.current)(
                            display,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        ) != 0
                };
                if !ok {
                    return Err("EGL previous context restore failed".into());
                }
                self.armed = false;
                Ok(())
            }
        }
    }
    impl Drop for SavedCurrent {
        fn drop(&mut self) {
            if let Err(e) = self.restore() {
                log::warn!("{e}");
            }
        }
    }
    /// A creation capability, not ownership of a current context. Closing the
    /// source Host invalidates it under the same lock used while creating a
    /// child. Once created, the child's EGL/display leases are independent.
    #[derive(Clone, Debug)]
    pub struct SharedSource(
        Arc<Mutex<Option<(usize, usize, bool, bool)>>>,
        pub(crate) Arc<crate::gpu::shared::ShareGroup>,
    );

    /// Keeps a capability to an externally owned UI context open. This guard
    /// never unbinds, destroys or terminates the external context/display.
    pub struct ExternalContext {
        source: SharedSource,
        _egl: Arc<Library>,
        _thread: PhantomData<Rc<()>>,
    }
    pub struct SharedConsumer {
        source: SharedSource,
        _egl: Arc<Library>,
    }
    /// Metadata lease for a root context whose native lifetime is owned by the
    /// embedding frontend. Drop closes creation capability without issuing GL.
    pub struct PersistentSharedContext {
        source: SharedSource,
        _egl: Arc<Library>,
    }
    impl PersistentSharedContext {
        /// # Safety
        /// Keep the captured native root and display alive until this lease and
        /// every child renderer/readback have been released. The caller owns
        /// destruction; this lease never destroys or makes the root current.
        pub unsafe fn capture_current() -> Result<Self, String> {
            let mut root = ExternalContext::capture_current()?;
            let source = std::mem::replace(
                &mut root.source,
                SharedSource(Arc::new(Mutex::new(None)), Default::default()),
            );
            Ok(Self {
                source,
                _egl: root._egl.clone(),
            })
        }
        pub fn source(&self) -> SharedSource {
            self.source.clone()
        }
        /// # Safety
        /// The current context must have been created sharing this root. Same
        /// display alone does not prove sharing. Its owner must drop the returned
        /// guard before destroying/replacing that current consumer context.
        pub unsafe fn capture_consumer(&self) -> Result<ExternalContext, String> {
            let root = self.source.0.lock().unwrap_or_else(|e| e.into_inner());
            let display = root.as_ref().ok_or("persistent root has closed")?.0;
            let mut ui = ExternalContext::capture_current()?;
            if ui
                .source
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(|s| s.0)
                != Some(display)
            {
                return Err("consumer and root use different EGL displays".into());
            }
            ui.source.1 = self.source.1.clone();
            Ok(ui)
        }
    }
    impl Drop for PersistentSharedContext {
        fn drop(&mut self) {
            self.source
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
        }
    }
    impl ExternalContext {
        /// Capture EGL on the UI's GL thread, without changing its current API,
        /// context or surfaces. GLX/WGL-only threads return an error.
        ///
        /// # Safety
        /// Close/drop this guard BEFORE destroying/replacing the external
        /// context. Do not terminate its display until every child renderer has
        /// stopped. The UI framework remains the owner of these native objects.
        pub unsafe fn capture_current() -> Result<Self, String> {
            let egl = Arc::new(Library::open(if cfg!(target_os = "android") {
                "libEGL.so"
            } else {
                "libEGL.so.1"
            })?);
            let get_context = egl.symbol(c"eglGetCurrentContext");
            let get_display = egl.symbol(c"eglGetCurrentDisplay");
            let query = egl.symbol(c"eglQueryContext");
            if get_context.is_null() || get_display.is_null() || query.is_null() {
                return Err("EGL context queries unavailable".into());
            }
            let get_context: GetCurrent = std::mem::transmute(get_context);
            let get_display: GetCurrent = std::mem::transmute(get_display);
            let query: unsafe extern "system" fn(Ptr, Ptr, i32, *mut i32) -> u32 =
                std::mem::transmute(query);
            let context = get_context();
            let display = get_display();
            if context.is_null() || display.is_null() {
                return Err("UI thread has no current EGL context".into());
            }
            #[cfg(target_os = "android")]
            {
                let mut leases = displays().lock().unwrap_or_else(|e| e.into_inner());
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    leases.entry(display as usize)
                {
                    let p = egl.symbol(c"eglInitialize");
                    if p.is_null() {
                        return Err("EGL initialization unavailable".into());
                    }
                    let initialize: unsafe extern "system" fn(Ptr, *mut i32, *mut i32) -> u32 =
                        std::mem::transmute(p);
                    if initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                        return Err("EGL display retention failed".into());
                    }
                    entry.insert(DisplayLease {
                        users: 0,
                        owned: true,
                    });
                }
            }
            let mut api = 0;
            if query(display, context, 0x3097, &mut api) == 0 || !matches!(api, 0x30a0 | 0x30a2) {
                return Err("UI EGL client API unavailable".into());
            }
            Ok(Self {
                source: SharedSource(
                    Arc::new(Mutex::new(Some((
                        display as usize,
                        context as usize,
                        api == 0x30a0,
                        true,
                    )))),
                    Default::default(),
                ),
                _egl: egl,
                _thread: PhantomData,
            })
        }
        pub fn consumer(&self) -> SharedConsumer {
            SharedConsumer {
                source: self.source.clone(),
                _egl: self._egl.clone(),
            }
        }
        pub fn source(&self) -> SharedSource {
            self.source.clone()
        }
        pub(crate) fn is_current(&self) -> bool {
            let source = self.source.0.lock().unwrap_or_else(|e| e.into_inner());
            let Some((display, context, _, _)) = *source else {
                return false;
            };
            unsafe {
                let c: GetCurrent = std::mem::transmute(self._egl.symbol(c"eglGetCurrentContext"));
                let d: GetCurrent = std::mem::transmute(self._egl.symbol(c"eglGetCurrentDisplay"));
                c() as usize == context && d() as usize == display
            }
        }
        pub(crate) fn load(&self, name: &CStr) -> Ptr {
            unsafe {
                let get: GetProc = std::mem::transmute(self._egl.symbol(c"eglGetProcAddress"));
                get(name.as_ptr())
            }
        }

        pub fn close(&mut self) {
            let _ = self.collect_retired_frames();
            self.source
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
        }
    }
    impl SharedConsumer {
        pub(crate) fn source(&self) -> SharedSource {
            self.source.clone()
        }
        pub(crate) fn is_current(&self) -> bool {
            let source = self.source.0.lock().unwrap_or_else(|e| e.into_inner());
            let Some((display, context, _, _)) = *source else {
                return false;
            };
            unsafe {
                let c: GetCurrent = std::mem::transmute(self._egl.symbol(c"eglGetCurrentContext"));
                let d: GetCurrent = std::mem::transmute(self._egl.symbol(c"eglGetCurrentDisplay"));
                c() as usize == context && d() as usize == display
            }
        }
        pub(crate) fn load(&self, name: &CStr) -> Ptr {
            unsafe {
                let get: GetProc = std::mem::transmute(self._egl.symbol(c"eglGetProcAddress"));
                get(name.as_ptr())
            }
        }
    }
    impl Drop for ExternalContext {
        fn drop(&mut self) {
            self.close();
        }
    }
    pub struct Host {
        egl: Arc<Library>,
        gl: Library,
        display: Display,
        surface: Ptr,
        context: Ptr,
        current: MakeCurrent,
        bind_api: unsafe extern "system" fn(u32) -> u32,
        current_context: GetCurrent,
        current_display: GetCurrent,
        current_surface: GetSurface,
        destroy_context: Destroy,
        destroy_surface: Destroy,
        terminate: Terminate,
        get_proc: GetProc,
        shared_source: SharedSource,
        pub es: bool,
        _thread: PhantomData<Rc<()>>,
    }
    impl Host {
        pub fn new() -> Result<Self, String> {
            Self::create(cfg!(target_os = "android"))
        }
        pub fn new_programmable() -> Result<Self, String> {
            Self::create(true)
        }
        #[cfg(test)]
        pub fn es_for_test() -> Result<Self, String> {
            Self::create(true)
        }
        fn create(es: bool) -> Result<Self, String> {
            Self::create_with_share(es, std::ptr::null_mut(), std::ptr::null_mut(), false)
        }
        pub fn shared_source(&self) -> SharedSource {
            self.shared_source.clone()
        }
        pub fn shared(source: &SharedSource) -> Result<Self, String> {
            let group = source.1.clone();
            let source = source.0.lock().unwrap_or_else(|e| e.into_inner());
            let (display, context, es, borrowed) =
                source.as_ref().copied().ok_or("shared source has closed")?;
            // Keep the source locked until both the EGL share group and the
            // display lease have been retained by the independently owned Host.
            let mut host = Self::create_with_share(es, display as Ptr, context as Ptr, borrowed)?;
            host.shared_source.1 = group;
            Ok(host)
        }
        fn create_with_share(
            es: bool,
            share_display: Ptr,
            share_context: Ptr,
            borrowed_display: bool,
        ) -> Result<Self, String> {
            unsafe {
                #[cfg(target_os = "android")]
                let egl = {
                    static EGL: OnceLock<Result<Arc<Library>, String>> = OnceLock::new();
                    EGL.get_or_init(|| Library::open("libEGL.so").map(Arc::new))
                        .clone()?
                };
                #[cfg(not(target_os = "android"))]
                let egl = Arc::new(Library::open("libEGL.so.1")?);
                let gl = Library::open(if cfg!(target_os = "android") {
                    "libGLESv2.so"
                } else {
                    "libGL.so.1"
                })?;
                macro_rules! sym {
                    ($name:literal,$t:ty) => {{
                        let p = egl.symbol(
                            CStr::from_bytes_with_nul(concat!($name, "\0").as_bytes()).unwrap(),
                        );
                        if p.is_null() {
                            return Err(concat!("missing ", $name).into());
                        }
                        std::mem::transmute::<Ptr, $t>(p)
                    }};
                }
                let get_display = sym!("eglGetDisplay", unsafe extern "system" fn(Ptr) -> Display);
                let initialize = sym!(
                    "eglInitialize",
                    unsafe extern "system" fn(Display, *mut i32, *mut i32) -> u32
                );
                let bind = sym!("eglBindAPI", unsafe extern "system" fn(u32) -> u32);
                let choose = sym!(
                    "eglChooseConfig",
                    unsafe extern "system" fn(Display, *const i32, *mut Ptr, i32, *mut i32) -> u32
                );
                let create_surface = sym!(
                    "eglCreatePbufferSurface",
                    unsafe extern "system" fn(Display, Ptr, *const i32) -> Ptr
                );
                let create_context = sym!(
                    "eglCreateContext",
                    unsafe extern "system" fn(Display, Ptr, Ptr, *const i32) -> Ptr
                );
                let current = sym!("eglMakeCurrent", MakeCurrent);
                let current_context = sym!("eglGetCurrentContext", GetCurrent);
                let current_display = sym!("eglGetCurrentDisplay", GetCurrent);
                let current_surface = sym!("eglGetCurrentSurface", GetSurface);
                let destroy_context = sym!("eglDestroyContext", Destroy);
                let destroy_surface = sym!("eglDestroySurface", Destroy);
                let terminate = sym!("eglTerminate", Terminate);
                let get_proc = sym!("eglGetProcAddress", GetProc);
                let mut display = get_display(std::ptr::null_mut());
                // Mesa's surfaceless platform is useful on headless Linux only.
                if !share_display.is_null() {
                    display = share_display;
                } else if !cfg!(target_os = "android") {
                    let p = get_proc(c"eglGetPlatformDisplayEXT".as_ptr());
                    if !p.is_null() {
                        let get: unsafe extern "system" fn(u32, Ptr, *const i32) -> Display =
                            std::mem::transmute(p);
                        let d = get(0x31dd, std::ptr::null_mut(), std::ptr::null());
                        if !d.is_null() {
                            display = d;
                        }
                    }
                }
                if display.is_null() {
                    return Err("EGL has no display".into());
                }
                if !share_display.is_null() && share_display != display {
                    return Err("shared context uses a different EGL display".into());
                }
                {
                    let mut d = displays().lock().unwrap();
                    let first = !d.contains_key(&(display as usize));
                    if first
                        && !borrowed_display
                        && initialize(display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
                    {
                        d.remove(&(display as usize));
                        return Err("EGL initialization failed".into());
                    }
                    d.entry(display as usize)
                        .or_insert(DisplayLease {
                            users: 0,
                            owned: !borrowed_display,
                        })
                        .users += 1;
                }
                let mut host = Self {
                    egl,
                    gl,
                    display,
                    surface: std::ptr::null_mut(),
                    context: std::ptr::null_mut(),
                    current,
                    bind_api: bind,
                    current_context,
                    current_display,
                    current_surface,
                    destroy_context,
                    destroy_surface,
                    terminate,
                    get_proc,
                    shared_source: SharedSource(Arc::new(Mutex::new(None)), Default::default()),
                    es,
                    _thread: PhantomData,
                };
                if bind(if host.es { 0x30a0 } else { 0x30a2 }) == 0 {
                    return Err("EGL API binding failed".into());
                }
                let attrs = [
                    0x3033,
                    1,
                    0x3040,
                    if host.es { 4 } else { 8 },
                    0x3024,
                    8,
                    0x3023,
                    8,
                    0x3022,
                    8,
                    0x3038,
                ];
                let mut config = std::ptr::null_mut();
                let mut count = 0;
                if choose(display, attrs.as_ptr(), &mut config, 1, &mut count) == 0 || count == 0 {
                    return Err("EGL pbuffer configuration unavailable".into());
                }
                host.surface =
                    create_surface(display, config, [0x3057, 1, 0x3056, 1, 0x3038].as_ptr());
                let ca = if host.es {
                    vec![0x3098, 2, 0x3038]
                } else {
                    vec![0x3038]
                };
                // Prefer core ES3 when available: exact fragment LOD is core
                // there. Existing GLSL100 programs remain valid. Older devices
                // retain the ES2 path and its advertised host extensions.
                if host.es {
                    host.context = create_context(
                        display,
                        config,
                        share_context,
                        [0x3098, 3, 0x3038].as_ptr(),
                    );
                }
                if host.context.is_null() {
                    host.context = create_context(display, config, share_context, ca.as_ptr());
                }
                if host.surface.is_null() || host.context.is_null() {
                    return Err("EGL offscreen creation failed".into());
                }
                host.make_current()?;
                *host
                    .shared_source
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) =
                    Some((display as usize, host.context as usize, host.es, false));
                Ok(host)
            }
        }
        pub fn bind_client_api(&self) -> Result<(), String> {
            if unsafe { (self.bind_api)(if self.es { 0x30a0 } else { 0x30a2 }) } == 0 {
                Err("EGL API binding failed".into())
            } else {
                Ok(())
            }
        }
        /// Activate an owned offscreen renderer whose context is only ever
        /// bound to its own drawable. Context identity is enough on the hot
        /// path. Bind the client API when a different context is current.
        pub fn make_programmable_current(&self) -> Result<(), String> {
            if unsafe { (self.current_context)() == self.context } {
                return Ok(());
            }
            self.bind_client_api()?;
            self.make_current()
        }
        pub fn make_current(&self) -> Result<(), String> {
            unsafe {
                // Preserve actual native thread state across parent/child switches.
                if (self.current_context)() == self.context
                    && (self.current_display)() == self.display
                    && (self.current_surface)(0x3059) == self.surface
                    && (self.current_surface)(0x305a) == self.surface
                {
                    return Ok(());
                }
                if (self.current)(self.display, self.surface, self.surface, self.context) == 0 {
                    Err("EGL context lost".into())
                } else {
                    Ok(())
                }
            }
        }
        pub fn load(&self, n: &CStr) -> Ptr {
            unsafe {
                let p = (self.get_proc)(n.as_ptr());
                if p.is_null() {
                    self.gl.symbol(n)
                } else {
                    p
                }
            }
        }
    }
    impl Drop for Host {
        fn drop(&mut self) {
            // Serialize source closure against any in-flight child creation.
            self.shared_source
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            unsafe {
                if !self.context.is_null() {
                    if (self.current_context)() == self.context
                        && (self.current_display)() == self.display
                    {
                        (self.current)(
                            self.display,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                    }
                    (self.destroy_context)(self.display, self.context);
                }
                if !self.surface.is_null() {
                    (self.destroy_surface)(self.display, self.surface);
                }
                let mut ds = displays().lock().unwrap();
                if let Some(n) = ds.get_mut(&(self.display as usize)) {
                    n.users -= 1;
                    // Android's UI shares EGL_DEFAULT_DISPLAY. Keep one
                    // process-wide initialization lease; destroy every game
                    // context and surface without terminating the UI display.
                    if n.users == 0 && (!n.owned || !cfg!(target_os = "android")) {
                        if n.owned {
                            (self.terminate)(self.display);
                        }
                        ds.remove(&(self.display as usize));
                    }
                }
                let _ = &self.egl;
            }
        }
    }
    #[cfg(test)]
    mod external_tests {
        use super::*;
        use crate::gpu::Renderer;
        use std::{sync::mpsc, time::Duration};

        #[test]
        #[ignore = "requires native EGL desktop GL and GLES"]
        fn external_ui_display_is_borrowed_and_never_terminated() {
            for es in [false, true] {
                let ui = if es {
                    Host::es_for_test().unwrap()
                } else {
                    Host::new().unwrap()
                };
                // Model a UI-created display: it has no pocket-gles lease.
                // Restore ownership even on failure so test resources release.
                let lease = displays()
                    .lock()
                    .unwrap()
                    .remove(&(ui.display as usize))
                    .unwrap();
                assert_eq!(lease.users, 1);
                struct Restore(usize, Option<DisplayLease>);
                impl Drop for Restore {
                    fn drop(&mut self) {
                        assert!(displays()
                            .lock()
                            .unwrap()
                            .insert(self.0, self.1.take().unwrap())
                            .is_none());
                    }
                }
                let _restore = Restore(ui.display as usize, Some(lease));
                let before = unsafe {
                    (
                        (ui.current_context)(),
                        (ui.current_display)(),
                        (ui.current_surface)(0x3059),
                        (ui.current_surface)(0x305a),
                    )
                };
                let mut external = unsafe { ExternalContext::capture_current().unwrap() };
                let after = unsafe {
                    (
                        (ui.current_context)(),
                        (ui.current_display)(),
                        (ui.current_surface)(0x3059),
                        (ui.current_surface)(0x305a),
                    )
                };
                assert_eq!(before, after, "capture changed the UI binding");
                let source = external.source();
                std::thread::scope(|scope| {
                    let (created, ready) = mpsc::sync_channel(1);
                    let (stop, stopping) = mpsc::sync_channel(1);
                    let worker = scope.spawn({
                        let source = source.clone();
                        move || {
                            let mut child = Renderer::new_shared(&source, 8, 8).unwrap();
                            created.send(()).unwrap();
                            stopping.recv_timeout(Duration::from_secs(10)).unwrap();
                            child.finish().unwrap();
                        }
                    });
                    ready.recv_timeout(Duration::from_secs(10)).unwrap();
                    let leases = displays().lock().unwrap();
                    assert_eq!(leases[&(ui.display as usize)].users, 1);
                    assert!(!leases[&(ui.display as usize)].owned);
                    drop(leases);
                    external.close();
                    assert!(Renderer::new_shared(&source, 8, 8).is_err());
                    stop.send(()).unwrap();
                    worker.join().unwrap();
                });
                assert!(!displays()
                    .lock()
                    .unwrap()
                    .contains_key(&(ui.display as usize)));
                ui.make_current()
                    .expect("child teardown terminated the external UI display");
                let replacement = unsafe { ExternalContext::capture_current().unwrap() };
                let child = Renderer::new_shared(&replacement.source(), 8, 8).unwrap();
                drop(child);
                drop(replacement);
                ui.make_current().unwrap();
            }
        }
    }
}
#[cfg(windows)]
mod native {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(C)]
    struct PixelFormat {
        size: u16,
        version: u16,
        flags: u32,
        ty: u8,
        color: u8,
        red: u8,
        redshift: u8,
        green: u8,
        greenshift: u8,
        blue: u8,
        blueshift: u8,
        alpha: u8,
        alphashift: u8,
        accum: u8,
        ar: u8,
        ag: u8,
        ab: u8,
        aa: u8,
        depth: u8,
        stencil: u8,
        aux: u8,
        layer: u8,
        reserved: u8,
        layermask: u32,
        visible: u32,
        damage: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn CreateWindowExW(
            ex: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: Ptr,
            menu: Ptr,
            instance: Ptr,
            param: Ptr,
        ) -> Ptr;
        fn DestroyWindow(w: Ptr) -> i32;
        fn GetDC(w: Ptr) -> Ptr;
        fn ReleaseDC(w: Ptr, d: Ptr) -> i32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn ChoosePixelFormat(d: Ptr, p: *const PixelFormat) -> i32;
        fn GetPixelFormat(d: Ptr) -> i32;
        fn DescribePixelFormat(d: Ptr, i: i32, size: u32, p: *mut PixelFormat) -> i32;
        fn SetPixelFormat(d: Ptr, i: i32, p: *const PixelFormat) -> i32;
    }
    #[link(name = "opengl32")]
    extern "system" {
        fn wglCreateContext(d: Ptr) -> Ptr;
        fn wglMakeCurrent(d: Ptr, c: Ptr) -> i32;
        fn wglGetCurrentContext() -> Ptr;
        fn wglGetCurrentDC() -> Ptr;
        fn wglDeleteContext(c: Ptr) -> i32;
        fn wglShareLists(source: Ptr, destination: Ptr) -> i32;
        fn wglGetProcAddress(n: *const c_char) -> Ptr;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> u32;
        fn SetLastError(error: u32);
    }
    type CreateContextAttribs = unsafe extern "system" fn(Ptr, Ptr, *const i32) -> Ptr;
    #[derive(Clone, Copy, Debug)]
    struct WglSourceInfo {
        context: usize,
        pixel_format: i32,
        descriptor: PixelFormat,
        create_attribs: usize,
        major: i32,
        minor: i32,
        profile: i32,
        flags: i32,
        reset_strategy: Option<i32>,
    }
    impl WglSourceInfo {
        unsafe fn capture(gl: &Library, dc: Ptr, context: Ptr) -> Result<Self, String> {
            let pixel_format = GetPixelFormat(dc);
            let mut descriptor: PixelFormat = std::mem::zeroed();
            if pixel_format == 0
                || DescribePixelFormat(
                    dc,
                    pixel_format,
                    std::mem::size_of::<PixelFormat>() as u32,
                    &mut descriptor,
                ) == 0
            {
                return Err(format!(
                    "WGL source pixel format unavailable (Win32={})",
                    GetLastError()
                ));
            }
            // Query while this context is current. wglGetProcAddress on the
            // emulation thread without a current context cannot bootstrap ARB creation.
            let create = wglGetProcAddress(b"wglCreateContextAttribsARB\0".as_ptr().cast());
            let create_attribs =
                if create.is_null() || matches!(create as usize, 1 | 2 | 3 | usize::MAX) {
                    0
                } else {
                    create as usize
                };
            let get_string = gl.symbol(CStr::from_bytes_with_nul_unchecked(b"glGetString\0"));
            let get_integer = gl.symbol(CStr::from_bytes_with_nul_unchecked(b"glGetIntegerv\0"));
            if get_string.is_null() || get_integer.is_null() {
                return Err("WGL GL version query unavailable".into());
            }
            let get_string: unsafe extern "system" fn(u32) -> *const c_char =
                std::mem::transmute(get_string);
            let get_integer: unsafe extern "system" fn(u32, *mut i32) =
                std::mem::transmute(get_integer);
            let version = get_string(0x1f02);
            if version.is_null() {
                return Err("WGL source GL version unavailable".into());
            }
            let text = CStr::from_ptr(version).to_string_lossy();
            let mut numbers = text.split_whitespace().next().unwrap_or("").split('.');
            let major = numbers
                .next()
                .and_then(|n| n.parse().ok())
                .ok_or("WGL source GL major version invalid")?;
            let minor = numbers
                .next()
                .and_then(|n| n.parse().ok())
                .ok_or("WGL source GL minor version invalid")?;
            let mut profile = 0;
            let mut gl_flags = 0;
            if (major, minor) >= (3, 2) {
                get_integer(0x9126, &mut profile);
            }
            if major >= 3 {
                get_integer(0x821e, &mut gl_flags);
            }
            let ext_proc = wglGetProcAddress(b"wglGetExtensionsStringARB\0".as_ptr().cast());
            let mut robust = gl_flags & 4 != 0;
            if !ext_proc.is_null() && !matches!(ext_proc as usize, 1 | 2 | 3 | usize::MAX) {
                let ext: unsafe extern "system" fn(Ptr) -> *const c_char =
                    std::mem::transmute(ext_proc);
                let text = ext(dc);
                if !text.is_null() {
                    robust |= CStr::from_ptr(text)
                        .to_string_lossy()
                        .split_whitespace()
                        .any(|name| name == "WGL_ARB_create_context_robustness");
                }
            }
            let reset_strategy = if robust {
                let mut reset = 0;
                get_integer(0x8256, &mut reset);
                if !matches!(reset, 0x8252 | 0x8261) {
                    return Err("WGL reset notification strategy unavailable".into());
                }
                Some(reset)
            } else {
                None
            };
            // GL and WGL use different bits for DEBUG/FORWARD_COMPATIBLE.
            let flags = (if gl_flags & 2 != 0 { 1 } else { 0 })
                | (if gl_flags & 1 != 0 { 2 } else { 0 })
                | (gl_flags & 4);
            Ok(Self {
                context: context as usize,
                pixel_format,
                descriptor,
                create_attribs,
                major,
                minor,
                profile: profile & 3,
                flags,
                reset_strategy,
            })
        }
        fn shared_renderer_attributes(&self) -> Vec<i32> {
            // The renderer uses GLSL 1.20, GL_EXTENSIONS string queries,
            // default VAO state and GL_POINT_SPRITE. It needs compatibility,
            // even when the UI share-group root is a core-profile context.
            // Preserve device, version and reset policy; forward-compatible
            // contexts cannot provide the deprecated functionality we require.
            let mut renderer = *self;
            if (renderer.major, renderer.minor) >= (3, 2) {
                renderer.profile = 2;
            }
            renderer.flags &= !2;
            renderer.attributes()
        }
        fn attributes(&self) -> Vec<i32> {
            let mut attrs = vec![0x2091, self.major, 0x2092, self.minor, 0x2094, self.flags];
            if self.profile != 0 {
                attrs.extend_from_slice(&[0x9126, self.profile]);
            }
            if let Some(reset) = self.reset_strategy {
                attrs.extend_from_slice(&[0x8256, reset]);
            }
            attrs.push(0);
            attrs
        }
    }
    #[cfg(test)]
    mod wgl_tests {
        use super::*;
        #[test]
        fn context_attributes_preserve_version_profile_and_termination() {
            let mut info = WglSourceInfo {
                context: 0,
                pixel_format: 1,
                descriptor: unsafe { std::mem::zeroed() },
                create_attribs: 1,
                major: 4,
                minor: 6,
                profile: 1,
                flags: 0,
                reset_strategy: None,
            };
            assert_eq!(
                info.attributes(),
                [0x2091, 4, 0x2092, 6, 0x2094, 0, 0x9126, 1, 0]
            );
            info.flags = 7;
            info.reset_strategy = Some(0x8261);
            assert_eq!(
                info.shared_renderer_attributes(),
                [0x2091, 4, 0x2092, 6, 0x2094, 5, 0x9126, 2, 0x8256, 0x8261, 0]
            );
            assert_eq!(info.profile, 1); // UI request is not mutated.
            info.flags = 0;
            info.reset_strategy = None;
            info.major = 3;
            info.minor = 1;
            info.profile = 0;
            assert_eq!(info.attributes(), [0x2091, 3, 0x2092, 1, 0x2094, 0, 0]);
        }
        #[test]
        #[ignore = "requires native WGL driver; creates a populated core-profile UI context"]
        fn wgl_core_profile_shares_populated_ui_context_on_worker_thread() {
            unsafe {
                let bootstrap = Host::new().unwrap();
                let mut info = bootstrap.shared_source.0.lock().unwrap().unwrap();
                assert_ne!(info.create_attribs, 0, "WGL_ARB_create_context unavailable");
                info.context = 0;
                info.major = 3;
                info.minor = 3;
                info.profile = 1;
                info.flags = 0;
                let ui = Host::create(Some(info)).unwrap();
                drop(bootstrap);
                let mut external = ExternalContext::capture_current().unwrap();
                assert_eq!(external.source.0.lock().unwrap().unwrap().profile, 1);
                assert_ne!(
                    external.source.0.lock().unwrap().unwrap().context,
                    ui.context as usize
                );
                assert_eq!(wglGetCurrentContext(), ui.context);
                assert!(external.consumer().is_current());
                let gen: unsafe extern "system" fn(i32, *mut u32) = std::mem::transmute(
                    ui.load(CStr::from_bytes_with_nul_unchecked(b"glGenTextures\0")),
                );
                let bind: unsafe extern "system" fn(u32, u32) = std::mem::transmute(
                    ui.load(CStr::from_bytes_with_nul_unchecked(b"glBindTexture\0")),
                );
                let image: unsafe extern "system" fn(
                    u32,
                    i32,
                    i32,
                    i32,
                    i32,
                    i32,
                    u32,
                    u32,
                    *const c_void,
                ) = std::mem::transmute(
                    ui.load(CStr::from_bytes_with_nul_unchecked(b"glTexImage2D\0")),
                );
                let finish: unsafe extern "system" fn() = std::mem::transmute(
                    ui.load(CStr::from_bytes_with_nul_unchecked(b"glFinish\0")),
                );
                let delete: unsafe extern "system" fn(i32, *const u32) = std::mem::transmute(
                    ui.load(CStr::from_bytes_with_nul_unchecked(b"glDeleteTextures\0")),
                );
                let mut texture = 0;
                gen(1, &mut texture);
                bind(0xde1, texture);
                let expected = [17u8, 45, 99, 255];
                image(
                    0xde1,
                    0,
                    0x8058,
                    1,
                    1,
                    0,
                    0x1908,
                    0x1401,
                    expected.as_ptr().cast(),
                );
                finish();
                let source = external.source();
                let actual = std::thread::spawn(move || -> Result<[u8; 4], String> {
                    let child = Host::shared(&source)?;
                    if child.shared_source.0.lock().unwrap().unwrap().profile != 1 {
                        return Err("child profile differs from UI".into());
                    }
                    let is_texture: unsafe extern "system" fn(u32) -> u8 = std::mem::transmute(
                        child.load(CStr::from_bytes_with_nul_unchecked(b"glIsTexture\0")),
                    );
                    if is_texture(texture) == 0 {
                        return Err("UI texture not visible to shared worker".into());
                    }
                    let bind: unsafe extern "system" fn(u32, u32) = std::mem::transmute(
                        child.load(CStr::from_bytes_with_nul_unchecked(b"glBindTexture\0")),
                    );
                    let read: unsafe extern "system" fn(u32, i32, u32, u32, *mut c_void) =
                        std::mem::transmute(
                            child.load(CStr::from_bytes_with_nul_unchecked(b"glGetTexImage\0")),
                        );
                    bind(0xde1, texture);
                    let mut pixels = [0u8; 4];
                    read(0xde1, 0, 0x1908, 0x1401, pixels.as_mut_ptr().cast());
                    Ok(pixels)
                })
                .join()
                .unwrap()
                .unwrap();
                assert_eq!(actual, expected);
                ui.make_current().unwrap();
                delete(1, &texture);
                external.close();
            }
        }
    }
    pub struct SavedCurrent {
        dc: Ptr,
        context: Ptr,
        armed: bool,
        _thread: PhantomData<Rc<()>>,
    }
    impl SavedCurrent {
        pub fn capture() -> Result<Self, String> {
            unsafe {
                Ok(Self {
                    dc: wglGetCurrentDC(),
                    context: wglGetCurrentContext(),
                    armed: true,
                    _thread: PhantomData,
                })
            }
        }
        pub fn restore(&mut self) -> Result<(), String> {
            if !self.armed {
                return Ok(());
            }
            if unsafe { wglMakeCurrent(self.dc, self.context) } == 0 {
                return Err("WGL previous context restore failed".into());
            }
            self.armed = false;
            Ok(())
        }
    }
    impl Drop for SavedCurrent {
        fn drop(&mut self) {
            if let Err(e) = self.restore() {
                log::warn!("{e}");
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct SharedSource(
        Arc<Mutex<Option<WglSourceInfo>>>,
        pub(crate) Arc<crate::gpu::shared::ShareGroup>,
    );
    pub struct ExternalContext {
        source: SharedSource,
        ui_context: usize,
        relay: Option<Host>,
        _gl: Arc<Library>,
        _thread: PhantomData<Rc<()>>,
    }
    pub struct SharedConsumer {
        source: SharedSource,
        ui_context: usize,
        _gl: Arc<Library>,
    }
    impl ExternalContext {
        /// # Safety
        /// Close/drop this guard before the external UI context is destroyed.
        /// Stop all child renderers before tearing down its graphics device.
        pub unsafe fn capture_current() -> Result<Self, String> {
            let gl = Arc::new(Library::open("opengl32.dll")?);
            let context = wglGetCurrentContext();
            let dc = wglGetCurrentDC();
            if context.is_null() || dc.is_null() {
                return Err("UI thread has no current WGL context".into());
            }
            let info = WglSourceInfo::capture(&gl, dc, context)?;
            let initial = SharedSource(Arc::new(Mutex::new(Some(info))), Default::default());
            let mut previous = SavedCurrent::capture()?;
            // Some WGL drivers reject sharing with a context current on another
            // thread (ERROR_BUSY). Bootstrap an idle relay on the UI thread,
            // with the UI context temporarily detached, before workers start.
            SetLastError(0);
            if wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
                let error = GetLastError();
                return Err(format!("WGL UI detach for relay failed (Win32={error})"));
            }
            let relay = Host::shared(&initial)
                .map_err(|error| format!("WGL UI sharing relay creation failed: {error}"))?;
            previous.restore()?;
            let source = relay.shared_source();
            log::info!("WGL sharing relay ready; UI binding restored and relay left non-current");
            Ok(Self {
                source,
                ui_context: context as usize,
                relay: Some(relay),
                _gl: gl,
                _thread: PhantomData,
            })
        }
        pub fn consumer(&self) -> SharedConsumer {
            SharedConsumer {
                source: self.source.clone(),
                ui_context: self.ui_context,
                _gl: self._gl.clone(),
            }
        }
        pub fn source(&self) -> SharedSource {
            self.source.clone()
        }
        pub(crate) fn is_current(&self) -> bool {
            let source = self.source.0.lock().unwrap_or_else(|e| e.into_inner());
            if source.is_none() {
                return false;
            }
            unsafe { wglGetCurrentContext() as usize == self.ui_context }
        }
        pub(crate) fn load(&self, name: &CStr) -> Ptr {
            unsafe {
                let p = wglGetProcAddress(name.as_ptr());
                if p.is_null() || matches!(p as usize, 1 | 2 | 3 | usize::MAX) {
                    self._gl.symbol(name)
                } else {
                    p
                }
            }
        }

        pub fn close(&mut self) {
            let _ = self.collect_retired_frames();
            self.source
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            self.relay.take();
        }
    }
    impl SharedConsumer {
        pub(crate) fn source(&self) -> SharedSource {
            self.source.clone()
        }
        pub(crate) fn is_current(&self) -> bool {
            let source = self.source.0.lock().unwrap_or_else(|e| e.into_inner());
            if source.is_none() {
                return false;
            }
            unsafe { wglGetCurrentContext() as usize == self.ui_context }
        }
        pub(crate) fn load(&self, name: &CStr) -> Ptr {
            unsafe {
                let p = wglGetProcAddress(name.as_ptr());
                if p.is_null() || matches!(p as usize, 1 | 2 | 3 | usize::MAX) {
                    self._gl.symbol(name)
                } else {
                    p
                }
            }
        }
    }
    impl Drop for ExternalContext {
        fn drop(&mut self) {
            self.close();
        }
    }
    pub struct Host {
        gl: Library,
        window: Ptr,
        dc: Ptr,
        context: Ptr,
        shared_source: SharedSource,
        pub es: bool,
        _thread: PhantomData<Rc<()>>,
    }
    impl Host {
        pub fn new() -> Result<Self, String> {
            Self::create(None)
        }
        pub fn new_programmable() -> Result<Self, String> {
            Self::new()
        }
        pub fn shared_source(&self) -> SharedSource {
            self.shared_source.clone()
        }
        pub fn shared(source: &SharedSource) -> Result<Self, String> {
            let group = source.1.clone();
            let source = source.0.lock().unwrap_or_else(|e| e.into_inner());
            let info = source.as_ref().copied().ok_or("shared source has closed")?;
            let mut host = Self::create(Some(info))?;
            host.shared_source.1 = group;
            Ok(host)
        }
        fn create(share: Option<WglSourceInfo>) -> Result<Self, String> {
            unsafe {
                let gl = Library::open("opengl32.dll")?;
                let mut h = Self {
                    gl,
                    window: std::ptr::null_mut(),
                    dc: std::ptr::null_mut(),
                    context: std::ptr::null_mut(),
                    shared_source: SharedSource(Arc::new(Mutex::new(None)), Default::default()),
                    es: false,
                    _thread: PhantomData,
                };
                let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
                h.window = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    0x80000000,
                    0,
                    0,
                    1,
                    1,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                if h.window.is_null() {
                    return Err("WGL hidden window unavailable".into());
                }
                h.dc = GetDC(h.window);
                if h.dc.is_null() {
                    return Err("WGL device context unavailable".into());
                }
                let mut p: PixelFormat = std::mem::zeroed();
                p.size = std::mem::size_of::<PixelFormat>() as u16;
                p.version = 1;
                p.flags = 0x24;
                p.color = 24;
                p.depth = 24;
                p.stencil = 8;
                let i = if let Some(info) = share {
                    let index = info.pixel_format;
                    let descriptor = info.descriptor;
                    let mut available: PixelFormat = std::mem::zeroed();
                    if DescribePixelFormat(
                        h.dc,
                        index,
                        std::mem::size_of::<PixelFormat>() as u32,
                        &mut available,
                    ) == 0
                        || available != descriptor
                    {
                        return Err(
                            "WGL shared pixel format does not match the source device".into()
                        );
                    }
                    p = descriptor;
                    index
                } else {
                    ChoosePixelFormat(h.dc, &p)
                };
                if i == 0 || SetPixelFormat(h.dc, i, &p) == 0 {
                    return Err("WGL pixel format unavailable".into());
                }
                if let Some(info) = share.filter(|info| info.create_attribs != 0) {
                    // Share at creation, but retain the compatibility API
                    // required by the renderer. An unshared explicit request
                    // (used by the core-profile UI smoke test) stays unchanged.
                    let create: CreateContextAttribs = std::mem::transmute(info.create_attribs);
                    let attrs = if info.context == 0 {
                        info.attributes()
                    } else {
                        info.shared_renderer_attributes()
                    };
                    SetLastError(0);
                    h.context = create(h.dc, info.context as Ptr, attrs.as_ptr());
                    if h.context.is_null() {
                        let error = GetLastError();
                        return Err(format!(
                        "wglCreateContextAttribsARB sharing failed (Win32={error}/0x{error:08x}, source GL={}.{}, profile=0x{:x}, flags=0x{:x}, pixel_format={})",
                        info.major,info.minor,info.profile,info.flags,info.pixel_format));
                    }
                } else {
                    SetLastError(0);
                    h.context = wglCreateContext(h.dc);
                    if h.context.is_null() {
                        let error = GetLastError();
                        return Err(format!("wglCreateContext failed (Win32={error})"));
                    }
                    if let Some(info) = share {
                        SetLastError(0);
                        if wglShareLists(info.context as Ptr, h.context) == 0 {
                            let error = GetLastError();
                            return Err(format!(
                                "wglShareLists failed (Win32={error}/0x{error:08x}, source GL={}.{}, profile=0x{:x}, pixel_format={}, ARB creation unavailable)",
                                info.major,info.minor,info.profile,info.pixel_format));
                        }
                    }
                }
                h.make_current()?;
                let info = WglSourceInfo::capture(&h.gl, h.dc, h.context)?;
                *h.shared_source.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(info);
                Ok(h)
            }
        }
        pub fn bind_client_api(&self) -> Result<(), String> {
            Ok(())
        }
        /// An owned offscreen context is only made current on its own DC.
        pub fn make_programmable_current(&self) -> Result<(), String> {
            if unsafe { wglGetCurrentContext() == self.context } {
                return Ok(());
            }
            self.make_current()
        }
        pub fn make_current(&self) -> Result<(), String> {
            unsafe {
                // Rebinding an already current WGL context can flush queued work.
                // Query native state so child/parent switches remain correct.
                if wglGetCurrentContext() == self.context && wglGetCurrentDC() == self.dc {
                    return Ok(());
                }
                if wglMakeCurrent(self.dc, self.context) == 0 {
                    Err("WGL context lost".into())
                } else {
                    Ok(())
                }
            }
        }
        pub fn load(&self, n: &CStr) -> Ptr {
            unsafe {
                let p = wglGetProcAddress(n.as_ptr());
                if p.is_null() || matches!(p as usize, 1 | 2 | 3 | usize::MAX) {
                    self.gl.symbol(n)
                } else {
                    p
                }
            }
        }
    }
    impl Drop for Host {
        fn drop(&mut self) {
            self.shared_source
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            unsafe {
                if !self.context.is_null() {
                    if wglGetCurrentContext() == self.context && wglGetCurrentDC() == self.dc {
                        wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut());
                    }
                    wglDeleteContext(self.context);
                }
                if !self.dc.is_null() {
                    ReleaseDC(self.window, self.dc);
                }
                if !self.window.is_null() {
                    DestroyWindow(self.window);
                }
            }
        }
    }
}
pub use native::ExternalContext;
#[cfg(unix)]
pub use native::PersistentSharedContext;
pub use native::{Host, SavedCurrent};
pub use native::{SharedConsumer, SharedSource};
