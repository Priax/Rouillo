use aero::input::{Input, KeyCode};

/// What a frame brings the screens.
pub struct Tick<'a> {
    pub input: &'a Input,
    /// Seconds since the previous frame.
    pub dt: f32,
    back: bool,
    /// A text field has the keyboard: the keys type, they are no shortcuts.
    typing: bool,
}

impl<'a> Tick<'a> {
    /// `back` is a step back asked elsewhere than by Escape: a gamepad, the browser.
    pub fn new(input: &'a Input, dt: f32, (back, typing): (bool, bool)) -> Self {
        Self { input, dt, back: back || input.was_pressed(KeyCode::Escape), typing }
    }

    /// Whether `key` went down this frame, unless a text field took it.
    pub fn pressed(&self, key: KeyCode) -> bool {
        !self.typing && self.input.was_pressed(key)
    }

    pub const fn typing(&self) -> bool {
        self.typing
    }

    /// Escape, or what stands for it.
    pub const fn back(&self) -> bool {
        self.back
    }
}
