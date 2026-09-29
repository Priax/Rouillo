use notan::draw::{Draw, DrawShapes, DrawTextSection};
use notan::math::{vec2, Mat3};
use notan::prelude::*;

use super::{Rect, Response, Ui};
use crate::theme;

const HOVER_GROW: f32 = 0.03;
const PRESS_SHRINK: f32 = 0.04;
const SHADOW_LAYERS: u8 = 4;
const SHADOW_ALPHA: f32 = 0.07;
const SHEEN_ALPHA: f32 = 0.05;
const BORDER_WIDTH: f32 = 1.5;
const TEXT_MAX: f32 = 28.0;
const TEXT_MIN: f32 = 11.0;

impl Ui {
    pub fn button(&self, draw: &mut Draw, font: &crate::Font, rect: Rect, label: &str) {
        self.button_enabled(draw, font, rect, label, true);
    }

    pub fn button_enabled(&self, draw: &mut Draw, font: &crate::Font, rect: Rect, label: &str, enabled: bool) {
        let r = self.interact(label, rect, enabled);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        paint(draw, font, rect, label, r, enabled);
    }
}

fn paint(draw: &mut Draw, font: &crate::Font, b: Rect, label: &str, r: Response, enabled: bool) {
    let center = vec2(b.x + b.w / 2.0, b.y + b.h / 2.0);
    let scale = 1.0 + HOVER_GROW * r.hover - PRESS_SHRINK * r.press;
    draw.transform()
        .push(Mat3::from_translation(center) * Mat3::from_scale(vec2(scale, scale)) * Mat3::from_translation(-center));

    if enabled {
        let lift = 1.0 + r.hover - r.press;
        for i in 1..=SHADOW_LAYERS {
            let spread = f32::from(i);
            draw.rect(
                (b.x - spread, b.y - spread + spread * lift),
                (b.w + 2.0 * spread, b.h + 2.0 * spread),
            )
            .corner_radius(theme::RADIUS + spread)
            .color(Color::BLACK.with_alpha(SHADOW_ALPHA));
        }
    }

    let (fill, border, text) = if enabled {
        (
            mix(theme::RAISED, theme::RAISED_HOVER, r.hover),
            mix(theme::BORDER, theme::ACCENT, r.hover),
            theme::TEXT,
        )
    } else {
        (theme::DISABLED, theme::DIVIDER, theme::TEXT_DISABLED)
    };
    draw.rect((b.x, b.y), (b.w, b.h))
        .corner_radius(theme::RADIUS)
        .color(fill);
    draw.rect((b.x, b.y), (b.w, b.h / 2.0))
        .top_left_radius(theme::RADIUS)
        .top_right_radius(theme::RADIUS)
        .color(Color::WHITE.with_alpha(SHEEN_ALPHA + SHEEN_ALPHA * r.hover));
    if r.flash > 0.0 {
        draw.rect((b.x, b.y), (b.w, b.h))
            .corner_radius(theme::RADIUS)
            .color(Color::WHITE.with_alpha(0.25 * r.flash));
    }
    draw.rect((b.x, b.y), (b.w, b.h))
        .corner_radius(theme::RADIUS)
        .stroke(BORDER_WIDTH)
        .color(border);

    let n = label.chars().count().max(1) as f32;
    let size = (TEXT_MAX * (b.w - 20.0) / (n * 17.5)).clamp(TEXT_MIN, TEXT_MAX);
    draw.text(font, label)
        .position(center.x, center.y)
        .size(size)
        .h_align_center()
        .v_align_middle()
        .color(text);

    draw.transform().pop();
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}
