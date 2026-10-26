use notan::draw::{Draw, DrawShapes};
use notan::math::{vec2, Mat3};
use notan::prelude::*;

use super::{Face, Fonts, Rect, Response, SharpText, Ui};
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

const ICON_LENGTH: f32 = 0.36;
const ICON_THICKNESS: f32 = 0.09;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Minus,
    Plus,
    First,
    Previous,
    Next,
    Last,
    Play,
    Pause,
    Close,
    /// Dots that pulse in turn, while something is on its way.
    Busy,
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
            Self::Icon(Icon::First) => "icon:first",
            Self::Icon(Icon::Previous) => "icon:previous",
            Self::Icon(Icon::Next) => "icon:next",
            Self::Icon(Icon::Last) => "icon:last",
            Self::Icon(Icon::Play) => "icon:play",
            Self::Icon(Icon::Pause) => "icon:pause",
            Self::Icon(Icon::Close) => "icon:close",
            Self::Icon(Icon::Busy) => "icon:busy",
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
        let time = self.time() as f32;
        paint(draw, &self.palette(), fonts, rect, (label, time), r, enabled);
    }
}

fn paint(
    draw: &mut Draw,
    pal: &Palette,
    fonts: &Fonts,
    b: Rect,
    (label, time): (Label, f32),
    r: Response,
    enabled: bool,
) {
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
            // As tall as the button allows, narrowed only if it would overflow.
            let tall = b.h * TEXT_HEIGHT;
            let width = fonts.width(Face::Display, label, tall);
            let room = b.w - TEXT_PADDING;
            let size = if width > room { tall * room / width } else { tall }.max(TEXT_MIN);
            draw.sharp_text(&fonts.display, label)
                .position(center.x, center.y)
                .size(size)
                .h_align_center()
                .v_align_middle()
                .color(text);
        }
        Label::Icon(icon) => paint_icon(draw, icon, (center.x, center.y), b.h.min(b.w), text, time),
    }

    draw.transform().pop();
}

fn paint_icon(draw: &mut Draw, icon: Icon, (cx, cy): (f32, f32), size: f32, color: Color, time: f32) {
    let (long, thick) = (size * ICON_LENGTH, size * ICON_THICKNESS);
    let bar = |draw: &mut Draw, x: f32, w: f32, h: f32| {
        draw.rect((x - w / 2.0, cy - h / 2.0), (w, h))
            .corner_radius(thick / 2.0)
            .color(color);
    };
    let half = long / 2.0;
    match icon {
        Icon::Minus => bar(draw, cx, long, thick),
        Icon::Plus => {
            bar(draw, cx, long, thick);
            bar(draw, cx, thick, long);
        }
        Icon::Previous | Icon::Next | Icon::First | Icon::Last => {
            let forward = matches!(icon, Icon::Next | Icon::Last);
            let tip = if forward { half / 2.0 } else { -half / 2.0 };
            let edge = icon == Icon::First || icon == Icon::Last;
            let to_bar = if edge { tip + 2.0 * thick * tip.signum() } else { 0.0 };
            let x = if edge { cx - (to_bar - tip) / 2.0 } else { cx };
            draw.path()
                .move_to(x - tip, cy - half)
                .line_to(x + tip, cy)
                .line_to(x - tip, cy + half)
                .stroke(thick)
                .round_cap()
                .round_join()
                .color(color);
            if edge {
                bar(draw, x + to_bar, thick, long);
            }
        }
        Icon::Play => {
            let left = cx - half * 0.7;
            draw.triangle((left, cy - half), (left, cy + half), (cx + half, cy))
                .color(color);
        }
        Icon::Pause => {
            bar(draw, cx - half * 0.45, thick * 1.4, long);
            bar(draw, cx + half * 0.45, thick * 1.4, long);
        }
        Icon::Close => {
            let arm = half * 0.8;
            for dy in [arm, -arm] {
                draw.line((cx - arm, cy - dy), (cx + arm, cy + dy))
                    .width(thick)
                    .color(color);
            }
        }
        Icon::Busy => {
            for i in 0..3u8 {
                let phase = time * 5.0 - f32::from(i) * 0.9;
                let alpha = 0.3 + 0.7 * (0.5 + 0.5 * phase.sin());
                draw.circle(size * 0.07)
                    .position(cx + (f32::from(i) - 1.0) * size * 0.2, cy)
                    .color(color.with_alpha(color.a * alpha));
            }
        }
    }
}
