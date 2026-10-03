use notan::draw::{Draw, DrawShapes};
use notan::prelude::Texture;

use super::{list_row, portrait, Fonts, Persona, Rect, SharpText, Ui};
use crate::theme;

const UNDERLINE: f32 = 3.0;

impl Ui {
    /// A tab: its label, and an underline that grows from the centre on
    /// hover and spans the whole tab once selected.
    pub fn tab(&self, draw: &mut Draw, fonts: &Fonts, rect: Rect, label: &str, active: bool) {
        let pal = self.palette();
        let r = self.interact(label, rect, !active);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let (cx, bottom) = (rect.x + rect.w / 2.0, rect.y + rect.h);
        let reach = if active { 1.0 } else { 0.35 * r.hover };
        if reach > 0.0 {
            let w = rect.w * reach;
            draw.rect((cx - w / 2.0, bottom - UNDERLINE), (w, UNDERLINE))
                .corner_radius(UNDERLINE / 2.0)
                .color(pal.accent);
        }
        let color = if active {
            pal.text
        } else {
            theme::mix(pal.text_muted, pal.text, r.hover)
        };
        draw.sharp_text(&fonts.display, label)
            .position(cx, rect.y + rect.h / 2.0 - UNDERLINE)
            .size(theme::size::HEADING)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    /// A round avatar showing the player's initial.
    pub fn avatar(&self, draw: &mut Draw, fonts: &Fonts, rect: Rect, name: &str, picture: Option<&Texture>) {
        let pal = self.palette();
        let r = self.interact(name, rect, true);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let radius = rect.w.min(rect.h) / 2.0 * (1.0 + 0.05 * r.hover);
        let who = Persona {
            name,
            glow: r.hover,
            picture,
        };
        portrait(draw, &pal, fonts, center, radius, &who);
    }

    /// Text that can be clicked: it takes the link colour and an underline
    /// when hovered. `enabled` false draws it as plain text.
    pub fn link(&self, draw: &mut Draw, fonts: &Fonts, rect: Rect, text: &str, enabled: bool) {
        let pal = self.palette();
        let r = self.interact(text, rect, enabled);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        draw.sharp_text(&fonts.text, text)
            .position(cx, cy)
            .size(theme::size::BODY)
            .h_align_center()
            .v_align_middle()
            .color(theme::mix(pal.text, theme::LINK, r.hover));
        if r.hover > 0.01 {
            let w = rect.w * 0.6 * r.hover;
            draw.rect((cx - w / 2.0, cy + 12.0), (w, 1.5))
                .color(theme::LINK.with_alpha(r.hover));
        }
    }

    /// A list row that reacts to the pointer: it lights up and grows an
    /// accent bar on its left edge. The caller draws what the row holds.
    pub fn row(&self, draw: &mut Draw, rect: Rect, index: usize, id: &str) {
        let pal = self.palette();
        let r = self.interact(id, rect, true);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        list_row(draw, &pal, rect, index);
        if r.hover > 0.0 {
            draw.rect((rect.x, rect.y), (rect.w, rect.h))
                .color(pal.raised_hover.with_alpha(0.7 * r.hover));
            draw.rect((rect.x, rect.y), (4.0 * r.hover, rect.h)).color(pal.accent);
        }
        if r.flash > 0.0 {
            draw.rect((rect.x, rect.y), (rect.w, rect.h))
                .color(notan::prelude::Color::WHITE.with_alpha(0.2 * r.flash));
        }
    }
}
