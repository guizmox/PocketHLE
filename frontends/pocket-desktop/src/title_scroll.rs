//! Time-based title marquee. Offsets are logical UI pixels, independent of FPS.
use std::time::Duration;

const SPEED: f64 = 32.0;
const HOLD: f64 = 1.5;

#[derive(Debug)]
pub struct Frame {
    pub offset: f32,
    pub repaint_after: Option<Duration>,
}

pub fn frame(elapsed: f64, overflow: f32) -> Frame {
    if !overflow.is_finite() || overflow <= 0.5 || !elapsed.is_finite() {
        return Frame { offset: 0.0, repaint_after: None };
    }
    let travel = f64::from(overflow) / SPEED;
    let phase = elapsed.max(0.0) % (2.0 * (travel + HOLD));
    let (offset, delay) = if phase < HOLD {
        (0.0, HOLD - phase)
    } else if phase < HOLD + travel {
        ((phase - HOLD) * SPEED, 1.0 / 30.0)
    } else if phase < 2.0 * HOLD + travel {
        (f64::from(overflow), 2.0 * HOLD + travel - phase)
    } else {
        (f64::from(overflow) - (phase - 2.0 * HOLD - travel) * SPEED, 1.0 / 30.0)
    };
    Frame {
        offset: (offset as f32).clamp(0.0, overflow),
        repaint_after: Some(Duration::from_secs_f64(delay.max(0.001))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fitting_titles_remain_still_without_animation_repaints() {
        for width in [-100.0,0.0,0.5] {
            for time in [0.0,2.0,100.0] {
                let f=frame(time,width);
                assert_eq!(f.offset,0.0);assert!(f.repaint_after.is_none());
            }
        }
    }
    #[test]
    fn holds_both_ends_and_reveals_the_complete_overflow() {
        // 64 px takes 2 seconds at 32 px/s.
        assert_eq!(frame(0.0,64.0).offset,0.0);
        assert_eq!(frame(1.49,64.0).offset,0.0);
        assert_eq!(frame(2.5,64.0).offset,32.0);
        assert_eq!(frame(3.5,64.0).offset,64.0);
        assert_eq!(frame(4.99,64.0).offset,64.0);
        assert_eq!(frame(6.0,64.0).offset,32.0);
        assert_eq!(frame(7.0,64.0).offset,0.0);
    }
    #[test]
    fn bounded_offsets_for_small_and_large_overflows() {
        for overflow in [0.51,1.0,32.0,180.0,2000.0] {
            for tick in 0..10000 {
                let f=frame(tick as f64/60.0,overflow);
                assert!(f.offset>=0.0 && f.offset<=overflow);
                assert!(f.repaint_after.unwrap()>Duration::ZERO);
            }
        }
    }
    #[test]
    fn movement_is_time_based_and_does_not_depend_on_render_frame_count() {
        assert_eq!(frame(2.0,320.0).offset,16.0);
        assert_eq!(frame(2.5,320.0).offset-frame(2.0,320.0).offset,16.0);
        assert_eq!(frame(3.0,320.0).offset-frame(2.5,320.0).offset,16.0);
    }
    #[test]
    fn holds_sleep_until_motion_and_motion_requests_only_thirty_fps() {
        assert_eq!(frame(0.0,64.0).repaint_after,Some(Duration::from_millis(1500)));
        assert_eq!(frame(3.5,64.0).repaint_after,Some(Duration::from_millis(1500)));
        assert!(frame(2.0,64.0).repaint_after.unwrap()>=Duration::from_millis(33));
    }
    #[test]
    fn invalid_geometry_and_clock_values_are_safe() {
        assert!(frame(0.0,f32::NAN).repaint_after.is_none());
        assert!(frame(f64::NAN,64.0).repaint_after.is_none());
        assert!(frame(0.0,f32::INFINITY).repaint_after.is_none());
        assert_eq!(frame(-4.0,64.0).offset,0.0);
    }
}
