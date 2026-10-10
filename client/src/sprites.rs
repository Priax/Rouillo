use std::f32::consts::TAU;

use notan::draw::{Draw, DrawShapes};
use notan::prelude::Color;
use shared::PuyoType;

use crate::theme::{game, mix};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Awake,
    Blinking,
    Popping,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Shadow,
    Body,
}

#[derive(Clone, Copy)]
pub enum Joint {
    Right,
    Down,
}

#[derive(Clone, Copy)]
pub struct Puyo {
    pub kind: PuyoType,
    pub center: (f32, f32),
    pub radius: f32,
    pub alpha: f32,
    pub squash: f32,
    pub flash: f32,
    pub mood: Mood,
    pub tint: Option<Color>,
}

impl Puyo {
    pub const fn new(kind: PuyoType, center: (f32, f32), radius: f32) -> Self {
        Self {
            kind,
            center,
            radius,
            alpha: 1.0,
            squash: 0.0,
            flash: 0.0,
            mood: Mood::Awake,
            tint: None,
        }
    }

    fn radii(&self) -> (f32, f32) {
        (
            self.radius * (1.0 + self.squash * 0.6),
            self.radius * (1.0 - self.squash),
        )
    }

    fn body_center(&self) -> (f32, f32) {
        let (_, ry) = self.radii();
        (self.center.0, self.center.1 + self.radius - ry)
    }

    fn base(&self) -> Color {
        mix(self.tint.unwrap_or(game::puyo(self.kind)), Color::WHITE, self.flash)
    }
}

fn shade(color: Color) -> Color {
    mix(color, Color::BLACK, 0.42)
}

const OUTLINE_POINTS: usize = 14;

fn lumps(kind: PuyoType) -> (f32, f32) {
    match kind {
        PuyoType::Red => (0.3, 2.1),
        PuyoType::Blue => (1.7, 0.4),
        PuyoType::Yellow => (2.9, 4.0),
        PuyoType::Green => (4.2, 1.2),
        PuyoType::Purple => (5.4, 3.1),
        PuyoType::Garbage => (0.9, 5.2),
    }
}

fn potato(draw: &mut Draw, p: &Puyo, (sx, sy): (f32, f32), dy: f32, color: Color) {
    let (rx, ry) = p.radii();
    let (cx, cy) = p.body_center();
    let (a, b) = lumps(p.kind);
    let points: Vec<(f32, f32)> = (0..OUTLINE_POINTS)
        .map(|i| {
            let t = i as f32 / OUTLINE_POINTS as f32 * TAU;
            let wobble = 1.0 + 0.045 * (2.0 * t + a).sin() + 0.03 * (3.0 * t + b).sin();
            let bottom = t.sin().max(0.0);
            (
                cx + t.cos() * rx * sx * wobble * (1.0 + 0.06 * bottom),
                cy + dy + t.sin() * ry * sy * wobble * (1.0 - 0.1 * bottom),
            )
        })
        .collect();
    let mid = |i: usize| {
        let (p, q) = (points[i % OUTLINE_POINTS], points[(i + 1) % OUTLINE_POINTS]);
        ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)
    };
    let mut path = draw.path();
    let start = mid(0);
    path.move_to(start.0, start.1);
    for i in 1..=OUTLINE_POINTS {
        let to = mid(i);
        path.quadratic_bezier_to(points[i % OUTLINE_POINTS], to);
    }
    path.close().fill().color(color.with_alpha(p.alpha));
}

pub fn layer(draw: &mut Draw, p: &Puyo, layer: Layer) {
    match layer {
        Layer::Shadow => potato(draw, p, (1.0, 1.0), p.radius * 0.06, shade(p.base())),
        Layer::Body => {
            potato(draw, p, (0.94, 0.91), -p.radius * 0.03, p.base());
            let (rx, ry) = p.radii();
            let (cx, cy) = p.body_center();
            draw.ellipse((cx - rx * 0.32, cy - ry * 0.48), (rx * 0.24, ry * 0.13))
                .rotate(-0.5)
                .color(Color::WHITE.with_alpha(0.5 * p.alpha));
        }
    }
}

pub fn face(draw: &mut Draw, p: &Puyo) {
    let (rx, ry) = p.radii();
    let (cx, cy) = p.body_center();
    let garbage = p.kind == PuyoType::Garbage;
    let ink = if garbage {
        game::GARBAGE_INK
    } else {
        mix(shade(game::puyo(p.kind)), Color::BLACK, 0.6)
    }
    .with_alpha(p.alpha);
    let white = Color::WHITE.with_alpha(p.alpha);
    let spread = rx * if garbage { 0.28 } else { 0.27 };
    let eye_y = cy - ry * 0.06;
    for side in [-1.0, 1.0] {
        let ex = cx + side * spread;
        match p.mood {
            Mood::Popping => {
                let w = rx * 0.18;
                draw.path()
                    .move_to(ex - w, eye_y + ry * 0.08)
                    .quadratic_bezier_to((ex, eye_y - ry * 0.18), (ex + w, eye_y + ry * 0.08))
                    .stroke(p.radius * 0.11)
                    .round_cap()
                    .color(ink);
            }
            Mood::Blinking => {
                draw.line((ex - rx * 0.15, eye_y + ry * 0.08), (ex + rx * 0.15, eye_y + ry * 0.08))
                    .width(p.radius * 0.1)
                    .color(ink);
            }
            Mood::Awake if garbage => {
                draw.ellipse((ex, eye_y + ry * 0.08), (rx * 0.1, ry * 0.13)).color(ink);
            }
            Mood::Awake => {
                draw.ellipse((ex, eye_y), (rx * 0.21, ry * 0.31))
                    .rotate(side * 0.18)
                    .color(white);
                let (px, py) = (ex - side * rx * 0.05, eye_y + ry * 0.09);
                draw.ellipse((px, py), (rx * 0.13, ry * 0.2)).color(ink);
                draw.circle(rx * 0.05)
                    .position(px - rx * 0.04, py - ry * 0.08)
                    .color(white);
            }
        }
    }
}

pub fn single(draw: &mut Draw, p: &Puyo) {
    layer(draw, p, Layer::Shadow);
    layer(draw, p, Layer::Body);
    face(draw, p);
}

pub fn bridge(draw: &mut Draw, p: &Puyo, joint: Joint, cell: f32, layer: Layer) {
    let (cx, cy) = p.center;
    let thick = p.radius * 1.5;
    let (color, dy, w) = match layer {
        Layer::Shadow => (shade(p.base()), p.radius * 0.06, thick),
        Layer::Body => (p.base(), 0.0, thick * 0.9),
    };
    let (pos, size) = match joint {
        Joint::Right => ((cx, cy - thick / 2.0 + dy), (cell, w)),
        Joint::Down => ((cx - w / 2.0, cy + dy), (w, cell)),
    };
    draw.rect(pos, size).color(color.with_alpha(p.alpha));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nuisance {
    Small,
    Big,
    Rock,
    Star,
    Moon,
    Crown,
}

impl Nuisance {
    const ALL: [Self; 6] = [Self::Crown, Self::Moon, Self::Star, Self::Rock, Self::Big, Self::Small];

    const fn worth(self) -> u32 {
        match self {
            Self::Small => 1,
            Self::Big => 6,
            Self::Rock => 30,
            Self::Star => 180,
            Self::Moon => 360,
            Self::Crown => 720,
        }
    }

    pub fn tray(mut count: u32, slots: usize) -> Vec<Self> {
        let mut icons = Vec::new();
        for icon in Self::ALL {
            while count >= icon.worth() && icons.len() < slots {
                icons.push(icon);
                count -= icon.worth();
            }
        }
        icons
    }
}

/// The small, big and rock icons take `tint`; `flash` whitens them all.
pub fn nuisance_icon(
    draw: &mut Draw,
    icon: Nuisance,
    (cx, cy): (f32, f32),
    unit: f32,
    backdrop: Color,
    tint: Color,
    flash: f32,
) {
    let lit = |color: Color| mix(color, Color::WHITE, flash);
    let blob = |draw: &mut Draw, center: (f32, f32), radius: f32| {
        let mut puyo = Puyo::new(PuyoType::Garbage, center, radius);
        puyo.tint = Some(tint);
        puyo.flash = flash;
        single(draw, &puyo);
    };
    match icon {
        Nuisance::Small => blob(draw, (cx, cy + unit * 0.15), unit * 0.6),
        Nuisance::Big => blob(draw, (cx, cy), unit * 0.95),
        Nuisance::Rock => {
            let (w, h) = (unit * 1.6, unit * 1.3);
            draw.rect((cx - w / 2.0, cy - h / 2.0 + unit * 0.1), (w, h))
                .corner_radius(unit * 0.35)
                .color(lit(shade(tint)));
            draw.rect((cx - w / 2.0, cy - h / 2.0), (w, h * 0.92))
                .corner_radius(unit * 0.35)
                .color(lit(tint));
            draw.rect((cx - w * 0.3, cy - h * 0.32), (w * 0.3, h * 0.14))
                .corner_radius(unit * 0.07)
                .color(Color::WHITE.with_alpha(0.35));
        }
        Nuisance::Star => {
            draw.star(5, unit * 0.95, unit * 0.45)
                .position(cx, cy + unit * 0.06)
                .color(lit(shade(game::STAR)));
            draw.star(5, unit * 0.9, unit * 0.42)
                .position(cx, cy)
                .color(lit(game::STAR));
        }
        Nuisance::Moon => {
            draw.circle(unit * 0.85).position(cx, cy).color(lit(game::MOON));
            draw.circle(unit * 0.72)
                .position(cx + unit * 0.42, cy - unit * 0.22)
                .color(backdrop);
        }
        Nuisance::Crown => {
            let (w, h) = (unit * 1.7, unit * 1.2);
            let (left, top) = (cx - w / 2.0, cy - h / 2.0);
            draw.path()
                .move_to(left, top + h)
                .line_to(left, top + h * 0.15)
                .line_to(left + w * 0.27, top + h * 0.55)
                .line_to(cx, top)
                .line_to(left + w * 0.73, top + h * 0.55)
                .line_to(left + w, top + h * 0.15)
                .line_to(left + w, top + h)
                .close()
                .fill()
                .color(lit(game::STAR));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_takes_the_biggest_icons_first() {
        use Nuisance::{Big, Crown, Rock, Small, Star};
        assert_eq!(Nuisance::tray(0, 6), vec![]);
        assert_eq!(Nuisance::tray(4, 6), vec![Small; 4]);
        assert_eq!(Nuisance::tray(37, 6), vec![Rock, Big, Small]);
        assert_eq!(Nuisance::tray(30 + 180 + 720, 6), vec![Crown, Star, Rock]);
        assert_eq!(Nuisance::tray(20, 2), vec![Big, Big]);
    }
}
