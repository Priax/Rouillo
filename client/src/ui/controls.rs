use notan::draw::{Draw, DrawShapes};

use super::{Fonts, Rect, SharpText, Ui};
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
    pub fn avatar(&self, draw: &mut Draw, fonts: &Fonts, rect: Rect, name: &str) {
        let pal = self.palette();
        let r = self.interact(name, rect, true);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let radius = rect.w.min(rect.h) / 2.0 * (1.0 + 0.05 * r.hover);
        draw.circle(radius)
            .position(cx, cy)
            .color(theme::mix(pal.avatar, pal.avatar_hover, r.hover));
        draw.circle(radius).position(cx, cy).stroke(2.0).color(pal.accent);
        let initial: String = name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        draw.sharp_text(&fonts.display, &initial)
            .position(cx, cy)
            .size(theme::size::HEADING)
            .h_align_center()
            .v_align_middle()
            .color(pal.text);
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
}
