//! ES 1.1 fixed-function texture combiner; shared state and reference evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combine {
    pub rgb: u32,
    pub alpha: u32,
    pub rgb_sources: [u32; 3],
    pub alpha_sources: [u32; 3],
    pub rgb_operands: [u32; 3],
    pub alpha_operands: [u32; 3],
    pub rgb_scale: u32,
    pub alpha_scale: u32,
}
impl Default for Combine {
    fn default() -> Self {
        Self {
            rgb: 0x2100,
            alpha: 0x2100,
            rgb_sources: [0x1702, 0x8578, 0x8576],
            alpha_sources: [0x1702, 0x8578, 0x8576],
            rgb_operands: [0x300, 0x300, 0x302],
            alpha_operands: [0x302; 3],
            rgb_scale: 1,
            alpha_scale: 1,
        }
    }
}
impl Combine {
    pub fn get(self, pname: u32) -> Option<u32> {
        Some(match pname {
            0x8571 => self.rgb,
            0x8572 => self.alpha,
            0x8573 => self.rgb_scale,
            0x0d1c => self.alpha_scale,
            0x8580..=0x8582 => self.rgb_sources[(pname - 0x8580) as usize],
            0x8588..=0x858a => self.alpha_sources[(pname - 0x8588) as usize],
            0x8590..=0x8592 => self.rgb_operands[(pname - 0x8590) as usize],
            0x8598..=0x859a => self.alpha_operands[(pname - 0x8598) as usize],
            _ => return None,
        })
    }
    /// Error enum, leaving state untouched on failure.
    pub fn set(&mut self, pname: u32, value: u32) -> Result<(), u32> {
        let valid = match pname {
            0x8571 => matches!(
                value,
                0x1e01 | 0x2100 | 0x104 | 0x8574 | 0x8575 | 0x84e7 | 0x86ae | 0x86af
            ),
            0x8572 => matches!(value, 0x1e01 | 0x2100 | 0x104 | 0x8574 | 0x8575 | 0x84e7),
            0x8573 | 0x0d1c => {
                if !matches!(value, 1 | 2 | 4) {
                    return Err(0x501);
                }
                true
            }
            0x8580..=0x8582 | 0x8588..=0x858a => matches!(value, 0x1702 | 0x8576 | 0x8577 | 0x8578),
            0x8590..=0x8592 => matches!(value, 0x300..=0x303),
            0x8598..=0x859a => matches!(value, 0x302 | 0x303),
            _ => false,
        };
        if !valid {
            return Err(0x500);
        }
        match pname {
            0x8571 => self.rgb = value,
            0x8572 => self.alpha = value,
            0x8573 => self.rgb_scale = value,
            0x0d1c => self.alpha_scale = value,
            0x8580..=0x8582 => self.rgb_sources[(pname - 0x8580) as usize] = value,
            0x8588..=0x858a => self.alpha_sources[(pname - 0x8588) as usize] = value,
            0x8590..=0x8592 => self.rgb_operands[(pname - 0x8590) as usize] = value,
            0x8598..=0x859a => self.alpha_operands[(pname - 0x8598) as usize] = value,
            _ => unreachable!(),
        };
        Ok(())
    }
    pub fn evaluate(
        self,
        previous: [f32; 4],
        texture: [f32; 4],
        constant: [f32; 4],
        primary: [f32; 4],
    ) -> [f32; 4] {
        let source = |s| match s {
            0x1702 => texture,
            0x8576 => constant,
            0x8577 => primary,
            _ => previous,
        };
        let rgb: [[f32; 3]; 3] = std::array::from_fn(|i| {
            let s = source(self.rgb_sources[i]);
            std::array::from_fn(|c| match self.rgb_operands[i] {
                0x300 => s[c],
                0x301 => 1. - s[c],
                0x302 => s[3],
                _ => 1. - s[3],
            })
        });
        let alpha: [f32; 3] = std::array::from_fn(|i| {
            let s = source(self.alpha_sources[i]);
            if self.alpha_operands[i] == 0x302 {
                s[3]
            } else {
                1. - s[3]
            }
        });
        let operation = |f, x: f32, y: f32, z: f32| match f {
            0x1e01 => x,
            0x2100 => x * y,
            0x104 => x + y,
            0x8574 => x + y - 0.5,
            0x8575 => x * z + y * (1. - z),
            0x84e7 => x - y,
            _ => 0.,
        };
        let dot = 4.
            * (0..3)
                .map(|c| (rgb[0][c] - 0.5) * (rgb[1][c] - 0.5))
                .sum::<f32>();
        let mut out = std::array::from_fn(|c| {
            if c < 3 {
                (if matches!(self.rgb, 0x86ae | 0x86af) {
                    dot
                } else {
                    operation(self.rgb, rgb[0][c], rgb[1][c], rgb[2][c])
                }) * self.rgb_scale as f32
            } else {
                operation(self.alpha, alpha[0], alpha[1], alpha[2]) * self.alpha_scale as f32
            }
        });
        if self.rgb == 0x86af {
            out[3] = dot * self.alpha_scale as f32;
        }
        out.map(|v| v.clamp(0., 1.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_combiner_operations_have_distinct_checked_results() {
        let mut c = Combine::default();
        let t = [0.8, 0.8, 0.8, 0.8];
        let p = [0.2, 0.2, 0.2, 0.4];
        for (op, want) in [
            (0x1e01, 0.8),
            (0x2100, 0.16),
            (0x104, 1.),
            (0x8574, 0.5),
            (0x8575, 0.5),
            (0x84e7, 0.6),
            (0x86ae, 0.),
            (0x86af, 0.),
        ] {
            c.set(0x8571, op).unwrap();
            let result = c.evaluate(p, t, [0.5; 4], p);
            assert!((result[0] - want).abs() < 0.00001, "op={op:x} {result:?}");
            if op == 0x86af {
                assert_eq!(result[3], 0.);
            }
        }
    }
    #[test]
    fn invalid_combiner_parameters_are_transactional() {
        let mut c = Combine::default();
        let before = c;
        assert_eq!(c.set(0x8572, 0x86ae), Err(0x500));
        assert_eq!(c.set(0x8573, 3), Err(0x501));
        assert_eq!(c, before);
    }
    #[test]
    fn dot3_rgba_overrides_alpha_operation_then_applies_separate_scales() {
        let mut c = Combine::default();
        c.rgb = 0x86af;
        c.alpha = 0x1e01;
        c.rgb_scale = 1;
        c.alpha_scale = 2;
        let result = c.evaluate([0.75; 4], [0.75; 4], [0.; 4], [0.; 4]);
        assert_eq!(result, [0.75, 0.75, 0.75, 1.]);
    }
}
