//! SPEC section 10.1: tween endpoints, box/center math, fail-safe geometry, pixel tolerance.

use std::time::Duration;

use autogui::prelude::*;
use autogui::{FakeBackend, Screenshot, tweens};

#[test]
fn tween_endpoints_are_fixed_for_every_easing() {
    let all: &[(&str, Tween)] = &[
        ("linear", tweens::linear),
        ("ease_in_quad", tweens::ease_in_quad),
        ("ease_out_quad", tweens::ease_out_quad),
        ("ease_in_out_quad", tweens::ease_in_out_quad),
        ("ease_in_cubic", tweens::ease_in_cubic),
        ("ease_out_cubic", tweens::ease_out_cubic),
        ("ease_in_out_cubic", tweens::ease_in_out_cubic),
        ("ease_in_quart", tweens::ease_in_quart),
        ("ease_out_quart", tweens::ease_out_quart),
        ("ease_in_out_quart", tweens::ease_in_out_quart),
        ("ease_in_quint", tweens::ease_in_quint),
        ("ease_out_quint", tweens::ease_out_quint),
        ("ease_in_out_quint", tweens::ease_in_out_quint),
        ("ease_in_sine", tweens::ease_in_sine),
        ("ease_out_sine", tweens::ease_out_sine),
        ("ease_in_out_sine", tweens::ease_in_out_sine),
        ("ease_in_expo", tweens::ease_in_expo),
        ("ease_out_expo", tweens::ease_out_expo),
        ("ease_in_out_expo", tweens::ease_in_out_expo),
        ("ease_in_circ", tweens::ease_in_circ),
        ("ease_out_circ", tweens::ease_out_circ),
        ("ease_in_out_circ", tweens::ease_in_out_circ),
        ("ease_in_elastic", tweens::ease_in_elastic),
        ("ease_out_elastic", tweens::ease_out_elastic),
        ("ease_in_out_elastic", tweens::ease_in_out_elastic),
        ("ease_in_back", tweens::ease_in_back),
        ("ease_out_back", tweens::ease_out_back),
        ("ease_in_out_back", tweens::ease_in_out_back),
        ("ease_in_bounce", tweens::ease_in_bounce),
        ("ease_out_bounce", tweens::ease_out_bounce),
        ("ease_in_out_bounce", tweens::ease_in_out_bounce),
    ];
    assert_eq!(all.len(), 31);
    for (name, f) in all {
        assert!(f(0.0).abs() < 1e-9, "{name}(0) = {}", f(0.0));
        assert!((f(1.0) - 1.0).abs() < 1e-9, "{name}(1) = {}", f(1.0));
    }
}

#[test]
fn center_uses_integer_division() {
    assert_eq!(center(Rect::new(10, 20, 100, 50)), Point { x: 60, y: 45 });
    assert_eq!(center(Rect::new(0, 0, 11, 11)), Point { x: 5, y: 5 });
    assert_eq!(center(Rect::new(3, 3, 1, 1)), Point { x: 3, y: 3 });
    let b: Box = Rect::new(1, 2, 3, 4);
    assert_eq!(<(i32, i32, i32, i32)>::from(b), (1, 2, 3, 4));
}

fn corner_gui(pos: Point) -> AutoGui {
    let mut fake = FakeBackend::new(100, 50);
    fake.pos = pos;
    let mut g = AutoGui::with_fake(fake);
    g.settings_mut().pause = Duration::ZERO;
    g
}

#[test]
fn failsafe_corners_hit() {
    for (x, y) in [(0, 0), (99, 0), (0, 49), (99, 49)] {
        let mut g = corner_gui(Point { x, y });
        assert!(
            matches!(g.click(), Err(Error::FailSafe(p)) if p == Point { x, y }),
            "corner ({x},{y})"
        );
    }
}

#[test]
fn failsafe_one_pixel_off_corner_misses() {
    for (x, y) in [
        (1, 0),
        (0, 1),
        (98, 0),
        (99, 1),
        (1, 49),
        (0, 48),
        (98, 49),
        (99, 48),
    ] {
        let mut g = corner_gui(Point { x, y });
        assert!(g.click().is_ok(), "({x},{y}) must not trigger");
    }
}

#[test]
fn failsafe_can_be_disabled_and_is_on_by_default() {
    let mut g = corner_gui(Point { x: 0, y: 0 });
    assert!(g.settings().failsafe);
    g.settings_mut().failsafe = false;
    assert!(g.click().is_ok());
}

#[test]
fn pixel_tolerance() {
    let mut fake = FakeBackend::new(10, 10);
    fake.screen
        .set_pixel(5, 5, Rgb { r: 12, g: 8, b: 10 })
        .unwrap();
    let mut g = AutoGui::with_fake(fake);
    g.settings_mut().pause = Duration::ZERO;
    g.settings_mut().failsafe = false;
    let c = Rgb {
        r: 10,
        g: 10,
        b: 10,
    };
    assert!(g.pixel_matches_color(5, 5, c, 2).unwrap());
    assert!(!g.pixel_matches_color(5, 5, c, 1).unwrap());
    assert_eq!(g.pixel(5, 5).unwrap(), Rgb { r: 12, g: 8, b: 10 });
}

#[test]
fn click_with_single_coordinate_is_invalid_argument() {
    let mut g = corner_gui(Point { x: 10, y: 10 });
    let e = g
        .click_ex(Some(1), None, 1, Duration::ZERO, MouseButton::Left)
        .unwrap_err();
    assert!(matches!(e, Error::InvalidArgument(_)));
    let _ = Screenshot::solid(1, 1, Rgb { r: 0, g: 0, b: 0 });
}
