use aero::draw::Draw;
use aero::geom::Color;
use aero::theme::Visual;

use super::{list_row, Rect};
use crate::theme::Palette;

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
