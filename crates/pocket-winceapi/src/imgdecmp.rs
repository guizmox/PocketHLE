//! IMGDECMP bitmap decoding and synchronous guest data callbacks.
use crate::CallCtx;
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{DispatchOutcome, GuestCallFrame, KernelError, gdi::ImageDecodeFrame};
const INVALID: u32 = 0x80070057;
const NO_IMAGE: u32 = 0x800b0100;
const NOT_IMPLEMENTED: u32 = 0x80004001;
const OUT_OF_MEMORY: u32 = 0x8007000e;
const MAX_COMPRESSED: usize = 128 * 1024 * 1024;

pub(crate) fn decompress(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let thread = ctx.kernel.current_thread;
    let thunk = ctx.thunk.thunk_va;
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    let mut state;
    if ctx.kernel.gdi.image_decode_frames.last().is_some_and(|f| f.thread == thread && f.thunk == thunk && f.frame.sp == sp) {
        state = ctx.kernel.gdi.image_decode_frames.pop().unwrap();
        let count = ctx.cpu.read_reg(ArmReg::R0)?;
        restore(ctx, &state.frame)?;
        if count > state.info[2] { return Ok(DispatchOutcome::ReturnedR0(INVALID)); }
        if state.bytes.len().saturating_add(count as usize) > MAX_COMPRESSED { return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY)); }
        if count != 0 {
            state.bytes.extend_from_slice(&ctx.cpu.read_mem(state.info[1], count)?);
            return request_data(ctx, state);
        }
    } else {
        let ptr = ctx.arg_u32(0)?;
        if ptr == 0 { return Ok(DispatchOutcome::ReturnedR0(INVALID)); }
        let size = ctx.cpu.read_u32_le(ptr)?;
        if size != 60 && size != 64 { return Ok(DispatchOutcome::ReturnedR0(INVALID)); }
        let raw = ctx.cpu.read_mem(ptr, 60)?;
        let info: [u32;15] = std::array::from_fn(|i| u32::from_le_bytes(raw[i*4..i*4+4].try_into().unwrap()));
        if info[1] == 0 || info[2] == 0 || info[3] > info[2] || !(1..=100).contains(&info[9]) {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        // Do not manufacture an IImageRender COM object or fake progress.
        // Stream-backed IStream and progressive-render interfaces need their
        // own implementations; bitmap and GETDATAFUNC paths below are real.
        if info[5] != 0 || info[13] != 0 || (size == 64 && ctx.cpu.read_u32_le(ptr + 60)? != 0) {
            return Ok(DispatchOutcome::ReturnedR0(NOT_IMPLEMENTED));
        }
        if info[4] == 0 { return Ok(DispatchOutcome::ReturnedR0(INVALID)); }
        ctx.cpu.write_mem(info[4], &0u32.to_le_bytes())?;
        if info[3] as usize > MAX_COMPRESSED { return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY)); }
        let initial = info[3];
        if initial as usize > MAX_COMPRESSED { return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY)); }
        let bytes = ctx.cpu.read_mem(info[1], initial)?;
        state = ImageDecodeFrame {thread,thunk,frame:GuestCallFrame {
            args:[ctx.arg_u32(0)?,ctx.arg_u32(1)?,ctx.arg_u32(2)?,ctx.arg_u32(3)?],sp,lr:ctx.cpu.read_reg(ArmReg::Lr)?
        },info,bytes};
        if info[12] != 0 { return request_data(ctx, state); }
    }
    decode_bitmap(ctx, &state.info, &state.bytes)
}
fn restore(ctx: &mut CallCtx<'_>, frame: &GuestCallFrame) -> Result<(), KernelError> {
    for (r,v) in [ArmReg::R0,ArmReg::R1,ArmReg::R2,ArmReg::R3].into_iter().zip(frame.args) {ctx.cpu.write_reg(r,v)?;}
    ctx.cpu.write_reg(ArmReg::Lr,frame.lr)?;
    Ok(())
}
fn request_data(ctx: &mut CallCtx<'_>, state: ImageDecodeFrame) -> Result<DispatchOutcome, KernelError> {
    let proc = state.info[12];
    ctx.cpu.write_reg(ArmReg::R0,state.info[1])?;
    ctx.cpu.write_reg(ArmReg::R1,state.info[2])?;
    ctx.cpu.write_reg(ArmReg::R2,state.info[7])?;
    ctx.cpu.write_reg(ArmReg::Lr,state.thunk)?;
    ctx.kernel.gdi.image_decode_frames.push(state);
    Ok(DispatchOutcome::JumpTo(proc))
}
fn decode_bitmap(ctx: &mut CallCtx<'_>, info: &[u32;15], bytes: &[u8]) -> Result<DispatchOutcome,KernelError> {
    let decoded = match image::load_from_memory(bytes) {Ok(image)=>image,Err(_)=>return Ok(DispatchOutcome::ReturnedR0(NO_IMAGE))};
    let (sw,sh) = (decoded.width(),decoded.height());
    let scale = info[9];
    let mut w = ((u64::from(sw)*u64::from(scale))/100).max(1) as u32;
    let mut h = ((u64::from(sh)*u64::from(scale))/100).max(1) as u32;
    if info[10] > 0 && w > info[10] {h=((u64::from(h)*u64::from(info[10]))/u64::from(w)).max(1) as u32; w=info[10];}
    if info[11] > 0 && h > info[11] {w=((u64::from(w)*u64::from(info[11]))/u64::from(h)).max(1) as u32; h=info[11];}
    let rgba = decoded.resize_exact(w,h,image::imageops::FilterType::Triangle).to_rgba8();
    let handle = ctx.kernel.gdi.create_compatible_bitmap(w,h);
    if let Some(bitmap) = ctx.kernel.gdi.bitmap_mut(handle) {
        for (dst,pixel) in bitmap.pixels.chunks_exact_mut(2).zip(rgba.pixels()) {
            let mut c = pixel.0;
            if c[3] == 0 && info[14] != u32::MAX {c[0]=info[14] as u8;c[1]=(info[14]>>8) as u8;c[2]=(info[14]>>16) as u8;}
            dst.copy_from_slice(&pocket_kernel::framebuffer::pack_rgb565(c[0],c[1],c[2]).to_le_bytes());
        }
    }
    ctx.cpu.write_mem(info[4],&handle.to_le_bytes())?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call,setup};
    use pocket_cpu::Cpu;
    fn bitmap() -> Vec<u8> {
        let mut data=Vec::new();
        image::codecs::bmp::BmpEncoder::new(&mut data).encode(&[255,0,0,0,255,0],2,1,image::ColorType::Rgb8).unwrap();
        data
    }
    #[test]
    fn ordinal_decodes_real_stream_chunks_and_restores_callback_continuation() {
        let (mut cpu,mut k,mut d)=setup();let bytes=bitmap();
        let header=[60u32,0x1800,32,0,0x2000,0,16,77,0,100,1000,1000,0x123400,0,u32::MAX];
        cpu.write_mem(0x1000,&header.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>()).unwrap();
        cpu.write_reg(ArmReg::Lr,0x8888).unwrap();
        assert_eq!(call(&mut cpu,&mut k,&mut d,"imgdecmp.dll","ord:1",&[0x1000]),DispatchOutcome::JumpTo(0x123400));
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(),0x1800);
        assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(),32);
        assert_eq!(cpu.read_reg(ArmReg::R2).unwrap(),77);
        for chunk in bytes.chunks(32) {
            cpu.write_mem(0x1800,chunk).unwrap();
            assert_eq!(call(&mut cpu,&mut k,&mut d,"imgdecmp.dll","ord:1",&[chunk.len() as u32]),DispatchOutcome::JumpTo(0x123400));
        }
        assert_eq!(call(&mut cpu,&mut k,&mut d,"imgdecmp.dll","ord:1",&[0]),DispatchOutcome::ReturnedR0(0));
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(),0x8888);
        assert!(k.gdi.image_decode_frames.is_empty());
        let h=cpu.read_u32_le(0x2000).unwrap();let b=k.gdi.bitmap(h).unwrap();
        assert_eq!((b.width,b.height),(2,1));
        assert_eq!(&b.pixels[..2],&0xf800u16.to_le_bytes());
        assert_eq!(&b.pixels[2..4],&0x07e0u16.to_le_bytes());
    }
    #[test]
    fn named_decode_obeys_scale_and_never_succeeds_with_invalid_data() {
        let (mut cpu,mut k,mut d)=setup();let bytes=bitmap();
        let header=[60u32,0x1800,bytes.len() as u32,bytes.len() as u32,0x2000,0,16,0,0,50,1000,1000,0,0,u32::MAX];
        cpu.write_mem(0x1000,&header.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>()).unwrap();
        cpu.write_mem(0x1800,&bytes).unwrap();
        assert_eq!(call(&mut cpu,&mut k,&mut d,"imgdecmp.dll","DecompressImageIndirect",&[0x1000]),DispatchOutcome::ReturnedR0(0));
        let b=k.gdi.bitmap(cpu.read_u32_le(0x2000).unwrap()).unwrap();assert_eq!((b.width,b.height),(1,1));
        cpu.write_mem(0x1800,&vec![0;bytes.len()]).unwrap();
        assert_eq!(call(&mut cpu,&mut k,&mut d,"imgdecmp.dll","ord:1",&[0x1000]),DispatchOutcome::ReturnedR0(NO_IMAGE));
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(),0);
    }
}
