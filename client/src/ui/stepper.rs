use aero::draw::Draw;
use aero::math::vec2;
use aero::theme::Visual;

use super::{Face, Fonts, Rect};
use crate::theme::{self, Palette};

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

/// A yes or no as a switch across the band under a tile's name, with what it says.
pub(super) fn paint_switch(draw: &mut Draw, fonts: &mut Fonts, pal: &Palette, band: Rect, on: bool, visual: Visual) {
    let track = Rect::at(band.x, band.y + (band.h - SWITCH_H) / 2.0, SWITCH_W, SWITCH_H);
    let fill = match (visual.enabled, on) {
        (false, _) => pal.disabled,
        (true, true) => pal.accent,
        (true, false) => theme::mix(pal.raised, pal.raised_hover, visual.hover),
    };
    draw.rect(track).radius(SWITCH_H / 2.0).color(fill);
    draw.rect(track).radius(SWITCH_H / 2.0).stroke(theme::mix(pal.border, pal.accent, visual.hover), 2.0);
    let knob = SWITCH_H / 2.0 - 4.0;
    let knob_x = if on { track.x + track.w - SWITCH_H / 2.0 } else { track.x + SWITCH_H / 2.0 };
    let ink = if visual.enabled { pal.text } else { pal.text_disabled };
    draw.circle(vec2(knob_x, track.y + track.h / 2.0), knob).color(ink);
    fonts
        .write(draw, Face::Display, if on { "Oui" } else { "Non" })
        .left(track.x + track.w + 14.0, track.y + track.h / 2.0)
        .size(theme::size::EMPHASIS)
        .color(theme::GOLD);
}

/// A stepper's label before it and its value between its buttons.
pub(super) fn paint_labelled(
    draw: &mut Draw,
    fonts: &mut Fonts,
    pal: &Palette,
    s: Stepper,
    (label, value): (&str, &str),
) {
    fonts.write(draw, Face::Text, label).right(s.label_x(), s.mid_y()).size(theme::size::EMPHASIS).color(pal.text_dim);
    fonts
        .write(draw, Face::Display, value)
        .center(s.value_x(), s.mid_y())
        .size(theme::size::EMPHASIS)
        .color(theme::GOLD);
}

/// A settings tile's name, above its controls.
pub(super) fn tile_label(draw: &mut Draw, fonts: &mut Fonts, pal: &Palette, tile: Rect, label: &str) {
    fonts
        .write(draw, Face::Text, label)
        .left(tile.x, tile.y + TILE_LABEL_H / 2.0 - 2.0)
        .size(theme::size::BODY)
        .color(pal.text_dim);
}

/// The value between a stepper's buttons, narrowed to fit between them.
pub(super) fn paint_value(draw: &mut Draw, fonts: &mut Fonts, s: Stepper, value: &str) {
    let room = s.plus.x - (s.minus.x + s.minus.w) - 12.0;
    let full = theme::size::EMPHASIS;
    let size = full.min(full * room / fonts.width(Face::Display, value, full));
    fonts.write(draw, Face::Display, value).center(s.value_x(), s.mid_y()).size(size).color(theme::GOLD);
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
