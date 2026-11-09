//! Bounded combiner observations; never materialize a GPU texture or framebuffer.
use super::*;
use crate::combiner_program::diagnostic_fingerprint;

#[derive(Default)]
pub(super) struct CombinerTrace {
    draws: HashMap<u64, u64>,
}

impl CombinerTrace {
    fn sample(&mut self, id: u64) -> Option<u64> {
        // Bound both retained program identities and output. Context teardown
        // resets this state; shader reloads cannot create an unbounded trace.
        if !self.draws.contains_key(&id) && self.draws.len() >= 128 {
            return None;
        }
        let draw = self.draws.entry(id).or_default();
        *draw = draw.saturating_add(1);
        matches!(*draw, 1 | 2 | 16 | 256).then_some(*draw)
    }
}

impl Context {
    pub(super) fn trace_combiner_draw(&mut self, mode: u32, count: usize) {
        let Some(program) = self.combiner_programs.active() else {
            return;
        };
        let Some(draw) = self.diagnostic_combiners.sample(program.id()) else {
            return;
        };
        let key = program.diagnostic_fingerprint();
        log::info!(
            "NV_DIAG event=draw program={key:016x} sample={draw} gpu={} mode=0x{mode:04x} count={count} target={}x{} framebuffer_fetch={} textures=0x{:x} coordinates=0x{:x} constants={:?} viewport={:?} blend={} alpha_test={} depth_test={} color_mask={:?}",
            self.gpu.is_some(), self.target.width, self.target.height,
            program.reads_framebuffer(), program.texture_units(), program.coordinate_units(),
            self.combiner_programs.constants(), self.state.viewport,
            self.state.blend, self.state.alpha_test, self.state.depth_test, self.state.color_mask,
        );
        if program.reads_framebuffer() && self.gpu.is_none() {
            let prefix = &self.target.color[..self.target.color.len().min(65536)];
            let center = self.target.color.len() / 8 * 4;
            log::info!("NV_DIAG event=source_framebuffer program={key:016x} sample={draw} rgba_bytes={} prefix_bytes={} prefix_hash={:016x} center={:?}", self.target.color.len(), prefix.len(), diagnostic_fingerprint(prefix), self.target.color.get(center..center.saturating_add(4)));
        }
        log::info!("NV_DIAG event=transform program={key:016x} sample={draw} color={:?} modelview={:?} projection={:?}", self.current_color, self.modelview.current(), self.projection.current());
        for unit in 0..MAX_TEXTURE_UNITS {
            if program.coordinate_units() & (1 << unit) != 0 {
                log::info!("NV_DIAG event=coordinate program={key:016x} sample={draw} unit={unit} array={:?} matrix={:?}", self.texture_units[unit].texcoord_array, self.texture_matrix[unit].current());
            }
        }
        for unit in 0..MAX_TEXTURE_UNITS {
            if program.texture_units() & (1 << unit) == 0 {
                continue;
            }
            let name = self.texture_units[unit].bound_texture;
            let Some(texture) = self.textures.get(&name) else {
                log::info!("NV_DIAG event=texture program={key:016x} sample={draw} unit={unit} name={name} missing=true");
                continue;
            };
            // Prefix hashes are explicitly labelled: this is not a checksum of
            // the whole image. No decode, allocation or readback is requested.
            let prefix = &texture.rgba[..texture.rgba.len().min(65536)];
            let hash = diagnostic_fingerprint(prefix);
            log::info!(
                "NV_DIAG event=texture program={key:016x} sample={draw} unit={unit} name={name} size={}x{} complete={} revision={} formats={:?} rgba_bytes={} rgba_prefix_bytes={} rgba_prefix_hash={hash:016x} gpu_copy={} min={:?} mag={:?} base={} max={} lod={:?} goforce={}",
                texture.width, texture.height, texture.is_complete(), texture.revision,
                texture.formats, texture.rgba.len(), prefix.len(), texture.gpu_copy.is_some(),
                texture.min_filter, texture.mag_filter, texture.base_level, texture.max_level,
                [texture.min_lod, texture.max_lod], texture.goforce_lod,
            );
            for (level, payload) in &texture.compressed {
                let prefix = &payload.bytes[..payload.bytes.len().min(65536)];
                log::info!(
                    "NV_DIAG event=compressed program={key:016x} sample={draw} unit={unit} level={level} size={}x{} format=0x{:x} bytes={} prefix_bytes={} prefix_hash={:016x}",
                    payload.width, payload.height, payload.format, payload.bytes.len(),
                    prefix.len(), diagnostic_fingerprint(prefix),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_is_bounded_and_keeps_later_observations() {
        let mut trace = CombinerTrace::default();
        let samples: Vec<_> = (0..1000).filter_map(|_| trace.sample(1)).collect();
        assert_eq!(samples, [1, 2, 16, 256]);
        for id in 2..=128 {
            assert_eq!(trace.sample(id), Some(1));
        }
        assert_eq!(trace.sample(129), None);
        assert_eq!(trace.draws.len(), 128);
    }

    #[test]
    fn framebuffer_observation_does_not_change_rendering_state() {
        use crate::combiner_program::{Architecture, Program, Stage};
        let mut context = Context::new(8, 8);
        let program = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![Stage {
                alpha_output: 0x0670,
                alpha_inputs: 0x0404_0000,
                flags: 0xe0,
                color_output: 0x8070,
                color_input_modifiers: 0x0200,
                color_inputs: 0xe484_0000,
                color_input_modes: 0x8000,
                texture_routing: 0x7fff_ffff,
            }],
        }
        .compile_basic()
        .unwrap();
        assert!(program.reads_framebuffer());
        context.combiner_programs.load_bound(program, 4).unwrap();
        context.combiner_programs.set_enabled(true);
        context.target.color.fill(123);
        let before = context.target.color.clone();
        let constants = context.combiner_programs.constants();
        crate::graphics_diagnostic::with_enabled(true, || {
            context.trace_combiner_draw(GL_TRIANGLES, 6)
        });
        assert_eq!(context.target.color, before);
        assert_eq!(context.combiner_programs.constants(), constants);
        assert!(context.combiner_programs.active().is_some());
    }

    #[test]
    fn fingerprints_are_byte_order_and_word_size_independent() {
        assert_eq!(diagnostic_fingerprint(b""), 0xcbf29ce484222325);
        assert_eq!(diagnostic_fingerprint(b"hello"), 0xa430d84680aabd0b);
    }
}
