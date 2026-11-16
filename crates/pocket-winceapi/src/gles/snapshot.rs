//! Persistence for the emulated EGL session. IDs are guest handles and
//! internal object names. All native objects are rebuilt on the destination.

use super::*;
use pocket_gles::context::{ContextSnapshot, SharedObjectsSnapshot, SurfaceSnapshot};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EglSessionSnapshot {
    loaded_context: ContextSnapshot,
    loaded_surface: SurfaceSnapshot,
    contexts: BTreeMap<u32, SavedContext>,
    groups: BTreeMap<u32, SharedObjectsSnapshot>,
    images: BTreeMap<u32, SavedImage>,
    threads: HashMap<usize, egl_context::EglThread>,
    initialized: bool,
    goforce_4500: bool,
    goforce_5500: bool,
    intel2700g: bool,
    surface_format: EglSurfaceFormat,
    config_locked: bool,
    current: u32,
    loaded: u32,
    next_context: u32,
    next_group: u32,
    thread: usize,
    active: u32,
    draw: u32,
    read: u32,
    bound: bool,
    next: u32,
    window_issued: bool,
    error: u32,
    pack: usize,
    unpack: usize,
    policy: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedImage {
    config: u32,
    width: u32,
    height: u32,
    pixmap: Option<u32>,
    window: Option<u32>,
    largest: bool,
    mipmap: bool,
    deleted: bool,
    buffers: Option<SurfaceSnapshot>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedContext {
    config: u32,
    state: Option<ContextSnapshot>,
    group: u32,
    deleted: bool,
    ever_bound: bool,
    owner: Option<usize>,
    pack: usize,
    unpack: usize,
}

/// Run between guest calls on the execution thread. Never switch the guest
/// binding during capture, including when an offscreen context is parked.
pub fn capture_egl_session() -> Result<EglSessionSnapshot, String> {
    let mut reader = None;
    SURFACES.with(|surfaces| {
        let surfaces = surfaces.borrow();
        with_ctx(|active| {
            let contexts = surfaces
                .contexts
                .iter()
                .map(|(&handle, record)| {
                    Ok((
                        handle,
                        SavedContext {
                            config: record.config,
                            state: record
                                .state
                                .as_ref()
                                .map(|c| c.capture_state(&mut reader))
                                .transpose()?,
                            group: record.group,
                            deleted: record.deleted,
                            ever_bound: record.ever_bound,
                            owner: record.owner,
                            pack: record.pack,
                            unpack: record.unpack,
                        },
                    ))
                })
                .collect::<Result<_, String>>()?;
            let groups = surfaces
                .groups
                .iter()
                .map(|(&id, objects)| {
                    objects
                        .capture_snapshot(&mut reader)
                        .map(|objects| (id, objects))
                })
                .collect::<Result<_, _>>()?;
            let images = surfaces
                .images
                .iter()
                .map(|(&handle, image)| {
                    Ok((
                        handle,
                        SavedImage {
                            config: image.config,
                            width: image.width,
                            height: image.height,
                            pixmap: image.pixmap,
                            window: image.window,
                            largest: image.largest,
                            mipmap: image.mipmap,
                            deleted: image.deleted,
                            buffers: image
                                .buffers
                                .as_ref()
                                .map(SurfaceBuffers::capture_snapshot)
                                .transpose()?,
                        },
                    ))
                })
                .collect::<Result<_, String>>()?;
            Ok(EglSessionSnapshot {
                loaded_context: active.capture_state(&mut reader)?,
                loaded_surface: active.capture_surface()?,
                contexts,
                groups,
                images,
                threads: surfaces.threads.clone(),
                initialized: surfaces.initialized,
                goforce_4500: surfaces.goforce_4500,
                goforce_5500: surfaces.goforce_5500,
                intel2700g: surfaces.intel2700g,
                surface_format: surfaces.surface_format,
                config_locked: surfaces.config_locked,
                current: surfaces.current,
                loaded: surfaces.loaded,
                next_context: surfaces.next_context,
                next_group: surfaces.next_group,
                thread: surfaces.thread,
                active: surfaces.active,
                draw: surfaces.draw,
                read: surfaces.read,
                bound: surfaces.bound,
                next: surfaces.next,
                window_issued: surfaces.window_issued,
                error: EGL_ERROR.with(Cell::get),
                pack: PACK_ALIGNMENT.with(Cell::get),
                unpack: UNPACK_ALIGNMENT.with(Cell::get),
                policy: active.render_resolution_policy(),
            })
        })
    })
}

impl EglSessionSnapshot {
    /// Recreate the graph of guest contexts, surfaces and shared resources.
    /// Keep the current session intact until all new native resources exist.
    pub fn restore(self) -> Result<(), String> {
        self.validate()?;
        let contexts = self
            .contexts
            .into_iter()
            .map(|(id, record)| {
                Ok((
                    id,
                    egl_context::EglContext {
                        config: record.config,
                        state: record.state.map(ContextSnapshot::restore).transpose()?,
                        group: record.group,
                        deleted: record.deleted,
                        ever_bound: record.ever_bound,
                        owner: record.owner,
                        pack: record.pack,
                        unpack: record.unpack,
                    },
                ))
            })
            .collect::<Result<_, String>>()?;
        let groups = self
            .groups
            .into_iter()
            .map(|(id, objects)| objects.restore().map(|objects| (id, objects)))
            .collect::<Result<_, _>>()?;
        let images = self
            .images
            .into_iter()
            .map(|(id, image)| {
                Ok((
                    id,
                    EglImage {
                        config: image.config,
                        width: image.width,
                        height: image.height,
                        pixmap: image.pixmap,
                        window: image.window,
                        largest: image.largest,
                        mipmap: image.mipmap,
                        deleted: image.deleted,
                        buffers: image.buffers.map(|b| b.restore(self.policy)).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?;

        let mut context = self.loaded_context.restore()?;
        let mut surface = self.loaded_surface.restore(self.policy)?;
        context.park_surface(&mut surface);
        let mut restored = EglSurfaces {
            initialized: self.initialized,
            goforce_4500: self.goforce_4500,
            goforce_5500: self.goforce_5500,
            intel2700g: self.intel2700g,
            surface_format: self.surface_format,
            config_locked: self.config_locked,
            contexts,
            groups,
            current: self.current,
            loaded: self.loaded,
            next_context: self.next_context,
            next_group: self.next_group,
            thread: self.thread,
            threads: self.threads,
            context_alive: false,
            context_deleted: false,
            context_ever_bound: false,
            images,
            active: self.active,
            draw: self.draw,
            read: self.read,
            bound: self.bound,
            next: self.next,
            window_issued: self.window_issued,
        };
        restored.refresh_context_flags();
        SHARED_WINDOW.with(|s| s.take());
        with_ctx(|slot| *slot = context);
        SURFACES.with(|slot| *slot.borrow_mut() = restored);
        egl_context::set_thread(self.thread);
        EGL_ERROR.with(|slot| slot.set(self.error));
        PACK_ALIGNMENT.with(|slot| slot.set(self.pack));
        UNPACK_ALIGNMENT.with(|slot| slot.set(self.unpack));
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        let valid_context = |handle: u32| handle == 0 || self.contexts.contains_key(&handle);
        let valid_surface = |handle: u32| handle == 0 || self.images.contains_key(&handle);
        let valid_alignment = |value| matches!(value, 1 | 2 | 4 | 8);
        if !valid_context(self.loaded)
            || !valid_context(self.current)
            || !valid_surface(self.active)
            || !valid_surface(self.draw)
            || !valid_surface(self.read)
            || !valid_alignment(self.pack)
            || !valid_alignment(self.unpack)
            || self.contexts.iter().any(|(&id, context)| {
                !self.groups.contains_key(&context.group)
                    || !valid_alignment(context.pack)
                    || !valid_alignment(context.unpack)
                    || (id == self.loaded) != context.state.is_none()
            })
            || self
                .images
                .get(&self.active)
                .is_some_and(|image| image.buffers.is_some())
            || self.threads.values().any(|thread| {
                !valid_context(thread.context)
                    || !valid_surface(thread.draw)
                    || !valid_surface(thread.read)
            })
        {
            return Err("invalid saved EGL object graph".into());
        }
        Ok(())
    }
}
