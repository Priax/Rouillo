use notan::draw::{Draw, DrawShapes};

use super::{card, Fonts, Rect, SharpText, Ui, View};
use crate::theme;

const MARGIN: f32 = 110.0;
const PADDING: f32 = 24.0;
const TITLE_H: f32 = 70.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modal {
    pub card: Rect,
}

impl Modal {
    pub fn new(view: View, width: f32) -> Self {
        Self {
            card: Rect::at(0.0, 0.0, view.w, view.h)
                .pad(MARGIN, 0.0, MARGIN, 0.0)
                .max_width(width),
        }
    }

    fn close(self) -> Rect {
        self.card.pad(14.0, 16.0, 0.0, 0.0).right(48.0).top(40.0)
    }

    pub fn body(self) -> Rect {
        self.card.pad(TITLE_H, PADDING, PADDING, PADDING)
    }

    pub fn dismissed(self, ui: &Ui) -> bool {
        let view = ui.view();
        let anywhere = Rect::at(0.0, 0.0, view.w, view.h);
        ui.clicked(self.close()) || (ui.clicked(anywhere) && !ui.clicked(self.card))
    }

    pub fn draw(self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, title: &str) {
        let pal = ui.palette();
        let view = ui.view();
        ui.set_input(true);
        draw.rect((0.0, 0.0), (view.w, view.h)).color(pal.scrim_strong);
        card(draw, &pal, self.card);
        draw.sharp_text(&fonts.display, title)
            .position(self.card.x + PADDING, self.card.y + TITLE_H / 2.0 - 1.0)
            .size(theme::size::HEADING)
            .v_align_middle()
            .color(pal.text);
        ui.button(draw, fonts, self.close(), "X");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_and_the_button_stay_inside_the_card() {
        let modal = Modal::new(View::default(), 600.0);
        let (card, body, close) = (modal.card, modal.body(), modal.close());
        assert!((card.x + card.w / 2.0 - View::default().w / 2.0).abs() < f32::EPSILON);
        assert!(body.x > card.x && body.x + body.w < card.x + card.w);
        assert!(body.y > close.y + close.h && body.y + body.h < card.y + card.h);
        assert!(close.x + close.w < card.x + card.w && close.y > card.y);
    }
}
