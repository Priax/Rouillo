use notan::draw::Draw;

use super::{card, divider, Fonts, Frame, Rect, SharpText, Stepper, View};
use crate::theme::{self, Palette};

const TITLE_H: f32 = 56.0;
const ROW_H: f32 = 64.0;
/// Past four rows the panel tightens, so the actions under it stay on screen.
const TIGHT_ROW_H: f32 = 56.0;
const TIGHTER_ROW_H: f32 = 52.0;
const ACTION_ROW_H: f32 = 60.0;
const SECTION_H: f32 = 34.0;
const TILE_H: f32 = 78.0;
const CARD_PAD: f32 = 20.0;
const COLUMN_GAP: f32 = 28.0;
const MAX_CARD_W: f32 = 640.0;

fn action_row(card: Rect, view_w: f32, k: usize) -> Rect {
    Rect::at(0.0, card.bottom() + 30.0, view_w, 0.0).nth_row(k, ACTION_ROW_H, 0.0)
}

fn draw_card(draw: &mut Draw, pal: &Palette, fonts: &Fonts, card_rect: Rect, title: &str) {
    card(draw, pal, card_rect);
    draw.sharp_text(&fonts.display, title)
        .position(card_rect.x + CARD_PAD, card_rect.y + TITLE_H / 2.0)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(pal.text_dim);
    divider(
        draw,
        pal,
        card_rect.x + CARD_PAD,
        card_rect.y + TITLE_H,
        card_rect.w - 2.0 * CARD_PAD,
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsPanel {
    pub card: Rect,
    view_w: f32,
    row_h: f32,
}

impl SettingsPanel {
    pub fn new(view: View, rows: usize) -> Self {
        let row_h = match rows {
            0..=4 => ROW_H,
            5 | 6 => TIGHT_ROW_H,
            _ => TIGHTER_ROW_H,
        };
        let h = TITLE_H + rows as f32 * row_h + 16.0;
        Self {
            card: Frame::of(view).body.top(h).max_width(MAX_CARD_W),
            view_w: view.w,
            row_h,
        }
    }

    pub fn stepper(self, i: usize) -> Stepper {
        Stepper::at(
            self.card.x + self.card.w / 2.0 + 20.0,
            self.card.y + TITLE_H + 8.0 + i as f32 * self.row_h,
        )
    }

    pub fn action_row(self, k: usize) -> Rect {
        action_row(self.card, self.view_w, k)
    }

    pub fn draw(self, draw: &mut Draw, pal: &Palette, fonts: &Fonts, title: &str) {
        draw_card(draw, pal, fonts, self.card, title);
    }
}

/// A settings card in two columns, each under its own heading, with a tile
/// per setting: its name above its controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsGrid {
    pub card: Rect,
    view_w: f32,
}

impl SettingsGrid {
    /// A grid whose longer column holds `rows` tiles.
    pub fn new(view: View, rows: usize) -> Self {
        let h = TITLE_H + SECTION_H + rows as f32 * TILE_H + 8.0;
        Self {
            card: Frame::of(view).body.top(h).max_width(MAX_CARD_W),
            view_w: view.w,
        }
    }

    fn column_x(self, column: usize) -> (f32, f32) {
        let w = (self.card.w - 2.0 * CARD_PAD - COLUMN_GAP) / 2.0;
        (self.card.x + CARD_PAD + column as f32 * (w + COLUMN_GAP), w)
    }

    pub fn tile(self, column: usize, row: usize) -> Rect {
        let (x, w) = self.column_x(column);
        Rect::at(x, self.card.y + TITLE_H + SECTION_H + row as f32 * TILE_H, w, TILE_H)
    }

    pub fn action_row(self, k: usize) -> Rect {
        action_row(self.card, self.view_w, k)
    }

    pub fn draw(self, draw: &mut Draw, pal: &Palette, fonts: &Fonts, title: &str, sections: [&str; 2]) {
        draw_card(draw, pal, fonts, self.card, title);
        for (column, section) in sections.into_iter().enumerate() {
            draw.sharp_text(&fonts.display, section)
                .position(self.column_x(column).0, self.card.y + TITLE_H + SECTION_H / 2.0 + 2.0)
                .size(theme::size::LABEL)
                .v_align_middle()
                .color(pal.accent);
        }
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

    #[test]
    fn grid_tiles_stay_in_their_column_and_actions_fit_under_the_card() {
        let view = View::default();
        let grid = SettingsGrid::new(view, 4);
        let (left, right) = (grid.tile(0, 3), grid.tile(1, 3));
        assert!(left.x + left.w < right.x, "the columns overlap");
        assert!(left.x >= grid.card.x && right.x + right.w <= grid.card.x + grid.card.w);
        assert!(left.y + left.h <= grid.card.y + grid.card.h);
        assert_eq!(grid.tile(0, 1).y, grid.tile(0, 0).y + TILE_H);
        let actions = grid.action_row(2);
        assert!(actions.y > grid.card.y + grid.card.h);
        assert!(
            actions.y + actions.h <= view.h,
            "{} > {}",
            actions.y + actions.h,
            view.h
        );
    }

    #[test]
    fn seven_rows_and_three_actions_fit_the_reference_view() {
        let view = View::default();
        let panel = SettingsPanel::new(view, 7);
        let last = panel.stepper(6);
        assert!(last.minus.y + last.minus.h <= panel.card.y + panel.card.h);
        let actions = panel.action_row(2);
        assert!(
            actions.y + actions.h <= view.h,
            "{} > {}",
            actions.y + actions.h,
            view.h
        );
    }
}
