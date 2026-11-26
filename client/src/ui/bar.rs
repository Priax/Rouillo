use aero::draw::Draw;
use aero::geom::{Color, Gradient};
use aero::math::Vec2;
use aero::theme::Visual;

use super::{Face, Fonts, Rect, Triangles, Typeset};
use crate::theme::Palette;

const WIDTH: f32 = 0.56;
const HOVER_WIDTH: f32 = 0.10;
const SLANT: f32 = 0.28;
const GLOW_WIDTH: f32 = 90.0;
const BAND: Color = Color::srgba(0.0, 0.0, 0.0, 0.28);
const TEXT_HEIGHT: f32 = 0.42;

/// A slanted bar across `row`, wider when hovered, with triangles rising through it.
pub(super) fn paint(
    draw: &mut Draw,
    fonts: &mut Fonts,
    (pal, time): (&Palette, f64),
    row: Rect,
    (label, color): (&str, Color),
    visual: Visual,
) {
    let color = if visual.enabled { color } else { pal.disabled };
    draw.rect(row).color(BAND);

    let cx = row.x + row.w / 2.0;
    let bar = Slanted::of(row, visual.hover);
    let slant = bar.slant;

    glow(draw, bar, color.with_alpha(0.45 * visual.hover));
    bar.fill(draw, color);

    let triangles = Triangles {
        seed: seed_of(label),
        size: row.h * 1.2,
        density: 2.5,
        speed: 0.15,
        color: Color::WHITE.with_alpha(0.13),
    };
    let area = Rect::at(bar.left - slant, row.y, bar.right - bar.left + 2.0 * slant, row.h);
    triangles.draw_clipped(draw, area, time, Some(&bar.corners()));

    if visual.flash > 0.0 {
        bar.fill(draw, Color::WHITE.with_alpha(0.3 * visual.flash));
    }
    let shrink = 1.0 - 0.03 * visual.press;
    fonts
        .write(draw, Face::Display, label)
        .center(cx, row.y + row.h / 2.0)
        .size(row.h * TEXT_HEIGHT * shrink)
        .color(if visual.enabled { Color::WHITE } else { pal.text_disabled });
}

/// Where a bar takes the pointer: its middle, as wide as it is drawn hovered or not.
pub(super) fn reach(row: Rect, hovered: bool) -> Rect {
    let bar = Slanted::of(row, f32::from(u8::from(hovered)));
    Rect::at(bar.left, bar.top, bar.right - bar.left, bar.bottom - bar.top)
}

#[derive(Clone, Copy)]
struct Slanted {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    slant: f32,
}

impl Slanted {
    fn of(row: Rect, hover: f32) -> Self {
        let cx = row.x + row.w / 2.0;
        let half = row.w * (WIDTH + HOVER_WIDTH * hover) / 2.0;
        Self { left: cx - half, right: cx + half, top: row.y, bottom: row.y + row.h, slant: row.h * SLANT }
    }

    fn corners(self) -> [(f32, f32); 4] {
        let Self { left, right, top, bottom, slant } = self;
        [(left + slant, top), (right + slant, top), (right - slant, bottom), (left - slant, bottom)]
    }

    fn fill(self, draw: &mut Draw, color: Color) {
        let [a, b, c, d] = self.corners();
        draw.polygon(&[a, b, c, d].map(Vec2::from)).color(color);
    }
}

fn glow(draw: &mut Draw, bar: Slanted, color: Color) {
    if color.a <= 0.0 {
        return;
    }
    let Slanted { left, right, top, bottom, slant } = bar;
    glow_quad(draw, (left + slant, top), (left - slant, bottom), -GLOW_WIDTH, color);
    glow_quad(draw, (right + slant, top), (right - slant, bottom), GLOW_WIDTH, color);
}

/// Fades from the edge `top`-`bottom` to nothing `reach` sideways, across the edge.
fn glow_quad(draw: &mut Draw, top: (f32, f32), bottom: (f32, f32), reach: f32, color: Color) {
    let (top, bottom, shift) = (Vec2::from(top), Vec2::from(bottom), Vec2::new(reach, 0.0));
    let along = (bottom - top).normalize_or_zero();
    let across = shift - along * shift.dot(along);
    let fade = Gradient::linear(top, color, top + across, color.with_alpha(0.0));
    draw.polygon(&[top, bottom, bottom + shift, top + shift]).color(fade);
}

fn seed_of(label: &str) -> u64 {
    label.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}
