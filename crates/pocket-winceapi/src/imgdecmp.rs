//! IMGDECMP bitmap decoding and synchronous guest data callbacks.
use crate::CallCtx;
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{gdi::ImageDecodeFrame, DispatchOutcome, GuestCallFrame, KernelError};
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
    if ctx
        .kernel
        .gdi
        .image_decode_frames
        .last()
        .is_some_and(|f| f.thread == thread && f.thunk == thunk && f.frame.sp == sp)
    {
        state = ctx.kernel.gdi.image_decode_frames.pop().unwrap();
        let count = ctx.cpu.read_reg(ArmReg::R0)?;
        restore(ctx, &state.frame)?;
        if count > state.info[2] {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        if state.bytes.len().saturating_add(count as usize) > MAX_COMPRESSED {
            return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY));
        }
        if count != 0 {
            state
                .bytes
                .extend_from_slice(&ctx.cpu.read_mem(state.info[1], count)?);
            return request_data(ctx, state);
        }
    } else {
        let ptr = ctx.arg_u32(0)?;
        if ptr == 0 {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        let size = ctx.cpu.read_u32_le(ptr)?;
        if size != 60 && size != 64 {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        let raw = ctx.cpu.read_mem(ptr, 60)?;
        let info: [u32; 15] =
            std::array::from_fn(|i| u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap()));
        if info[1] == 0 || info[2] == 0 || info[3] > info[2] || !(1..=100).contains(&info[9]) {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        // Do not manufacture an IImageRender COM object or fake progress.
        // Stream-backed IStream and progressive-render interfaces need their
        // own implementations; bitmap and GETDATAFUNC paths below are real.
        if info[5] != 0 || info[13] != 0 || (size == 64 && ctx.cpu.read_u32_le(ptr + 60)? != 0) {
            return Ok(DispatchOutcome::ReturnedR0(NOT_IMPLEMENTED));
        }
        if info[4] == 0 {
            return Ok(DispatchOutcome::ReturnedR0(INVALID));
        }
        ctx.cpu.write_mem(info[4], &0u32.to_le_bytes())?;
        if info[3] as usize > MAX_COMPRESSED {
            return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY));
        }
        let initial = info[3];
        if initial as usize > MAX_COMPRESSED {
            return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY));
        }
        let bytes = ctx.cpu.read_mem(info[1], initial)?;
        state = ImageDecodeFrame {
            thread,
            thunk,
            frame: GuestCallFrame {
                args: [
                    ctx.arg_u32(0)?,
                    ctx.arg_u32(1)?,
                    ctx.arg_u32(2)?,
                    ctx.arg_u32(3)?,
                ],
                sp,
                lr: ctx.cpu.read_reg(ArmReg::Lr)?,
            },
            info,
            bytes,
        };
        if info[12] != 0 {
            return request_data(ctx, state);
        }
    }
    decode_bitmap(ctx, &state.info, &state.bytes)
}
fn restore(ctx: &mut CallCtx<'_>, frame: &GuestCallFrame) -> Result<(), KernelError> {
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(frame.args)
    {
        ctx.cpu.write_reg(r, v)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, frame.lr)?;
    Ok(())
}
fn request_data(
    ctx: &mut CallCtx<'_>,
    state: ImageDecodeFrame,
) -> Result<DispatchOutcome, KernelError> {
    let proc = state.info[12];
    ctx.cpu.write_reg(ArmReg::R0, state.info[1])?;
    ctx.cpu.write_reg(ArmReg::R1, state.info[2])?;
    ctx.cpu.write_reg(ArmReg::R2, state.info[7])?;
    ctx.cpu.write_reg(ArmReg::Lr, state.thunk)?;
    ctx.kernel.gdi.image_decode_frames.push(state);
    Ok(DispatchOutcome::JumpTo(proc))
}
fn decode_bitmap(
    ctx: &mut CallCtx<'_>,
    info: &[u32; 15],
    bytes: &[u8],
) -> Result<DispatchOutcome, KernelError> {
    let decoded = match image::load_from_memory(bytes) {
        Ok(image) => image,
        Err(_) => return Ok(DispatchOutcome::ReturnedR0(NO_IMAGE)),
    };
    let (sw, sh) = (decoded.width(), decoded.height());
    let scale = info[9];
    let mut w = ((u64::from(sw) * u64::from(scale)) / 100).max(1) as u32;
    let mut h = ((u64::from(sh) * u64::from(scale)) / 100).max(1) as u32;
    if info[10] > 0 && w > info[10] {
        h = ((u64::from(h) * u64::from(info[10])) / u64::from(w)).max(1) as u32;
        w = info[10];
    }
    if info[11] > 0 && h > info[11] {
        w = ((u64::from(w) * u64::from(info[11])) / u64::from(h)).max(1) as u32;
        h = info[11];
    }
    let rgba = decoded
        .resize_exact(w, h, image::imageops::FilterType::Triangle)
        .to_rgba8();
    // IMGDECMP returns a DIB: applications may obtain and read bmBits.
    // Keep the public requested depth separate from the host RGB565 cache.
    let bpp = if info[6] == 0 {
        ctx.kernel.framebuffer.bpp as u16
    } else {
        info[6] as u16
    };
    if !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32) {
        return Ok(DispatchOutcome::ReturnedR0(INVALID));
    }
    let stride64 = (u64::from(w) * u64::from(bpp)).div_ceil(32) * 4;
    let Some(size) = stride64
        .checked_mul(u64::from(h))
        .and_then(|n| u32::try_from(n).ok())
    else {
        return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY));
    };
    let Some(bits) = ctx.kernel.heap.alloc(size) else {
        return Ok(DispatchOutcome::ReturnedR0(OUT_OF_MEMORY));
    };
    let stride = stride64 as u32;
    let colours: Vec<[u8; 3]> = match bpp {
        1 => vec![[0, 0, 0], [255, 255, 255]],
        4 => (0..16u8)
            .map(|i| {
                let level = if i & 8 != 0 { 255 } else { 128 };
                [
                    if i & 4 != 0 { level } else { 0 },
                    if i & 2 != 0 { level } else { 0 },
                    if i & 1 != 0 { level } else { 0 },
                ]
            })
            .collect(),
        8 => (0..256u32)
            .map(|i| {
                [
                    ((i >> 5) * 255 / 7) as u8,
                    (((i >> 2) & 7) * 255 / 7) as u8,
                    ((i & 3) * 255 / 3) as u8,
                ]
            })
            .collect(),
        _ => Vec::new(),
    };
    let palette = colours
        .iter()
        .map(|c| pocket_kernel::framebuffer::pack_rgb565(c[0], c[1], c[2]))
        .collect();
    let mut bitmap =
        pocket_kernel::gdi::Bitmap::new_dib(w, h, bpp, bits, stride, true, palette, false);
    let mut raw = vec![0u8; size as usize];
    for (i, pixel) in rgba.pixels().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let row = ((h - 1 - y) * stride) as usize;
        let mut c = pixel.0;
        if c[3] == 0 && info[14] != u32::MAX {
            c[0] = info[14] as u8;
            c[1] = (info[14] >> 8) as u8;
            c[2] = (info[14] >> 16) as u8;
        }
        match bpp {
            24 => {
                let at = row + x as usize * 3;
                raw[at..at + 3].copy_from_slice(&[c[2], c[1], c[0]]);
            }
            32 => {
                let at = row + x as usize * 4;
                raw[at..at + 4].copy_from_slice(&[c[2], c[1], c[0], c[3]]);
            }
            16 => {
                let at = row + x as usize * 2;
                raw[at..at + 2].copy_from_slice(
                    &pocket_kernel::framebuffer::pack_rgb565(c[0], c[1], c[2]).to_le_bytes(),
                );
            }
            _ => {
                let index = colours
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, p)| {
                        (0..3)
                            .map(|n| (i32::from(c[n]) - i32::from(p[n])).pow(2))
                            .sum::<i32>()
                    })
                    .unwrap()
                    .0 as u8;
                c[..3].copy_from_slice(&colours[index as usize]);
                match bpp {
                    8 => raw[row + x as usize] = index,
                    4 => raw[row + x as usize / 2] |= index << if x % 2 == 0 { 4 } else { 0 },
                    _ => raw[row + x as usize / 8] |= index << (7 - x % 8),
                }
            }
        }
        bitmap.pixels[i * 2..i * 2 + 2].copy_from_slice(
            &pocket_kernel::framebuffer::pack_rgb565(c[0], c[1], c[2]).to_le_bytes(),
        );
    }
    if let Err(error) = ctx.cpu.write_mem(bits, &raw) {
        ctx.kernel.heap.free(bits);
        return Err(error.into());
    }
    let handle = ctx.kernel.gdi.register_dib(bitmap);
    ctx.cpu.write_mem(info[4], &handle.to_le_bytes())?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{Cpu, Prot};
    fn bitmap() -> Vec<u8> {
        let mut data = Vec::new();
        image::codecs::bmp::BmpEncoder::new(&mut data)
            .encode(&[255, 0, 0, 0, 255, 0], 2, 1, image::ColorType::Rgb8)
            .unwrap();
        data
    }
    #[test]
    fn decoded_24_bit_dib_exposes_bgr_bottom_up_rows_through_get_object() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let mut data = Vec::new();
        image::codecs::bmp::BmpEncoder::new(&mut data)
            .encode(
                &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
                2,
                2,
                image::ColorType::Rgb8,
            )
            .unwrap();
        let header = [
            60u32,
            0x1800,
            data.len() as u32,
            data.len() as u32,
            0x2000,
            0,
            24,
            0,
            0,
            100,
            1000,
            1000,
            0,
            0,
            u32::MAX,
        ];
        cpu.write_mem(
            0x1000,
            &header
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cpu.write_mem(0x1800, &data).unwrap();
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "imgdecmp.dll", "ord:1", &[0x1000]),
            DispatchOutcome::ReturnedR0(0)
        );
        let h = cpu.read_u32_le(0x2000).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetObjectW",
                &[h, 24, 0x2100]
            ),
            DispatchOutcome::ReturnedR0(24)
        );
        assert_eq!(cpu.read_u32_le(0x210c).unwrap(), 8);
        assert_eq!(cpu.read_mem(0x2112, 2).unwrap(), 24u16.to_le_bytes());
        let bits = cpu.read_u32_le(0x2114).unwrap();
        assert_ne!(bits, 0);
        assert_eq!(
            cpu.read_mem(bits, 16).unwrap(),
            [255, 0, 0, 255, 255, 255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 0]
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DeleteObject",
            &[h],
        );
        assert!(k.gdi.bitmap(h).is_none());
        assert_eq!(k.heap.alloc(16), Some(bits));
    }
    #[test]
    fn ordinal_decodes_real_stream_chunks_and_restores_callback_continuation() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let bytes = bitmap();
        let header = [
            60u32,
            0x1800,
            32,
            0,
            0x2000,
            0,
            16,
            77,
            0,
            100,
            1000,
            1000,
            0x123400,
            0,
            u32::MAX,
        ];
        cpu.write_mem(
            0x1000,
            &header
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cpu.write_reg(ArmReg::Lr, 0x8888).unwrap();
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "imgdecmp.dll", "ord:1", &[0x1000]),
            DispatchOutcome::JumpTo(0x123400)
        );
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 0x1800);
        assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 32);
        assert_eq!(cpu.read_reg(ArmReg::R2).unwrap(), 77);
        for chunk in bytes.chunks(32) {
            cpu.write_mem(0x1800, chunk).unwrap();
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "imgdecmp.dll",
                    "ord:1",
                    &[chunk.len() as u32]
                ),
                DispatchOutcome::JumpTo(0x123400)
            );
        }
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "imgdecmp.dll", "ord:1", &[0]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x8888);
        assert!(k.gdi.image_decode_frames.is_empty());
        let h = cpu.read_u32_le(0x2000).unwrap();
        let b = k.gdi.bitmap(h).unwrap();
        assert_eq!((b.width, b.height), (2, 1));
        assert_eq!(&b.pixels[..2], &0xf800u16.to_le_bytes());
        assert_eq!(&b.pixels[2..4], &0x07e0u16.to_le_bytes());
    }
    #[test]
    fn named_decode_obeys_scale_and_never_succeeds_with_invalid_data() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let bytes = bitmap();
        let header = [
            60u32,
            0x1800,
            bytes.len() as u32,
            bytes.len() as u32,
            0x2000,
            0,
            16,
            0,
            0,
            50,
            1000,
            1000,
            0,
            0,
            u32::MAX,
        ];
        cpu.write_mem(
            0x1000,
            &header
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cpu.write_mem(0x1800, &bytes).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "imgdecmp.dll",
                "DecompressImageIndirect",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let b = k.gdi.bitmap(cpu.read_u32_le(0x2000).unwrap()).unwrap();
        assert_eq!((b.width, b.height), (1, 1));
        cpu.write_mem(0x1800, &vec![0; bytes.len()]).unwrap();
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "imgdecmp.dll", "ord:1", &[0x1000]),
            DispatchOutcome::ReturnedR0(NO_IMAGE)
        );
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), 0);
    }
}
