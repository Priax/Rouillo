use notan::draw::{Draw, DrawShapes};
use notan::prelude::Color;

use super::Rect;
use crate::theme::Palette;

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
