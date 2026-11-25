use std::collections::{HashMap, HashSet};

pub use aero::input::KeyCode;
use aero::input::{Event as AeroEvent, MouseButton, TouchPhase, WheelUnit};
use aero::math::Vec2;
use web_time::Instant;

/// What notan's event callback receives: Rouillo reads only typed characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    ReceivedCharacter(char),
}

/// notan's input state, kept from Aero's events and cleared after each frame.
pub struct App {
    pub keyboard: Keyboard,
    pub mouse: Mouse,
    pub touch: Touch,
    pub timer: Timer,
    window: AppWindow,
}

impl App {
    pub(crate) fn new() -> Self {
        Self {
            keyboard: Keyboard::default(),
            mouse: Mouse::default(),
            touch: Touch::default(),
            timer: Timer::new(),
            window: AppWindow { size: (0, 0), dpi: 1.0 },
        }
    }

    pub fn window(&mut self) -> &mut AppWindow {
        &mut self.window
    }

    pub(crate) fn set_window(&mut self, info: aero::WindowInfo) {
        self.window =
            AppWindow { size: (info.size.x.round() as u32, info.size.y.round() as u32), dpi: f64::from(info.scale) };
    }

    /// Applies `event`; returns the characters it typed.
    pub(crate) fn apply(&mut self, event: &AeroEvent) -> Vec<Event> {
        let delta = self.timer.delta;
        match event {
            AeroEvent::Key { code, pressed, text, .. } => {
                self.keyboard.apply(*code, *pressed, delta);
                let typed = text.as_deref().filter(|_| *pressed).unwrap_or_default();
                return typed.chars().map(Event::ReceivedCharacter).collect();
            }
            AeroEvent::Ime { commit: Some(text), .. } => return text.chars().map(Event::ReceivedCharacter).collect(),
            AeroEvent::PointerMoved { at, .. } => self.mouse.move_to(*at),
            AeroEvent::Button { button, pressed, at, .. } => {
                self.mouse.move_to(*at);
                self.mouse.apply(*button, *pressed, delta);
            }
            AeroEvent::Wheel { delta, unit, .. } => self.mouse.wheel(*delta, *unit),
            AeroEvent::Touch { id, at, phase, .. } => self.touch.apply(*id, *at, *phase, delta),
            AeroEvent::Focus { focused: false, .. } => {
                self.keyboard.down.clear();
                self.mouse.down.clear();
            }
            _ => {}
        }
        Vec::new()
    }

    pub(crate) fn end_frame(&mut self) {
        self.keyboard.pressed.clear();
        self.keyboard.released.clear();
        self.mouse.end_frame();
        self.touch.pressed.clear();
        self.touch.released.clear();
    }
}

pub struct AppWindow {
    size: (u32, u32),
    dpi: f64,
}

impl AppWindow {
    /// In logical pixels.
    pub const fn size(&self) -> (u32, u32) {
        self.size
    }

    pub const fn dpi(&self) -> f64 {
        self.dpi
    }
}

pub struct Timer {
    start: Instant,
    last: Option<Instant>,
    delta: f32,
}

impl Timer {
    fn new() -> Self {
        Self { start: Instant::now(), last: None, delta: 0.0 }
    }

    pub(crate) fn tick(&mut self) {
        let now = Instant::now();
        self.delta = self.last.map_or(0.0, |last| (now - last).as_secs_f32());
        self.last = Some(now);
    }

    /// Seconds since the last frame.
    pub const fn delta_f32(&self) -> f32 {
        self.delta
    }

    /// Seconds since the app started.
    pub fn elapsed_f32(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }
}

#[derive(Default)]
pub struct Keyboard {
    /// Went down this frame.
    pub pressed: HashSet<KeyCode>,
    /// Held, with for how long.
    pub down: HashMap<KeyCode, f32>,
    pub released: HashSet<KeyCode>,
}

impl Keyboard {
    pub fn was_pressed(&self, key: KeyCode) -> bool {
        self.pressed.contains(&key)
    }

    pub fn was_released(&self, key: KeyCode) -> bool {
        self.released.contains(&key)
    }

    pub fn is_down(&self, key: KeyCode) -> bool {
        self.down.contains_key(&key)
    }

    pub fn ctrl(&self) -> bool {
        self.is_down(KeyCode::ControlLeft) || self.is_down(KeyCode::ControlRight)
    }

    pub fn alt(&self) -> bool {
        self.is_down(KeyCode::AltLeft) || self.is_down(KeyCode::AltRight)
    }

    pub fn shift(&self) -> bool {
        self.is_down(KeyCode::ShiftLeft) || self.is_down(KeyCode::ShiftRight)
    }

    pub fn logo(&self) -> bool {
        self.is_down(KeyCode::SuperLeft) || self.is_down(KeyCode::SuperRight)
    }

    /// A key held down repeats without being pressed again.
    fn apply(&mut self, key: KeyCode, pressed: bool, delta: f32) {
        press(&mut self.pressed, &mut self.down, &mut self.released, key, pressed, delta);
    }
}

#[derive(Default)]
pub struct Mouse {
    /// In logical pixels.
    pub x: f32,
    pub y: f32,
    pub pressed: HashSet<MouseButton>,
    pub down: HashMap<MouseButton, f32>,
    pub released: HashSet<MouseButton>,
    /// Kept for the frame it came in.
    pub wheel_delta: Vec2,
    scrolled: bool,
}

impl Mouse {
    pub const fn position(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    pub fn is_down(&self, button: MouseButton) -> bool {
        self.down.contains_key(&button)
    }

    pub fn was_pressed(&self, button: MouseButton) -> bool {
        self.pressed.contains(&button)
    }

    pub fn was_released(&self, button: MouseButton) -> bool {
        self.released.contains(&button)
    }

    pub fn left_is_down(&self) -> bool {
        self.is_down(MouseButton::Left)
    }

    pub fn left_was_pressed(&self) -> bool {
        self.was_pressed(MouseButton::Left)
    }

    pub fn left_was_released(&self) -> bool {
        self.was_released(MouseButton::Left)
    }

    const fn move_to(&mut self, at: Vec2) {
        self.x = at.x;
        self.y = at.y;
    }

    fn apply(&mut self, button: MouseButton, pressed: bool, delta: f32) {
        press(&mut self.pressed, &mut self.down, &mut self.released, button, pressed, delta);
    }

    /// In notan's units: 50 per line, a tenth of the pixels on desktops, the pixels in browsers.
    fn wheel(&mut self, delta: Vec2, unit: WheelUnit) {
        let away_from_zero = |v: f32| if v > 0.0 { (v / 10.0).max(0.1) } else { (v / 10.0).min(-0.1) };
        self.wheel_delta = match unit {
            WheelUnit::Lines => delta * 50.0,
            WheelUnit::Pixels if cfg!(target_arch = "wasm32") => delta,
            WheelUnit::Pixels => Vec2::new(away_from_zero(delta.x), away_from_zero(delta.y)),
        };
        self.scrolled = true;
    }

    fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        if !self.scrolled {
            self.wheel_delta = Vec2::ZERO;
        }
        self.scrolled = false;
    }
}

/// Pressed once when it goes down, then held, as notan counts keys and buttons.
fn press<K: Copy + Eq + std::hash::Hash>(
    pressed: &mut HashSet<K>,
    down: &mut HashMap<K, f32>,
    released: &mut HashSet<K>,
    key: K,
    is_down: bool,
    delta: f32,
) {
    if is_down {
        match down.get_mut(&key) {
            Some(held) => *held += delta,
            None => {
                down.insert(key, 0.0);
                pressed.insert(key);
            }
        }
    } else {
        down.remove(&key);
        pressed.remove(&key);
        released.insert(key);
    }
}

/// Fingers by slot, the lowest free one first, as notan numbers them.
#[derive(Default)]
pub struct Touch {
    pub pressed: HashSet<u8>,
    pub down: HashMap<u8, f32>,
    pub released: HashSet<u8>,
    slots: [Option<(u64, Vec2)>; SLOTS],
}

const SLOTS: usize = 20;

impl Touch {
    /// In logical pixels.
    pub fn position(&self, slot: u8) -> Option<(f32, f32)> {
        self.slots.get(usize::from(slot))?.map(|(_, at)| (at.x, at.y))
    }

    pub fn was_pressed(&self, slot: u8) -> bool {
        self.pressed.contains(&slot)
    }

    pub fn was_released(&self, slot: u8) -> bool {
        self.released.contains(&slot)
    }

    pub fn down(&self, slot: u8) -> bool {
        self.down.contains_key(&slot)
    }

    fn slot_of(&self, finger: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.is_some_and(|(id, _)| id == finger))
    }

    fn apply(&mut self, finger: u64, at: Vec2, phase: TouchPhase, delta: f32) {
        match phase {
            TouchPhase::Start => {
                if let Some(slot) = self.slots.iter().position(Option::is_none) {
                    self.slots[slot] = Some((finger, at));
                    self.pressed.insert(slot as u8);
                    self.down.insert(slot as u8, 0.0);
                }
            }
            TouchPhase::Move => {
                if let Some(slot) = self.slot_of(finger) {
                    self.slots[slot] = Some((finger, at));
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                if let Some(slot) = self.slot_of(finger) {
                    self.slots[slot] = None;
                    self.down.remove(&(slot as u8));
                    self.pressed.remove(&(slot as u8));
                    self.released.insert(slot as u8);
                }
            }
        }
        self.down.values_mut().for_each(|held| *held += delta);
    }
}

#[cfg(test)]
mod tests {
    use aero::input::{Event as AeroEvent, Modifiers, MouseButton, TouchPhase, WheelUnit};
    use aero::math::Vec2;
    use aero::WindowId;

    use super::{App, Event, KeyCode};

    fn key(code: KeyCode, pressed: bool, text: Option<&str>) -> AeroEvent {
        AeroEvent::Key {
            window: WindowId::MAIN,
            code,
            key: None,
            pressed,
            repeat: false,
            text: text.map(String::from),
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn keys_are_pressed_once_held_then_released() {
        let mut app = App::new();
        assert_eq!(app.apply(&key(KeyCode::KeyA, true, Some("a"))), [Event::ReceivedCharacter('a')]);
        assert!(app.keyboard.was_pressed(KeyCode::KeyA) && app.keyboard.is_down(KeyCode::KeyA));
        app.end_frame();
        assert_eq!(app.apply(&key(KeyCode::KeyA, true, Some("a"))), [Event::ReceivedCharacter('a')], "a repeat types");
        assert!(!app.keyboard.was_pressed(KeyCode::KeyA), "but is no new press");
        app.apply(&key(KeyCode::KeyA, false, None));
        assert!(app.keyboard.was_released(KeyCode::KeyA) && !app.keyboard.is_down(KeyCode::KeyA));
    }

    #[test]
    fn buttons_wheel_and_fingers() {
        let mut app = App::new();
        let at = Vec2::new(12.0, 34.0);
        let modifiers = Modifiers::default();
        app.apply(&AeroEvent::Button {
            window: WindowId::MAIN,
            button: MouseButton::Left,
            pressed: true,
            at,
            modifiers,
        });
        assert!(app.mouse.left_was_pressed() && app.mouse.left_is_down());
        assert_eq!(app.mouse.position(), (12.0, 34.0));
        app.apply(&AeroEvent::Wheel { window: WindowId::MAIN, delta: Vec2::new(0.0, -1.0), unit: WheelUnit::Lines });
        assert_eq!(app.mouse.wheel_delta.y, -50.0);
        app.end_frame();
        assert!(!app.mouse.left_was_pressed() && app.mouse.left_is_down());
        assert_eq!(app.mouse.wheel_delta.y, -50.0, "the wheel lasts the frame it came in");
        app.end_frame();
        assert_eq!(app.mouse.wheel_delta.y, 0.0);

        let touch = |id, phase| AeroEvent::Touch { window: WindowId::MAIN, id, at, phase };
        app.apply(&touch(7, TouchPhase::Start));
        app.apply(&touch(9, TouchPhase::Start));
        assert!(app.touch.was_pressed(0) && app.touch.was_pressed(1));
        app.apply(&touch(7, TouchPhase::End));
        assert!(app.touch.was_released(0) && app.touch.position(0).is_none());
        app.apply(&touch(11, TouchPhase::Start));
        assert_eq!(app.touch.position(0), Some((12.0, 34.0)), "a new finger takes the lowest free slot");
    }
}
