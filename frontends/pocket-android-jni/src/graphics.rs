//! Android owns a persistent EGL root; each SurfaceView context shares it.
//! Only immutable SharedFrames cross threads. GL guards stay on the GL thread.
use crate::runner::FrameSnapshot;
#[cfg(unix)]
use gles::PersistentSharedContext;
use jni::{
    objects::{JClass, JObject},
    sys::{jboolean, jlong, jstring},
    JNIEnv,
};
use pocket_core::winceapi::gles::{self, ExternalGpuContext, SharedContextSource};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

struct Mailbox<T> {
    pending: Option<T>,
    current: Option<T>,
}
impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self {
            pending: None,
            current: None,
        }
    }
}
impl<T: Clone> Mailbox<T> {
    fn replace(&mut self, value: T) -> Option<T> {
        self.pending.replace(value)
    }
    fn acquire(&mut self) -> (Option<T>, Option<T>) {
        let old = self.pending.take().and_then(|p| self.current.replace(p));
        (self.current.clone(), old)
    }
}
#[derive(Clone)]
struct Image {
    token: u64,
    frame: FrameSnapshot,
}

pub(crate) struct Presentation {
    #[cfg(unix)]
    root: Mutex<Option<PersistentSharedContext>>,
    pub enabled: Arc<AtomicBool>,
    ui_ready: AtomicBool,
    cpu_filter: AtomicBool,
    failed: AtomicBool,
    pub cpu_refresh: AtomicBool,
    next_image: AtomicU64,
    images: Mutex<Mailbox<Image>>,
}
impl Presentation {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(unix)]
            root: Mutex::new(None),
            enabled: Arc::new(AtomicBool::new(false)),
            ui_ready: AtomicBool::new(false),
            cpu_filter: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            cpu_refresh: AtomicBool::new(false),
            next_image: AtomicU64::new(1),
            images: Mutex::new(Mailbox::default()),
        }
    }
    pub fn source(&self) -> Option<SharedContextSource> {
        #[cfg(unix)]
        {
            self.root.lock().ok()?.as_ref().map(|root| root.source())
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
    fn refresh(&self) {
        self.enabled.store(
            self.ui_ready.load(Ordering::Acquire)
                && !self.cpu_filter.load(Ordering::Acquire)
                && !self.failed.load(Ordering::Acquire),
            Ordering::Release,
        );
    }
    fn fail(&self, error: &str) {
        if !self.failed.swap(true, Ordering::AcqRel) {
            log::warn!("PRESENTATION=CPU_UPLOAD reason=android_shared_failed detail={error}");
        }
        self.refresh();
        self.cpu_refresh.store(true, Ordering::Release);
    }
    /// Latest-only mailbox: drop displaced leases after releasing its mutex.
    pub fn publish(&self, frame: FrameSnapshot) -> u64 {
        let token = self.next_image.fetch_add(1, Ordering::Relaxed);
        let old = self
            .images
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Image { token, frame });
        drop(old);
        token
    }
    fn acquire(&self) -> Option<Image> {
        let (image, old) = self
            .images
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .acquire();
        drop(old);
        image
    }
    fn clear(&self) {
        let old = std::mem::take(&mut *self.images.lock().unwrap_or_else(|e| e.into_inner()));
        drop(old);
    }
}
static NEXT: AtomicU64 = AtomicU64::new(1);
fn registry() -> &'static Mutex<HashMap<u64, Arc<Presentation>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, Arc<Presentation>>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
pub(crate) fn get(id: jlong) -> Option<Arc<Presentation>> {
    registry().lock().ok()?.get(&(id as u64)).cloned()
}
thread_local! { static UI: RefCell<HashMap<u64, ExternalGpuContext>> = RefCell::new(HashMap::new()); }

#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeCreatePresentation(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jlong {
    crate::init_logger();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, Arc::new(Presentation::new()));
    id as jlong
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeCapturePresentationRoot(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) -> jboolean {
    let Some(p) = get(id) else { return 0 };
    #[cfg(unix)]
    {
        let mut root = p.root.lock().unwrap_or_else(|e| e.into_inner());
        if root.is_some() {
            return 1;
        }
        // Caller made its independently retained Java-owned root current.
        match unsafe { PersistentSharedContext::capture_current() } {
            Ok(r) => {
                *root = Some(r);
                return 1;
            }
            Err(e) => p.fail(&e),
        }
    }
    0
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeAttachPresentation(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    cpu_filter: jboolean,
) -> jboolean {
    let Some(p) = get(id) else { return 0 };
    #[cfg(unix)]
    {
        let root = p.root.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(root) = root.as_ref() {
            // Java created this context with the retained root as share_context.
            match unsafe { root.capture_consumer() } {
                Ok(ui) => {
                    UI.with(|slot| {
                        slot.borrow_mut().insert(id as u64, ui);
                    });
                    p.ui_ready.store(true, Ordering::Release);
                    p.cpu_filter.store(cpu_filter != 0, Ordering::Release);
                    p.refresh();
                    return 1;
                }
                Err(e) => p.fail(&e),
            }
        }
    }
    0
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeDetachPresentation(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) {
    if let Some(p) = get(id) {
        p.ui_ready.store(false, Ordering::Release);
        p.refresh();
    }
    UI.with(|slot| {
        slot.borrow_mut().remove(&(id as u64));
    });
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativePresentationCpuFilter(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    required: jboolean,
) {
    if let Some(p) = get(id) {
        let previous = p.cpu_filter.swap(required != 0, Ordering::AcqRel);
        p.refresh();
        if required != 0 && !previous {
            p.cpu_refresh.store(true, Ordering::Release);
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeReleasePresentationFrame(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) {
    if let Some(p) = get(id) {
        let old = p
            .images
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current
            .take();
        drop(old);
    }
    UI.with(|slot| {
        if let Some(ui) = slot.borrow_mut().get_mut(&(id as u64)) {
            let _ = ui.collect_retired_frames();
        }
    });
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeClosePresentation(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
) {
    // Called after every session worker/child is joined, before root destruction.
    let old = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&(id as u64));
    if let Some(p) = old {
        p.enabled.store(false, Ordering::Release);
        p.clear();
        #[cfg(unix)]
        {
            p.root.lock().unwrap_or_else(|e| e.into_inner()).take();
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeSharedDisplayShader(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jstring {
    env.new_string(gles::SHARED_DISPLAY_FRAGMENT)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_com_pockethle_app_NativeBridge_nativeDrawPresentation(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    renderer: JObject<'_>,
) -> jboolean {
    let Some(p) = get(id) else { return 0 };
    if p.failed.load(Ordering::Acquire) {
        return 0;
    }
    let Some(image) = p.acquire() else { return 0 };
    let frame = &image.frame;
    let Some(shared) = frame.shared.as_ref() else {
        return 0;
    };
    let result = UI.with(|slot| {
        let mut slot = slot.borrow_mut();
        let ui = slot
            .get_mut(&(id as u64))
            .ok_or("Android consumer context is unavailable".to_string())?;
        // Texture never escapes the synchronized consumer callback. The existing
        // SharedFrame code records a completion fence, also on callback failure.
        unsafe {
            shared.with_texture(ui, |texture| {
                env.call_method(
                    &renderer,
                    "drawSharedTexture",
                    "(IIIIIIJ)V",
                    &[
                        (texture as i32).into(),
                        (frame.width as i32).into(),
                        (frame.height as i32).into(),
                        (frame.texture_width as i32).into(),
                        (frame.texture_height as i32).into(),
                        (frame.backlight as i32).into(),
                        (image.token as i64).into(),
                    ],
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
            })?
        }
    });
    match result {
        Ok(()) => 1,
        Err(e) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            p.fail(&e);
            0
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mailbox_is_bounded_and_moves_latest_without_replaying_old_pending() {
        let mut m: Mailbox<u32> = Mailbox::default();
        assert_eq!(m.replace(1), None);
        assert_eq!(m.replace(2), Some(1));
        assert_eq!(m.acquire(), (Some(2), None));
        assert_eq!(m.acquire(), (Some(2), None));
        m.replace(3);
        assert_eq!(m.acquire(), (Some(3), Some(2)));
    }
    #[test]
    fn capabilities_require_consumer_and_respect_cpu_filter_and_failure() {
        let p = Presentation::new();
        p.refresh();
        assert!(!p.enabled.load(Ordering::Acquire));
        p.ui_ready.store(true, Ordering::Release);
        p.refresh();
        assert!(p.enabled.load(Ordering::Acquire));
        p.cpu_filter.store(true, Ordering::Release);
        p.refresh();
        assert!(!p.enabled.load(Ordering::Acquire));
        p.cpu_filter.store(false, Ordering::Release);
        p.refresh();
        assert!(p.enabled.load(Ordering::Acquire));
        p.fail("test");
        assert!(!p.enabled.load(Ordering::Acquire));
    }
}
