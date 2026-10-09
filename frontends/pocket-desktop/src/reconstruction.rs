//! Native GPU reconstruction, cached SMAA, and bounded xBRZ CPU preprocessing.
use eframe::glow::{self, HasContext};
use crate::runner::FrameSnapshot;

pub struct Reconstruction {
    program: glow::Program,
    vao: glow::VertexArray,
    texture: glow::Texture,
    size: [u32;2],
    pending: Option<FrameSnapshot>,
    smaa: Option<crate::smaa::Smaa>,
    smaa_dirty: bool,
    smaa_mode: i32,
}
impl Reconstruction {
    pub fn new(gl: &glow::Context) -> Result<Self,String> {
        let prefix = if gl.version().is_embedded {
            "#version 300 es\nprecision highp float;\nprecision highp int;\n"
        } else { "#version 140\n" };
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind,source) in [(glow::VERTEX_SHADER,include_str!("shaders/reconstruction.vert")),
                (glow::FRAGMENT_SHADER,include_str!("shaders/reconstruction.frag"))] {
                let shader = match gl.create_shader(kind) {
                    Ok(s) => s,
                    Err(error) => { for s in shaders { gl.delete_shader(s); } gl.delete_program(program); return Err(error); }
                };
                gl.shader_source(shader,&format!("{prefix}{source}"));
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let error = gl.get_shader_info_log(shader);
                    gl.delete_shader(shader);
                    for s in shaders { gl.delete_shader(s); }
                    gl.delete_program(program);
                    return Err(error);
                }
                gl.attach_shader(program,shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            for shader in shaders { gl.detach_shader(program,shader); gl.delete_shader(shader); }
            if !gl.get_program_link_status(program) {
                let error = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(error);
            }
            let vao = match gl.create_vertex_array() { Ok(v)=>v,Err(e)=>{gl.delete_program(program);return Err(e);} };
            let texture = match gl.create_texture() { Ok(t)=>t,Err(e)=>{gl.delete_vertex_array(vao);gl.delete_program(program);return Err(e);} };
            let smaa = match crate::smaa::Smaa::new(gl) {
                Ok(s) => Some(s),
                Err(e) => { log::warn!("SMAA unavailable, using reconstruction: {e}"); None }
            };
            Ok(Self { program,vao,texture,size:[0,0],pending:None,smaa,smaa_dirty:true,smaa_mode:-1 })
        }
    }
    pub fn queue(&mut self,frame:&FrameSnapshot,filter:i32) {
        self.pending=Some(prepare_frame(frame,filter));
        self.smaa_dirty=true;
    }
    pub fn paint(&mut self,gl:&glow::Context,uv:[f32;6],filter:i32) {
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D,Some(self.texture));
            if let Some(frame)=self.pending.take() {
                if frame.rgba.len()!=frame.width as usize*frame.height as usize*4 { return; }
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT,1);
                if self.size==[frame.width,frame.height] {
                    gl.tex_sub_image_2d(glow::TEXTURE_2D,0,0,0,frame.width as i32,frame.height as i32,
                        glow::RGBA,glow::UNSIGNED_BYTE,glow::PixelUnpackData::Slice(&frame.rgba));
                } else {
                    gl.tex_image_2d(glow::TEXTURE_2D,0,glow::RGBA8 as i32,frame.width as i32,frame.height as i32,
                        0,glow::RGBA,glow::UNSIGNED_BYTE,Some(&frame.rgba));
                    self.size=[frame.width,frame.height];
                }
                for name in [glow::TEXTURE_MIN_FILTER,glow::TEXTURE_MAG_FILTER] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D,name,glow::LINEAR as i32);
                }
                for name in [glow::TEXTURE_WRAP_S,glow::TEXTURE_WRAP_T] {
                    gl.tex_parameter_i32(glow::TEXTURE_2D,name,glow::CLAMP_TO_EDGE as i32);
                }
            }
            if self.size[0]==0 { return; }
            let mut output_texture=self.texture;
            let output_filter=match filter { 5=>3,6|7=>1,_=>filter };
            if filter==5 || filter==7 {
                if let Some(smaa)=self.smaa.as_mut() {
                    if self.smaa_dirty || self.smaa_mode!=filter {
                        if let Err(e)=smaa.process(gl,self.texture,self.size,filter==7) {
                            log::warn!("SMAA render target unavailable: {e}");
                        }
                        self.smaa_dirty=false; self.smaa_mode=filter;
                    }
                    if let Some(texture)=smaa.output(self.size) { output_texture=texture; }
                }
            }
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D,Some(output_texture));
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.uniform_1_i32(gl.get_uniform_location(self.program,"u_frame").as_ref(),0);
            gl.uniform_1_i32(gl.get_uniform_location(self.program,"u_filter").as_ref(),output_filter);
            gl.uniform_2_f32(gl.get_uniform_location(self.program,"u_size").as_ref(),self.size[0] as f32,self.size[1] as f32);
            for (name,offset) in [("u_origin",0),("u_dx",2),("u_dy",4)] {
                gl.uniform_2_f32(gl.get_uniform_location(self.program,name).as_ref(),uv[offset],uv[offset+1]);
            }
            gl.draw_arrays(glow::TRIANGLES,0,3);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_texture(glow::TEXTURE_2D,None);
        }
    }
    pub fn destroy(&mut self,gl:&glow::Context) {
        if let Some(smaa)=self.smaa.take() { smaa.destroy(gl); }
        unsafe { gl.delete_texture(self.texture); gl.delete_vertex_array(self.vao); gl.delete_program(self.program); }
    }
}

fn prepare_frame(frame:&FrameSnapshot,filter:i32)->FrameSnapshot {
    let mut output=frame.clone();
    if filter==6 && frame.width>0 && frame.height>0 &&
        frame.rgba.len()==frame.width as usize*frame.height as usize*4 {
        // Bound CPU work to x3; monitor scaling remains a separate GPU operation.
        output.rgba=xbrz::scale_rgba(&frame.rgba,frame.width as usize,frame.height as usize,3);
        output.width*=3;output.height*=3;
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xbrz_blends_diagonals_without_mutating_native_frame() {
        let mut rgba=vec![0;16*16*4];
        for y in 0..16 { for x in 0..16 {
            let c=if x>y {255} else {0};let i=(y*16+x)*4;
            rgba[i..i+4].copy_from_slice(&[c,c,c,255]);
        }}
        let native=FrameSnapshot{width:16,height:16,rgba};
        let scaled=prepare_frame(&native,6);
        assert_eq!((scaled.width,scaled.height),(48,48));
        assert_eq!(scaled.rgba.len(),48*48*4);
        assert!(scaled.rgba.chunks_exact(4).any(|p|p[0]>0 && p[0]<255));
        let pixel=|x:usize,y:usize| &scaled.rgba[(y*48+x)*4..(y*48+x)*4+3];
        assert_eq!(pixel(40,6),[255,255,255]);
        assert_eq!(pixel(6,40),[0,0,0]);
        // Switching to SMAA or another GPU mode must upload the original image.
        for mode in [0,1,2,3,4,5,7] {
            let next=prepare_frame(&native,mode);
            assert_eq!((next.width,next.height),(16,16));assert_eq!(next.rgba,native.rgba);
        }
    }
}
