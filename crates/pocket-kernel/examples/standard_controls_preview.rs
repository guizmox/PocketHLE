//! Renderer fixture, not an execution or screenshot of CoPilot.
use pocket_kernel::{
    controls::{ControlClass as C, Controls},
    gdi::{GdiState, Surface},
    Framebuffer,
};
fn main() -> std::io::Result<()> {
    let mut controls = Controls::default();
    let mut gdi = GdiState::new();
    let font = gdi.create_font(12);
    let parent = 0xdead0001;
    let mut add = |class, id, text: &str, style: u32, x, y, w, h| {
        let hwnd = controls.create(
            parent,
            class,
            id,
            text.into(),
            0x50000000 | style,
            x,
            y,
            w,
            h,
        );
        controls.get_mut(hwnd).unwrap().font = font;
        hwnd
    };
    let check = add(C::Button, 1, "Enable POI Alerts", 3, 12, 8, 260, 22);
    let distance = add(C::ComboBox, 2, "", 3, 70, 36, 96, 90);
    add(C::Static, 3, "ahead", 0x200, 174, 36, 100, 22);
    add(C::Static, 4, "Category", 2 | 0x200, 2, 74, 60, 22);
    let category = add(C::ComboBox, 5, "", 3, 72, 74, 236, 90);
    add(C::Static, 6, "Type", 2 | 0x200, 2, 108, 60, 22);
    let ty = add(C::ComboBox, 7, "", 3, 72, 108, 236, 90);
    add(C::Static, 8, "On Route", 1, 4, 154, 42, 32);
    let track = add(C::TrackBar, 9, "", 1, 62, 145, 190, 35);
    add(C::Static, 10, "Near Route", 1, 265, 154, 45, 32);
    add(C::Button, 11, "OK", 1, 62, 192, 80, 32);
    add(C::Button, 12, "Cancel", 0, 182, 192, 80, 32);
    controls.get_mut(check).unwrap().checked = true;
    for (hwnd, value) in [
        (distance, "3 mi"),
        (category, "Filling Stations"),
        (ty, "Petrol Station"),
    ] {
        let c = controls.get_mut(hwnd).unwrap();
        c.list_items.push((value.into(), 0));
        c.list_selected = Some(0);
    }
    let c = controls.get_mut(track).unwrap();
    c.track_range = (0, 10);
    c.track_pos = 0;
    c.track_tick_freq = 1;
    controls.update_choice_fonts(&gdi);
    let mut fb = Framebuffer::new(320, 240);
    let mut surface = Surface::Screen(&mut fb);
    surface.fill_rect(
        0,
        0,
        320,
        240,
        pocket_kernel::framebuffer::colorref_to_rgb565(0xefe2d6),
    );
    controls.render_with_images(&mut surface, &gdi);
    std::fs::write(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "standard-controls.ppm".into()),
        fb.snapshot_ppm(),
    )
}
