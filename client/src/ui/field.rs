use std::borrow::Cow;

use notan::draw::{Draw, DrawShapes};

use super::{Fonts, Rect, SharpText};
use crate::theme;

pub struct Field<'a> {
    pub placeholder: &'a str,
    pub value: &'a str,
    pub focused: bool,
    pub secret: bool,
}

const PADDING: f32 = 12.0;

pub fn text_field(draw: &mut Draw, fonts: &Fonts, r: Rect, field: &Field) {
    let border = if field.focused { theme::ACCENT } else { theme::BORDER };
    draw.rect((r.x, r.y), (r.w, r.h))
        .corner_radius(theme::RADIUS)
        .color(theme::SURFACE);
    draw.rect((r.x, r.y), (r.w, r.h))
        .corner_radius(theme::RADIUS)
        .stroke(2.0)
        .color(border);

    let (text, color) = if field.value.is_empty() {
        (Cow::Borrowed(field.placeholder), theme::TEXT_MUTED)
    } else if field.secret {
        (Cow::Owned("*".repeat(field.value.chars().count())), theme::TEXT)
    } else {
        (Cow::Borrowed(field.value), theme::TEXT)
    };
    draw.sharp_text(&fonts.text, &text)
        .position(r.x + PADDING, r.y + r.h / 2.0)
        .size((r.h * 0.45).clamp(16.0, 24.0))
        .v_align_middle()
        .color(color);
}
