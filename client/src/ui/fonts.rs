use std::borrow::Cow;

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont as _};
use notan::draw::{CreateFont, Font};
use notan::prelude::Graphics;

const TEXT: &[u8] = include_bytes!("../../../assets/fonts/Nunito-ExtraBold.ttf");
const DISPLAY: &[u8] = include_bytes!("../../../assets/fonts/SairaCondensed-Bold.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Text,
    Display,
}

pub struct Fonts {
    pub text: Font,
    pub display: Font,
    text_metrics: FontRef<'static>,
    display_metrics: FontRef<'static>,
}

impl Fonts {
    pub fn load(gfx: &mut Graphics) -> Result<Self, String> {
        let metrics = |bytes| FontRef::try_from_slice(bytes).map_err(|e| e.to_string());
        Ok(Self {
            text: gfx.create_font(TEXT)?,
            display: gfx.create_font(DISPLAY)?,
            text_metrics: metrics(TEXT)?,
            display_metrics: metrics(DISPLAY)?,
        })
    }

    /// How wide `text` is at `size`, measured the way notan lays it out:
    /// each glyph's advance plus the kerning between neighbours.
    pub fn width(&self, face: Face, text: &str, size: f32) -> f32 {
        advances(self.metrics(face), text, size).last().map_or(0.0, |(_, w)| w)
    }

    fn metrics(&self, face: Face) -> &FontRef<'static> {
        match face {
            Face::Text => &self.text_metrics,
            Face::Display => &self.display_metrics,
        }
    }

    /// `text` as is if it fits in `max` at `size`, else as much of it as fits
    /// followed by an ellipsis.
    pub fn fit<'a>(&self, face: Face, text: &'a str, size: f32, max: f32) -> Cow<'a, str> {
        fit(self.metrics(face), text, size, max)
    }
}

fn fit<'a>(font: &FontRef<'static>, text: &'a str, size: f32, max: f32) -> Cow<'a, str> {
    let width = |t: &str| advances(font, t, size).last().map_or(0.0, |(_, w)| w);
    if width(text) <= max {
        return Cow::Borrowed(text);
    }
    let room = max - width("…");
    let end = advances(font, text, size)
        .take_while(|&(_, w)| w <= room)
        .last()
        .map_or(0, |(end, _)| end);
    Cow::Owned(format!("{}…", text[..end].trim_end()))
}

/// For each character of `text`: the byte index just past it, and the width
/// of the text up to there, laid out the way notan does it (each glyph's
/// advance plus the kerning between neighbours).
fn advances<'a>(font: &'a FontRef<'static>, text: &'a str, size: f32) -> impl Iterator<Item = (usize, f32)> + 'a {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut width = 0.0;
    let mut previous = None;
    text.char_indices().map(move |(i, c)| {
        let id = scaled.glyph_id(c);
        if let Some(prev) = previous {
            width += scaled.kern(prev, id);
        }
        width += scaled.h_advance(id);
        previous = Some(id);
        (i + c.len_utf8(), width)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> FontRef<'static> {
        FontRef::try_from_slice(TEXT).expect("bundled font")
    }

    fn width(text: &str, size: f32) -> f32 {
        advances(&font(), text, size).last().map_or(0.0, |(_, w)| w)
    }

    #[test]
    fn fitting_text_is_left_alone() {
        assert_eq!(fit(&font(), "Salon", 20.0, 500.0), "Salon");
    }

    #[test]
    fn overlong_text_is_cut_to_fit_with_an_ellipsis() {
        let worst = "W".repeat(24);
        let fitted = fit(&font(), &worst, 48.0, 400.0);
        assert!(fitted.ends_with('…') && fitted.len() < worst.len() + 3);
        assert!(width(&fitted, 48.0) <= 400.0, "{} wide", width(&fitted, 48.0));
        assert_eq!(fit(&font(), "Pas du tout la place", 20.0, 1.0), "…");
    }

    #[test]
    fn width_grows_with_text_and_size() {
        assert!(width("", 20.0).abs() < f32::EPSILON);
        assert!(width("WWW", 20.0) > width("iii", 20.0));
        let (small, big) = (width("Rouillo", 10.0), width("Rouillo", 20.0));
        assert!((big - 2.0 * small).abs() < 0.01, "width scales with size");
    }
}
