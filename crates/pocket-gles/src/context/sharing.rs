//! Object ownership is transferred only at EGL context switches. Draws keep
//! direct HashMap access, without shared locks, refcounts or object copies.
use super::*;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ObjectNames {
    pub textures: HashMap<u32, u32>,
    pub buffers: HashMap<u32, u32>,
    texture_names: HashMap<u32, u32>,
    buffer_names: HashMap<u32, u32>,
    next_texture_id: u32,
    next_buffer_id: u32,
}
#[derive(Default, Clone)]
pub struct ObjectReferences {
    textures: HashSet<u32>,
    buffers: HashSet<u32>,
}
impl ObjectReferences {
    pub fn extend(&mut self, other: &Self) {
        self.textures.extend(&other.textures);
        self.buffers.extend(&other.buffers);
    }
}
#[derive(Default)]
pub struct SharedObjects {
    programs: crate::combiner_program::ProgramObjects,
    textures: HashMap<u32, Texture>,
    buffers: HashMap<u32, Buffer>,
    names: ObjectNames,
    texture_objects: HashSet<u32>,
    buffer_objects: HashSet<u32>,
    next_texture_name: u32,
    next_buffer_name: u32,
}
impl SharedObjects {
    pub fn set_goforce_profile(&mut self, enabled: bool) {
        for tex in self.textures.values_mut() {
            tex.goforce_lod = enabled;
        }
    }
    pub fn prepare_software(&mut self) {
        for tex in self.textures.values_mut() {
            tex.ensure_rgba();
        }
    }
    pub fn texture_ids(&self) -> Vec<u32> {
        self.textures.keys().copied().collect()
    }
}
impl Context {
    /// Fresh independent GL state with the session's rendering policy.
    pub fn new_egl_context(&self) -> Self {
        let mut c = Self::new_rgb565(1, 1);
        c.gpu_requested = self.gpu_requested;
        c.internal_resolution = self.internal_resolution;
        c.gizmondo_goforce_20051013 = self.gizmondo_goforce_20051013;
        c.nv_combiner_client = self.nv_combiner_client;
        c.nv_combiner_allowed = self.nv_combiner_allowed;
        c.state.intel2700g_depth_bias = self.state.intel2700g_depth_bias;
        c
    }
    pub fn set_egl_gpu_policy(&mut self, enabled: bool) {
        if enabled {
            self.gpu_requested = true;
        } else {
            self.disable_gpu();
        }
    }
    /// Texture zero is context-local, including its image and parameters.
    pub fn exchange_shared_objects(&mut self, group: &mut SharedObjects) {
        self.combiner_programs
            .exchange_namespace(&mut group.programs);
        let zero = self.textures.remove(&0);
        std::mem::swap(&mut self.textures, &mut group.textures);
        std::mem::swap(&mut self.buffers, &mut group.buffers);
        std::mem::swap(&mut self.names, &mut group.names);
        std::mem::swap(&mut self.es11.texture_objects, &mut group.texture_objects);
        std::mem::swap(&mut self.es11.buffer_objects, &mut group.buffer_objects);
        std::mem::swap(&mut self.next_texture_name, &mut group.next_texture_name);
        std::mem::swap(&mut self.next_buffer_name, &mut group.next_buffer_name);
        self.next_texture_name = self.next_texture_name.max(1);
        self.next_buffer_name = self.next_buffer_name.max(1);
        if let Some(zero) = zero {
            self.textures.insert(0, zero);
        }
        self.software_mips.clear();
    }
    pub fn object_references(&self) -> ObjectReferences {
        let mut refs = ObjectReferences::default();
        refs.textures.extend(
            self.texture_units
                .iter()
                .map(|u| u.bound_texture)
                .filter(|n| *n != 0),
        );
        refs.buffers.extend(
            [
                self.array_buffer,
                self.element_array_buffer,
                self.vertex_array.buffer,
                self.normal_array.buffer,
                self.color_array.buffer,
                self.point_size_array.buffer,
            ]
            .into_iter()
            .filter(|n| *n != 0),
        );
        refs.buffers.extend(
            self.texture_units
                .iter()
                .map(|u| u.texcoord_array.buffer)
                .filter(|n| *n != 0),
        );
        refs
    }
    pub fn set_external_object_references(&mut self, refs: ObjectReferences) {
        self.external_references = refs;
        self.collect_shared_objects();
    }
    /// Collect unlinked objects only after all contexts have released them.
    pub(super) fn collect_shared_objects(&mut self) {
        let mut refs = self.object_references();
        refs.extend(&self.external_references);
        refs.textures.extend(self.names.textures.values().copied());
        refs.buffers.extend(self.names.buffers.values().copied());
        self.textures
            .retain(|id, _| *id == 0 || refs.textures.contains(id));
        self.buffers.retain(|id, _| refs.buffers.contains(id));
        self.names
            .texture_names
            .retain(|id, _| refs.textures.contains(id));
        self.names
            .buffer_names
            .retain(|id, _| refs.buffers.contains(id));
        self.es11
            .texture_objects
            .retain(|id| refs.textures.contains(id));
        self.es11
            .buffer_objects
            .retain(|id| refs.buffers.contains(id));
        self.software_mips
            .retain(|id, _| *id == 0 || refs.textures.contains(id));
    }
    pub(super) fn release_unlinked_texture(&mut self, id: u32) {
        if !self.external_references.textures.contains(&id) {
            self.textures.remove(&id);
            self.software_mips.remove(&id);
            self.es11.texture_objects.remove(&id);
            self.names.texture_names.remove(&id);
        }
    }
    pub(super) fn release_unlinked_buffer(&mut self, id: u32) {
        if !self.external_references.buffers.contains(&id) {
            self.buffers.remove(&id);
            self.es11.buffer_objects.remove(&id);
            self.names.buffer_names.remove(&id);
        }
    }
    pub fn texture_object_ids(&self, names: &[u32]) -> Vec<u32> {
        names
            .iter()
            .filter_map(|n| self.names.textures.get(n).copied())
            .collect()
    }
    pub fn texture_object_name(&self, id: u32) -> u32 {
        self.names.texture_names.get(&id).copied().unwrap_or(id)
    }
    pub fn buffer_object_name(&self, id: u32) -> u32 {
        self.names.buffer_names.get(&id).copied().unwrap_or(id)
    }
    pub(super) fn ensure_texture_name(&mut self, name: u32) -> u32 {
        if name == 0 {
            return 0;
        }
        if let Some(id) = self.names.textures.get(&name) {
            return *id;
        }
        let mut id = name;
        if self.names.texture_names.contains_key(&id) {
            self.names.next_texture_id = self.names.next_texture_id.max(0x80000000);
            while self.textures.contains_key(&self.names.next_texture_id) {
                self.names.next_texture_id = self.names.next_texture_id.wrapping_add(1).max(1);
            }
            id = self.names.next_texture_id;
            self.names.next_texture_id = id.wrapping_add(1);
        }
        self.names.textures.insert(name, id);
        self.names.texture_names.insert(id, name);
        id
    }
    pub(super) fn ensure_buffer_name(&mut self, name: u32) -> u32 {
        if name == 0 {
            return 0;
        }
        if let Some(id) = self.names.buffers.get(&name) {
            return *id;
        }
        let mut id = name;
        if self.names.buffer_names.contains_key(&id) {
            self.names.next_buffer_id = self.names.next_buffer_id.max(0x80000000);
            while self.buffers.contains_key(&self.names.next_buffer_id) {
                self.names.next_buffer_id = self.names.next_buffer_id.wrapping_add(1).max(1);
            }
            id = self.names.next_buffer_id;
            self.names.next_buffer_id = id.wrapping_add(1);
        }
        self.names.buffers.insert(name, id);
        self.names.buffer_names.insert(id, name);
        id
    }
}
