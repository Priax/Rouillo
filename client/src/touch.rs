use std::f32::consts::{FRAC_PI_2, PI};

use aero::draw::Draw;
use aero::geom::Color;
use aero::input::{Finger, Input};
use aero::math::{vec2, Vec2};
use aero::path::Cap;

use crate::controls::{Action, ACTIONS};
use crate::theme;
use crate::ui::{self, Face, Fonts, Rect, Typeset, View};

const SIZE: f32 = 120.0;
const GAP: f32 = 14.0;
const MARGIN: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Button {
    Act(Action),
    Pause,
}

fn layout(view: View) -> [(Button, Rect); 7] {
    let (w, h) = view.size();
    let low = h - MARGIN - SIZE;
    let high = low - SIZE - GAP;
    let at = |x: f32, y: f32| Rect::at(x, y, SIZE, SIZE);
    [
        (Button::Act(Action::Left), at(MARGIN, high)),
        (Button::Act(Action::Right), at(MARGIN + 2.0 * (SIZE + GAP), high)),
        (Button::Act(Action::SoftDrop), at(MARGIN + SIZE + GAP, low)),
        (Button::Act(Action::RotateCcw), at(w - MARGIN - 3.0 * SIZE - 2.0 * GAP, high)),
        (Button::Act(Action::RotateCw), at(w - MARGIN - SIZE, high)),
        (Button::Act(Action::HardDrop), at(w - MARGIN - 2.0 * SIZE - GAP, low)),
        (Button::Pause, Rect::at(w - MARGIN - 96.0, MARGIN, 96.0, 72.0)),
    ]
}

/// The on-screen pad shown in games once the player has touched the screen.
#[derive(Default)]
pub struct TouchPad {
    pub active: bool,
    held: [bool; ACTIONS],
}

impl TouchPad {
    /// Returns the actions held by fingers and whether pause was tapped.
    pub fn read(&mut self, input: &Input, view: View, in_game: bool) -> ([bool; ACTIONS], bool) {
        let fingers = input.fingers();
        if fingers.iter().any(|f| f.new) {
            self.active = true;
        }
        let mut held = [false; ACTIONS];
        let mut pause = false;
        if self.active && in_game {
            for (button, rect) in layout(view) {
                let on = |f: &Finger| rect.contains(aero::math::vec2(f.at.x / view.scale, f.at.y / view.scale));
                match button {
                    Button::Act(action) => held[action as usize] = fingers.iter().filter(|f| !f.lifted).any(on),
                    Button::Pause => pause = fingers.iter().filter(|f| f.new).any(on),
                }
            }
        }
        self.held = held;
        (held, pause)
    }

    pub fn draw(&self, draw: &mut Draw, fonts: &mut Fonts, view: View) {
        if !self.active {
            return;
        }
        for (button, rect) in layout(view) {
            let down = matches!(button, Button::Act(a) if self.held[a as usize]);
            let (cx, cy) = (rect.x() + rect.w() / 2.0, rect.y() + rect.h() / 2.0);
            let fill = Color::WHITE.with_alpha(if down { 0.32 } else { 0.12 });
            let ink = Color::WHITE.with_alpha(if down { 0.95 } else { 0.7 });
            match button {
                Button::Pause => {
                    draw.rect(rect).radius(16.0).color(fill);
                    fonts.write(draw, Face::Display, "II").center(cx, cy).size(theme::size::TITLE).color(ink);
                }
                Button::Act(action) => {
                    draw.circle(vec2(cx, cy), rect.w() / 2.0).color(fill);
                    icon(draw, action, cx, cy, rect.w() * 0.22, ink);
                }
            }
        }
    }
}

fn arrow(draw: &mut Draw, cx: f32, cy: f32, r: f32, angle: f32, color: Color) {
    let point = |a: f32, d: f32| (cx + d * a.cos(), cy + d * a.sin());
    draw.triangle(Vec2::from(point(angle, r)), Vec2::from(point(angle + 2.4, r)), Vec2::from(point(angle - 2.4, r)))
        .color(color);
}

fn curl(draw: &mut Draw, cx: f32, cy: f32, r: f32, clockwise: bool, color: Color) {
    let (start, sweep) = (-PI * 0.75, PI * 1.5);
    let dir = if clockwise { 1.0 } else { -1.0 };
    let at = |t: f32| {
        let a = -FRAC_PI_2 + dir * (start + sweep * t);
        (cx + r * a.cos(), cy + r * a.sin())
    };
    draw.path(|path| {
        path.move_to(Vec2::from(at(0.0)));
        for i in 1..=24 {
            path.line_to(Vec2::from(at(i as f32 / 24.0)));
        }
    })
    .stroke_with(color, ui::pointed(7.0).cap(Cap::Round));
    let end = -FRAC_PI_2 + dir * (start + sweep);
    let (ex, ey) = at(1.0);
    arrow(draw, ex, ey, r * 0.55, end + dir * FRAC_PI_2, color);
}

fn icon(draw: &mut Draw, action: Action, cx: f32, cy: f32, r: f32, color: Color) {
    match action {
        Action::Left => arrow(draw, cx, cy, r * 1.2, PI, color),
        Action::Right => arrow(draw, cx, cy, r * 1.2, 0.0, color),
        Action::SoftDrop => arrow(draw, cx, cy, r * 1.2, FRAC_PI_2, color),
        Action::HardDrop => {
            arrow(draw, cx, cy - r * 0.55, r, FRAC_PI_2, color);
            arrow(draw, cx, cy + r * 0.55, r, FRAC_PI_2, color);
        }
        Action::RotateCw => curl(draw, cx, cy, r, true, color),
        Action::RotateCcw => curl(draw, cx, cy, r, false, color),
        Action::Strategy | Action::NextTarget => {}
    }
}

/// A game needs a landscape window: a phone held upright gets a request to
/// turn instead.
pub fn draw_turn_hint(draw: &mut Draw, fonts: &mut Fonts, view: View) {
    let (w, h) = view.size();
    draw.rect(Rect::at(0.0, 0.0, w, h)).color(Color::srgb(0.07, 0.07, 0.09));
    fonts
        .write(draw, Face::Display, "Tournez votre téléphone")
        .center(w / 2.0, h / 2.0)
        .size(theme::size::TITLE)
        .color(Color::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_never_overlap_and_stay_on_screen() {
        for view in [View::fit(1280.0, 800.0), View::fit(844.0, 390.0), View::fit(2400.0, 1080.0)] {
            let rects = layout(view).map(|(_, r)| r);
            for (i, a) in rects.iter().enumerate() {
                assert!(a.x() >= 0.0 && a.y() >= 0.0 && a.x() + a.w() <= view.w && a.y() + a.h() <= view.h);
                for b in &rects[i + 1..] {
                    let apart = a.x() + a.w() <= b.x()
                        || b.x() + b.w() <= a.x()
                        || a.y() + a.h() <= b.y()
                        || b.y() + b.h() <= a.y();
                    assert!(apart, "{a:?} overlaps {b:?}");
                }
            }
        }
    }
}
