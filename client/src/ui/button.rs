use notan::draw::{Draw, DrawShapes, Font};
use notan::math::{vec2, Mat3};
use notan::prelude::*;

use super::{Fonts, Rect, Response, SharpText, Ui};
use crate::theme::{self, Palette};

const HOVER_GROW: f32 = 0.03;
const PRESS_SHRINK: f32 = 0.04;
const SHADOW_LAYERS: u8 = 4;
const SHADOW_ALPHA: f32 = 0.07;
const SHEEN_ALPHA: f32 = 0.05;
const BORDER_WIDTH: f32 = 1.5;
const TEXT_HEIGHT: f32 = 0.52;
const TEXT_MIN: f32 = 12.0;
const CHAR_WIDTH: f32 = 0.46;
const TEXT_PADDING: f32 = 24.0;

const ICON_LENGTH: f32 = 0.36;
const ICON_THICKNESS: f32 = 0.09;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Minus,
    Plus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label<'a> {
    Text(&'a str),
    Icon(Icon),
}

impl<'a> From<&'a str> for Label<'a> {
    fn from(text: &'a str) -> Self {
        Self::Text(text)
    }
}

impl From<Icon> for Label<'_> {
    fn from(icon: Icon) -> Self {
        Self::Icon(icon)
    }
}

impl<'a> Label<'a> {
    fn id(self) -> &'a str {
        match self {
            Self::Text(text) => text,
            Self::Icon(Icon::Minus) => "icon:minus",
            Self::Icon(Icon::Plus) => "icon:plus",
        }
    }
}

impl Ui {
    pub fn button<'a>(&self, draw: &mut Draw, fonts: &Fonts, rect: Rect, label: impl Into<Label<'a>>) {
        self.button_enabled(draw, fonts, rect, label, true);
    }

    pub fn button_enabled<'a>(
        &self,
        draw: &mut Draw,
        fonts: &Fonts,
        rect: Rect,
        label: impl Into<Label<'a>>,
        enabled: bool,
    ) {
        let label = label.into();
        let r = self.interact(label.id(), rect, enabled);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        paint(draw, &self.palette(), &fonts.display, rect, label, r, enabled);
    }
}

fn paint(draw: &mut Draw, pal: &Palette, font: &Font, b: Rect, label: Label, r: Response, enabled: bool) {
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
            theme::mix(pal.raised, pal.raised_hover, r.hover),
            theme::mix(pal.border, pal.accent, r.hover),
            pal.text,
        )
    } else {
        (pal.disabled, pal.divider, pal.text_disabled)
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

    match label {
        Label::Text(label) => {
            let n = label.chars().count().max(1) as f32;
            let fits = (b.w - TEXT_PADDING) / (n * CHAR_WIDTH);
            let size = (b.h * TEXT_HEIGHT).min(fits).max(TEXT_MIN);
            draw.sharp_text(font, label)
                .position(center.x, center.y)
                .size(size)
                .h_align_center()
                .v_align_middle()
                .color(text);
        }
        Label::Icon(icon) => paint_icon(draw, icon, center.x, center.y, b.h.min(b.w), text),
    }

    draw.transform().pop();
}

fn paint_icon(draw: &mut Draw, icon: Icon, cx: f32, cy: f32, size: f32, color: Color) {
    let (long, thick) = (size * ICON_LENGTH, size * ICON_THICKNESS);
    let bar = |draw: &mut Draw, w: f32, h: f32| {
        draw.rect((cx - w / 2.0, cy - h / 2.0), (w, h))
            .corner_radius(thick / 2.0)
            .color(color);
    };
    bar(draw, long, thick);
    if icon == Icon::Plus {
        bar(draw, thick, long);
    }
}
