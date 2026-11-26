use aero::draw::Draw;
use aero::geom::Color;
use aero::image::Image;
use aero::math::vec2;

use super::{Face, Fonts, Rect, Typeset};
use crate::theme::{self, Palette};

/// A line of a list, its background alternating so rows read apart.
pub fn list_row(draw: &mut Draw, pal: &Palette, rect: Rect, index: usize) {
    let fill = if index.is_multiple_of(2) { pal.surface } else { pal.surface_alt };
    draw.rect(rect).color(fill);
}

/// A thin rule separating two parts of a screen.
pub fn divider(draw: &mut Draw, pal: &Palette, x: f32, y: f32, w: f32) {
    draw.rect(Rect::at(x, y, w, 1.0)).color(pal.divider);
}

#[derive(Clone, Copy)]
pub enum Edge {
    Top,
    Bottom,
}

/// A band across the screen with a coloured line on the edge facing the
/// rest of the interface. Its text is the caller's.
pub fn banner(draw: &mut Draw, rect: Rect, fill: Color, line: Color, edge: Edge) {
    draw.rect(rect).color(fill);
    let line_y = match edge {
        Edge::Top => rect.y(),
        Edge::Bottom => rect.y() + rect.h() - 2.0,
    };
    draw.rect(Rect::at(rect.x(), line_y, rect.w(), 2.0)).color(line);
}

/// A raised panel grouping related content.
pub fn card(draw: &mut Draw, pal: &Palette, rect: Rect) {
    for spread in [6.0, 3.0] {
        draw.rect(Rect::at(
            rect.x() - spread,
            rect.y() - spread + 4.0,
            rect.w() + 2.0 * spread,
            rect.h() + 2.0 * spread,
        ))
        .radius(theme::RADIUS + spread)
        .color(Color::BLACK.with_alpha(0.12));
    }
    draw.rect(rect).radius(theme::RADIUS).color(pal.surface);
    draw.rect(rect).radius(theme::RADIUS).stroke(pal.divider, 1.0);
}

/// A rounded label: a tag, a result, a rating.
pub struct Pill<'a> {
    pub text: &'a str,
    pub color: Color,
    pub size: f32,
}

impl<'a> Pill<'a> {
    pub const fn small(text: &'a str, color: Color) -> Self {
        Self { text, color, size: theme::size::SMALL }
    }

    pub fn width(&self, fonts: &mut Fonts) -> f32 {
        fonts.width(Face::Text, self.text, self.size) + self.size * 1.4
    }

    /// Draws the pill from its left edge `x`, centred on `cy`.
    pub fn draw(&self, draw: &mut Draw, fonts: &mut Fonts, (x, cy): (f32, f32)) {
        let (w, h) = (self.width(fonts), self.size * 1.7);
        draw.rect(Rect::at(x, cy - h / 2.0, w, h)).radius(h / 2.0).color(self.color.with_alpha(0.18));
        draw.rect(Rect::at(x, cy - h / 2.0, w, h)).radius(h / 2.0).stroke(self.color, 1.5);
        fonts.write(draw, Face::Text, self.text).center(x + w / 2.0, cy).size(self.size).color(self.color);
    }
}

/// Draws `pills` in a row ending at `right`, centred on `cy`, and returns
/// where the row starts, so the text before it can stop short.
pub fn pills_ending_at(draw: &mut Draw, fonts: &mut Fonts, pills: &[Pill], right: f32, cy: f32) -> f32 {
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
    pub picture: Option<&'a Image>,
}

pub fn portrait(draw: &mut Draw, pal: &Palette, fonts: &mut Fonts, (cx, cy): (f32, f32), radius: f32, who: &Persona) {
    let Persona { name, glow, picture } = *who;
    draw.circle(vec2(cx, cy), radius).color(theme::mix(pal.avatar, pal.avatar_hover, glow));
    if let Some(picture) = picture {
        draw.image(picture, Rect::at(cx - radius, cy - radius, 2.0 * radius, 2.0 * radius));
        draw.circle(vec2(cx, cy), radius).stroke(pal.accent, 2.0);
        return;
    }
    draw.circle(vec2(cx, cy), radius).stroke(pal.accent, 2.0);
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    fonts.write(draw, Face::Display, &initial).center(cx, cy).size(radius * 0.8).color(pal.text);
}
