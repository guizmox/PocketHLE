//! Fixed-function state without a native renderer or attached EGL surface.
use super::*;
use crate::combiner_program::ProgramObjectSnapshot;
use crate::texture::TextureSnapshot;
use std::collections::BTreeMap;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ContextSnapshot {
    es11: es11::Es11State,
    gpu_requested: bool,
    internal_resolution: u8,
    matrix_mode: MatrixMode,
    modelview: MatrixStack,
    palette_matrices: [MatrixStack; 32],
    palette_index: usize,
    projection: MatrixStack,
    texture_matrix: [MatrixStack; MAX_TEXTURE_UNITS],
    current_color: [f32; 4],
    current_texcoord: [f32; 4],
    current_normal: [f32; 3],
    lighting: crate::lighting::LightingState,
    vertex_array: ArrayPointer,
    point_size_array: ArrayPointer,
    color_array: ArrayPointer,
    normal_array: ArrayPointer,
    client_active_texture: u32,
    active_texture: u32,
    texture_units: [TextureUnitState; MAX_TEXTURE_UNITS],
    bound_texture: u32,
    next_texture_name: u32,
    buffers: HashMap<u32, Buffer>,
    array_buffer: u32,
    element_array_buffer: u32,
    next_buffer_name: u32,
    names: sharing::ObjectNames,
    external_references: ObjectReferences,
    state: PipelineState,
    clear_color: [f32; 4],
    clear_depth: f32,
    gizmondo_goforce_20051013: bool,
    nv_combiner_client: bool,
    nv_combiner_allowed: bool,
    cull_enabled: bool,
    scissor_enabled: bool,
    scissor_box: Option<(i32, i32, i32, i32)>,
    cull_mode: CullMode,
    error: u32,
    textures: BTreeMap<u32, TextureSnapshot>,
    combiner_programs: ProgramObjectSnapshot,
}

/// Payload for an EGL window, pixmap or pbuffer. The software cache is
/// preserved for the software path. Native attachments remain authoritative
/// on hardware and include depth at the original render resolution.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SurfaceSnapshot {
    width: u32,
    height: u32,
    rgb565: bool,
    color: Vec<u8>,
    depth: Vec<f32>,
    native: Option<crate::gpu::NativeSurfaceSnapshot>,
    failed: bool,
}

impl SurfaceSnapshot {
    /// The archive, rather than the requested host policy, identifies the live backend.
    pub fn requires_native_renderer(&self) -> bool {
        self.native.is_some()
    }

    fn capture(
        target: &RenderTarget,
        renderer: Option<&crate::gpu::Renderer>,
        failed: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            width: target.width,
            height: target.height,
            rgb565: target.rgb565,
            color: target.color.clone(),
            depth: target.depth.clone(),
            native: renderer
                .map(|gpu| gpu.capture_native_surface())
                .transpose()?,
            failed,
        })
    }

    pub fn restore(self, render_policy: u8) -> Result<SurfaceBuffers, String> {
        let pixels = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or("saved surface dimensions overflow")?;
        if pixels.checked_mul(4) != Some(self.color.len())
            || (!self.depth.is_empty() && self.depth.len() != pixels)
        {
            return Err("invalid saved EGL surface geometry".into());
        }
        let gpu = self
            .native
            .map(|frame| frame.build_renderer(self.width, self.height, render_policy))
            .transpose()?;
        Ok(SurfaceBuffers {
            target: RenderTarget {
                width: self.width,
                height: self.height,
                rgb565: self.rgb565,
                color: self.color,
                depth: self.depth,
            },
            gpu,
            failed: self.failed,
        })
    }
}

impl SurfaceBuffers {
    pub fn capture_snapshot(&self) -> Result<SurfaceSnapshot, String> {
        SurfaceSnapshot::capture(&self.target, self.gpu.as_ref(), self.failed)
    }
}

impl Context {
    pub fn capture_surface(&self) -> Result<SurfaceSnapshot, String> {
        SurfaceSnapshot::capture(&self.target, self.gpu.as_ref(), self.gpu_failed)
    }

    /// Capture state independently of the draw surface. EGL owns that surface
    /// and must capture it separately, including the native depth attachment.
    pub fn capture_state(
        &self,
        reader: &mut Option<crate::gpu::SharedReadback>,
    ) -> Result<ContextSnapshot, String> {
        let textures = self
            .textures
            .iter()
            .map(|(&id, texture)| {
                texture
                    .capture_snapshot(reader)
                    .map(|texture| (id, texture))
            })
            .collect::<Result<_, _>>()?;

        Ok(ContextSnapshot {
            es11: self.es11.clone(),
            gpu_requested: self.gpu_requested,
            internal_resolution: self.internal_resolution,
            matrix_mode: self.matrix_mode,
            modelview: self.modelview.clone(),
            palette_matrices: self.palette_matrices.clone(),
            palette_index: self.palette_index,
            projection: self.projection.clone(),
            texture_matrix: self.texture_matrix.clone(),
            current_color: self.current_color,
            current_texcoord: self.current_texcoord,
            current_normal: self.current_normal,
            lighting: self.lighting,
            vertex_array: self.vertex_array,
            point_size_array: self.point_size_array,
            color_array: self.color_array,
            normal_array: self.normal_array,
            client_active_texture: self.client_active_texture,
            active_texture: self.active_texture,
            texture_units: self.texture_units,
            bound_texture: self.bound_texture,
            next_texture_name: self.next_texture_name,
            buffers: self.buffers.clone(),
            array_buffer: self.array_buffer,
            element_array_buffer: self.element_array_buffer,
            next_buffer_name: self.next_buffer_name,
            names: self.names.clone(),
            external_references: self.external_references.clone(),
            state: self.state.clone(),
            clear_color: self.clear_color,
            clear_depth: self.clear_depth,
            gizmondo_goforce_20051013: self.gizmondo_goforce_20051013,
            nv_combiner_client: self.nv_combiner_client,
            nv_combiner_allowed: self.nv_combiner_allowed,
            cull_enabled: self.cull_enabled,
            scissor_enabled: self.scissor_enabled,
            scissor_box: self.scissor_box,
            cull_mode: self.cull_mode,
            error: self.error,
            textures,
            combiner_programs: self.combiner_programs.capture_snapshot(),
        })
    }
}

impl ContextSnapshot {
    /// Reconstruct guest state. The caller attaches the restored surface
    /// before making this context current.
    pub fn restore(self) -> Result<Context, String> {
        if self.palette_index >= self.palette_matrices.len()
            || self.active_texture as usize >= MAX_TEXTURE_UNITS
            || self.client_active_texture as usize >= MAX_TEXTURE_UNITS
        {
            return Err("invalid saved GLES state index".into());
        }
        let programs = self
            .combiner_programs
            .restore()
            .map_err(|_| "invalid saved combiner program")?;
        let mut context = Context::new_rgb565(1, 1);
        context.es11 = self.es11;
        context.gpu_requested = self.gpu_requested;
        context.internal_resolution = self.internal_resolution;
        context.matrix_mode = self.matrix_mode;
        context.modelview = self.modelview;
        context.palette_matrices = self.palette_matrices;
        context.palette_index = self.palette_index;
        context.projection = self.projection;
        context.texture_matrix = self.texture_matrix;
        context.current_color = self.current_color;
        context.current_texcoord = self.current_texcoord;
        context.current_normal = self.current_normal;
        context.lighting = self.lighting;
        context.vertex_array = self.vertex_array;
        context.point_size_array = self.point_size_array;
        context.color_array = self.color_array;
        context.normal_array = self.normal_array;
        context.client_active_texture = self.client_active_texture;
        context.active_texture = self.active_texture;
        context.texture_units = self.texture_units;
        context.bound_texture = self.bound_texture;
        context.next_texture_name = self.next_texture_name;
        context.buffers = self.buffers;
        context.array_buffer = self.array_buffer;
        context.element_array_buffer = self.element_array_buffer;
        context.next_buffer_name = self.next_buffer_name;
        context.names = self.names;
        context.external_references = self.external_references;
        context.state = self.state;
        context.clear_color = self.clear_color;
        context.clear_depth = self.clear_depth;
        context.gizmondo_goforce_20051013 = self.gizmondo_goforce_20051013;
        context.nv_combiner_client = self.nv_combiner_client;
        context.nv_combiner_allowed = self.nv_combiner_allowed;
        context.cull_enabled = self.cull_enabled;
        context.scissor_enabled = self.scissor_enabled;
        context.scissor_box = self.scissor_box;
        context.cull_mode = self.cull_mode;
        context.error = self.error;
        context.combiner_programs = programs;
        context.textures = self
            .textures
            .into_iter()
            .map(|(id, texture)| texture.restore().map(|texture| (id, texture)))
            .collect::<Result<_, _>>()?;
        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_state_restores_vbo_and_retains_deleted_texture_objects() {
        let mut context = Context::new_rgb565(4, 4);
        context.bind_buffer(GL_ARRAY_BUFFER, 18);
        context.buffer_data(GL_ARRAY_BUFFER, 4, Some(&[1, 2, 3, 4]), GL_STATIC_DRAW);
        context.vertex_array.buffer = context.array_buffer;
        context.bind_buffer(GL_ARRAY_BUFFER, 0);
        context.bind_texture(GL_TEXTURE_2D, 12);
        let id = context.bound_texture;
        context
            .textures
            .get_mut(&id)
            .unwrap()
            .set_level(0, 1, 1, vec![10, 20, 30, 255]);
        // Keep a local reference after a peer removes the guest-visible name.
        let refs = context.object_references();
        context.set_external_object_references(refs);
        context.delete_textures(&[12]);
        context.current_color = [1., 0.25, 0.5, 1.];
        assert!(context.modelview.push());
        let mut matrix = matrix::IDENTITY;
        matrix[12] = 2.;
        context.modelview.load(matrix);

        let bytes = serde_json::to_vec(&context.capture_state(&mut None).unwrap()).unwrap();
        let snapshot: ContextSnapshot = serde_json::from_slice(&bytes).unwrap();
        let mut restored = snapshot.restore().unwrap();
        assert_eq!(
            restored.buffers[&restored.vertex_array.buffer].data,
            [1, 2, 3, 4]
        );
        assert_eq!(restored.current_color, [1., 0.25, 0.5, 1.]);
        assert_eq!(restored.modelview.current(), &matrix);
        assert!(restored.modelview.pop());
        assert_eq!(restored.modelview.current(), &matrix::IDENTITY);
        assert_eq!(restored.textures[&id].rgba, [10, 20, 30, 255]);
        assert!(restored.texture_object_ids(&[12]).is_empty());
    }
}
