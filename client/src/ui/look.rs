use aero::geom::{Color, Rect as Area};
use aero::math::{vec2, Affine2, Vec2};
use aero::theme::{Content, Painter, Theme, Visual};

use super::{Face, Faces, Rect};
use crate::theme::{self, Palette};

const HOVER_GROW: f32 = 0.03;
const PRESS_SHRINK: f32 = 0.04;
const SHADOW_LAYERS: u8 = 4;
const SHADOW_ALPHA: f32 = 0.07;
const SHEEN_ALPHA: f32 = 0.05;
const BORDER_WIDTH: f32 = 1.5;
const TEXT_HEIGHT: f32 = 0.52;
const TEXT_MIN: f32 = 12.0;
const TEXT_PADDING: f32 = 24.0;
const UNDERLINE: f32 = 3.0;

/// How the game's widgets look: raised buttons that grow under the pointer, underlined tabs, links, in the colors of
/// the screen shown.
#[derive(Clone)]
pub struct Look {
    pub faces: Faces,
    pub palette: Palette,
}

impl Look {
    /// `text` centred on `center`.
    fn write(&self, p: &mut Painter<'_>, (face, text, size): (Face, &str, f32), center: Vec2, color: Color) {
        let layout = p.fonts.layout_kept(text, &self.faces.style(face, size));
        p.draw.text_colored(p.fonts, &layout, center - layout.size() * 0.5, color);
    }
}

impl Theme for Look {
    fn button(&self, p: &mut Painter<'_>, area: Area, content: Content<'_>, visual: Visual) {
        let pal = &self.palette;
        let b = Rect::at(area.min.x, area.min.y, area.size().x, area.size().y);
        let center = area.center();
        let scale = 1.0 + HOVER_GROW * visual.hover - PRESS_SHRINK * visual.press;
        p.draw.push_transform(
            Affine2::from_translation(center)
                * Affine2::from_scale(Vec2::splat(scale))
                * Affine2::from_translation(-center),
        );

        if visual.enabled {
            let lift = 1.0 + visual.hover - visual.press;
            for i in 1..=SHADOW_LAYERS {
                let spread = f32::from(i);
                let shadow =
                    Rect::at(b.x - spread, b.y - spread + spread * lift, b.w + 2.0 * spread, b.h + 2.0 * spread);
                p.draw.rect(shadow).radius(theme::RADIUS + spread).color(Color::BLACK.with_alpha(SHADOW_ALPHA));
            }
        }

        let (fill, border, ink) = if visual.enabled {
            let fill = theme::mix(pal.raised, pal.raised_hover, visual.hover);
            (fill, theme::mix(pal.border, pal.accent, visual.hover), pal.text)
        } else {
            (pal.disabled, pal.divider, pal.text_disabled)
        };
        p.draw.rect(b).radius(theme::RADIUS).color(fill);
        p.draw
            .rect(Rect::at(b.x, b.y, b.w, b.h / 2.0))
            .radii([theme::RADIUS, theme::RADIUS, 0.0, 0.0])
            .color(Color::WHITE.with_alpha(SHEEN_ALPHA + SHEEN_ALPHA * visual.hover));
        if visual.flash > 0.0 {
            p.draw.rect(b).radius(theme::RADIUS).color(Color::WHITE.with_alpha(0.25 * visual.flash));
        }
        p.draw.rect(b).radius(theme::RADIUS).stroke(border, BORDER_WIDTH);

        match content {
            Content::Text(text) => {
                // As tall as the button allows, narrowed only if it would overflow.
                let tall = b.h * TEXT_HEIGHT;
                let width = p.fonts.layout_kept(text, &self.faces.style(Face::Display, tall)).size().x;
                let room = b.w - TEXT_PADDING;
                let size = if width > room { tall * room / width } else { tall }.max(TEXT_MIN);
                self.write(p, (Face::Display, text, size), center, ink);
            }
            Content::Paint(paint) => paint(p.draw, area, ink),
        }

        p.draw.pop_transform();
    }

    /// Its label, and an underline that grows from the centre on hover and spans the whole tab once selected.
    fn tab(&self, p: &mut Painter<'_>, area: Area, text: &str, selected: bool, visual: Visual) {
        let pal = &self.palette;
        let (cx, bottom) = (area.center().x, area.max.y);
        let reach = if selected { 1.0 } else { 0.35 * visual.hover };
        if reach > 0.0 {
            let w = area.size().x * reach;
            p.draw
                .rect(Rect::at(cx - w / 2.0, bottom - UNDERLINE, w, UNDERLINE))
                .radius(UNDERLINE / 2.0)
                .color(pal.accent);
        }
        let color = if selected { pal.text } else { theme::mix(pal.text_muted, pal.text, visual.hover) };
        let center = vec2(cx, area.center().y - UNDERLINE);
        self.write(p, (Face::Display, text, theme::size::HEADING), center, color);
    }

    /// The link colour and an underline when hovered; plain text when disabled.
    fn link(&self, p: &mut Painter<'_>, area: Area, text: &str, visual: Visual) {
        let center = area.center();
        let color = theme::mix(self.palette.text, theme::LINK, visual.hover);
        self.write(p, (Face::Text, text, theme::size::BODY), center, color);
        if visual.hover > 0.01 {
            let w = area.size().x * 0.6 * visual.hover;
            p.draw
                .rect(Rect::at(center.x - w / 2.0, center.y + 12.0, w, 1.5))
                .color(theme::LINK.with_alpha(visual.hover));
        }
    }
}
