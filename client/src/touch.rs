use std::f32::consts::{FRAC_PI_2, PI};

use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;

use crate::controls::{Action, ACTIONS};
use crate::theme;
use crate::ui::{Fonts, Rect, SharpText, View};

const SIZE: f32 = 120.0;
const GAP: f32 = 14.0;
const MARGIN: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Button {
    Act(Action),
    Pause,
}

fn layout(view: View) -> [(Button, Rect); ACTIONS + 1] {
    let (w, h) = view.size();
    let low = h - MARGIN - SIZE;
    let high = low - SIZE - GAP;
    let at = |x: f32, y: f32| Rect::at(x, y, SIZE, SIZE);
    [
        (Button::Act(Action::Left), at(MARGIN, high)),
        (Button::Act(Action::Right), at(MARGIN + 2.0 * (SIZE + GAP), high)),
        (Button::Act(Action::SoftDrop), at(MARGIN + SIZE + GAP, low)),
        (
            Button::Act(Action::RotateCcw),
            at(w - MARGIN - 3.0 * SIZE - 2.0 * GAP, high),
        ),
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
    pub fn read(&mut self, app: &App, view: View, in_game: bool) -> ([bool; ACTIONS], bool) {
        let touch = &app.touch;
        if !touch.pressed.is_empty() {
            self.active = true;
        }
        let mut held = [false; ACTIONS];
        let mut pause = false;
        if self.active && in_game {
            for (button, rect) in layout(view) {
                let on = |id: &u8| {
                    touch
                        .position(*id)
                        .is_some_and(|(x, y)| rect.contains(x / view.scale, y / view.scale))
                };
                match button {
                    Button::Act(action) => held[action as usize] = touch.down.keys().any(on),
                    Button::Pause => pause = touch.pressed.iter().any(on),
                }
            }
        }
        self.held = held;
        (held, pause)
    }

    pub fn draw(&self, draw: &mut Draw, fonts: &Fonts, view: View) {
        if !self.active {
            return;
        }
        for (button, rect) in layout(view) {
            let down = matches!(button, Button::Act(a) if self.held[a as usize]);
            let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            let fill = Color::WHITE.with_alpha(if down { 0.32 } else { 0.12 });
            let ink = Color::WHITE.with_alpha(if down { 0.95 } else { 0.7 });
            match button {
                Button::Pause => {
                    draw.rect((rect.x, rect.y), (rect.w, rect.h))
                        .corner_radius(16.0)
                        .color(fill);
                    draw.sharp_text(&fonts.display, "II")
                        .position(cx, cy)
                        .size(theme::size::TITLE)
                        .h_align_center()
                        .v_align_middle()
                        .color(ink);
                }
                Button::Act(action) => {
                    draw.circle(rect.w / 2.0).position(cx, cy).color(fill);
                    icon(draw, action, cx, cy, rect.w * 0.22, ink);
                }
            }
        }
    }
}

fn arrow(draw: &mut Draw, cx: f32, cy: f32, r: f32, angle: f32, color: Color) {
    let point = |a: f32, d: f32| (cx + d * a.cos(), cy + d * a.sin());
    draw.triangle(point(angle, r), point(angle + 2.4, r), point(angle - 2.4, r))
        .color(color);
}

fn curl(draw: &mut Draw, cx: f32, cy: f32, r: f32, clockwise: bool, color: Color) {
    let (start, sweep) = (-PI * 0.75, PI * 1.5);
    let dir = if clockwise { 1.0 } else { -1.0 };
    let at = |t: f32| {
        let a = -FRAC_PI_2 + dir * (start + sweep * t);
        (cx + r * a.cos(), cy + r * a.sin())
    };
    {
        let mut path = draw.path();
        let (x, y) = at(0.0);
        path.move_to(x, y);
        for i in 1..=24 {
            let (x, y) = at(i as f32 / 24.0);
            path.line_to(x, y);
        }
        path.stroke(7.0).round_cap().color(color);
    }
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
    }
}

/// Most screens are laid out for a landscape window; on those, a phone held
/// upright gets a request to turn instead.
pub fn draw_turn_hint(draw: &mut Draw, fonts: &Fonts, view: View) {
    let (w, h) = view.size();
    draw.rect((0.0, 0.0), (w, h)).color(Color::from_rgb(0.07, 0.07, 0.09));
    draw.sharp_text(&fonts.display, "Tournez votre téléphone")
        .position(w / 2.0, h / 2.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(Color::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_never_overlap_and_stay_on_screen() {
        for view in [
            View::fit(1280.0, 800.0),
            View::fit(844.0, 390.0),
            View::fit(2400.0, 1080.0),
        ] {
            let rects = layout(view).map(|(_, r)| r);
            for (i, a) in rects.iter().enumerate() {
                assert!(a.x >= 0.0 && a.y >= 0.0 && a.x + a.w <= view.w && a.y + a.h <= view.h);
                for b in &rects[i + 1..] {
                    let apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                    assert!(apart, "{a:?} overlaps {b:?}");
                }
            }
        }
    }
}
