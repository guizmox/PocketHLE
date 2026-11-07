//! Pixel preparation in the host GPU, independently of guest drawing state.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Quantization {
    Preserve,
    DisplayRgb565,
    LogicalRgb565,
}

pub(super) struct TransferPass {
    program: u32,
    buffer: u32,
    source: i32,
    output_size: i32,
    image_size: i32,
}

impl Renderer {
    pub(super) unsafe fn transfer_pixels(
        &mut self,
        texture: u32,
        destination: u32,
        width: u32,
        height: u32,
        reduction: u32,
        flip: bool,
        quantize: Quantization,
        source_size: (u32, u32),
    ) -> Result<(), String> {
        let key = (reduction, flip, quantize);
        if !self.transfers.contains_key(&key) {
            let fragment = if reduction == 0 {
                String::from("#ifdef GL_ES\nprecision highp float;\n#endif\nuniform sampler2D sourceImage;uniform vec2 outputSize,imageSize;vec2 encode565(float index){vec2 uv=(vec2(mod(index,imageSize.x),floor(index/imageSize.x))+vec2(0.5))/imageSize;vec3 b=floor(texture2D(sourceImage,uv).rgb*255.0+0.5);vec3 q=floor(b/vec3(8.0,4.0,8.0));float v=q.r*2048.0+q.g*32.0+q.b;return vec2(mod(v,256.0),floor(v/256.0))/255.0;}void main(){float i=2.0*(floor(gl_FragCoord.y)*outputSize.x+floor(gl_FragCoord.x));gl_FragColor=vec4(encode565(i),encode565(i+1.0));}")
            } else {
                let mut fragment = String::from(
                "#ifdef GL_ES\nprecision highp float;\n#endif\nuniform sampler2D sourceImage;uniform vec2 outputSize;void main(){vec2 p=gl_FragCoord.xy;vec4 sum=vec4(0.0);",
            );
                if flip {
                    fragment.push_str("p.y=outputSize.y-p.y;");
                }
                for y in 0..reduction {
                    for x in 0..reduction {
                        fragment.push_str(&format!(
                        "sum+=floor(texture2D(sourceImage,(floor(p)*{reduction}.0+vec2({x}.5,{y}.5))/(outputSize*{reduction}.0))*255.0+0.5);"
                    ));
                    }
                }
                fragment.push_str(&format!(
                    "vec4 b=floor(sum/{}.0+0.015625);",
                    reduction * reduction
                ));
                if quantize == Quantization::DisplayRgb565 {
                    fragment.push_str("vec3 q=floor(b.rgb/vec3(8.0,4.0,8.0));b=vec4(q*vec3(8.0,4.0,8.0)+floor(q/vec3(4.0,16.0,4.0)),255.0);");
                }
                if quantize == Quantization::LogicalRgb565 {
                    fragment.push_str("vec3 levels=vec3(31.0,63.0,31.0);vec3 q=floor((b.rgb*levels+127.0)/255.0);b=vec4(floor((q*255.0+floor(levels/2.0))/levels),255.0);");
                }
                fragment.push_str("gl_FragColor=b/255.0;}");
                fragment
            };
            // compile_program supplies the host's desktop/ES precision prefix.
            let program = self.compile_program(
                "attribute vec2 position;void main(){gl_Position=vec4(position,0.0,1.0);}",
                &fragment,
            )?;
            let mut buffer = 0;
            (self.gl.GenBuffers)(1, &mut buffer);
            if buffer == 0 {
                (self.gl.DeleteProgram)(program);
                return Err("pixel transfer buffer allocation failed".into());
            }
            (self.gl.BindBuffer)(0x8892, buffer);
            let quad: [f32; 8] = [-1., -1., 1., -1., -1., 1., 1., 1.];
            (self.gl.BufferData)(0x8892, 32, quad.as_ptr().cast(), 0x88e4);
            let pass = TransferPass {
                source: (self.gl.GetUniformLocation)(program, c"sourceImage".as_ptr()),
                output_size: (self.gl.GetUniformLocation)(program, c"outputSize".as_ptr()),
                image_size: (self.gl.GetUniformLocation)(program, c"imageSize".as_ptr()),
                program,
                buffer,
            };
            self.transfers.insert(key, pass);
        }
        let pass = &self.transfers[&key];
        let mut dither = 0;
        (self.gl.GetIntegerv)(0x0bd0, &mut dither);
        (self.gl.BindFramebuffer)(0x8d40, destination);
        (self.gl.Viewport)(0, 0, width as i32, height as i32);
        for cap in [
            GL_BLEND,
            GL_DEPTH_TEST,
            GL_CULL_FACE,
            GL_SCISSOR_TEST,
            GL_POLYGON_OFFSET_FILL,
            0x0bd0,
        ] {
            (self.gl.Disable)(cap);
        }
        (self.gl.ColorMask)(1, 1, 1, 1);
        (self.gl.UseProgram)(pass.program);
        (self.gl.ActiveTexture)(GL_TEXTURE0);
        (self.gl.BindTexture)(GL_TEXTURE_2D, texture);
        (self.gl.Uniform1i)(pass.source, 0);
        (self.gl.Uniform2f)(pass.output_size, width as f32, height as f32);
        (self.gl.Uniform2f)(pass.image_size, source_size.0 as f32, source_size.1 as f32);
        (self.gl.BindBuffer)(0x8892, pass.buffer);
        for index in 0..8 {
            (self.gl.DisableVertexAttribArray)(index);
        }
        (self.gl.EnableVertexAttribArray)(0);
        (self.gl.VertexAttribPointer)(0, 2, GL_FLOAT, 0, 8, std::ptr::null());
        (self.gl.DrawArrays)(GL_TRIANGLE_STRIP, 0, 4);
        let result = self.check();
        (self.gl.BindFramebuffer)(0x8d40, self.fbo);
        (self.gl.Viewport)(0, 0, self.width as i32, self.height as i32);
        (self.gl.UseProgram)(self.program);
        (self.gl.BindBuffer)(0x8892, self.buffer);
        (self.gl.BindBuffer)(0x8893, self.index_buffer);
        if dither != 0 {
            (self.gl.Enable)(0x0bd0);
        }
        self.pipeline = None;
        self.attribute_layout = None;
        self.stream_layout = None;
        result
    }

    unsafe fn transfer_target(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32, u32, u32), String> {
        let (mut fbo, mut texture) = (0, 0);
        (self.gl.GenFramebuffers)(1, &mut fbo);
        (self.gl.GenTextures)(1, &mut texture);
        (self.gl.ActiveTexture)(GL_TEXTURE0);
        (self.gl.BindTexture)(GL_TEXTURE_2D, texture);
        (self.gl.TexImage2D)(
            GL_TEXTURE_2D,
            0,
            GL_RGBA as i32,
            width as i32,
            height as i32,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            std::ptr::null(),
        );
        self.params(
            Filter::Nearest,
            Filter::Nearest,
            Wrap::ClampToEdge,
            Wrap::ClampToEdge,
        );
        (self.gl.BindFramebuffer)(0x8d40, fbo);
        (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, texture, 0);
        let complete = (self.gl.CheckFramebufferStatus)(0x8d40) == 0x8cd5;
        (self.gl.BindFramebuffer)(0x8d40, self.fbo);
        if !complete {
            (self.gl.DeleteFramebuffers)(1, &fbo);
            (self.gl.DeleteTextures)(1, &texture);
            return Err("pixel transfer framebuffer incomplete".into());
        }
        Ok((fbo, texture, width, height))
    }

    pub(super) unsafe fn pack_rgb565_readback(
        &mut self,
        texture: u32,
        source_width: u32,
        source_height: u32,
    ) -> Result<(), String> {
        let width = source_width.div_ceil(2);
        let height = source_height;
        if self
            .packed_target
            .is_some_and(|(_, _, w, h)| (w, h) != (width, height))
        {
            let (fbo, texture, _, _) = self.packed_target.take().unwrap();
            (self.gl.DeleteFramebuffers)(1, &fbo);
            (self.gl.DeleteTextures)(1, &texture);
        }
        if self.packed_target.is_none() {
            self.packed_target = Some(self.transfer_target(width, height)?);
        }
        let fbo = self.packed_target.unwrap().0;
        self.transfer_pixels(
            texture,
            fbo,
            width,
            height,
            0,
            false,
            Quantization::Preserve,
            (source_width, source_height),
        )?;
        self.packed_readback
            .resize(width as usize * height as usize * 4, 0);
        (self.gl.BindFramebuffer)(0x8d40, fbo);
        (self.gl.PixelStorei)(0x0d05, 1);
        (self.gl.ReadPixels)(
            0,
            0,
            width as i32,
            height as i32,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            self.packed_readback.as_mut_ptr().cast(),
        );
        let result = self.check();
        (self.gl.BindFramebuffer)(0x8d40, self.fbo);
        result?;
        self.packed_readback
            .truncate(source_width as usize * source_height as usize * 2);
        Ok(())
    }

    pub(super) unsafe fn prepare_display_readback(&mut self) -> Result<(), String> {
        if self
            .display_target
            .is_some_and(|(_, _, width, height)| (width, height) != (self.width, self.height))
        {
            let (fbo, texture, _, _) = self.display_target.take().unwrap();
            (self.gl.DeleteFramebuffers)(1, &fbo);
            (self.gl.DeleteTextures)(1, &texture);
        }
        if self.display_target.is_none() {
            self.display_target = Some(self.transfer_target(self.width, self.height)?);
        }
        let fbo = self.display_target.unwrap().0;
        self.transfer_pixels(
            self.color,
            fbo,
            self.width,
            self.height,
            1,
            true,
            Quantization::DisplayRgb565,
            (self.width, self.height),
        )?;
        self.readback
            .resize(self.width as usize * self.height as usize * 4, 0);
        (self.gl.BindFramebuffer)(0x8d40, fbo);
        (self.gl.PixelStorei)(0x0d05, 1);
        (self.gl.ReadPixels)(
            0,
            0,
            self.width as i32,
            self.height as i32,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            self.readback.as_mut_ptr().cast(),
        );
        let result = self.check();
        (self.gl.BindFramebuffer)(0x8d40, self.fbo);
        result?;
        self.pack_rgb565_readback(self.display_target.unwrap().1, self.width, self.height)?;
        self.display_prepared = true;
        Ok(())
    }

    pub(super) unsafe fn delete_transfers(&mut self) {
        if let Some((fbo, texture, _, _)) = self.display_target.take() {
            (self.gl.DeleteFramebuffers)(1, &fbo);
            (self.gl.DeleteTextures)(1, &texture);
        }
        if let Some((fbo, texture, _, _)) = self.packed_target.take() {
            (self.gl.DeleteFramebuffers)(1, &fbo);
            (self.gl.DeleteTextures)(1, &texture);
        }
        for (_, pass) in self.transfers.drain() {
            (self.gl.DeleteProgram)(pass.program);
            (self.gl.DeleteBuffers)(1, &pass.buffer);
        }
    }
}
