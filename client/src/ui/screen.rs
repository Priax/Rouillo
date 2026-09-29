use notan::draw::{Draw, DrawShapes};
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

    /// A page header: a band in the section's colour with triangles rising
    /// through it and a line of accent along its foot.
    pub fn header_band(&self, draw: &mut Draw, rect: Rect) {
        let pal = self.palette();
        draw.rect((rect.x, rect.y), (rect.w, rect.h)).color(pal.banner);
        let triangles = Triangles {
            seed: 0xba5d,
            size: rect.h * 0.9,
            density: 0.6,
            speed: 0.03,
            color: pal.accent.with_alpha(0.08),
        };
        triangles.draw_clipped(draw, rect, self.time(), Some(&rect.corners()));
        draw.rect((rect.x, rect.y + rect.h - 2.0), (rect.w, 2.0))
            .color(pal.accent);
    }
}
