use notan::draw::Draw;

use super::{card, divider, Fonts, Rect, SharpText, Stepper, View};
use crate::theme::{self, Palette};

const TITLE_H: f32 = 56.0;
const ROW_H: f32 = 64.0;
const ACTION_ROW_H: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsPanel {
    pub card: Rect,
    view_w: f32,
}

impl SettingsPanel {
    pub fn new(view: View, rows: usize) -> Self {
        let h = TITLE_H + rows as f32 * ROW_H + 16.0;
        Self {
            card: Rect::at(view.w / 2.0 - 320.0, theme::HEADER_H + 30.0, 640.0, h),
            view_w: view.w,
        }
    }

    pub fn stepper(self, i: usize) -> Stepper {
        Stepper::at(
            self.card.x + self.card.w / 2.0 + 20.0,
            self.card.y + TITLE_H + 8.0 + i as f32 * ROW_H,
        )
    }

    pub fn action_row(self, k: usize) -> Rect {
        Rect::at(
            0.0,
            self.card.y + self.card.h + 30.0 + k as f32 * ACTION_ROW_H,
            self.view_w,
            ACTION_ROW_H,
        )
    }

    pub fn draw(self, draw: &mut Draw, pal: &Palette, fonts: &Fonts, title: &str) {
        let card_rect = self.card;
        card(draw, pal, card_rect);
        draw.sharp_text(&fonts.display, title)
            .position(card_rect.x + 20.0, card_rect.y + TITLE_H / 2.0)
            .size(theme::size::EMPHASIS)
            .v_align_middle()
            .color(pal.text_dim);
        divider(draw, pal, card_rect.x + 20.0, card_rect.y + TITLE_H, card_rect.w - 40.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_stay_inside_the_card_and_actions_come_under_it() {
        let panel = SettingsPanel::new(View::default(), 3);
        let last = panel.stepper(2);
        assert!(last.minus.y + last.minus.h <= panel.card.y + panel.card.h);
        assert!(panel.action_row(0).y > panel.card.y + panel.card.h);
        assert_eq!(panel.action_row(1).y, panel.action_row(0).y + ACTION_ROW_H);
    }
}
