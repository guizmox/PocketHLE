//! A deleted image can outlive its GL name through an unbound framebuffer.
//! Private FBOs retain that same GPU image, without copying or reading pixels.
use super::*;

#[derive(Default)]
pub(super) struct RetainedImages {
    pub(super) next: u64,
    pub(super) images: HashMap<u64, KeptImage>,
    pub(super) attachments: HashMap<(u32, u32), u64>,
    pub(super) failure: Option<String>,
}
#[derive(Clone)]
pub(super) struct KeptImage {
    pub(super) native: u32,
    pub(super) keeper: u32,
    pub(super) kind: u32,
    pub(super) guest: u32,
    pub(super) face: u32,
    pub(super) level: i32,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) internal: u32,
}
impl Renderer {
    pub(super) fn preserve_native_errors(&mut self) -> Result<()> {
        for _ in 0..8 {
            let value = invoke!(self, "glGetError", (), u32);
            if value == 0 {
                break;
            }
            self.reject(value);
        }
        Ok(())
    }
    pub(super) fn retain_deleted_images(
        &mut self,
        kind: Kind,
        deleted: &[(u32, u32)],
    ) -> Result<()> {
        if self.host.es || deleted.is_empty() || self.guest_framebuffers.is_empty() {
            return Ok(());
        }
        self.preserve_native_errors()?;
        let bound = if self.framebuffer == 0 {
            self.default_fbo
        } else {
            self.framebuffer
        };
        let mut texture_2d = 0;
        let mut texture_cube = 0;
        let mut renderbuffer = 0;
        invoke!(self,"glGetIntegerv",(0x8069=>u32,&mut texture_2d=>*mut i32),());
        invoke!(self,"glGetIntegerv",(0x8514=>u32,&mut texture_cube=>*mut i32),());
        invoke!(self,"glGetIntegerv",(0x8ca7=>u32,&mut renderbuffer=>*mut i32),());
        let object_kind = if matches!(kind, Kind::Renderbuffer) {
            RENDERBUFFER
        } else {
            0x1702
        };
        let mut registered = HashMap::new();
        let result = (|| -> Result<()> {
            for framebuffer in self.guest_framebuffers.clone() {
                // GL deletes attachments from the currently bound FBO itself.
                if framebuffer == bound
                    || invoke!(self,"glIsFramebuffer",(framebuffer=>u32),u8) == 0
                {
                    continue;
                }
                for point in [0x8ce0, 0x8d00, 0x8d20] {
                    if self
                        .retained
                        .attachments
                        .contains_key(&(framebuffer, point))
                    {
                        continue;
                    }
                    invoke!(self,"glBindFramebuffer",(FRAMEBUFFER=>u32,framebuffer=>u32),());
                    let mut actual_kind = 0;
                    let mut name = 0;
                    invoke!(self,"glGetFramebufferAttachmentParameteriv",(FRAMEBUFFER=>u32,point=>u32,0x8cd0=>u32,&mut actual_kind=>*mut i32),());
                    if actual_kind as u32 != object_kind {
                        continue;
                    }
                    invoke!(self,"glGetFramebufferAttachmentParameteriv",(FRAMEBUFFER=>u32,point=>u32,0x8cd1=>u32,&mut name=>*mut i32),());
                    let Some(&(_, guest)) = deleted.iter().find(|&&(host, _)| host == name as u32)
                    else {
                        continue;
                    };
                    let mut level = 0;
                    let mut face = 0;
                    if object_kind != RENDERBUFFER {
                        invoke!(self,"glGetFramebufferAttachmentParameteriv",(FRAMEBUFFER=>u32,point=>u32,0x8cd2=>u32,&mut level=>*mut i32),());
                        invoke!(self,"glGetFramebufferAttachmentParameteriv",(FRAMEBUFFER=>u32,point=>u32,0x8cd3=>u32,&mut face=>*mut i32),());
                    }
                    let key = (name, face, level);
                    if let Some(&id) = registered.get(&key) {
                        self.retained.attachments.insert((framebuffer, point), id);
                        continue;
                    }
                    let mut dimensions = [0; 3];
                    if object_kind == RENDERBUFFER {
                        invoke!(self,"glBindRenderbuffer",(RENDERBUFFER=>u32,name as u32=>u32),());
                        for (pname, value) in
                            [0x8d42, 0x8d43, 0x8d44].into_iter().zip(&mut dimensions)
                        {
                            invoke!(self,"glGetRenderbufferParameteriv",(RENDERBUFFER=>u32,pname=>u32,value=>*mut i32),());
                        }
                    } else {
                        let target = if face == 0 { TEXTURE_2D } else { 0x8513 };
                        let image_target = if face == 0 { TEXTURE_2D } else { face as u32 };
                        invoke!(self,"glBindTexture",(target=>u32,name as u32=>u32),());
                        for (pname, value) in
                            [0x1000, 0x1001, 0x1003].into_iter().zip(&mut dimensions)
                        {
                            invoke!(self,"glGetTexLevelParameteriv",(image_target=>u32,level=>i32,pname=>u32,value=>*mut i32),());
                        }
                    }
                    let format = checkpoint::image_format(dimensions[2] as u32);
                    let mut keeper = 0;
                    invoke!(self,"glGenFramebuffers",(1=>i32,&mut keeper=>*mut u32),());
                    let id = self
                        .retained
                        .next
                        .checked_add(1)
                        .ok_or_else(|| error("retained image identity exhausted"))?;
                    self.retained.next = id;
                    self.retained.images.insert(
                        id,
                        KeptImage {
                            native: name as u32,
                            keeper,
                            kind: object_kind,
                            guest,
                            face: face as u32,
                            level,
                            width: dimensions[0],
                            height: dimensions[1],
                            internal: dimensions[2] as u32,
                        },
                    );
                    self.retained.attachments.insert((framebuffer, point), id);
                    registered.insert(key, id);
                    invoke!(self,"glBindFramebuffer",(FRAMEBUFFER=>u32,keeper=>u32),());
                    let attachment = checkpoint::attachment_point(format);
                    if object_kind == RENDERBUFFER {
                        invoke!(self,"glFramebufferRenderbuffer",(FRAMEBUFFER=>u32,attachment=>u32,RENDERBUFFER=>u32,name as u32=>u32),());
                    } else {
                        let target = if face == 0 { TEXTURE_2D } else { face as u32 };
                        invoke!(self,"glFramebufferTexture2D",(FRAMEBUFFER=>u32,attachment=>u32,target=>u32,name as u32=>u32,level=>i32),());
                    }
                    let color = if format == RGBA { 0x8ce0 } else { 0 };
                    invoke!(self,"glDrawBuffer",(color=>u32),());
                    invoke!(self,"glReadBuffer",(color=>u32),());
                    let status = invoke!(self,"glCheckFramebufferStatus",(FRAMEBUFFER=>u32),u32);
                    if status != 0x8cd5 {
                        return Err(error("cannot retain deleted framebuffer image"));
                    }
                }
            }
            Ok(())
        })();
        invoke!(self,"glBindTexture",(TEXTURE_2D=>u32,texture_2d as u32=>u32),());
        invoke!(self,"glBindTexture",(0x8513=>u32,texture_cube as u32=>u32),());
        invoke!(self,"glBindRenderbuffer",(RENDERBUFFER=>u32,renderbuffer as u32=>u32),());
        invoke!(self,"glBindFramebuffer",(FRAMEBUFFER=>u32,bound=>u32),());
        let mut native_error = 0;
        for _ in 0..8 {
            let value = invoke!(self, "glGetError", (), u32);
            if value == 0 {
                break;
            }
            native_error = value;
        }
        result?;
        if native_error != 0 {
            return Err(error(format!(
                "cannot retain deleted image: GL error {native_error:#x}"
            )));
        }
        Ok(())
    }
    pub(super) fn collect_retained_images(&mut self) -> Result<()> {
        let used: HashSet<_> = self.retained.attachments.values().copied().collect();
        let unused: Vec<_> = self
            .retained
            .images
            .keys()
            .filter(|id| !used.contains(id))
            .copied()
            .collect();
        for id in unused {
            if let Some(image) = self.retained.images.remove(&id) {
                invoke!(self,"glDeleteFramebuffers",(1=>i32,&image.keeper=>*const u32),());
            }
        }
        Ok(())
    }
    pub(super) fn attachment_changed(&mut self, point: u32) -> Result<()> {
        self.retained.attachments.remove(&(self.framebuffer, point));
        self.collect_retained_images()
    }
}
