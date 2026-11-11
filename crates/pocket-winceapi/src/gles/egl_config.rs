//! Virtual RGB565/RGBA8 configurations, shared by selection and queries.
//! EGL 1.1 tables 3.1/3.3; renderable-type queries retain ES 1.x compatibility.
use super::*;
use pocket_gles::*;

const DONT_CARE: i32 = -1;
const BIND_TO_TEXTURE_RGB: u32 = 0x3039;
const BIND_TO_TEXTURE_RGBA: u32 = 0x303a;
const MIN_SWAP_INTERVAL: u32 = 0x303b;
const MAX_SWAP_INTERVAL: u32 = 0x303c;
const TRANSPARENT_RED: u32 = 0x3037;
const TRANSPARENT_GREEN: u32 = 0x3036;
const TRANSPARENT_BLUE: u32 = 0x3035;
const SLOW_CONFIG: i32 = 0x3050;
const NON_CONFORMANT_CONFIG: i32 = 0x3051;
const TRANSPARENT_RGB: i32 = 0x3052;

pub(super) struct Config {
    depth: i32,
    rgba: bool,
    pbuffer_only: bool,
}
impl Config {
    pub(super) fn handles() -> &'static [u32] {
        let rgba_only = SURFACES.with(|s| s.borrow().rgba8888_only);
        match (rgba_only, goforce_5500_enabled()) {
            (true, true) => &[CONFIG_RGBA_HANDLE, CONFIG_RGBA_COLOR_PBUFFER],
            (false, true) => &[CONFIG_HANDLE, CONFIG_RGBA_HANDLE, CONFIG_RGBA_COLOR_PBUFFER],
            (true, false) => &[CONFIG_RGBA_HANDLE],
            (false, false) => &[CONFIG_HANDLE, CONFIG_RGBA_HANDLE],
        }
    }

    pub(super) fn current() -> Self {
        Self {
            depth: with_ctx(|c| c.state.depth_bits() as i32),
            rgba: false,
            pbuffer_only: false,
        }
    }

    pub(super) fn from_handle(handle: u32) -> Option<Self> {
        if !Self::handles().contains(&handle) {
            return None;
        }
        let rgba = match handle {
            CONFIG_HANDLE => false,
            CONFIG_RGBA_HANDLE | CONFIG_RGBA_COLOR_PBUFFER => true,
            _ => return None,
        };
        Some(Self {
            rgba,
            pbuffer_only: handle == CONFIG_RGBA_COLOR_PBUFFER,
            ..Self::current()
        })
    }
    pub(super) fn id(handle: u32) -> i32 {
        match handle {
            CONFIG_RGBA_COLOR_PBUFFER => 3,
            CONFIG_RGBA_HANDLE => 2,
            _ => 1,
        }
    }

    fn config_id(&self) -> i32 {
        if self.pbuffer_only {
            Self::id(CONFIG_RGBA_COLOR_PBUFFER)
        } else if self.rgba {
            Self::id(CONFIG_RGBA_HANDLE)
        } else {
            Self::id(CONFIG_HANDLE)
        }
    }

    pub(super) fn attribute(&self, key: u32) -> Option<i32> {
        Some(match key {
            EGL_BUFFER_SIZE => {
                if self.rgba {
                    32
                } else {
                    16
                }
            }
            EGL_RED_SIZE | EGL_BLUE_SIZE => {
                if self.rgba {
                    8
                } else {
                    5
                }
            }
            EGL_GREEN_SIZE => {
                if self.rgba {
                    8
                } else {
                    6
                }
            }
            EGL_ALPHA_SIZE => {
                if self.rgba {
                    8
                } else {
                    0
                }
            }
            EGL_STENCIL_SIZE | EGL_SAMPLES | EGL_SAMPLE_BUFFERS => 0,
            EGL_DEPTH_SIZE => {
                if self.pbuffer_only {
                    0
                } else {
                    self.depth
                }
            }
            EGL_SURFACE_TYPE if self.pbuffer_only => EGL_PBUFFER_BIT as i32,
            EGL_SURFACE_TYPE => {
                (EGL_WINDOW_BIT | EGL_PBUFFER_BIT | if self.rgba { 0 } else { EGL_PIXMAP_BIT })
                    as i32
            }
            EGL_CONFIG_ID => self.config_id(),
            EGL_RENDERABLE_TYPE => 1,
            EGL_CONFIG_CAVEAT | EGL_TRANSPARENT_TYPE => EGL_NONE as i32,
            EGL_NATIVE_RENDERABLE | BIND_TO_TEXTURE_RGB | BIND_TO_TEXTURE_RGBA => EGL_FALSE as i32,
            EGL_LEVEL | EGL_NATIVE_VISUAL_ID | EGL_NATIVE_VISUAL_TYPE => 0,
            TRANSPARENT_RED | TRANSPARENT_GREEN | TRANSPARENT_BLUE => 0,
            EGL_MAX_PBUFFER_WIDTH | EGL_MAX_PBUFFER_HEIGHT => 1024,
            EGL_MAX_PBUFFER_PIXELS => 1024 * 1024,
            // The modeled LCD always observes the existing one-refresh pacing.
            MIN_SWAP_INTERVAL | MAX_SWAP_INTERVAL => 1,
            _ => return None,
        })
    }

    /// EGL 1.1 section 3.4.1: prefer more bits in explicitly requested
    /// nonzero color components, then the smaller overall color buffer.
    /// All other sort criteria are identical for our two configurations.
    pub(super) fn sort_key(&self, pairs: &[(u32, i32)]) -> (std::cmp::Reverse<i32>, i32, i32) {
        let attrs: HashMap<_, _> = pairs.iter().copied().collect();
        let color_bits = [EGL_RED_SIZE, EGL_GREEN_SIZE, EGL_BLUE_SIZE, EGL_ALPHA_SIZE]
            .into_iter()
            .filter(|key| attrs.get(key).is_some_and(|&value| value > 0))
            .map(|key| self.attribute(key).unwrap())
            .sum();
        (
            std::cmp::Reverse(color_bits),
            self.attribute(EGL_BUFFER_SIZE).unwrap(),
            self.config_id(),
        )
    }

    pub(super) fn matches(&self, pairs: &[(u32, i32)]) -> Result<bool, u32> {
        let mut attrs = HashMap::new();
        for &(key, value) in pairs {
            if self.attribute(key).is_none() {
                return Err(EGL_BAD_ATTRIBUTE);
            }
            if value == DONT_CARE && key != EGL_LEVEL {
                attrs.insert(key, value);
                continue;
            }
            let valid = match key {
                EGL_LEVEL => value != DONT_CARE,
                EGL_CONFIG_CAVEAT => {
                    matches!(value, SLOW_CONFIG | NON_CONFORMANT_CONFIG) || value == EGL_NONE as i32
                }
                EGL_TRANSPARENT_TYPE => value == EGL_NONE as i32 || value == TRANSPARENT_RGB,
                EGL_NATIVE_RENDERABLE | BIND_TO_TEXTURE_RGB | BIND_TO_TEXTURE_RGBA => {
                    matches!(value, 0 | 1)
                }
                EGL_SURFACE_TYPE => value >= 0 && value & !7 == 0,
                // Known later API bits may be requested but cannot match ES 1.x.
                EGL_RENDERABLE_TYPE => value >= 0 && value & !0x4f == 0,
                EGL_NATIVE_VISUAL_TYPE
                | EGL_NATIVE_VISUAL_ID
                | EGL_MAX_PBUFFER_WIDTH
                | EGL_MAX_PBUFFER_HEIGHT
                | EGL_MAX_PBUFFER_PIXELS => true,
                _ => value >= 0,
            };
            if !valid {
                return Err(EGL_BAD_ATTRIBUTE);
            }
            attrs.insert(key, value);
        }
        // Validate the whole list even when CONFIG_ID overrides selection.
        if let Some(&id) = attrs.get(&EGL_CONFIG_ID).filter(|&&v| v != DONT_CARE) {
            return Ok(id == self.config_id());
        }
        attrs.entry(EGL_LEVEL).or_insert(0);
        attrs
            .entry(EGL_SURFACE_TYPE)
            .or_insert(EGL_WINDOW_BIT as i32);
        attrs.entry(EGL_TRANSPARENT_TYPE).or_insert(EGL_NONE as i32);
        let window = attrs[&EGL_SURFACE_TYPE] != DONT_CARE
            && attrs[&EGL_SURFACE_TYPE] & EGL_WINDOW_BIT as i32 != 0;
        let transparent = attrs[&EGL_TRANSPARENT_TYPE] == TRANSPARENT_RGB;
        for (&key, &want) in &attrs {
            if want == DONT_CARE {
                continue;
            }
            if matches!(
                key,
                EGL_MAX_PBUFFER_WIDTH
                    | EGL_MAX_PBUFFER_HEIGHT
                    | EGL_MAX_PBUFFER_PIXELS
                    | EGL_NATIVE_VISUAL_ID
            ) || key == EGL_NATIVE_VISUAL_TYPE && !window
                || matches!(key, TRANSPARENT_RED | TRANSPARENT_GREEN | TRANSPARENT_BLUE)
                    && !transparent
            {
                continue;
            }
            let have = self.attribute(key).unwrap();
            let matched = match key {
                EGL_BUFFER_SIZE | EGL_RED_SIZE | EGL_GREEN_SIZE | EGL_BLUE_SIZE
                | EGL_ALPHA_SIZE | EGL_DEPTH_SIZE | EGL_STENCIL_SIZE | EGL_SAMPLE_BUFFERS
                | EGL_SAMPLES => have >= want,
                EGL_SURFACE_TYPE | EGL_RENDERABLE_TYPE => have & want == want,
                _ => have == want,
            };
            if !matched {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Bounded and checked guest attribute reads. No output/state is changed on error.
pub(super) fn read_pairs(ctx: &mut CallCtx<'_>, mut ptr: u32) -> Result<Vec<(u32, i32)>, u32> {
    let mut out = Vec::new();
    if ptr == 0 {
        return Ok(out);
    }
    for _ in 0..256 {
        let key = ctx.cpu.read_u32_le(ptr).map_err(|_| EGL_BAD_ATTRIBUTE)?;
        if key == EGL_NONE {
            return Ok(out);
        }
        let value_ptr = ptr.checked_add(4).ok_or(EGL_BAD_ATTRIBUTE)?;
        let value = ctx
            .cpu
            .read_u32_le(value_ptr)
            .map_err(|_| EGL_BAD_ATTRIBUTE)? as i32;
        out.push((key, value));
        ptr = ptr.checked_add(8).ok_or(EGL_BAD_ATTRIBUTE)?;
    }
    Err(EGL_BAD_ATTRIBUTE)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn goforce_driver_preserves_depth_for_both_color_configurations() {
        let _g = super::super::tests::guard();
        reset_for_test(8, 8);
        set_gizmondo_goforce_driver(true);
        for handle in [CONFIG_HANDLE, CONFIG_RGBA_HANDLE] {
            let config = Config::from_handle(handle).unwrap();
            assert_eq!(config.attribute(EGL_DEPTH_SIZE), Some(16));
            assert_eq!(config.matches(&[(EGL_DEPTH_SIZE, 16)]), Ok(true));
        }
        set_gizmondo_goforce_driver(false);
    }

    #[test]
    fn goforce_5500_color_pbuffer_is_separate_from_window_configs() {
        let _g = super::super::tests::guard();
        reset_for_test(8, 8);
        assert!(Config::from_handle(CONFIG_RGBA_COLOR_PBUFFER).is_none());
        set_goforce_5500_profile(true);
        let config = Config::from_handle(CONFIG_RGBA_COLOR_PBUFFER).unwrap();
        assert_eq!(config.attribute(EGL_DEPTH_SIZE), Some(0));
        assert_eq!(config.attribute(EGL_ALPHA_SIZE), Some(8));
        assert_eq!(config.matches(&[]), Ok(false));
        assert_eq!(
            config.matches(&[(EGL_SURFACE_TYPE, EGL_PBUFFER_BIT as i32)]),
            Ok(true)
        );
        set_goforce_5500_profile(false);
    }

    #[test]
    fn matching_uses_minimum_exact_mask_and_dont_care_rules() {
        let c = Config {
            depth: 16,
            rgba: false,
            pbuffer_only: false,
        };
        for (pairs, want) in [
            (vec![], true),
            (
                vec![
                    (EGL_RED_SIZE, 5),
                    (EGL_GREEN_SIZE, 6),
                    (EGL_BLUE_SIZE, 5),
                    (EGL_DEPTH_SIZE, 16),
                ],
                true,
            ),
            (vec![(EGL_RED_SIZE, 8)], false),
            (vec![(EGL_ALPHA_SIZE, 1)], false),
            (vec![(EGL_STENCIL_SIZE, 8)], false),
            (vec![(EGL_SAMPLES, 2)], false),
            (vec![(EGL_SAMPLE_BUFFERS, 1)], false),
            (vec![(EGL_LEVEL, 1)], false),
            (vec![(EGL_NATIVE_RENDERABLE, 1)], false),
            (vec![(EGL_CONFIG_CAVEAT, SLOW_CONFIG)], false),
            (vec![(EGL_SURFACE_TYPE, 7)], true),
            (vec![(EGL_RENDERABLE_TYPE, 4)], false),
            (
                vec![(EGL_RED_SIZE, DONT_CARE), (EGL_STENCIL_SIZE, DONT_CARE)],
                true,
            ),
            (vec![(EGL_TRANSPARENT_TYPE, TRANSPARENT_RGB)], false),
            (vec![(BIND_TO_TEXTURE_RGB, 1)], false),
        ] {
            assert_eq!(c.matches(&pairs), Ok(want), "{pairs:?}");
        }
        assert_eq!(
            Config {
                depth: 24,
                rgba: false,
                pbuffer_only: false,
            }
            .matches(&[(EGL_DEPTH_SIZE, 24)]),
            Ok(true)
        );
        assert_eq!(c.matches(&[(EGL_DEPTH_SIZE, 24)]), Ok(false));
    }
    #[test]
    fn id_override_and_query_only_attributes_follow_selection_rules() {
        let c = Config {
            depth: 16,
            rgba: false,
            pbuffer_only: false,
        };
        assert_eq!(
            c.matches(&[(EGL_CONFIG_ID, 1), (EGL_STENCIL_SIZE, 8)]),
            Ok(true)
        );
        assert_eq!(c.matches(&[(EGL_CONFIG_ID, 2)]), Ok(false));
        assert_eq!(
            c.matches(&[(EGL_MAX_PBUFFER_WIDTH, 9999), (EGL_NATIVE_VISUAL_ID, 99)]),
            Ok(true)
        );
        assert_eq!(
            c.matches(&[(EGL_SURFACE_TYPE, 1), (EGL_NATIVE_VISUAL_TYPE, 99)]),
            Ok(true)
        );
        assert_eq!(c.matches(&[(TRANSPARENT_RED, 99)]), Ok(true));
        assert_eq!(c.matches(&[(EGL_RED_SIZE, 8), (EGL_RED_SIZE, 5)]), Ok(true));
    }
    #[test]
    fn invalid_names_values_and_level_dont_care_are_errors() {
        let c = Config {
            depth: 16,
            rgba: false,
            pbuffer_only: false,
        };
        for pair in [
            (0xdead, 0),
            (EGL_RED_SIZE, -2),
            (EGL_LEVEL, -1),
            (EGL_NATIVE_RENDERABLE, 2),
            (EGL_CONFIG_CAVEAT, 0),
            (EGL_TRANSPARENT_TYPE, 0),
            (EGL_SURFACE_TYPE, 8),
            (EGL_RENDERABLE_TYPE, 0x100),
            (BIND_TO_TEXTURE_RGBA, 2),
        ] {
            assert_eq!(c.matches(&[pair]), Err(EGL_BAD_ATTRIBUTE), "{pair:?}");
        }
        assert_eq!(
            c.matches(&[(EGL_CONFIG_ID, 1), (0xdead, 0)]),
            Err(EGL_BAD_ATTRIBUTE)
        );
    }
}
