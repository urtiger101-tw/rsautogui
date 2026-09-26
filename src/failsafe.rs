use crate::error::{Error, Result};
use crate::settings::Settings;
use crate::types::Point;

pub fn triggered(settings: &Settings, pos: Point) -> bool {
    if !settings.failsafe {
        return false;
    }
    settings
        .failsafe_points
        .iter()
        .any(|p| p.x == pos.x && p.y == pos.y)
}

pub fn check(settings: &Settings, pos: Point) -> Result<()> {
    if triggered(settings, pos) {
        Err(Error::FailSafe(pos))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_hit() {
        let s = Settings::for_screen(100, 50);
        assert!(triggered(&s, Point::new(0, 0)));
        assert!(triggered(&s, Point::new(99, 0)));
        assert!(triggered(&s, Point::new(0, 49)));
        assert!(triggered(&s, Point::new(99, 49)));
        assert!(!triggered(&s, Point::new(1, 0)));
        assert!(!triggered(&s, Point::new(0, 1)));
    }

    #[test]
    fn disabled() {
        let mut s = Settings::for_screen(100, 50);
        s.failsafe = false;
        assert!(!triggered(&s, Point::new(0, 0)));
    }
}
