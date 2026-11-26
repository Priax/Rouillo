use aero::input::{Input, MouseButton};
mod bar;
mod button;
mod controls;
mod deco;
mod field;
mod fonts;
mod input;
mod keys;
mod layout;
mod look;
mod modal;
mod pager;
mod panel;
mod rect;
mod screen;
mod status;
mod stepper;
mod text;
mod triangles;
mod view;
pub mod widgets;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use aero::path::{Cap, Join, Stroke};
pub use button::{Icon, Label};
pub use deco::{banner, card, divider, list_row, pills_ending_at, portrait, Edge, Persona, Pill};
mod clipboard;
pub use clipboard::get as clipboard_text;
pub use field::{area_clicked, area_height, area_keys, field_clicked, text_area, text_field, Field};
pub use fonts::{register as register_fonts, Face, Faces, Fonts};
pub use input::TextInput;
pub use keys::EditKeys;
#[cfg(target_arch = "wasm32")]
pub use keys::Shortcuts;
pub use layout::{Frame, ListCard, Size, FILL};
pub use look::Look;
pub use modal::Modal;
pub use pager::{page_count, Pager, PAGER_H};
pub use panel::{action_row, card_body, card_height, panel_card, SettingsGrid, SettingsPanel, TILE_H};
pub use rect::Rect;
pub use status::Status;
pub use stepper::Stepper;
pub use triangles::Triangles;
pub use view::View;

/// A stroke with flat ends and pointed corners, bevelled where a corner would point past 10 half widths.
pub fn pointed(width: f32) -> Stroke {
    Stroke::new(width).cap(Cap::Butt).join(Join::Miter(10.0))
}

/// The option `dir` steps away from `at`, going round.
pub fn cycle<T: Copy + PartialEq>(options: &[T], at: T, dir: i32) -> T {
    let i = options.iter().position(|&o| o == at).unwrap_or(0) as i32;
    options[(i + dir.signum()).rem_euclid(options.len() as i32) as usize]
}

use crate::theme::{hue, Palette};

const MAX_DT: f32 = 0.1;
pub const TRANSITION: f32 = 0.3;

const CARET_BLINK: f64 = 0.5;
const DOT_STEP: f64 = 0.5;

/// What screens draw with: the widgets, the fonts, and the pictures
/// fetched from the server.
pub struct Gui {
    pub ui: Ui,
    pub fonts: Fonts,
    pub images: crate::images::Images,
}

impl Gui {
    pub fn new(fonts: Fonts) -> Self {
        Self { ui: Ui::default(), fonts, images: crate::images::Images::default() }
    }
}

pub struct Ui {
    dt: f32,
    time: f64,
    view: View,
    hue: f32,
    from_hue: f32,
    target_hue: f32,
    screen: Option<usize>,
    entered_at: f64,
    fade: f32,
    palette: Palette,
    mouse: Mouse,
    caret_text: Option<u64>,
    caret_since: f64,
    /// Whether the gamepad steers: the mouse moving hands control back to it.
    pad: bool,
    pointer: (f32, f32),
    /// Something shown moves on its own: the next frame is wanted.
    moving: bool,
    /// Seconds of the real clock: the caret blinks on it, frames or not.
    clock: f64,
    /// In how many seconds something shown changes, such as the caret.
    change_in: Option<f64>,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            dt: 0.0,
            time: 0.0,
            view: View::default(),
            hue: hue::PURPLE,
            from_hue: hue::PURPLE,
            target_hue: hue::PURPLE,
            screen: None,
            entered_at: f64::NEG_INFINITY,
            fade: TRANSITION,
            palette: Palette::new(hue::PURPLE),
            mouse: Mouse::default(),
            caret_text: None,
            caret_since: 0.0,
            pad: false,
            pointer: (0.0, 0.0),
            moving: false,
            clock: 0.0,
            change_in: None,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Mouse {
    pub x: f32,
    pub y: f32,
    pub down: bool,
    pub pressed: bool,
}

impl Mouse {
    /// A finger is the left button: browsers report touches only as such.
    pub fn of(input: &Input, view: View) -> Self {
        let finger = input.fingers().first();
        let at = finger.map(|f| f.at).or_else(|| input.pointer()).unwrap_or_default() / view.scale;
        Self {
            x: at.x,
            y: at.y,
            down: input.is_button_down(MouseButton::Left) || finger.is_some_and(|f| !f.lifted),
            pressed: input.was_clicked(MouseButton::Left) || finger.is_some_and(|f| f.new),
        }
    }
}

impl Ui {
    /// Called at the start of each frame, before the screen reacts to input.
    pub fn begin_frame(&mut self, dt: f32, view: View, mouse: Mouse) {
        self.dt = dt.clamp(0.0, MAX_DT);
        self.time += f64::from(self.dt);
        self.view = view;
        let turn = (self.target_hue - self.from_hue + 540.0).rem_euclid(360.0) - 180.0;
        let progress = ease_out(transition_progress(self.time, self.entered_at, self.fade));
        self.moving |= progress < 1.0;
        self.hue = (self.from_hue + turn * progress).rem_euclid(360.0);
        self.palette = Palette::new(self.hue);
        self.mouse = mouse;
    }

    /// Tells which screen is shown, and its section's hue. A change of
    /// screen starts a transition of `fade` seconds: the new one fades and
    /// slides in while the palette moves to its hue, both along the same
    /// curve. The first screen shows up at once.
    pub fn set_screen(&mut self, screen: usize, hue: f32, fade: f32) {
        match self.screen {
            Some(current) if current == screen => {}
            Some(_) => {
                self.screen = Some(screen);
                self.from_hue = self.hue;
                self.target_hue = hue;
                self.entered_at = self.time;
                self.fade = fade;
            }
            None => {
                self.screen = Some(screen);
                self.hue = hue;
                self.from_hue = hue;
                self.target_hue = hue;
                self.palette = Palette::new(hue);
            }
        }
    }

    /// How far the current screen is into its entrance, eased, in `0..=1`.
    fn transition(&self) -> f32 {
        ease_out(transition_progress(self.time, self.entered_at, self.fade))
    }

    /// The look of the game's widgets, in this screen's colors.
    pub fn look(&self, fonts: &Fonts) -> Look {
        Look { faces: fonts.faces(), palette: self.palette }
    }

    pub fn keep_moving(&mut self) {
        self.moving = true;
    }

    /// Whether this frame showed anything that moves, and forgets it for the next.
    pub fn take_moving(&mut self) -> bool {
        std::mem::take(&mut self.moving)
    }

    pub fn waiting_dots(&mut self) -> String {
        self.change_after(DOT_STEP - self.clock.rem_euclid(DOT_STEP));
        ".".repeat(1 + (self.clock / DOT_STEP) as usize % 3)
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn palette(&self) -> Palette {
        self.palette
    }

    pub fn view(&self) -> View {
        self.view
    }

    /// Keeps this frame's click from what lies under the widget that took it.
    pub fn swallow_click(&mut self) {
        self.mouse.pressed = false;
    }

    pub fn clicked(&self, r: Rect) -> bool {
        self.click_in(r).is_some()
    }

    pub fn click_in(&self, r: Rect) -> Option<(f32, f32)> {
        let m = self.mouse;
        (m.pressed && r.contains(m.x, m.y)).then_some((m.x, m.y))
    }

    /// Leaves gamepad control as soon as the mouse moves.
    pub fn note_pointer(&mut self, real: Mouse) {
        if (real.x, real.y) != self.pointer {
            self.pointer = (real.x, real.y);
            self.pad = false;
        }
    }

    /// The gamepad steers until the mouse moves.
    pub fn use_pad(&mut self) {
        self.pad = true;
    }

    pub fn pad_active(&self) -> bool {
        self.pad
    }

    pub fn mouse(&self) -> Mouse {
        self.mouse
    }

    /// Whether the caret of the focused field, standing at `at` in `text`,
    /// is lit this frame. It blinks, and starts over lit whenever the text
    /// changes or the caret moves, so that it never vanishes under the typing.
    pub fn caret_on(&mut self, text: &str, at: usize) -> bool {
        let text = Some(hash_of(&(text, at)));
        if self.caret_text != text {
            self.caret_text = text;
            self.caret_since = self.clock;
        }
        let phase = (self.clock - self.caret_since) % CARET_BLINK;
        self.change_after(CARET_BLINK - phase);
        (self.clock - self.caret_since) % (2.0 * CARET_BLINK) < CARET_BLINK
    }

    /// The real clock, in seconds.
    pub fn set_clock(&mut self, now: f64) {
        self.clock = now;
    }

    /// Something shown changes in `delay` seconds without moving until then.
    pub fn change_after(&mut self, delay: f64) {
        self.change_in = Some(self.change_in.map_or(delay, |current| current.min(delay)));
    }

    /// A clock shown in whole seconds of the real clock: the next frame comes when it ticks.
    pub fn next_second(&mut self) {
        self.change_after(1.0 - self.clock.rem_euclid(1.0));
    }

    /// In how many seconds something shown next changes on its own, and forgets it for the next frame.
    pub fn take_change_in(&mut self) -> Option<f64> {
        self.change_in.take()
    }
}

fn hash_of<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn transition_progress(time: f64, entered_at: f64, fade: f32) -> f32 {
    ((time - entered_at) / f64::from(fade)).clamp(0.0, 1.0) as f32
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AWAY: Mouse = Mouse { x: 500.0, y: 500.0, down: false, pressed: false };

    #[test]
    fn frames_are_wanted_only_while_something_moves() {
        let mut ui = Ui::default();
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
        ui.set_screen(0, hue::GREEN, TRANSITION);
        frame(&mut ui);
        assert!(!ui.take_moving(), "a still screen wants no frame");
        ui.set_screen(1, hue::PURPLE, TRANSITION);
        frame(&mut ui);
        assert!(ui.take_moving(), "a new screen slides in");
        for _ in 0..60 {
            frame(&mut ui);
            ui.take_moving();
        }
        frame(&mut ui);
        assert!(!ui.take_moving(), "and settles");
    }

    #[test]
    fn the_first_screen_shows_up_at_once() {
        let mut ui = Ui::default();
        ui.set_screen(0, hue::GREEN, TRANSITION);
        ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
        assert!((ui.transition() - 1.0).abs() < f32::EPSILON);
        assert!((ui.hue - hue::GREEN).abs() < f32::EPSILON);
    }

    #[test]
    fn a_new_screen_brings_its_colours_the_short_way_round() {
        let mut ui = Ui::default();
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
        ui.set_screen(0, hue::PURPLE, TRANSITION);
        frame(&mut ui);
        ui.set_screen(1, hue::PINK, TRANSITION);
        assert!(ui.transition() < f32::EPSILON, "the new screen starts hidden");
        frame(&mut ui);
        let h = ui.hue;
        assert!(h > hue::PURPLE && h < hue::PINK, "from 255 towards 333, got {h}");
        for _ in 0..30 {
            frame(&mut ui);
        }
        assert!((ui.transition() - 1.0).abs() < f32::EPSILON, "done in {TRANSITION}s");
        assert!((ui.hue - hue::PINK).abs() < 1e-3);
        ui.set_screen(2, hue::ORANGE, TRANSITION);
        frame(&mut ui);
        let h = ui.hue;
        assert!(h > hue::PINK || h < hue::ORANGE, "333 to 45 crosses 360, got {h}");
        ui.set_screen(2, hue::ORANGE, TRANSITION);
        assert!(ui.transition() > 0.0, "staying on a screen does not restart it");
    }

    #[test]
    fn a_transition_lasts_its_own_fade() {
        let mut ui = Ui::default();
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default(), AWAY);
        ui.set_screen(0, hue::PURPLE, TRANSITION);
        frame(&mut ui);
        ui.set_screen(1, hue::PURPLE, 1.2);
        for _ in 0..30 {
            frame(&mut ui);
        }
        assert!(ui.transition() < 1.0, "still fading after {TRANSITION}s");
        for _ in 0..45 {
            frame(&mut ui);
        }
        assert!((ui.transition() - 1.0).abs() < f32::EPSILON, "done in 1.2s");
    }

    #[test]
    fn the_caret_blinks_and_typing_lights_it_again() {
        let mut ui = Ui::default();
        let at = |ui: &mut Ui, frames: u32, text: &str, at: usize| {
            ui.set_clock(ui.clock + f64::from(frames) / 60.0);
            ui.caret_on(text, at)
        };
        let after = |ui: &mut Ui, frames: u32, text: &str| at(ui, frames, text, text.len());
        assert!(after(&mut ui, 1, "a"));
        assert!(after(&mut ui, 15, "a"), "lit for half a second");
        assert!(!after(&mut ui, 21, "a"), "then off");
        assert!(after(&mut ui, 1, "ab"), "a keystroke must not type blind");
        assert!(!after(&mut ui, 36, "ab"));
        assert!(after(&mut ui, 30, "ab"), "and back on");
        assert!(!after(&mut ui, 36, "ab"));
        assert!(at(&mut ui, 1, "ab", 1), "nor may it move unseen");
    }
}
