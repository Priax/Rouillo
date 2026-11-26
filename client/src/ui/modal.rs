use aero::draw::Draw;

use super::{card, Face, Fonts, Rect, Typeset, View};
use crate::theme::{self, Palette};

const MARGIN: f32 = 110.0;
const PADDING: f32 = 24.0;
const TITLE_H: f32 = 70.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modal {
    pub card: Rect,
}

impl Modal {
    pub fn new(view: View, width: f32) -> Self {
        Self { card: Rect::at(0.0, 0.0, view.w, view.h).pad(MARGIN, 0.0, MARGIN, 0.0).max_width(width) }
    }

    pub(super) fn close(self) -> Rect {
        let size = 36.0;
        self.card.pad((TITLE_H - size) / 2.0, PADDING, 0.0, 0.0).right(size).top(size)
    }

    pub fn body(self) -> Rect {
        self.card.pad(TITLE_H, PADDING, PADDING, PADDING)
    }

    /// The scrim over the screen, the card and its title.
    pub(super) fn paint(self, draw: &mut Draw, fonts: &mut Fonts, pal: &Palette, (view, title): (View, &str)) {
        draw.rect(Rect::at(0.0, 0.0, view.w, view.h)).color(pal.scrim_strong);
        card(draw, pal, self.card);
        fonts
            .write(draw, Face::Display, title)
            .left(self.card.x + PADDING, self.card.y + TITLE_H / 2.0 - 1.0)
            .size(theme::size::HEADING)
            .color(pal.text);
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
