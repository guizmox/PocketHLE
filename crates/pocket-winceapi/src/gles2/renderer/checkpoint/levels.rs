//! ES 3.0 cannot query a texture level's size or read compressed blocks back.
//! Keep allocation metadata and current compressed blocks, never a call history.
use super::*;
#[derive(Clone)]
pub(in crate::gles2::renderer) struct Level {
    pub width: i32,
    pub height: i32,
    pub internal: u32,
    pub blocks: Option<Vec<u8>>,
    pub unsupported_blocks: bool,
}
impl Renderer {
    pub(in crate::gles2::renderer) fn accepted_image_call(&mut self) -> Result<bool> {
        let code = invoke!(self, "glGetError", (), u32);
        if code != 0 {
            self.reject(code);
        }
        Ok(code == 0)
    }
    fn image_binding(&mut self, face: u32) -> Result<u32> {
        integer(self, if face == TEXTURE_2D { 0x8069 } else { 0x8514 }).map(|v| v as u32)
    }
    pub(in crate::gles2::renderer) fn defined_level(
        &mut self,
        face: u32,
        level: i32,
        width: i32,
        height: i32,
        internal: u32,
        blocks: Option<Vec<u8>>,
    ) -> Result<()> {
        let texture = self.image_binding(face)?;
        self.texture_levels.insert(
            (texture, face, level),
            Level {
                width,
                height,
                internal,
                blocks,
                unsupported_blocks: false,
            },
        );
        Ok(())
    }
    pub(in crate::gles2::renderer) fn generated_levels(&mut self, target: u32) -> Result<()> {
        let texture = self.image_binding(target)?;
        for face in if target == TEXTURE_2D {
            vec![target]
        } else {
            (0x8515..=0x851a).collect()
        } {
            let Some(mut base) = self.texture_levels.get(&(texture, face, 0)).cloned() else {
                continue;
            };
            let mut level = 0;
            while base.width > 1 || base.height > 1 {
                level += 1;
                base.width = (base.width / 2).max(1);
                base.height = (base.height / 2).max(1);
                // ES 2 forbids generation from compressed storage. A permissive
                // host must still never produce an unverifiable checkpoint.
                base.unsupported_blocks = base.blocks.is_some() || base.unsupported_blocks;
                base.blocks = None;
                self.texture_levels
                    .insert((texture, face, level), base.clone());
            }
        }
        Ok(())
    }
    pub(in crate::gles2::renderer) fn updated_blocks(
        &mut self,
        face: u32,
        level: i32,
        rect: [i32; 4],
        bytes: &[u8],
    ) -> Result<()> {
        let texture = self.image_binding(face)?;
        let Some(image) = self.texture_levels.get_mut(&(texture, face, level)) else {
            return Err(error("missing compressed texture allocation"));
        };
        // The driver accepted this call; an empty update cannot invalidate or
        // repair a previously captured compressed image.
        if rect[2] == 0 || rect[3] == 0 {
            return Ok(());
        }
        if rect == [0, 0, image.width, image.height] {
            let blocks = image.blocks.get_or_insert_with(Vec::new);
            blocks.clear();
            blocks.extend_from_slice(bytes);
            image.unsupported_blocks = false;
            return Ok(());
        }
        let Some(blocks) = image.blocks.as_mut() else {
            return Ok(());
        };
        if !super::blocks::update(
            image.internal,
            [image.width, image.height],
            rect,
            blocks,
            bytes,
        ) {
            image.unsupported_blocks = true;
        }
        Ok(())
    }
}
