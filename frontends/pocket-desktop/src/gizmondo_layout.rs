//! Gizmondo host controls in UI points; game fitting uses rendered physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoxRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl BoxRect {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn center(self) -> [f32; 2] {
        [self.x + self.w / 2.0, self.y + self.h / 2.0]
    }
    pub fn contains(self, p: [f32; 2], round: bool) -> bool {
        if p[0] < self.x || p[1] < self.y || p[0] >= self.x + self.w || p[1] >= self.y + self.h {
            return false;
        }
        if !round {
            return true;
        }
        let c = self.center();
        let r = self.w.min(self.h) / 2.0;
        (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) <= r * r
    }
}
pub const HEADER: f32 = 116.0;
pub const GAP: f32 = 12.0;
#[derive(Debug)]
pub struct Layout {
    pub screen: BoxRect,
    pub functions: [BoxRect; 5],
    pub shoulders: [BoxRect; 2],
    pub directions: [BoxRect; 4], // N,W,E,S
    pub dpad: BoxRect,
    pub actions: [BoxRect; 4], // Stop,Rewind,Forward,Play
    pub host: [BoxRect; 2],    // Exit,Full
}
pub fn window_body(native: [u32; 2], factor: u32, dpi: f32) -> [f32; 2] {
    let dpi = dpi.max(0.25);
    // Two physical pixels prevent fractional DPI rounding from losing a zoom step.
    let screen_w = native[0] as f32 * factor.max(1) as f32 / dpi + 2.0 / dpi;
    let screen_h = native[1] as f32 * factor.max(1) as f32 / dpi + 2.0 / dpi;
    [
        ((screen_w + 2.0 * GAP) / 0.58)
            .min(screen_w + 2.0 * (280.0 + GAP))
            .max(720.0),
        (screen_h + HEADER + GAP).max(480.0),
    ]
}

pub fn calculate(
    size: [f32; 2],
    native: [u32; 2],
    dpi: f32,
    maximum: Option<u32>,
    deck: bool,
) -> Layout {
    let w = size[0].max(1.0);
    let h = size[1].max(1.0);
    let dpi = dpi.max(0.25);
    // Reduce the whole control bank only for exceptionally small viewports.
    let unit = (w / 640.0).min(h / 360.0).clamp(0.1, 1.0);
    let gap = GAP * unit;
    let top = HEADER * unit;
    let side = (w * 0.21).clamp(140.0 * unit, 280.0 * unit);
    let room = BoxRect::new(
        side + gap,
        top,
        (w - 2.0 * (side + gap)).max(1.0),
        (h - top - gap).max(1.0),
    );
    let physical = [
        (room.w * dpi).floor().max(1.0) as u32,
        (room.h * dpi).floor().max(1.0) as u32,
    ];
    let fit =
        crate::fullscreen_layout::integer_fit_capped(physical, native, maximum.unwrap_or(u32::MAX));
    let fitted =
        if (deck && maximum.is_none()) || fit.width > physical[0] || fit.height > physical[1] {
            crate::fullscreen_layout::aspect_fit(physical, native)
        } else {
            [fit.left, fit.top, fit.width, fit.height]
        };
    let screen = BoxRect::new(
        (room.x * dpi).round() / dpi + fitted[0] as f32 / dpi,
        (room.y * dpi).round() / dpi + fitted[1] as f32 / dpi,
        fitted[2] as f32 / dpi,
        fitted[3] as f32 / dpi,
    );
    let fw = ((room.w - 4.0 * 8.0 * unit) / 5.0).min(90.0 * unit);
    let fx = w / 2.0 - (fw * 5.0 + 32.0 * unit) / 2.0;
    let functions = std::array::from_fn(|i| {
        BoxRect::new(
            fx + i as f32 * (fw + 8.0 * unit),
            40.0 * unit,
            fw,
            64.0 * unit,
        )
    });
    let shoulder_h = (room.h * 0.20)
        .clamp(64.0 * unit, 92.0 * unit)
        .min(room.h * 0.30);
    let shoulders = [
        BoxRect::new(gap, top, side - gap, shoulder_h),
        BoxRect::new(w - side, top, side - gap, shoulder_h),
    ];
    let span = (side - 12.0 * unit).min(room.h * 0.48);
    let cy = (top + room.h * 0.65)
        .max(top + shoulder_h + gap + span / 2.0)
        .min(h - gap - span / 2.0);
    let lx = side / 2.0 + gap / 2.0;
    let rx = w - lx;
    let dpad = BoxRect::new(lx - span / 2.0, cy - span / 2.0, span, span);
    let bar = span / 3.0;
    let arm = span / 3.0;
    let directions = [
        BoxRect::new(lx - bar / 2.0, cy - span / 2.0, bar, arm),
        BoxRect::new(lx - span / 2.0, cy - bar / 2.0, arm, bar),
        BoxRect::new(lx + bar / 2.0, cy - bar / 2.0, arm, bar),
        BoxRect::new(lx - bar / 2.0, cy + bar / 2.0, bar, arm),
    ];
    let d = span / 3.0;
    let step = span / 3.0;
    let actions = [
        BoxRect::new(rx - d / 2.0, cy - step - d / 2.0, d, d),
        BoxRect::new(rx - step - d / 2.0, cy - d / 2.0, d, d),
        BoxRect::new(rx + step - d / 2.0, cy - d / 2.0, d, d),
        BoxRect::new(rx - d / 2.0, cy + step - d / 2.0, d, d),
    ];
    let host = [
        BoxRect::new(w - 156.0 * unit, 6.0 * unit, 68.0 * unit, 26.0 * unit),
        BoxRect::new(w - 80.0 * unit, 6.0 * unit, 68.0 * unit, 26.0 * unit),
    ];
    Layout {
        screen,
        functions,
        shoulders,
        directions,
        dpad,
        actions,
        host,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn overlaps(a: BoxRect, b: BoxRect) -> bool {
        a.x < b.x + b.w - 0.01
            && b.x < a.x + a.w - 0.01
            && a.y < b.y + b.h - 0.01
            && b.y < a.y + a.h - 0.01
    }
    #[test]
    fn all_controls_stay_outside_the_game_and_inside_the_viewport() {
        for size in [
            [640., 360.],
            [720., 480.],
            [960., 600.],
            [1280., 800.],
            [1920., 1080.],
            [1920., 1200.],
            [2560., 1080.],
        ] {
            for native in [[320, 240], [640, 480], [1280, 960]] {
                for dpi in [1.0, 1.25, 1.5, 2.0] {
                    for maximum in [None, Some(1), Some(2)] {
                        let g = calculate(size, native, dpi, maximum, true);
                        let controls = g
                            .functions
                            .iter()
                            .chain(g.shoulders.iter())
                            .chain(g.directions.iter())
                            .chain(g.actions.iter())
                            .chain(g.host.iter());
                        for r in controls {
                            assert!(
                                !overlaps(*r, g.screen),
                                "{size:?} {native:?} {r:?} {:?}",
                                g.screen
                            );
                            assert!(
                                r.x >= 0.
                                    && r.y >= 0.
                                    && r.x + r.w <= size[0] + 0.1
                                    && r.y + r.h <= size[1] + 0.1
                            );
                        }
                        assert!(
                            g.screen.x >= 0.
                                && g.screen.y >= 0.
                                && g.screen.x + g.screen.w <= size[0] + 0.1
                                && g.screen.y + g.screen.h <= size[1] + 0.1
                        );
                        assert!((g.screen.w / g.screen.h - 4.0 / 3.0).abs() < 0.02);
                        assert_eq!(g.shoulders[0].y, g.shoulders[1].y);
                        assert_eq!(g.dpad.center()[1], g.actions[1].center()[1]);
                        for i in 0..4 {
                            for j in i + 1..4 {
                                assert!(!overlaps(g.actions[i], g.actions[j]));
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn window_zoom_uses_rendered_pixels_and_round_hitboxes_reject_corners() {
        for native in [[320, 240], [640, 480]] {
            for factor in [1, 2] {
                for dpi in [1.0, 1.5, 2.0] {
                    let body = window_body(native, factor, dpi);
                    let g = calculate(body, native, dpi, Some(factor), false);
                    assert_eq!((g.screen.w * dpi).round() as u32, native[0] * factor);
                    assert_eq!((g.screen.h * dpi).round() as u32, native[1] * factor);
                }
            }
        }
        let a = calculate([1280., 800.], [320, 240], 1., None, true).actions[0];
        assert!(a.contains(a.center(), true));
        assert!(!a.contains([a.x + 0.1, a.y + 0.1], true));
        assert!(a.contains([a.x + 0.1, a.y + 0.1], false));
    }
}
