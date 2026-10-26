//! Fullscreen geometry is computed in physical pixels, independently of UI DPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegerFit {
    pub factor: u32,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}
pub fn integer_fit(available: [u32; 2], native: [u32; 2]) -> IntegerFit {
    integer_fit_capped(available, native, u32::MAX)
}
pub fn integer_fit_capped(available: [u32; 2], native: [u32; 2], maximum: u32) -> IntegerFit {
    let [w, h] = [native[0].max(1), native[1].max(1)];
    let factor = (available[0] / w)
        .min(available[1] / h)
        .max(1)
        .min(maximum.max(1));
    let width = w * factor;
    let height = h * factor;
    IntegerFit {
        factor,
        width,
        height,
        left: available[0].saturating_sub(width) / 2,
        top: available[1].saturating_sub(height) / 2,
    }
}
/// Largest aspect-preserving rectangle, including fractional up/downscaling.
/// Steam Deck's physical screen need not be a whole multiple of guest pixels.
pub fn aspect_fit(available: [u32; 2], native: [u32; 2]) -> [u32; 4] {
    let [w, h] = [native[0].max(1) as u64, native[1].max(1) as u64];
    let [aw, ah] = available.map(u64::from);
    let (width, height) = if aw * h <= ah * w {
        (aw, aw * h / w)
    } else {
        (ah * w / h, ah)
    };
    [
        ((aw - width) / 2) as u32,
        ((ah - height) / 2) as u32,
        width as u32,
        height as u32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deck_fullscreen_uses_available_height_for_qvga_and_vga() {
        for native in [[320, 240], [640, 480]] {
            assert_eq!(aspect_fit([1280, 800], native), [107, 0, 1066, 800]);
        }
        for native in [[240, 320], [480, 640]] {
            assert_eq!(aspect_fit([1280, 800], native), [340, 0, 600, 800]);
        }
        assert_eq!(aspect_fit([200, 150], [640, 480]), [0, 0, 200, 150]);
        assert_eq!(aspect_fit([0, 0], [320, 240]), [0, 0, 0, 0]);
    }

    #[test]
    fn landscape_modes_choose_the_largest_whole_factor() {
        for (size, factor, image) in [
            ([1920, 1080], 4, [1280, 960]),
            ([2560, 1440], 6, [1920, 1440]),
            ([3840, 2160], 9, [2880, 2160]),
            ([3440, 1440], 6, [1920, 1440]),
            ([1080, 1920], 3, [960, 720]),
        ] {
            let fit = integer_fit(size, [320, 240]);
            assert_eq!(fit.factor, factor);
            assert_eq!([fit.width, fit.height], image);
            assert!(fit.width <= size[0] && fit.height <= size[1]);
            assert_eq!(fit.width * 3, fit.height * 4);
            assert!((factor + 1) * 320 > size[0] || (factor + 1) * 240 > size[1]);
        }
    }
    #[test]
    fn pocketpc_caps_integer_scale_and_preserves_rotated_native_geometry() {
        for native in [[240, 320], [320, 240], [480, 640], [640, 480]] {
            for available in [
                [1280, 800],
                [1920, 1080],
                [3840, 2160],
                [700, 400],
                [200, 200],
            ] {
                let fit = integer_fit_capped(available, native, 4);
                assert!((1..=4).contains(&fit.factor));
                assert_eq!(fit.width, native[0] * fit.factor);
                assert_eq!(fit.height, native[1] * fit.factor);
            }
        }
        assert_eq!(integer_fit_capped([7680, 4320], [240, 320], 4).factor, 4);
        assert_eq!(integer_fit([7680, 4320], [320, 240]).factor, 18);
    }
    #[test]
    fn centering_stays_on_physical_pixels_and_never_uses_fractional_scale() {
        let fit = integer_fit([1921, 1081], [320, 240]);
        assert_eq!(
            fit,
            IntegerFit {
                factor: 4,
                left: 320,
                top: 60,
                width: 1280,
                height: 960
            }
        );
        // At 125% DPI these dimensions become 1024x768 UI points,
        // still exactly 1280x960 physical pixels, rather than a 3x image.
        assert_eq!(fit.width as f32 / 1.25, 1024.0);
        assert_eq!(integer_fit([319, 239], [320, 240]).factor, 1);
    }
}

// Window sizing and the PocketPC display rectangle must reserve identical chrome.
pub const POCKET_PC_TOOLBAR_HEIGHT: f32 = 36.0;
pub const POCKET_PC_GAP: f32 = 6.0;
pub fn pocket_pc_window_body(screen: [u32; 2], factor: u32, dpi: f32) -> [f32; 2] {
    [
        (screen[0] as f32 * factor as f32 / dpi + 312.0).max(600.0),
        (screen[1] as f32 * factor as f32 / dpi + POCKET_PC_TOOLBAR_HEIGHT + POCKET_PC_GAP)
            .max(340.0),
    ]
}
#[cfg(test)]
mod pocket_pc_window_tests {
    use super::*;
    #[test]
    fn doubled_window_reserves_the_entire_toolbar_and_bottom_gap() {
        for native in [[240, 320], [320, 240], [480, 640], [640, 480]] {
            for dpi in [1.0, 1.25, 1.5, 2.0] {
                let body = pocket_pc_window_body(native, 2, dpi);
                let room = [
                    ((body[0] - 304.0) * dpi).round() as u32,
                    ((body[1] - POCKET_PC_TOOLBAR_HEIGHT - POCKET_PC_GAP) * dpi).round() as u32,
                ];
                assert_eq!(
                    integer_fit_capped(room, native, 4).factor,
                    2,
                    "{native:?} at DPI {dpi}"
                );
            }
        }
        // Previously the body only reserved 36 points, losing six screen rows.
        assert_eq!(integer_fit_capped([488, 634], [240, 320], 4).factor, 1);
    }
}
