use notan::draw::{Draw, DrawTextSection, DrawTransform, Font};
use notan::prelude::Color;

/// Text drawn at the resolution it is shown at. The canvas is scaled to fit
/// the window, and notan rasterises glyphs at their nominal size before any
/// transform, so scaled text would be a stretched, blurry bitmap. This builder
/// rasterises at the on-screen size instead and shrinks the result back by the
/// same factor around its anchor, which the canvas then scales up again.
pub trait SharpText {
    fn sharp_text<'a>(&'a mut self, font: &'a Font, text: &'a str) -> Text<'a>;
}

impl SharpText for Draw {
    fn sharp_text<'a>(&'a mut self, font: &'a Font, text: &'a str) -> Text<'a> {
        Text {
            draw: self,
            font,
            text,
            pos: (0.0, 0.0),
            size: 16.0,
            color: Color::WHITE,
            h: HAlign::Left,
            middle: false,
        }
    }
}

#[derive(Clone, Copy)]
enum HAlign {
    Left,
    Center,
    Right,
}

pub struct Text<'a> {
    draw: &'a mut Draw,
    font: &'a Font,
    text: &'a str,
    pos: (f32, f32),
    size: f32,
    color: Color,
    h: HAlign,
    middle: bool,
}

impl Text<'_> {
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

    pub fn v_align_middle(&mut self) -> &mut Self {
        self.middle = true;
        self
    }
}

impl Drop for Text<'_> {
    fn drop(&mut self) {
        let scale = self.draw.transform().matrix().x_axis.x.abs().max(f32::EPSILON);
        let raster = raster_size(self.size, scale);
        let (pos, h, middle) = (self.pos, self.h, self.middle);
        let mut t = self.draw.text(self.font, self.text);
        t.position(pos.0, pos.1).size(raster).color(self.color);
        match h {
            HAlign::Left => {}
            HAlign::Center => {
                t.h_align_center();
            }
            HAlign::Right => {
                t.h_align_right();
            }
        }
        if middle {
            t.v_align_middle();
        }
        let shrink = self.size / raster;
        if (shrink - 1.0).abs() > f32::EPSILON {
            t.scale_from(pos, (shrink, shrink));
        }
    }
}

/// The on-screen size, rounded to half a pixel so an animated scale reuses a
/// handful of rasterised sizes instead of filling the glyph cache.
fn raster_size(size: f32, scale: f32) -> f32 {
    ((size * scale * 2.0).round() / 2.0).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_its_on_screen_size() {
        for (size, scale) in [(18.0, 1.0), (18.0, 1.35), (24.0, 1.37), (15.0, 0.75)] {
            let raster = raster_size(size, scale);
            let shown = raster * (size / raster) * scale;
            assert!((shown - size * scale).abs() < 1e-3, "{size} at {scale}");
            assert!((raster - size * scale).abs() <= 0.25);
        }
    }

    #[test]
    fn a_hover_animation_reuses_few_sizes() {
        let sizes: std::collections::HashSet<u32> = (0..=30)
            .map(|i| raster_size(26.0, 1.35 * (1.0 + 0.03 * i as f32 / 30.0)).to_bits())
            .collect();
        assert!(sizes.len() <= 3, "{} sizes", sizes.len());
    }
}
