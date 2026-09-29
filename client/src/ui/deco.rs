use notan::draw::{Draw, DrawShapes};
use notan::prelude::Color;

use super::{Fonts, Rect, SharpText};
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
    /// Estimated from Nunito's width, about 0.58 em a character in
    /// capitals; generous so the pill never clips its label.
    pub fn width(&self) -> f32 {
        self.text.chars().count() as f32 * self.size * 0.58 + self.size * 1.4
    }

    /// Draws the pill from its left edge `x`, centred on `cy`.
    pub fn draw(&self, draw: &mut Draw, fonts: &Fonts, (x, cy): (f32, f32)) {
        let (w, h) = (self.width(), self.size * 1.7);
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

/// A player's round portrait with their initial. `glow` in `0..=1` lightens
/// it, for avatars that react to the pointer.
pub fn portrait(
    draw: &mut Draw,
    pal: &Palette,
    fonts: &Fonts,
    (cx, cy): (f32, f32),
    radius: f32,
    name: &str,
    glow: f32,
) {
    draw.circle(radius)
        .position(cx, cy)
        .color(theme::mix(pal.avatar, pal.avatar_hover, glow));
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
