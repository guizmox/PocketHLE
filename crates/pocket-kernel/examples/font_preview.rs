//! Small repeatable preview of the same text rasterizer used by WinCE GDI.
use pocket_kernel::{
    gdi::{Bitmap, Font, Surface},
    text,
};
fn main() -> std::io::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "font-preview.ppm".into());
    let mut bitmap = Bitmap::new(480, 150);
    let mut surface = Surface::Bitmap(&mut bitmap);
    surface.fill_rect(0, 0, 480, 150, 0xffff);
    for (row, name, height, weight, caption) in [
        (0, "Arial", -16, 400, "CoPilot: Going To > Address"),
        (1, "Tahoma", -14, 400, "Options / Guidance / Favorites"),
        (2, "Arial", -20, 700, "Play Demo - Navigation"),
        (3, "Times New Roman", -16, 400, "Place: Londres, été, 1234"),
        (4, "Tahoma", -12, 700, "Accept     Cancel     POI Alerts"),
    ] {
        let mut raw = [0u8; 92];
        raw[..4].copy_from_slice(&i32::to_le_bytes(height));
        raw[16..20].copy_from_slice(&i32::to_le_bytes(weight));
        for (i, u) in name.encode_utf16().enumerate() {
            raw[28 + i * 2..30 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        let font = Font::from_logfont(raw);
        let line = text::layout(Some(&font), &caption.encode_utf16().collect::<Vec<_>>());
        line.draw(&mut surface, 8, 4 + row * 29, 0x0010, [0, 0, 480, 150]);
    }
    let mut ppm = b"P6\n480 150\n255\n".to_vec();
    for bytes in surface.pixels().chunks_exact(2) {
        let p = u16::from_le_bytes([bytes[0], bytes[1]]);
        ppm.extend([
            (((p >> 11) & 31) * 255 / 31) as u8,
            (((p >> 5) & 63) * 255 / 63) as u8,
            ((p & 31) * 255 / 31) as u8,
        ]);
    }
    std::fs::write(path, ppm)
}
