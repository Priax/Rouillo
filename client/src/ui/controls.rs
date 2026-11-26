use aero::draw::Draw;
use aero::geom::Color;
use aero::image::Image;
use aero::theme::{Painter, Theme};

use super::{list_row, portrait, Fonts, Persona, Rect, Ui};

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

    /// A round avatar showing the player's initial.
    pub fn avatar(&mut self, draw: &mut Draw, fonts: &mut Fonts, rect: Rect, name: &str, picture: Option<&Image>) {
        let pal = self.palette();
        let r = self.interact_in(name, rect, super::Area::Pointer, true);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let radius = rect.w.min(rect.h) / 2.0 * (1.0 + 0.05 * r.hover);
        let who = Persona { name, glow: r.hover, picture };
        portrait(draw, &pal, fonts, center, radius, &who);
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
        let pal = self.palette();
        let r = self.interact(id, rect, true);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        list_row(draw, &pal, rect, index);
        if r.hover > 0.0 {
            draw.rect(rect).color(pal.raised_hover.with_alpha(0.7 * r.hover));
            draw.rect(Rect::at(rect.x, rect.y, 4.0 * r.hover, rect.h)).color(pal.accent);
        }
        if r.flash > 0.0 {
            draw.rect(rect).color(Color::WHITE.with_alpha(0.2 * r.flash));
        }
    }
}
