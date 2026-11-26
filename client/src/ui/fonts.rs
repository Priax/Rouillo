use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use aero::draw::Draw;
use aero::geom::Color;
use aero::math::Vec2;
pub use aero::text::Fonts;
use aero::text::{TextLayout, TextStyle};

use super::text::Text;

const TEXT: &[u8] = include_bytes!("../../../assets/fonts/Nunito-ExtraBold.ttf");
const DISPLAY: &[u8] = include_bytes!("../../../assets/fonts/SairaCondensed-Bold.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Text,
    Display,
}

/// The game's two families and how their sizes turn into Aero's: a size spans the font's ascent and descent, as the
/// game's sizes always have, rather than its em.
pub struct Faces([Family; 2]);

struct Family {
    name: String,
    em_per_size: f32,
}

static FACES: OnceLock<Faces> = OnceLock::new();

impl Faces {
    /// A size spanning the font's ascent and descent, its lines as tall.
    pub fn style(&self, face: Face, size: f32) -> TextStyle<'_> {
        let family = &self.0[face as usize];
        TextStyle::new().family(&family.name).size(size * family.em_per_size).line_height(1.0 / family.em_per_size)
    }
}

/// The game's faces, known once [`register`] added them.
pub fn faces() -> &'static Faces {
    FACES.get().expect("the game's fonts are registered at startup")
}

/// Adds the game's faces to `fonts`, Aero's own, which the game and its widgets draw with.
pub fn register(fonts: &mut Fonts) {
    let mut family = |data: &'static [u8]| Family {
        name: fonts.add(data).into_iter().next().expect("the bundled fonts are valid"),
        em_per_size: em_per_size(data).expect("the bundled fonts have metrics"),
    };
    let faces = Faces([family(TEXT), family(DISPLAY)]);
    let _ = FACES.set(faces);
}

/// Text the game's way, in its two faces and its sizes, on Aero's fonts.
pub trait Typeset {
    /// One line of text, drawn once the builder goes: at the top left of its position until told otherwise.
    fn write<'a>(&'a mut self, draw: &'a mut Draw, face: Face, text: &'a str) -> Text<'a>;
    /// `text` laid out, kept while it is shown every frame.
    fn laid(&mut self, face: Face, text: &str, size: f32) -> Arc<TextLayout>;
    /// Draws `text` with its top left corner at `at`.
    fn draw_line(&mut self, draw: &mut Draw, line: (Face, &str, f32), at: Vec2, color: Color);
    fn width(&mut self, face: Face, text: &str, size: f32) -> f32;
    /// `text`, cut with an ellipsis if wider than `max`.
    fn fit<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> Cow<'a, str>;
    /// `text` in lines no wider than `max`, broken between words when it can be, keeping its line breaks.
    fn wrap<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> Vec<&'a str>;
}

impl Typeset for Fonts {
    fn write<'a>(&'a mut self, draw: &'a mut Draw, face: Face, text: &'a str) -> Text<'a> {
        Text::new(draw, self, face, text)
    }

    fn laid(&mut self, face: Face, text: &str, size: f32) -> Arc<TextLayout> {
        self.layout_kept(text, &faces().style(face, size))
    }

    fn draw_line(&mut self, draw: &mut Draw, (face, text, size): (Face, &str, f32), at: Vec2, color: Color) {
        let laid = self.laid(face, text, size);
        draw.text_colored(self, &laid, at, color);
    }

    fn width(&mut self, face: Face, text: &str, size: f32) -> f32 {
        advances(self, face, text, size).last().map_or(0.0, |&(_, w)| w)
    }

    fn fit<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> Cow<'a, str> {
        let advances = advances(self, face, text, size);
        if advances.last().is_none_or(|&(_, w)| w <= max) {
            return Cow::Borrowed(text);
        }
        let room = max - self.width(face, "…", size);
        let end = advances.iter().take_while(|&&(_, w)| w <= room).last().map_or(0, |&(end, _)| end);
        Cow::Owned(format!("{}…", text[..end].trim_end()))
    }

    fn wrap<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> Vec<&'a str> {
        let mut lines = Vec::new();
        for mut rest in text.split('\n') {
            loop {
                let (line, after) = rest.split_at(line_end(self, face, rest, size, max));
                lines.push(line);
                rest = after;
                if rest.is_empty() {
                    break;
                }
            }
        }
        lines
    }
}

/// Where each cluster of `text` ends, and how wide the text is up to there.
fn advances(fonts: &mut Fonts, face: Face, text: &str, size: f32) -> Vec<(usize, f32)> {
    fonts.laid(face, text, size).clusters().map(|c| (c.bytes.end, c.x + c.advance)).collect()
}

fn line_end(fonts: &mut Fonts, face: Face, text: &str, size: f32, max: f32) -> usize {
    let first = text.chars().next().map_or(0, char::len_utf8);
    let advances = advances(fonts, face, text, size);
    let fits = advances.iter().take_while(|&&(_, w)| w <= max).last().map_or(first, |&(end, _)| end);
    if fits == text.len() {
        return fits;
    }
    text[..fits].rfind(' ').map_or(fits, |space| space + 1)
}

/// The em of a font whose ascent and descent span 1.
fn em_per_size(data: &[u8]) -> Result<f32, String> {
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::new(data).map_err(|e| e.to_string())?;
    let metrics = font.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
    let height = metrics.ascent - metrics.descent;
    if height <= 0.0 {
        return Err("a font without ascent nor descent".into());
    }
    Ok(f32::from(metrics.units_per_em) / height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        let mut fonts = Fonts::new();
        register(&mut fonts);
        fonts
    }

    fn width(text: &str, size: f32) -> f32 {
        fonts().width(Face::Text, text, size)
    }

    #[test]
    fn fitting_text_is_left_alone() {
        assert_eq!(fonts().fit(Face::Text, "Salon", 20.0, 500.0), "Salon");
    }

    #[test]
    fn overlong_text_is_cut_to_fit_with_an_ellipsis() {
        let worst = "W".repeat(24);
        let fitted = fonts().fit(Face::Text, &worst, 48.0, 400.0);
        assert!(fitted.ends_with('…') && fitted.len() < worst.len() + 3);
        assert!(width(&fitted, 48.0) <= 400.0, "{} wide", width(&fitted, 48.0));
        assert_eq!(fonts().fit(Face::Text, "Pas du tout la place", 20.0, 1.0), "…");
    }

    #[test]
    fn wrapping_breaks_between_words_and_loses_nothing() {
        let text = "une bio assez longue pour ne pas tenir sur une seule ligne";
        let lines = fonts().wrap(Face::Text, text, 20.0, 200.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), text);
        for line in &lines {
            assert!(width(line, 20.0) <= 200.0, "{line:?}");
        }
        assert!(lines[..lines.len() - 1].iter().all(|l| l.ends_with(' ')));
    }

    #[test]
    fn wrapping_keeps_line_breaks_and_cuts_overlong_words() {
        assert_eq!(fonts().wrap(Face::Text, "a\n\nb\n", 20.0, 200.0), ["a", "", "b", ""]);
        assert_eq!(fonts().wrap(Face::Text, "", 20.0, 200.0), [""]);
        let word = "W".repeat(40);
        let lines = fonts().wrap(Face::Text, &word, 20.0, 200.0);
        assert!(lines.len() > 1 && lines.concat() == word);
        assert!(lines.iter().all(|l| width(l, 20.0) <= 200.0));
        assert_eq!(fonts().wrap(Face::Text, "WW", 20.0, 1.0), ["W", "W"]);
    }

    #[test]
    fn width_grows_with_text_and_size() {
        assert!(width("", 20.0).abs() < f32::EPSILON);
        assert!(width("WWW", 20.0) > width("iii", 20.0));
        let (small, big) = (width("Rouillo", 10.0), width("Rouillo", 20.0));
        assert!((big - 2.0 * small).abs() < 0.01, "width scales with size");
    }
}
