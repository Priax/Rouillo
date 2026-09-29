use notan::draw::{Draw, DrawTextSection};

use super::{Fonts, Icon, Rect, Ui};
use crate::theme;

const BUTTON: f32 = 50.0;
const LABEL_GAP: f32 = 40.0;
const VALUE_WIDTH: f32 = 90.0;

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
        draw.text(&fonts.text, label)
            .position(s.label_x(), s.mid_y())
            .size(theme::size::EMPHASIS)
            .h_align_right()
            .v_align_middle()
            .color(theme::TEXT_DIM);
        draw.text(&fonts.display, value)
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
