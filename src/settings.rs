use std::time::Duration;

use crate::types::Point;

#[derive(Clone, Debug)]
pub struct Settings {
    pub pause: Duration,
    pub failsafe: bool,
    pub failsafe_points: Vec<Point>,
    pub minimum_duration: Duration,
    pub darwin_catch_up: Duration,
}

impl Settings {
    pub fn for_screen(width: i32, height: i32) -> Self {
        let w = (width - 1).max(0);
        let h = (height - 1).max(0);
        Self {
            pause: Duration::from_millis(100),
            failsafe: true,
            failsafe_points: vec![
                Point::new(0, 0),
                Point::new(w, 0),
                Point::new(0, h),
                Point::new(w, h),
            ],
            minimum_duration: Duration::from_millis(0),
            darwin_catch_up: Duration::from_millis(10),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::for_screen(1920, 1080)
    }
}
