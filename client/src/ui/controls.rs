use aero::draw::Draw;
use aero::geom::Color;
use aero::theme::{Painter, Theme, Visual};

use super::{list_row, Fonts, Rect, Ui};
use crate::theme::Palette;

impl Ui {
    /// A tab, which takes no click while it is the one shown.
    pub fn tab(&mut self, draw: &mut Draw, fonts: &mut Fonts, rect: Rect, label: &str, active: bool) {
        let r = self.interact(label, rect, !active);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let look = self.look(fonts);
        look.tab(&mut Painter { draw, fonts: fonts.library() }, rect.into(), label, active, r.visual(true));
    }

    pub fn link(&mut self, draw: &mut Draw, fonts: &mut Fonts, rect: Rect, text: &str, enabled: bool) {
        let r = self.interact(text, rect, enabled);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let look = self.look(fonts);
        look.link(&mut Painter { draw, fonts: fonts.library() }, rect.into(), text, r.visual(enabled));
    }

    /// A list row that reacts to the pointer: it lights up and grows an
    /// accent bar on its left edge. The caller draws what the row holds.
    pub fn row(&mut self, draw: &mut Draw, rect: Rect, index: usize, id: &str) {
        let r = self.interact(id, rect, true);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        paint_row(draw, &self.palette(), rect, index, r.visual(true));
    }
}

pub(super) fn paint_row(draw: &mut Draw, pal: &Palette, rect: Rect, index: usize, visual: Visual) {
    list_row(draw, pal, rect, index);
    if visual.hover > 0.0 {
        draw.rect(rect).color(pal.raised_hover.with_alpha(0.7 * visual.hover));
        draw.rect(Rect::at(rect.x, rect.y, 4.0 * visual.hover, rect.h)).color(pal.accent);
    }
    if visual.flash > 0.0 {
        draw.rect(rect).color(Color::WHITE.with_alpha(0.2 * visual.flash));
    }
}
