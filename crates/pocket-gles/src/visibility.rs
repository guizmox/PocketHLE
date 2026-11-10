//! Internal coverage for the empirical GoForce 4500 visibility replay.
//! A replay may repair occlusion against a preceding batch, but must not paint
//! over fragments that the current batch already made visible normally.
use crate::{
    matrix::Matrix4,
    raster::{PipelineState, TextureStage},
    texture::Texture,
};
use std::collections::HashMap;

#[derive(Clone, PartialEq)]
struct TextureStamp {
    revision: u64,
    size: (u32, u32),
    filters: (crate::texture::Filter, crate::texture::Filter),
    wraps: (crate::texture::Wrap, crate::texture::Wrap),
    levels: (u32, u32),
    lod: (f32, f32),
}
impl From<&Texture> for TextureStamp {
    fn from(t: &Texture) -> Self {
        Self {
            revision: t.revision,
            size: (t.width, t.height),
            filters: (t.min_filter, t.mag_filter),
            wraps: (t.wrap_s, t.wrap_t),
            levels: (t.base_level, t.max_level),
            lod: (t.min_lod, t.max_lod),
        }
    }
}
#[derive(Clone, PartialEq)]
pub(crate) struct Key {
    state: PipelineState,
    names: [u32; 6],
    revisions: [Option<TextureStamp>; 6],
    projection: Matrix4,
    modelview: Matrix4,
    stages: [Option<TextureStage>; 6],
}
impl Key {
    pub(crate) fn new(
        state: &PipelineState,
        names: [u32; 6],
        projection: Matrix4,
        modelview: Matrix4,
        stages: &[TextureStage],
        textures: &HashMap<u32, Texture>,
    ) -> Self {
        Self {
            state: state.clone(),
            names,
            revisions: names.map(|n| textures.get(&n).map(TextureStamp::from)),
            projection,
            modelview,
            stages: std::array::from_fn(|i| stages.get(i).copied()),
        }
    }
}
#[derive(Default)]
pub(crate) struct Coverage {
    key: Option<Key>,
    pixels: Vec<u8>,
    mode: u8,
    stamp: u8,
}
impl Coverage {
    pub(crate) fn begin(&mut self, key: Key, count: usize) {
        if self.key.as_ref() != Some(&key) || self.pixels.len() != count {
            self.key = Some(key);
            if self.pixels.len() != count {
                self.pixels.resize(count, 0);
                self.pixels.fill(0);
                self.stamp = 0;
            }
            self.stamp = self.stamp.wrapping_add(1);
            if self.stamp == 0 {
                self.pixels.fill(0);
                self.stamp = 1;
            }
        }
    }
    pub(crate) fn pass(&mut self, replay: bool) {
        self.mode = if replay { 2 } else { 1 };
    }
    pub(crate) fn finish(&mut self) {
        self.mode = 0;
    }
    pub(crate) fn reset(&mut self) {
        self.key = None;
        self.mode = 0;
    }
    #[inline]
    pub(crate) fn accept(&mut self, pixel: usize) -> bool {
        match self.mode {
            1 => {
                self.pixels[pixel] = self.stamp;
                true
            }
            2 => self.pixels[pixel] != self.stamp,
            _ => true,
        }
    }
}
