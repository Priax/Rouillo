use aero::draw::Draw;
use aero::geom::Color;
use aero::math::Vec2;

use super::{Face, Fonts, Typeset};

#[derive(Clone, Copy)]
enum HAlign {
    Left,
    Center,
    Right,
}

pub struct Text<'a> {
    draw: &'a mut Draw,
    fonts: &'a mut Fonts,
    face: Face,
    text: &'a str,
    pos: (f32, f32),
    size: f32,
    color: Color,
    h: HAlign,
    middle: bool,
}

impl<'a> Text<'a> {
    pub(super) fn new(draw: &'a mut Draw, fonts: &'a mut Fonts, face: Face, text: &'a str) -> Self {
        Self {
            draw,
            fonts,
            face,
            text,
            pos: (0.0, 0.0),
            size: 16.0,
            color: Color::WHITE,
            h: HAlign::Left,
            middle: false,
        }
    }

    pub fn position(&mut self, x: f32, y: f32) -> &mut Self {
        self.pos = (x, y);
        self
    }

    pub fn size(&mut self, size: f32) -> &mut Self {
        self.size = size;
        self
    }

    pub fn color(&mut self, color: Color) -> &mut Self {
        self.color = color;
        self
    }

    pub fn h_align_center(&mut self) -> &mut Self {
        self.h = HAlign::Center;
        self
    }

    pub fn h_align_right(&mut self) -> &mut Self {
        self.h = HAlign::Right;
        self
    }

    fn v_align_middle(&mut self) -> &mut Self {
        self.middle = true;
        self
    }

    /// Starts at `x`, centred on `y`.
    pub fn left(&mut self, x: f32, y: f32) -> &mut Self {
        self.position(x, y).v_align_middle()
    }

    /// Centred on `(x, y)`.
    pub fn center(&mut self, x: f32, y: f32) -> &mut Self {
        self.left(x, y).h_align_center()
    }

    /// Ends at `x`, centred on `y`.
    pub fn right(&mut self, x: f32, y: f32) -> &mut Self {
        self.left(x, y).h_align_right()
    }
}

impl Drop for Text<'_> {
    fn drop(&mut self) {
        let line = (self.face, self.text, self.size);
        let shown = self.fonts.laid(line.0, line.1, line.2).size();
        let x = match self.h {
            HAlign::Left => 0.0,
            HAlign::Center => shown.x * 0.5,
            HAlign::Right => shown.x,
        };
        let y = if self.middle { shown.y * 0.5 } else { 0.0 };
        let at = Vec2::new(self.pos.0 - x, self.pos.1 - y);
        self.fonts.draw_line(self.draw, line, at, self.color);
    }
}
