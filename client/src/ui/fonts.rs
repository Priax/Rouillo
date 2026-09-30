use std::borrow::Cow;
use std::ops::Deref;

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
    metrics: Metrics,
}

pub struct Metrics {
    text: FontRef<'static>,
    display: FontRef<'static>,
}

impl Deref for Fonts {
    type Target = Metrics;

    fn deref(&self) -> &Metrics {
        &self.metrics
    }
}

impl Fonts {
    pub fn load(gfx: &mut Graphics) -> Result<Self, String> {
        Ok(Self {
            text: gfx.create_font(TEXT)?,
            display: gfx.create_font(DISPLAY)?,
            metrics: Metrics::load()?,
        })
    }
}

impl Metrics {
    pub fn load() -> Result<Self, String> {
        let metrics = |bytes| FontRef::try_from_slice(bytes).map_err(|e| e.to_string());
        Ok(Self {
            text: metrics(TEXT)?,
            display: metrics(DISPLAY)?,
        })
    }

    pub fn width(&self, face: Face, text: &str, size: f32) -> f32 {
        advances(self.of(face), text, size).last().map_or(0.0, |(_, w)| w)
    }

    const fn of(&self, face: Face) -> &FontRef<'static> {
        match face {
            Face::Text => &self.text,
            Face::Display => &self.display,
        }
    }

    pub fn fit<'a>(&self, face: Face, text: &'a str, size: f32, max: f32) -> Cow<'a, str> {
        fit(self.of(face), text, size, max)
    }

    pub fn tail<'a>(&self, face: Face, text: &'a str, size: f32, max: f32) -> &'a str {
        tail(self.of(face), text, size, max)
    }

    pub fn head<'a>(&self, face: Face, text: &'a str, size: f32, max: f32) -> &'a str {
        head(self.of(face), text, size, max)
    }

    pub fn index_at(&self, face: Face, text: &str, size: f32, x: f32) -> usize {
        index_at(self.of(face), text, size, x)
    }

    pub fn wrap<'a>(&self, face: Face, text: &'a str, size: f32, max: f32) -> Vec<&'a str> {
        wrap(self.of(face), text, size, max)
    }
}

fn tail<'a>(font: &FontRef<'static>, text: &'a str, size: f32, max: f32) -> &'a str {
    let width = |t: &str| advances(font, t, size).last().map_or(0.0, |(_, w)| w);
    text.char_indices()
        .map(|(start, _)| &text[start..])
        .find(|rest| width(rest) <= max)
        .unwrap_or_default()
}

fn head<'a>(font: &FontRef<'static>, text: &'a str, size: f32, max: f32) -> &'a str {
    let end = advances(font, text, size)
        .take_while(|&(_, w)| w <= max)
        .last()
        .map_or(0, |(end, _)| end);
    &text[..end]
}

fn index_at(font: &FontRef<'static>, text: &str, size: f32, x: f32) -> usize {
    let mut before = (0, 0.0);
    for (end, width) in advances(font, text, size) {
        if x < f32::midpoint(before.1, width) {
            break;
        }
        before = (end, width);
    }
    before.0
}

fn wrap<'a>(font: &FontRef<'static>, text: &'a str, size: f32, max: f32) -> Vec<&'a str> {
    let mut lines = Vec::new();
    for mut rest in text.split('\n') {
        loop {
            let (line, after) = rest.split_at(line_end(font, rest, size, max));
            lines.push(line);
            rest = after;
            if rest.is_empty() {
                break;
            }
        }
    }
    lines
}

fn line_end(font: &FontRef<'static>, text: &str, size: f32, max: f32) -> usize {
    let first = text.chars().next().map_or(0, char::len_utf8);
    let fits = advances(font, text, size)
        .take_while(|&(_, w)| w <= max)
        .last()
        .map_or(first, |(end, _)| end);
    if fits == text.len() {
        return fits;
    }
    text[..fits].rfind(' ').map_or(fits, |space| space + 1)
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
    fn the_tail_is_the_end_that_fits() {
        assert_eq!(tail(&font(), "Salon", 20.0, 500.0), "Salon");
        let long = "abcdefghij".repeat(10);
        let end = tail(&font(), &long, 20.0, 200.0);
        assert!(long.ends_with(end) && end.len() < long.len() && !end.is_empty());
        assert!(width(end, 20.0) <= 200.0);
        assert_eq!(tail(&font(), "Pas la place", 20.0, 1.0), "");
    }

    #[test]
    fn the_head_is_the_start_that_fits() {
        assert_eq!(head(&font(), "Salon", 20.0, 500.0), "Salon");
        let long = "abcdefghij".repeat(10);
        let start = head(&font(), &long, 20.0, 200.0);
        assert!(long.starts_with(start) && start.len() < long.len() && !start.is_empty());
        assert!(width(start, 20.0) <= 200.0);
        assert_eq!(head(&font(), "Pas la place", 20.0, 1.0), "");
    }

    #[test]
    fn a_point_falls_on_the_nearest_gap_between_characters() {
        let text = "héWi";
        let gaps = [0, 1, 3, 4, 5].map(|end| (end, width(&text[..end], 20.0)));
        for (end, x) in gaps {
            assert_eq!(index_at(&font(), text, 20.0, x + 0.4), end);
            assert_eq!(index_at(&font(), text, 20.0, x - 0.4), end);
        }
        assert_eq!(index_at(&font(), text, 20.0, -50.0), 0);
        assert_eq!(index_at(&font(), text, 20.0, 5000.0), text.len());
        assert_eq!(index_at(&font(), "", 20.0, 10.0), 0);
    }

    #[test]
    fn wrapping_breaks_between_words_and_loses_nothing() {
        let text = "une bio assez longue pour ne pas tenir sur une seule ligne";
        let lines = wrap(&font(), text, 20.0, 200.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), text);
        for line in &lines {
            assert!(width(line, 20.0) <= 200.0, "{line:?}");
        }
        assert!(lines[..lines.len() - 1].iter().all(|l| l.ends_with(' ')));
    }

    #[test]
    fn wrapping_keeps_line_breaks_and_cuts_overlong_words() {
        assert_eq!(wrap(&font(), "a\n\nb\n", 20.0, 200.0), ["a", "", "b", ""]);
        assert_eq!(wrap(&font(), "", 20.0, 200.0), [""]);
        let word = "W".repeat(40);
        let lines = wrap(&font(), &word, 20.0, 200.0);
        assert!(lines.len() > 1 && lines.concat() == word);
        assert!(lines.iter().all(|l| width(l, 20.0) <= 200.0));
        assert_eq!(wrap(&font(), "WW", 20.0, 1.0), ["W", "W"]);
    }

    #[test]
    fn width_grows_with_text_and_size() {
        assert!(width("", 20.0).abs() < f32::EPSILON);
        assert!(width("WWW", 20.0) > width("iii", 20.0));
        let (small, big) = (width("Rouillo", 10.0), width("Rouillo", 20.0));
        assert!((big - 2.0 * small).abs() < 0.01, "width scales with size");
    }
}
