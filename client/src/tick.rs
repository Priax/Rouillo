use aero::input::{Input, KeyCode};

/// What a frame brings the screens.
pub struct Tick<'a> {
    pub input: &'a Input,
    /// Seconds since the previous frame.
    pub dt: f32,
    back: bool,
}

impl<'a> Tick<'a> {
    /// `back` is a step back asked elsewhere than by Escape: a gamepad, the browser.
    pub fn new(input: &'a Input, dt: f32, back: bool) -> Self {
        Self { input, dt, back: back || input.was_pressed(KeyCode::Escape) }
    }

    pub fn pressed(&self, key: KeyCode) -> bool {
        self.input.was_pressed(key)
    }

    /// Escape, or what stands for it.
    pub const fn back(&self) -> bool {
        self.back
    }
}
