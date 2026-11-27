mod bar;
mod button;
mod controls;
mod deco;
mod fonts;
mod layout;
mod look;
mod modal;
mod pager;
mod panel;
mod screen;
mod status;
mod stepper;
mod text;
mod triangles;
mod view;
pub mod widgets;

pub use aero::geom::Rect;
use aero::path::{Cap, Join, Stroke};
pub use button::{Icon, Label};
pub use deco::{banner, card, divider, list_row, pills_ending_at, portrait, Edge, Persona, Pill};
pub use fonts::{faces, register as register_fonts, Face, Fonts, Typeset};
pub use layout::{px, Frame, ListCard, Size, FILL};
pub use look::Look;
pub use modal::Modal;
pub use pager::{page_count, Pager, PAGER_H};
pub use panel::{action_row, card_body, card_height, panel_card, SettingsGrid, SettingsPanel, TILE_H};
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

use aero::Cx;

use crate::theme::{hue, Palette};

const MAX_DT: f32 = 0.1;
pub const TRANSITION: f32 = 0.3;

const DOT_STEP: f64 = 0.5;

/// What screens draw with: the widgets, the fonts, and the pictures
/// fetched from the server.
pub struct Gui {
    pub ui: Ui,
    pub images: crate::images::Images,
}

impl Gui {
    pub fn new() -> Self {
        Self { ui: Ui::default(), images: crate::images::Images::default() }
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
    /// Whether the gamepad steers: the mouse moving hands control back to it.
    pad: bool,
    pointer: Option<aero::math::Vec2>,
    /// Seconds of the real clock: the caret blinks on it, frames or not.
    clock: f64,
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
            pad: false,
            pointer: None,
            clock: 0.0,
        }
    }
}

impl Ui {
    /// Called at the start of each frame, before the screen reacts to input.
    pub fn begin_frame(&mut self, dt: f32, view: View) {
        self.dt = dt.clamp(0.0, MAX_DT);
        self.time += f64::from(self.dt);
        self.view = view;
        let turn = (self.target_hue - self.from_hue + 540.0).rem_euclid(360.0) - 180.0;
        let progress = ease_out(transition_progress(self.time, self.entered_at, self.fade));
        self.hue = (self.from_hue + turn * progress).rem_euclid(360.0);
        self.palette = Palette::new(self.hue);
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
    pub const fn look(&self) -> Look {
        Look { palette: self.palette }
    }

    /// Whether a screen is still coming in.
    pub fn transitioning(&self) -> bool {
        self.transition() < 1.0
    }

    pub fn waiting_dots(&self, cx: &mut Cx) -> String {
        redraw_after(cx, DOT_STEP - self.clock.rem_euclid(DOT_STEP));
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

    /// Leaves gamepad control as soon as the mouse moves.
    pub fn note_pointer(&mut self, at: Option<aero::math::Vec2>) {
        if at != self.pointer {
            self.pointer = at;
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

    /// The real clock, in seconds.
    pub fn set_clock(&mut self, now: f64) {
        self.clock = now;
    }

    /// A clock shown in whole seconds of the real clock: the next frame comes when it ticks.
    pub fn next_second(&self, cx: &mut Cx) {
        redraw_after(cx, 1.0 - self.clock.rem_euclid(1.0));
    }
}

/// Something shown changes in `delay` seconds without moving until then.
pub fn redraw_after(cx: &mut Cx, delay: f64) {
    cx.request_redraw_in(std::time::Duration::from_secs_f64(delay.max(0.0)));
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

    #[test]
    fn a_new_screen_comes_in_then_settles() {
        let mut ui = Ui::default();
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default());
        ui.set_screen(0, hue::GREEN, TRANSITION);
        frame(&mut ui);
        assert!(!ui.transitioning(), "the first screen is there at once");
        ui.set_screen(1, hue::PURPLE, TRANSITION);
        assert!(ui.transitioning(), "a new screen slides in");
        for _ in 0..60 {
            frame(&mut ui);
        }
        assert!(!ui.transitioning(), "and settles");
    }

    #[test]
    fn the_first_screen_shows_up_at_once() {
        let mut ui = Ui::default();
        ui.set_screen(0, hue::GREEN, TRANSITION);
        ui.begin_frame(1.0 / 60.0, View::default());
        assert!((ui.transition() - 1.0).abs() < f32::EPSILON);
        assert!((ui.hue - hue::GREEN).abs() < f32::EPSILON);
    }

    #[test]
    fn a_new_screen_brings_its_colours_the_short_way_round() {
        let mut ui = Ui::default();
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default());
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
        let frame = |ui: &mut Ui| ui.begin_frame(1.0 / 60.0, View::default());
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
}
