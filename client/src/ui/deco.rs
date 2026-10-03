use notan::draw::{Draw, DrawImages, DrawShapes};
use notan::prelude::{Color, Texture};

use super::{Face, Fonts, Rect, SharpText};
use crate::theme::{self, Palette};

/// A line of a list, its background alternating so rows read apart.
pub fn list_row(draw: &mut Draw, pal: &Palette, rect: Rect, index: usize) {
    let fill = if index.is_multiple_of(2) {
        pal.surface
    } else {
        pal.surface_alt
    };
    draw.rect((rect.x, rect.y), (rect.w, rect.h)).color(fill);
}

/// A thin rule separating two parts of a screen.
pub fn divider(draw: &mut Draw, pal: &Palette, x: f32, y: f32, w: f32) {
    draw.rect((x, y), (w, 1.0)).color(pal.divider);
}

#[derive(Clone, Copy)]
pub enum Edge {
    Top,
    Bottom,
}

/// A band across the screen with a coloured line on the edge facing the
/// rest of the interface. Its text is the caller's.
pub fn banner(draw: &mut Draw, rect: Rect, fill: Color, line: Color, edge: Edge) {
    draw.rect((rect.x, rect.y), (rect.w, rect.h)).color(fill);
    let line_y = match edge {
        Edge::Top => rect.y,
        Edge::Bottom => rect.y + rect.h - 2.0,
    };
    draw.rect((rect.x, line_y), (rect.w, 2.0)).color(line);
}

/// A raised panel grouping related content.
pub fn card(draw: &mut Draw, pal: &Palette, rect: Rect) {
    for spread in [6.0, 3.0] {
        draw.rect(
            (rect.x - spread, rect.y - spread + 4.0),
            (rect.w + 2.0 * spread, rect.h + 2.0 * spread),
        )
        .corner_radius(theme::RADIUS + spread)
        .color(Color::BLACK.with_alpha(0.12));
    }
    draw.rect((rect.x, rect.y), (rect.w, rect.h))
        .corner_radius(theme::RADIUS)
        .color(pal.surface);
    draw.rect((rect.x, rect.y), (rect.w, rect.h))
        .corner_radius(theme::RADIUS)
        .stroke(1.0)
        .color(pal.divider);
}

/// A rounded label: a tag, a result, a rating.
pub struct Pill<'a> {
    pub text: &'a str,
    pub color: Color,
    pub size: f32,
}

impl Pill<'_> {
    pub fn width(&self, fonts: &Fonts) -> f32 {
        fonts.width(Face::Text, self.text, self.size) + self.size * 1.4
    }

    /// Draws the pill from its left edge `x`, centred on `cy`.
    pub fn draw(&self, draw: &mut Draw, fonts: &Fonts, (x, cy): (f32, f32)) {
        let (w, h) = (self.width(fonts), self.size * 1.7);
        draw.rect((x, cy - h / 2.0), (w, h))
            .corner_radius(h / 2.0)
            .color(self.color.with_alpha(0.18));
        draw.rect((x, cy - h / 2.0), (w, h))
            .corner_radius(h / 2.0)
            .stroke(1.5)
            .color(self.color);
        draw.sharp_text(&fonts.text, self.text)
            .position(x + w / 2.0, cy)
            .size(self.size)
            .h_align_center()
            .v_align_middle()
            .color(self.color);
    }
}

/// Draws `pills` in a row ending at `right`, centred on `cy`, and returns
/// where the row starts, so the text before it can stop short.
pub fn pills_ending_at(draw: &mut Draw, fonts: &Fonts, pills: &[Pill], right: f32, cy: f32) -> f32 {
    let mut x = right;
    for (i, pill) in pills.iter().rev().enumerate() {
        if i > 0 {
            x -= PILL_GAP;
        }
        x -= pill.width(fonts);
        pill.draw(draw, fonts, (x, cy));
    }
    x
}

const PILL_GAP: f32 = 10.0;

/// A player's round portrait with their initial. `glow` in `0..=1` lightens
/// it, for avatars that react to the pointer.
/// Who a portrait shows: a picture when there is one, else the name's initial.
pub struct Persona<'a> {
    pub name: &'a str,
    pub glow: f32,
    pub picture: Option<&'a Texture>,
}

pub fn portrait(draw: &mut Draw, pal: &Palette, fonts: &Fonts, (cx, cy): (f32, f32), radius: f32, who: &Persona) {
    let Persona { name, glow, picture } = *who;
    draw.circle(radius)
        .position(cx, cy)
        .color(theme::mix(pal.avatar, pal.avatar_hover, glow));
    if let Some(picture) = picture {
        draw.image(picture)
            .position(cx - radius, cy - radius)
            .size(2.0 * radius, 2.0 * radius);
        draw.circle(radius).position(cx, cy).stroke(2.0).color(pal.accent);
        return;
    }
    draw.circle(radius).position(cx, cy).stroke(2.0).color(pal.accent);
    let initial: String = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    draw.sharp_text(&fonts.display, &initial)
        .position(cx, cy)
        .size(radius * 0.8)
        .h_align_center()
        .v_align_middle()
        .color(pal.text);
}
