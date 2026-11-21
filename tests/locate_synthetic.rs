//! SPEC section 10.1: locate on synthetic images; never touches the real screen.

use std::time::Duration;

use autogui::prelude::*;
use autogui::{FakeBackend, Screenshot};

const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
const BLUE: Rgb = Rgb { r: 0, g: 0, b: 255 };

fn gui() -> AutoGui {
    let mut g = AutoGui::with_fake(FakeBackend::new(300, 300));
    g.settings_mut().pause = Duration::ZERO;
    g.settings_mut().failsafe = false;
    g
}

fn haystack(blocks: &[(i32, i32)]) -> Screenshot {
    let mut s = Screenshot::solid(200, 200, RED);
    for &(bx, by) in blocks {
        for y in by..by + 10 {
            for x in bx..bx + 10 {
                s.set_pixel(x, y, BLUE).unwrap();
            }
        }
    }
    s
}

#[test]
fn locate_finds_blue_block() {
    let g = gui();
    let needle = Screenshot::solid(10, 10, BLUE);
    let hit = g
        .locate(&needle, &haystack(&[(37, 61)]), &LocateOptions::default())
        .unwrap();
    assert_eq!(hit, Rect::new(37, 61, 10, 10));
    assert_eq!(center(hit), Point { x: 42, y: 66 });
}

#[test]
fn locate_missing_is_image_not_found() {
    let g = gui();
    let needle = Screenshot::solid(10, 10, BLUE);
    let err = g
        .locate(&needle, &haystack(&[]), &LocateOptions::default())
        .unwrap_err();
    assert!(matches!(err, Error::ImageNotFound));
}

#[test]
fn locate_all_two_separate_blocks() {
    let g = gui();
    let needle = Screenshot::solid(10, 10, BLUE);
    let hits = g
        .locate_all(
            &needle,
            &haystack(&[(10, 10), (120, 150)]),
            &LocateOptions::default(),
        )
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.contains(&Rect::new(10, 10, 10, 10)));
    assert!(hits.contains(&Rect::new(120, 150, 10, 10)));
}

#[test]
fn raise_image_not_found_false_returns_empty() {
    let mut g = gui();
    let needle = Screenshot::solid(10, 10, BLUE);
    assert!(matches!(
        g.locate_all(&needle, &haystack(&[]), &LocateOptions::default()),
        Err(Error::ImageNotFound)
    ));
    g.set_raise_image_not_found(false);
    let hits = g
        .locate_all(&needle, &haystack(&[]), &LocateOptions::default())
        .unwrap();
    assert!(hits.is_empty());
}

#[test]
fn needle_larger_than_haystack_is_invalid_argument() {
    let g = gui();
    let needle = Screenshot::solid(300, 300, BLUE);
    let err = g
        .locate(&needle, &haystack(&[]), &LocateOptions::default())
        .unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)));
}

#[cfg(not(feature = "opencv"))]
#[test]
fn confidence_without_opencv_is_feature_disabled() {
    let g = gui();
    let needle = Screenshot::solid(10, 10, BLUE);
    let opt = LocateOptions {
        confidence: Some(0.9),
        ..Default::default()
    };
    let err = g.locate(&needle, &haystack(&[(5, 5)]), &opt).unwrap_err();
    assert!(matches!(err, Error::FeatureDisabled(_)));
}

#[test]
fn locate_on_screen_uses_fake_screen_and_region_offsets() {
    let mut fake = FakeBackend::new(100, 80);
    for y in 31..34 {
        for x in 42..45 {
            fake.screen.set_pixel(x, y, BLUE).unwrap();
        }
    }
    let mut g = AutoGui::with_fake(fake);
    g.settings_mut().pause = Duration::ZERO;
    g.settings_mut().failsafe = false;
    let needle = Screenshot::solid(3, 3, BLUE);
    let opt = LocateOptions {
        region: Some(Rect::new(25, 20, 40, 30)),
        ..Default::default()
    };
    assert_eq!(
        g.locate_screenshot_on_screen(&needle, &opt).unwrap(),
        Rect::new(42, 31, 3, 3)
    );
}

#[test]
fn window_apis_follow_platform_contract() {
    let g = gui();
    if cfg!(windows) {
        assert!(g.get_windows_at(-100_000, -100_000).unwrap().is_empty());
        let none = g
            .get_windows_with_title("\u{1}no such window title\u{1}")
            .unwrap();
        assert!(none.is_empty());
        assert!(
            g.get_windows_with_title_ci("\u{1}no such window title\u{1}")
                .unwrap()
                .is_empty()
        );
    } else {
        assert!(matches!(
            g.get_all_windows(),
            Err(Error::UnsupportedPlatform)
        ));
        assert!(matches!(
            g.get_all_titles(),
            Err(Error::UnsupportedPlatform)
        ));
        assert!(matches!(
            g.get_active_window(),
            Err(Error::UnsupportedPlatform)
        ));
        assert!(matches!(
            g.get_windows_with_title("x"),
            Err(Error::UnsupportedPlatform)
        ));
        assert!(matches!(
            g.get_windows_with_title_ci("x"),
            Err(Error::UnsupportedPlatform)
        ));
        assert!(matches!(
            g.get_windows_at(0, 0),
            Err(Error::UnsupportedPlatform)
        ));
    }
}
