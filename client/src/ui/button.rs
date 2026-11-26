use aero::draw::Draw;
use aero::geom::{Color, Rect as Area};
use aero::math::vec2;
use aero::path::Cap;
use aero::theme::{Content, Painter, Theme};

use super::{Fonts, Rect, Ui};

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
    Up,
    Down,
    Play,
    Pause,
    Close,
    Star,
    StarOutline,
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
    /// `text`, or the busy icon while waiting.
    pub const fn busy_or(busy: bool, text: &'a str) -> Self {
        if busy {
            Self::Icon(Icon::Busy)
        } else {
            Self::Text(text)
        }
    }

    fn id(self) -> &'a str {
        match self {
            Self::Text(text) => text,
            Self::Icon(Icon::Minus) => "icon:minus",
            Self::Icon(Icon::Plus) => "icon:plus",
            Self::Icon(Icon::First) => "icon:first",
            Self::Icon(Icon::Previous) => "icon:previous",
            Self::Icon(Icon::Next) => "icon:next",
            Self::Icon(Icon::Last) => "icon:last",
            Self::Icon(Icon::Up) => "icon:up",
            Self::Icon(Icon::Down) => "icon:down",
            Self::Icon(Icon::Play) => "icon:play",
            Self::Icon(Icon::Pause) => "icon:pause",
            Self::Icon(Icon::Close) => "icon:close",
            Self::Icon(Icon::Star) => "icon:star",
            Self::Icon(Icon::StarOutline) => "icon:star-outline",
            Self::Icon(Icon::Busy) => "icon:busy",
        }
    }
}

impl Ui {
    pub fn button<'a>(&mut self, draw: &mut Draw, fonts: &mut Fonts, rect: Rect, label: impl Into<Label<'a>>) {
        self.button_enabled(draw, fonts, rect, label, true);
    }

    pub fn button_enabled<'a>(
        &mut self,
        draw: &mut Draw,
        fonts: &mut Fonts,
        rect: Rect,
        label: impl Into<Label<'a>>,
        enabled: bool,
    ) {
        let label = label.into();
        if label == Label::Icon(Icon::Busy) {
            self.keep_moving();
        }
        let r = self.interact(label.id(), rect, enabled);
        if r.entered {
            crate::audio::play_ui_hover();
        }
        let time = self.time() as f32;
        let look = self.look(fonts);
        let visual = r.visual(enabled);
        let painter = &mut Painter { draw, fonts: fonts.library() };
        match label {
            Label::Text(text) => look.button(painter, rect.into(), Content::Text(text), visual),
            Label::Icon(icon) => {
                let paint = |draw: &mut Draw, area: Area, color: Color| {
                    let center = area.center();
                    paint_icon(draw, icon, (center.x, center.y), area.size().min_element(), color, time);
                };
                look.button(painter, rect.into(), Content::Paint(&paint), visual);
            }
        }
    }
}

fn paint_icon(draw: &mut Draw, icon: Icon, (cx, cy): (f32, f32), size: f32, color: Color, time: f32) {
    let (long, thick) = (size * ICON_LENGTH, size * ICON_THICKNESS);
    let bar = |draw: &mut Draw, x: f32, w: f32, h: f32| {
        draw.rect(Rect::at(x - w / 2.0, cy - h / 2.0, w, h)).radius(thick / 2.0).color(color);
    };
    let half = long / 2.0;
    // Pointing along `(dx, dy)`, a unit step on one axis.
    let chevron = |draw: &mut Draw, (x, y): (f32, f32), (dx, dy): (f32, f32)| {
        let (tx, ty) = (dx * half / 2.0, dy * half / 2.0);
        let (ax, ay) = (-dy * half, dx * half);
        let points = [vec2(x - tx - ax, y - ty - ay), vec2(x + tx, y + ty), vec2(x - tx + ax, y - ty + ay)];
        draw.path(|p| {
            p.move_to(points[0]).line_to(points[1]).line_to(points[2]);
        })
        .stroke(color, thick);
    };
    match icon {
        Icon::Minus => bar(draw, cx, long, thick),
        Icon::Plus => {
            bar(draw, cx, long, thick);
            bar(draw, cx, thick, long);
        }
        Icon::Previous | Icon::Next | Icon::First | Icon::Last => {
            let dir = if matches!(icon, Icon::Next | Icon::Last) { 1.0 } else { -1.0 };
            let edge = matches!(icon, Icon::First | Icon::Last);
            let tip = dir * half / 2.0;
            let to_bar = if edge { tip + 2.0 * thick * dir } else { 0.0 };
            let x = if edge { cx - (to_bar - tip) / 2.0 } else { cx };
            chevron(draw, (x, cy), (dir, 0.0));
            if edge {
                bar(draw, x + to_bar, thick, long);
            }
        }
        Icon::Up | Icon::Down => chevron(draw, (cx, cy), (0.0, if icon == Icon::Down { 1.0 } else { -1.0 })),
        Icon::Play => {
            let left = cx - half * 0.7;
            draw.triangle(vec2(left, cy - half), vec2(left, cy + half), vec2(cx + half, cy)).color(color);
        }
        Icon::Pause => {
            bar(draw, cx - half * 0.45, thick * 1.4, long);
            bar(draw, cx + half * 0.45, thick * 1.4, long);
        }
        Icon::Close => {
            let arm = half * 0.8;
            for dy in [arm, -arm] {
                draw.line_with(vec2(cx - arm, cy - dy), vec2(cx + arm, cy + dy), thick, Cap::Butt).color(color);
            }
        }
        Icon::Star | Icon::StarOutline => {
            let star = draw.star(vec2(cx, cy), 5, half * 0.55, half * 1.2);
            if icon == Icon::StarOutline {
                star.stroke_with(color, super::pointed(thick * 0.7));
            } else {
                star.color(color);
            }
        }
        Icon::Busy => {
            for i in 0..3u8 {
                let phase = time * 5.0 - f32::from(i) * 0.9;
                let alpha = 0.3 + 0.7 * (0.5 + 0.5 * phase.sin());
                draw.circle(vec2(cx + (f32::from(i) - 1.0) * size * 0.2, cy), size * 0.07)
                    .color(color.with_alpha(color.a * alpha));
            }
        }
    }
}
