use notan::draw::Draw;
use notan::math::{vec2, Mat3};
use notan::prelude::Graphics;

use super::{Rect, Triangles, Ui};

/// How far a new screen slides in from, rightwards.
const SLIDE: f32 = 40.0;

impl Ui {
    /// The canvas a screen draws on: its section's background with triangles
    /// drifting up through it, then the screen's own content, which fades and
    /// slides in while a transition runs. The backdrop stays put under it.
    pub fn screen_canvas(&self, gfx: &mut Graphics) -> Draw {
        let mut draw = self.canvas(gfx);
        let pal = self.palette();
        let view = self.view();
        draw.clear(pal.background);
        let backdrop = Triangles {
            seed: 0x5eed,
            size: 220.0,
            density: 0.35,
            speed: 0.015,
            color: pal.raised.with_alpha(0.35),
        };
        backdrop.draw(&mut draw, Rect::at(0.0, 0.0, view.w, view.h), self.time());

        let t = self.transition();
        draw.set_alpha(t);
        draw.transform()
            .push(Mat3::from_translation(vec2((1.0 - t) * SLIDE, 0.0)));
        draw
    }
}
