//! Frozen GDI record version 1, before deferred bitmap-deletion ownership.
//! Data conversion only: runtime behavior remains in GdiState. Do not add current
//! fields here or regenerate the immutable WinCE fixture to hide schema changes.
use super::*;
#[derive(serde::Deserialize)]
struct Version1 {
    font_enum_frames: Vec<FontEnumFrame>,
    image_decode_frames: Vec<ImageDecodeFrame>,
    image_renderers: HashMap<u32, ImageRenderer>,
    saved_dcs: HashMap<u32, Vec<Dc>>,
    shell_menus: crate::shell_menu::ShellMenus,
    objects: HashMap<u32, GdiObject>,
    next_handle: u32,
    class_values: HashMap<(String, i32), u32>,
    regions: HashMap<u32, Vec<[i32; 4]>>,
    scroll_erases: HashMap<u32, Vec<[i32; 4]>>,
    icons: HashMap<u32, crate::imaging::Image>,
    image_lists: HashMap<u32, crate::imaging::ImageList>,
    system_brushes: HashMap<i32, u32>,
    dialog_bitmaps: HashMap<(u32, u32), u32>,
    owner_draw_frames: Vec<OwnerDrawFrame>,
    list_paint_frames: Vec<ListPaintFrame>,
    control_color_frames: Vec<ControlColorFrame>,
    dialog_backgrounds: HashMap<u32, u32>,
    list_paint_stamps: HashMap<u32, u64>,
    paint_erase_frames: Vec<PaintEraseFrame>,
    pending_window_erases: std::collections::HashSet<u32>,
    pending_window_updates: HashMap<u32, Vec<[i32; 4]>>,
    generated_window_paints: std::collections::HashSet<u32>,
    owner_draw_states: HashMap<u32, (bool, bool, bool, i32, i32)>,
}
pub(super) fn read(
    header: pocket_snapshot::fields::Header,
    input: &mut dyn std::io::Read,
) -> std::io::Result<GdiSnapshot> {
    let old: Version1 = pocket_snapshot::fields::read_value(header, input)?;
    Ok(GdiSnapshot(GdiState {
        font_enum_frames: old.font_enum_frames,
        image_decode_frames: old.image_decode_frames,
        image_renderers: old.image_renderers,
        saved_dcs: old.saved_dcs,
        shell_menus: old.shell_menus,
        objects: old.objects,
        next_handle: old.next_handle,
        class_values: old.class_values,
        regions: old.regions,
        scroll_erases: old.scroll_erases,
        icons: old.icons,
        image_lists: old.image_lists,
        system_brushes: old.system_brushes,
        dialog_bitmaps: old.dialog_bitmaps,
        owner_draw_frames: old.owner_draw_frames,
        list_paint_frames: old.list_paint_frames,
        control_color_frames: old.control_color_frames,
        dialog_backgrounds: old.dialog_backgrounds,
        list_paint_stamps: old.list_paint_stamps,
        paint_erase_frames: old.paint_erase_frames,
        pending_window_erases: old.pending_window_erases,
        pending_window_updates: old.pending_window_updates,
        generated_window_paints: old.generated_window_paints,
        owner_draw_states: old.owner_draw_states,
        ..GdiState::default()
    }))
}
