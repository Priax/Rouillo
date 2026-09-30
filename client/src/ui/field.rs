use std::borrow::Cow;

use notan::draw::{Draw, DrawShapes};

use super::{Face, Fonts, Rect, SharpText, Ui};
use crate::theme::{self, Palette};

pub struct Field<'a> {
    pub placeholder: &'a str,
    pub value: &'a str,
    pub focused: bool,
    pub secret: bool,
}

const PADDING: f32 = 12.0;
const CARET_W: f32 = 2.0;
const BLINK_SECS: f64 = 0.5;
const AREA_TEXT: f32 = theme::size::LABEL;
const AREA_LINE_H: f32 = 26.0;

fn frame(draw: &mut Draw, pal: &Palette, r: Rect, focused: bool) {
    let border = if focused { pal.accent } else { pal.border };
    draw.rect((r.x, r.y), (r.w, r.h))
        .corner_radius(theme::RADIUS)
        .color(pal.surface);
    draw.rect((r.x, r.y), (r.w, r.h))
        .corner_radius(theme::RADIUS)
        .stroke(2.0)
        .color(border);
}

fn caret(draw: &mut Draw, ui: &Ui, x: f32, mid: f32, size: f32) {
    if ui.time() % (2.0 * BLINK_SECS) < BLINK_SECS {
        let h = size * 1.2;
        draw.rect((x + 1.0, mid - h / 2.0), (CARET_W, h))
            .color(ui.palette().text);
    }
}

pub fn text_field(draw: &mut Draw, ui: &Ui, fonts: &Fonts, r: Rect, field: &Field) {
    let pal = ui.palette();
    frame(draw, &pal, r, field.focused);

    let size = (r.h * 0.45).clamp(16.0, 24.0);
    let (x, mid) = (r.x + PADDING, r.y + r.h / 2.0);
    let typed: Cow<str> = if field.secret {
        Cow::Owned("*".repeat(field.value.chars().count()))
    } else {
        Cow::Borrowed(field.value)
    };
    let shown = fonts.tail(Face::Text, &typed, size, r.w - 2.0 * PADDING - CARET_W - 1.0);
    let (text, color) = if typed.is_empty() {
        (field.placeholder, pal.text_muted)
    } else {
        (shown, pal.text)
    };
    draw.sharp_text(&fonts.text, text)
        .position(x, mid)
        .size(size)
        .v_align_middle()
        .color(color);
    if field.focused {
        caret(draw, ui, x + fonts.width(Face::Text, shown, size), mid, size);
    }
}

pub const fn area_height(lines: usize) -> f32 {
    2.0 * PADDING + lines as f32 * AREA_LINE_H
}

pub fn text_area(draw: &mut Draw, ui: &Ui, fonts: &Fonts, r: Rect, field: &Field) {
    let pal = ui.palette();
    frame(draw, &pal, r, field.focused);

    let x = r.x + PADDING;
    let line_mid = |i: usize| r.y + PADDING + (i as f32 + 0.5) * AREA_LINE_H;
    let lines = fonts.wrap(Face::Text, field.value, AREA_TEXT, r.w - 2.0 * PADDING - CARET_W - 1.0);
    let visible = (((r.h - 2.0 * PADDING) / AREA_LINE_H) as usize).max(1);
    let shown = &lines[lines.len().saturating_sub(visible)..];

    if field.value.is_empty() {
        draw.sharp_text(&fonts.text, field.placeholder)
            .position(x, line_mid(0))
            .size(AREA_TEXT)
            .v_align_middle()
            .color(pal.text_muted);
    }
    for (i, line) in shown.iter().enumerate() {
        draw.sharp_text(&fonts.text, line)
            .position(x, line_mid(i))
            .size(AREA_TEXT)
            .v_align_middle()
            .color(pal.text);
    }
    if field.focused {
        let last = shown.last().copied().unwrap_or_default();
        let row = shown.len().saturating_sub(1);
        caret(
            draw,
            ui,
            x + fonts.width(Face::Text, last, AREA_TEXT),
            line_mid(row),
            AREA_TEXT,
        );
    }
}
