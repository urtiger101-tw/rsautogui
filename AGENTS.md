# AGENTS.md — autogui-rs

You are building **autogui-rs**, a Rust library that clones PyAutoGUI 0.9.x behavior, including screenshot image locate and **Windows window control**.

Read `SPEC.md` first. Follow it over guesswork. Do not invent extra public APIs. Do not implement macOS/Linux window control in v1.

## Hard rules

- Language: Rust 2024 edition, `rustc` 1.85+ if available, else latest stable.
- Public crate name: `autogui`.
- Errors: `thiserror`. No `unwrap()` in library paths.
- Input backend: `enigo` for mouse/keyboard. Wrap it; do not leak enigo types in the public API.
- Screenshot: `xcap` or `screenshots` crate. Image matching: `image` + optional OpenCV via feature `opencv`.
- Windows window control: `windows` crate (Win32 `user32` / `WinUser`). Must work without extra Python deps.
- Coordinates: origin top-left of the **primary** monitor. Integers are `i32`.
- After every public mutating call: run fail-safe check, then sleep `settings.pause`.
- Default fail-safe **on**. Corners of the primary monitor abort with `Error::FailSafe`.
- No network. No telemetry.
- Message boxes (`alert`/`confirm`/`prompt`) are **out of scope for v1**.
- Multi-monitor virtual desktop coordinates are **out of scope for v1** except reading primary size.
- Do not disable fail-safe in examples that move the mouse.

## Build / test commands

```
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Integration tests that move the real mouse/keyboard must be gated:

```
#[ignore]
```

and only run with `AUTOGUI_LIVE=1 cargo test -- --ignored`.

Windows window tests must compile on all targets but only execute on `cfg(windows)`.

## File layout (do not flatten)

See SPEC.md §8.

## Definition of done

A feature is done only when:

1. Public API matches SPEC.md names and types.
2. Unit tests exist for pure logic (tween, fail-safe geometry, box/center math, pixel tolerance, title match).
3. `cargo test` (non-ignored) is green on the current OS.
4. README shows a 20-line example equivalent to PyAutoGUI cheat sheet.
5. CHANGELOG notes the feature.

Do not mark locate or window APIs done without tests for the error types.
