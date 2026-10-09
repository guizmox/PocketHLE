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
    let [w,h] = [native[0].max(1), native[1].max(1)];
    let factor = (available[0] / w).min(available[1] / h).max(1);
    let width = w * factor;
    let height = h * factor;
    IntegerFit { factor, width, height,
        left: available[0].saturating_sub(width) / 2,
        top: available[1].saturating_sub(height) / 2 }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn landscape_modes_choose_the_largest_whole_factor() {
        for (size, factor, image) in [([1920,1080],4,[1280,960]),
            ([2560,1440],6,[1920,1440]),([3840,2160],9,[2880,2160]),
            ([3440,1440],6,[1920,1440]),([1080,1920],3,[960,720])] {
            let fit = integer_fit(size,[320,240]);
            assert_eq!(fit.factor,factor);
            assert_eq!([fit.width,fit.height],image);
            assert!(fit.width <= size[0] && fit.height <= size[1]);
            assert_eq!(fit.width*3,fit.height*4);
            assert!((factor+1)*320>size[0] || (factor+1)*240>size[1]);
        }
    }
    #[test]
    fn centering_stays_on_physical_pixels_and_never_uses_fractional_scale() {
        let fit = integer_fit([1921,1081],[320,240]);
        assert_eq!(fit,IntegerFit { factor:4,left:320,top:60,width:1280,height:960 });
        // At 125% DPI these dimensions become 1024x768 UI points,
        // still exactly 1280x960 physical pixels, rather than a 3x image.
        assert_eq!(fit.width as f32 / 1.25,1024.0);
        assert_eq!(integer_fit([319,239],[320,240]).factor,1);
    }
}
