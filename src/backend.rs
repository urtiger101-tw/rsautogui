use crate::error::{Error, Result};
use crate::types::{MouseButton, Point, Rect, Screenshot, Size};
use std::sync::{Arc, Mutex};

pub trait Backend {
    fn mouse_pos(&self) -> Result<Point>;
    fn move_mouse(&mut self, p: Point) -> Result<()>;
    fn move_mouse_desktop(&mut self, p: Point) -> Result<()> {
        self.move_mouse(p)
    }
    fn button(&mut self, b: MouseButton, down: bool) -> Result<()>;
    fn scroll(&mut self, dy: i32, dx: i32) -> Result<()>;
    fn key(&mut self, key: &str, down: bool) -> Result<()>;
    fn text(&mut self, s: &str) -> Result<()>;
    fn screen_size(&self) -> Result<Size>;
    fn capture(&self, region: Option<Rect>) -> Result<Screenshot>;
}

#[derive(Clone, Debug)]
pub struct FakeBackend {
    pub pos: Point,
    pub size: Size,
    pub screen: Screenshot,
    pub events: Vec<String>,
    pub held: Vec<String>,
    shared_events: Arc<Mutex<Vec<String>>>,
}

impl FakeBackend {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            pos: Point::new(width / 2, height / 2),
            size: Size::new(width, height),
            screen: Screenshot::solid(width as u32, height as u32, crate::types::Rgb::new(0, 0, 0)),
            events: Vec::new(),
            held: Vec::new(),
            shared_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[doc(hidden)]
    pub fn shared_events(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.shared_events)
    }

    fn record(&mut self, event: String) -> Result<()> {
        self.events.push(event.clone());
        self.shared_events
            .lock()
            .map_err(|_| Error::Input("fake event log is poisoned".into()))?
            .push(event);
        Ok(())
    }
}

impl Backend for FakeBackend {
    fn mouse_pos(&self) -> Result<Point> {
        Ok(self.pos)
    }

    fn move_mouse(&mut self, p: Point) -> Result<()> {
        self.pos = p;
        self.record(format!("move {},{}", p.x, p.y))
    }

    fn button(&mut self, b: MouseButton, down: bool) -> Result<()> {
        self.record(format!(
            "button {:?} {}",
            b,
            if down { "down" } else { "up" }
        ))
    }

    fn scroll(&mut self, dy: i32, dx: i32) -> Result<()> {
        self.record(format!("scroll dy={dy} dx={dx}"))
    }

    fn key(&mut self, key: &str, down: bool) -> Result<()> {
        let name = crate::keys::normalize(key).ok_or(Error::InvalidArgument("unknown key"))?;
        if down {
            self.held.push(name.clone());
            self.record(format!("{name} down"))?;
        } else {
            self.held.retain(|k| k != &name);
            self.record(format!("{name} up"))?;
        }
        Ok(())
    }

    fn text(&mut self, s: &str) -> Result<()> {
        self.record(format!("text {s}"))
    }

    fn screen_size(&self) -> Result<Size> {
        Ok(self.size)
    }

    fn capture(&self, region: Option<Rect>) -> Result<Screenshot> {
        match region {
            None => Ok(self.screen.clone()),
            Some(r) => self.screen.region(r),
        }
    }
}

pub fn default_backend() -> Result<Box<dyn Backend>> {
    #[cfg(feature = "os")]
    {
        Ok(Box::new(OsBackend::new()?))
    }

    #[cfg(not(feature = "os"))]
    {
        Ok(Box::new(FakeBackend::new(1920, 1080)))
    }
}

#[cfg(feature = "os")]
struct OsBackend {
    enigo: enigo::Enigo,
}

#[cfg(feature = "os")]
impl OsBackend {
    fn new() -> Result<Self> {
        use enigo::Settings;

        let _dpi = crate::dpi::DpiGuard::enter()?;
        let enigo = enigo::Enigo::new(&Settings::default())
            .map_err(|error| Error::Input(error.to_string()))?;
        Ok(Self { enigo })
    }

    fn primary_monitor() -> Result<xcap::Monitor> {
        let monitors =
            xcap::Monitor::all().map_err(|error| Error::Screenshot(error.to_string()))?;
        let mut fallback = None;
        for monitor in monitors {
            if monitor
                .is_primary()
                .map_err(|error| Error::Screenshot(error.to_string()))?
            {
                return Ok(monitor);
            }
            fallback.get_or_insert(monitor);
        }
        fallback.ok_or_else(|| Error::Screenshot("no monitor found".into()))
    }
}

#[cfg(feature = "os")]
impl Backend for OsBackend {
    fn move_mouse_desktop(&mut self, p: Point) -> Result<()> {
        #[cfg(windows)]
        {
            crate::display::move_cursor(p)
        }
        #[cfg(not(windows))]
        {
            let _ = p;
            Err(Error::UnsupportedPlatform)
        }
    }

    fn mouse_pos(&self) -> Result<Point> {
        #[cfg(windows)]
        {
            crate::dpi::cursor_position()
        }
        #[cfg(not(windows))]
        {
            use enigo::Mouse;
            self.enigo
                .location()
                .map(|(x, y)| Point::new(x, y))
                .map_err(|error| Error::Input(error.to_string()))
        }
    }

    fn move_mouse(&mut self, p: Point) -> Result<()> {
        use enigo::{Coordinate, Mouse};

        let _dpi = crate::dpi::DpiGuard::enter()?;
        self.enigo
            .move_mouse(p.x, p.y, Coordinate::Abs)
            .map_err(|error| Error::Input(error.to_string()))
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<()> {
        use enigo::{Button, Direction, Mouse};

        let button = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Middle => Button::Middle,
            MouseButton::Right => Button::Right,
        };
        self.enigo
            .button(
                button,
                if down {
                    Direction::Press
                } else {
                    Direction::Release
                },
            )
            .map_err(|error| Error::Input(error.to_string()))
    }

    fn scroll(&mut self, dy: i32, dx: i32) -> Result<()> {
        use enigo::{Axis, Mouse};

        if dy != 0 {
            self.enigo
                .scroll(
                    dy.checked_neg()
                        .ok_or(Error::InvalidArgument("scroll count out of range"))?,
                    Axis::Vertical,
                )
                .map_err(|error| Error::Input(error.to_string()))?;
        }
        if dx != 0 {
            self.enigo
                .scroll(dx, Axis::Horizontal)
                .map_err(|error| Error::Input(error.to_string()))?;
        }
        Ok(())
    }

    fn key(&mut self, key: &str, down: bool) -> Result<()> {
        use enigo::{Direction, Keyboard};

        let name = crate::keys::normalize(key).ok_or(Error::InvalidArgument("unknown key"))?;
        let key = enigo_key(&name)?;
        self.enigo
            .key(
                key,
                if down {
                    Direction::Press
                } else {
                    Direction::Release
                },
            )
            .map_err(|error| Error::Input(error.to_string()))
    }

    fn text(&mut self, text: &str) -> Result<()> {
        use enigo::Keyboard;

        self.enigo
            .text(text)
            .map_err(|error| Error::Input(error.to_string()))
    }

    fn screen_size(&self) -> Result<Size> {
        let _dpi = crate::dpi::DpiGuard::enter()?;
        let monitor = Self::primary_monitor()?;
        let width = monitor
            .width()
            .map_err(|error| Error::Screenshot(error.to_string()))?;
        let height = monitor
            .height()
            .map_err(|error| Error::Screenshot(error.to_string()))?;
        let width =
            i32::try_from(width).map_err(|_| Error::InvalidArgument("screen width overflow"))?;
        let height =
            i32::try_from(height).map_err(|_| Error::InvalidArgument("screen height overflow"))?;
        Ok(Size::new(width, height))
    }

    fn capture(&self, region: Option<Rect>) -> Result<Screenshot> {
        let _dpi = crate::dpi::DpiGuard::enter()?;
        let monitor = Self::primary_monitor()?;
        let image = if let Some(region) = region {
            let width = monitor
                .width()
                .map_err(|e| Error::Screenshot(e.to_string()))?;
            let height = monitor
                .height()
                .map_err(|e| Error::Screenshot(e.to_string()))?;
            let clipped = region
                .intersect(&Rect::new(0, 0, width as i32, height as i32))
                .ok_or(Error::InvalidArgument("region does not intersect screen"))?;
            monitor.capture_region(
                clipped.left as u32,
                clipped.top as u32,
                clipped.width as u32,
                clipped.height as u32,
            )
        } else {
            monitor.capture_image()
        }
        .map_err(|error| Error::Screenshot(error.to_string()))?;
        let (width, height) = (image.width(), image.height());
        let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
        Screenshot::new(width, height, rgb.into_raw())
    }
}

#[cfg(feature = "os")]
fn enigo_key(name: &str) -> Result<enigo::Key> {
    use enigo::Key;

    let key = match name {
        "enter" => Key::Return,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "esc" => Key::Escape,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "shift" => Key::Shift,
        "shiftright" => Key::RShift,
        "ctrl" => Key::Control,
        "ctrlright" => Key::RControl,
        "alt" => Key::Alt,
        #[cfg(windows)]
        "altright" => Key::Other(0xA5),
        #[cfg(not(windows))]
        "altright" => Key::Alt,
        "meta" => Key::Meta,
        "capslock" => Key::CapsLock,
        "numlock" => Key::Numlock,
        "scrolllock" => Key::Other(0x91),
        "printscreen" => Key::PrintScr,
        "pause" => Key::Pause,
        "volumedown" => Key::VolumeDown,
        "volumeup" => Key::VolumeUp,
        "volumemute" => Key::VolumeMute,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        "f13" => Key::F13,
        "f14" => Key::F14,
        "f15" => Key::F15,
        "f16" => Key::F16,
        "f17" => Key::F17,
        "f18" => Key::F18,
        "f19" => Key::F19,
        "f20" => Key::F20,
        "f21" => Key::F21,
        "f22" => Key::F22,
        "f23" => Key::F23,
        "f24" => Key::F24,
        "sleep" => Key::Other(0x5F),
        single if single.chars().count() == 1 => {
            let ch = single
                .chars()
                .next()
                .ok_or(Error::InvalidArgument("unknown key"))?;
            #[cfg(target_os = "windows")]
            {
                let vk = match ch {
                    'a'..='z' => ch.to_ascii_uppercase() as u32,
                    '0'..='9' => ch as u32,
                    '-' => 0xBD,
                    '=' => 0xBB,
                    '[' => 0xDB,
                    ']' => 0xDD,
                    '\\' => 0xDC,
                    ';' => 0xBA,
                    '\'' => 0xDE,
                    ',' => 0xBC,
                    '.' => 0xBE,
                    '/' => 0xBF,
                    '`' => 0xC0,
                    _ => return Ok(Key::Unicode(ch)),
                };
                Key::Other(vk)
            }
            #[cfg(not(target_os = "windows"))]
            {
                Key::Unicode(ch)
            }
        }
        _ => return Err(Error::InvalidArgument("unknown key")),
    };
    Ok(key)
}
