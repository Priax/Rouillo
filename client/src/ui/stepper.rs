use notan::draw::{Draw, DrawShapes};

use super::{Face, Fonts, Icon, Rect, SharpText, Ui};
use crate::theme;

const BUTTON: f32 = 50.0;
const LABEL_GAP: f32 = 40.0;
const VALUE_WIDTH: f32 = 90.0;
const TILE_LABEL_H: f32 = 28.0;
const TILE_BUTTON: f32 = 40.0;
const SWITCH_W: f32 = 60.0;
const SWITCH_H: f32 = 32.0;

/// A labelled value with a button on each side to step it, laid out from the
/// point where the label ends and the controls begin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stepper {
    pub minus: Rect,
    pub plus: Rect,
}

impl Stepper {
    pub fn at(x: f32, y: f32) -> Self {
        let minus = Rect::at(x + LABEL_GAP, y, BUTTON, BUTTON);
        let plus = Rect::at(minus.x + BUTTON + VALUE_WIDTH, y, BUTTON, BUTTON);
        Self { minus, plus }
    }

    /// The controls of a settings tile: under its name, a button at each end.
    pub fn in_tile(tile: Rect) -> Self {
        Self::across(Self::tile_band(tile))
    }

    /// A square button at each end of `band`.
    pub fn across(band: Rect) -> Self {
        Self {
            minus: Rect::at(band.x, band.y, band.h, band.h),
            plus: Rect::at(band.x + band.w - band.h, band.y, band.h, band.h),
        }
    }

    /// The whole band under a tile's name, which a switch answers to.
    pub fn tile_band(tile: Rect) -> Rect {
        Rect::at(tile.x, tile.y + TILE_LABEL_H, tile.w, TILE_BUTTON)
    }

    fn label_x(self) -> f32 {
        self.minus.x - LABEL_GAP
    }

    fn value_x(self) -> f32 {
        (self.minus.x + self.minus.w + self.plus.x) / 2.0
    }

    fn mid_y(self) -> f32 {
        self.minus.y + self.minus.h / 2.0
    }
}

impl Ui {
    /// Draws the stepper; its buttons only when `editable`.
    pub fn stepper(&self, draw: &mut Draw, fonts: &Fonts, s: Stepper, label: &str, value: &str, editable: bool) {
        let pal = self.palette();
        draw.sharp_text(&fonts.text, label)
            .position(s.label_x(), s.mid_y())
            .size(theme::size::EMPHASIS)
            .h_align_right()
            .v_align_middle()
            .color(pal.text_dim);
        draw.sharp_text(&fonts.display, value)
            .position(s.value_x(), s.mid_y())
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(theme::GOLD);
        if editable {
            self.button(draw, fonts, s.minus, Icon::Minus);
            self.button(draw, fonts, s.plus, Icon::Plus);
        }
    }

    fn tile_label(&self, draw: &mut Draw, fonts: &Fonts, tile: Rect, label: &str) {
        draw.sharp_text(&fonts.text, label)
            .position(tile.x, tile.y + TILE_LABEL_H / 2.0 - 2.0)
            .size(theme::size::BODY)
            .v_align_middle()
            .color(self.palette().text_dim);
    }

    /// A settings tile: its name, then its value between the buttons that
    /// step it, the buttons only when `editable`.
    pub fn stepper_tile(&self, draw: &mut Draw, fonts: &Fonts, tile: Rect, label: &str, value: &str, editable: bool) {
        self.tile_label(draw, fonts, tile, label);
        self.stepper_band(draw, fonts, Stepper::tile_band(tile), value, editable);
    }

    /// A value between the buttons that step it, across `band`.
    pub fn stepper_band(&self, draw: &mut Draw, fonts: &Fonts, band: Rect, value: &str, editable: bool) {
        let s = Stepper::across(band);
        let room = s.plus.x - (s.minus.x + s.minus.w) - 12.0;
        let full = theme::size::EMPHASIS;
        let size = full.min(full * room / fonts.width(Face::Display, value, full));
        draw.sharp_text(&fonts.display, value)
            .position(s.value_x(), s.mid_y())
            .size(size)
            .h_align_center()
            .v_align_middle()
            .color(theme::GOLD);
        if editable {
            self.button(draw, fonts, s.minus, Icon::Minus);
            self.button(draw, fonts, s.plus, Icon::Plus);
        }
    }

    /// A settings tile whose value is a button over `Stepper::tile_band`,
    /// for a setting changed somewhere else.
    pub fn button_tile(&self, draw: &mut Draw, fonts: &Fonts, tile: Rect, label: &str, value: &str) {
        self.tile_label(draw, fonts, tile, label);
        self.button(draw, fonts, Stepper::tile_band(tile), value);
    }

    /// A settings tile for a yes or no: its name, then a switch and what it
    /// says. Clicks on `Stepper::tile_band` flip it, when `editable`.
    pub fn switch_tile(&self, draw: &mut Draw, fonts: &Fonts, tile: Rect, label: &str, on: bool, editable: bool) {
        let pal = self.palette();
        self.tile_label(draw, fonts, tile, label);
        let band = Stepper::tile_band(tile);
        let r = self.interact(label, band, editable);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let track = Rect::at(band.x, band.y + (band.h - SWITCH_H) / 2.0, SWITCH_W, SWITCH_H);
        let fill = match (editable, on) {
            (false, _) => pal.disabled,
            (true, true) => pal.accent,
            (true, false) => theme::mix(pal.raised, pal.raised_hover, r.hover),
        };
        draw.rect((track.x, track.y), (track.w, track.h))
            .corner_radius(SWITCH_H / 2.0)
            .color(fill);
        draw.rect((track.x, track.y), (track.w, track.h))
            .corner_radius(SWITCH_H / 2.0)
            .stroke(2.0)
            .color(theme::mix(pal.border, pal.accent, r.hover));
        let knob = SWITCH_H / 2.0 - 4.0;
        let knob_x = if on {
            track.x + track.w - SWITCH_H / 2.0
        } else {
            track.x + SWITCH_H / 2.0
        };
        draw.circle(knob)
            .position(knob_x, track.y + track.h / 2.0)
            .color(if editable { pal.text } else { pal.text_disabled });
        draw.sharp_text(&fonts.display, if on { "Oui" } else { "Non" })
            .position(track.x + track.w + 14.0, track.y + track.h / 2.0)
            .size(theme::size::EMPHASIS)
            .v_align_middle()
            .color(theme::GOLD);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_sits_midway_between_the_buttons() {
        let s = Stepper::at(100.0, 0.0);
        let left_gap = s.value_x() - (s.minus.x + s.minus.w);
        let right_gap = s.plus.x - s.value_x();
        assert!((left_gap - right_gap).abs() < 1e-3);
        assert!(s.label_x() < s.minus.x);
    }
}
