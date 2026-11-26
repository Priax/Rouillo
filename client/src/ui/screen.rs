use std::ops::{Deref, DerefMut};

use aero::draw::Draw;
use aero::math::{vec2, Affine2};

use super::{Face, Fonts, Rect, Triangles, Ui};
use crate::theme;

/// How far a new screen slides in from, rightwards.
const SLIDE: f32 = 40.0;

impl Ui {
    /// The canvas a screen draws on: its section's background with triangles
    /// drifting up through it, then the screen's own content, which fades and
    /// slides in while a transition runs. The backdrop stays put under it.
    pub fn screen_canvas<'a, C: Canvas + ?Sized>(&mut self, canvas: &'a mut C) -> ScreenCanvas<'a, C> {
        self.keep_moving();
        let pal = self.palette();
        let view = self.view();
        let draw = canvas.draw();
        draw.background(pal.background);
        let backdrop =
            Triangles { seed: 0x5eed, size: 220.0, density: 0.35, speed: 0.015, color: pal.raised.with_alpha(0.35) };
        backdrop.draw(draw, Rect::at(0.0, 0.0, view.w, view.h), self.time());

        let t = self.transition();
        draw.push_alpha(t);
        draw.push_transform(Affine2::from_translation(vec2((1.0 - t) * SLIDE, 0.0)));
        ScreenCanvas(canvas)
    }

    /// The header band along the top of a page, with its title.
    pub fn page_header(&mut self, draw: &mut Draw, fonts: &mut Fonts, title: &str) {
        self.header_band(draw, Rect::at(0.0, 0.0, self.view().w, theme::HEADER_H));
        fonts
            .write(draw, Face::Display, title)
            .left(60.0, theme::HEADER_H / 2.0)
            .size(theme::size::TITLE)
            .color(self.palette().text);
    }

    /// A page header: a band in the section's colour with triangles rising
    /// through it and a line of accent along its foot.
    pub fn header_band(&mut self, draw: &mut Draw, rect: Rect) {
        self.keep_moving();
        let pal = self.palette();
        draw.rect(rect).color(pal.banner);
        let triangles = Triangles {
            seed: 0xba5d,
            size: rect.h * 0.9,
            density: 0.6,
            speed: 0.03,
            color: pal.accent.with_alpha(0.08),
        };
        triangles.draw_clipped(draw, rect, self.time(), Some(&rect.corners()));
        draw.rect(Rect::at(rect.x, rect.y + rect.h - 2.0, rect.w, 2.0)).color(pal.accent);
    }
}

/// What a screen draws on: the frame itself, or Aero's interface over it.
pub trait Canvas {
    fn draw(&mut self) -> &mut Draw;
}

impl Canvas for Draw {
    fn draw(&mut self) -> &mut Draw {
        self
    }
}

impl Canvas for aero::ui::Ui<'_> {
    fn draw(&mut self) -> &mut Draw {
        self.draw
    }
}

/// The frame while a screen draws on it, as the transition fades and slides it.
pub struct ScreenCanvas<'a, C: Canvas + ?Sized>(&'a mut C);

impl<C: Canvas + ?Sized> Deref for ScreenCanvas<'_, C> {
    type Target = C;

    fn deref(&self) -> &C {
        self.0
    }
}

impl<C: Canvas + ?Sized> DerefMut for ScreenCanvas<'_, C> {
    fn deref_mut(&mut self) -> &mut C {
        self.0
    }
}

impl<C: Canvas + ?Sized> Drop for ScreenCanvas<'_, C> {
    fn drop(&mut self) {
        let draw = self.0.draw();
        draw.pop_transform();
        draw.pop_alpha();
    }
}
