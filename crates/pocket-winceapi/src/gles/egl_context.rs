//! EGL ownership, shared namespaces and guest-thread bindings. The renderer's
//! direct draw path is unchanged; state/objects move only on context switches.
use super::*;

pub(super) struct EglContext {
    pub config: u32,
    pub state: Option<Context>,
    pub group: u32,
    pub deleted: bool,
    pub ever_bound: bool,
    pub owner: Option<usize>,
    pub pack: usize,
    pub unpack: usize,
}
#[derive(Default)]
pub(super) struct EglThread {
    pub context: u32,
    pub draw: u32,
    pub read: u32,
    pub error: u32,
}
thread_local! { static THREAD: Cell<usize> = const { Cell::new(0) }; }
#[inline]
pub(crate) fn needs_thread_switch(thread: usize) -> bool {
    THREAD.with(|t| t.get() != thread)
}
pub(crate) fn set_thread(thread: usize) {
    THREAD.with(|t| t.set(thread));
}

impl EglSurfaces {
    pub(super) fn refresh_context_flags(&mut self) {
        self.context_alive = self.contexts.contains_key(&self.current);
        self.context_deleted = self.contexts.get(&self.current).is_some_and(|c| c.deleted);
        self.context_ever_bound = self
            .contexts
            .get(&self.current)
            .is_some_and(|c| c.ever_bound);
    }
    pub(super) fn activate(&mut self, handle: u32) {
        if self.loaded == handle {
            return;
        }
        let shared_group = self
            .contexts
            .get(&self.loaded)
            .zip(self.contexts.get(&handle))
            .is_some_and(|(outgoing, incoming)| outgoing.group == incoming.group);
        with_ctx(|c| {
            self.select(c, 0);
            c.flush();
            if let Some(record) = self.contexts.get_mut(&self.loaded) {
                c.exchange_shared_objects(self.groups.get_mut(&record.group).unwrap());
                record.pack = PACK_ALIGNMENT.with(Cell::get);
                record.unpack = UNPACK_ALIGNMENT.with(Cell::get);
                let blank = c.new_egl_context();
                record.state = Some(std::mem::replace(c, blank));
            }
            if let Some(record) = self.contexts.get_mut(&handle) {
                let mut incoming = record.state.take().expect("parked EGL context");
                incoming.exchange_shared_objects(self.groups.get_mut(&record.group).unwrap());
                *c = incoming;
                PACK_ALIGNMENT.with(|a| a.set(record.pack));
                UNPACK_ALIGNMENT.with(|a| a.set(record.unpack));
            } else {
                *c = c.new_egl_context();
                PACK_ALIGNMENT.with(|a| a.set(4));
                UNPACK_ALIGNMENT.with(|a| a.set(4));
            }
            // Same-group object identities and image revisions remain valid.
            // Keep those native copies instead of deleting/reuploading static
            // images at every pbuffer context switch. Prune unlinked objects;
            // texture zero is always local. Unrelated groups may reuse numeric
            // IDs/revisions, so their surface caches still require a full reset.
            let live: std::collections::HashSet<_> = c.textures.keys().copied().collect();
            for image in self.images.values_mut() {
                if let Some(b) = image.buffers.as_mut() {
                    if shared_group {
                        b.retain_shared_texture_cache(&live);
                    } else {
                        b.invalidate_texture_cache();
                    }
                }
            }
        });
        self.loaded = handle;
        self.refresh_references();
    }
    pub(super) fn refresh_references(&mut self) {
        let Some(group) = self.contexts.get(&self.loaded).map(|c| c.group) else {
            return;
        };
        let mut refs = ObjectReferences::default();
        for record in self.contexts.values().filter(|r| r.group == group) {
            if let Some(c) = record.state.as_ref() {
                refs.extend(&c.object_references());
            }
        }
        with_ctx(|c| c.set_external_object_references(refs));
    }
    pub(super) fn collect_contexts(&mut self) {
        let dead: Vec<_> = self
            .contexts
            .iter()
            .filter(|(_, c)| c.deleted && c.owner.is_none())
            .map(|(h, _)| *h)
            .collect();
        if dead.is_empty() {
            self.refresh_context_flags();
            return;
        }
        if dead.contains(&self.loaded) {
            self.activate(0);
        }
        for handle in dead {
            self.contexts.remove(&handle);
        }
        let unused: Vec<_> = self
            .groups
            .keys()
            .filter(|group| !self.contexts.values().any(|c| c.group == **group))
            .copied()
            .collect();
        for group in unused {
            let objects = self.groups.remove(&group).unwrap();
            let ids = objects.texture_ids();
            for image in self.images.values_mut() {
                if let Some(buffers) = image.buffers.as_mut() {
                    buffers.delete_textures(&ids);
                }
            }
        }
        self.refresh_context_flags();
        self.refresh_references();
    }
    pub(super) fn create_context(&mut self, share: u32) -> u32 {
        if self.contexts.len() >= 64 {
            set_egl_error(pocket_gles::EGL_BAD_ALLOC);
            return 0;
        }
        let group = if share != 0 {
            let Some(parent) = self.contexts.get(&share).filter(|c| !c.deleted) else {
                set_egl_error(pocket_gles::EGL_BAD_CONTEXT);
                return 0;
            };
            parent.group
        } else {
            let group = self.next_group;
            self.next_group += 1;
            self.groups.insert(group, SharedObjects::default());
            group
        };
        let handle = self.next_context;
        let Some(next) = (if handle == CONTEXT_HANDLE {
            Some(0x45472000)
        } else {
            handle.checked_add(1)
        }) else {
            set_egl_error(pocket_gles::EGL_BAD_ALLOC);
            return 0;
        };
        self.next_context = next;
        self.contexts.insert(
            handle,
            EglContext {
                config: CONFIG_HANDLE,
                state: Some(with_ctx(|c| c.new_egl_context())),
                group,
                deleted: false,
                ever_bound: false,
                owner: None,
                pack: 4,
                unpack: 4,
            },
        );
        handle
    }
    #[cfg(test)]
    pub(super) fn adopt_test_context(&mut self) {
        self.contexts.insert(
            CONTEXT_HANDLE,
            EglContext {
                config: CONFIG_HANDLE,
                state: None,
                group: 1,
                deleted: false,
                ever_bound: true,
                owner: Some(0),
                pack: 4,
                unpack: 4,
            },
        );
        self.groups.insert(1, SharedObjects::default());
        self.next_group = 2;
        self.next_context = 0x45472000;
        self.current = CONTEXT_HANDLE;
        self.loaded = CONTEXT_HANDLE;
        self.refresh_context_flags();
    }
}

/// Some GoForce applications upload from a loader thread without making an
/// EGL binding there (Jump). Preserve the sole active context for GL-only
/// calls from a thread that has never entered EGL. Explicit thread bindings
/// and all multi-context sessions retain normal EGL isolation.
pub(crate) fn enter_gl_thread(thread: usize) {
    let legacy_loader = with_ctx(|c| c.gizmondo_goforce_20051013)
        && SURFACES.with(|s| {
            let s = s.borrow();
            s.bound && s.current != 0 && s.contexts.len() == 1 && !s.threads.contains_key(&thread)
        });
    if !legacy_loader {
        enter_thread(thread);
    }
}

pub(crate) fn enter_thread(thread: usize) {
    if !needs_thread_switch(thread) {
        return;
    }
    SURFACES.with(|s| {
        let mut s = s.borrow_mut();
        let old = s.thread;
        let previous = EglThread {
            context: s.current,
            draw: s.draw,
            read: s.read,
            error: EGL_ERROR.with(Cell::get),
        };
        s.threads.insert(old, previous);
        let next = s.threads.remove(&thread).unwrap_or_default();
        s.activate(next.context);
        s.current = next.context;
        s.draw = next.draw;
        s.read = next.read;
        s.bound = next.context != 0;
        s.thread = thread;
        if next.draw != 0 {
            with_ctx(|c| s.select(c, next.draw));
        }
        EGL_ERROR.with(|e| {
            e.set(if next.error == 0 {
                EGL_SUCCESS
            } else {
                next.error
            })
        });
        s.refresh_context_flags();
    });
    set_thread(thread);
}

/// Guest thread termination implicitly releases its EGL binding. Reap at EGL
/// lifetime/binding calls, without adding a thread-list scan to GL draws.
pub(super) fn reap_finished_threads(kernel: &pocket_kernel::KernelState) {
    SURFACES.with(|s| {
        let mut s = s.borrow_mut();
        let finished: Vec<_> = s
            .threads
            .keys()
            .copied()
            .filter(|thread| {
                if *thread == 0 {
                    kernel.main_thread.exit_code.is_some()
                } else {
                    kernel.threads.get(*thread - 1).is_some_and(|t| t.finished)
                }
            })
            .collect();
        if finished.is_empty() {
            return;
        }
        for thread in finished {
            if let Some(binding) = s.threads.remove(&thread) {
                if let Some(context) = s.contexts.get_mut(&binding.context) {
                    context.owner = None;
                }
            }
        }
        s.collect_contexts();
        s.collect();
    });
}
