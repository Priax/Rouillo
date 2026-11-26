use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use aero::draw::Draw;
use aero::geom::Color;
use aero::math::Vec2;
use aero::text::{TextLayout, TextStyle};

const TEXT: &[u8] = include_bytes!("../../../assets/fonts/Nunito-ExtraBold.ttf");
const DISPLAY: &[u8] = include_bytes!("../../../assets/fonts/SairaCondensed-Bold.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Text,
    Display,
}

/// The game's two faces, laid out by Aero. A size spans the font's ascent and descent, as the game's sizes always
/// have, rather than its em.
pub struct Fonts {
    library: aero::text::Fonts,
    faces: Faces,
    kept: HashMap<u64, Kept>,
}

/// The two families and how their sizes turn into Aero's, for whatever draws text the game's way.
#[derive(Clone)]
pub struct Faces(Arc<[Family; 2]>);

struct Family {
    name: String,
    em_per_size: f32,
}

impl Faces {
    /// A size spanning the font's ascent and descent, its lines as tall.
    pub fn style(&self, face: Face, size: f32) -> TextStyle<'_> {
        let family = &self.0[face as usize];
        TextStyle::new().family(&family.name).size(size * family.em_per_size).line_height(1.0 / family.em_per_size)
    }
}

/// A layout kept while it is drawn or measured every frame.
struct Kept {
    face: Face,
    size: f32,
    text: String,
    layout: TextLayout,
    used: bool,
}

impl Fonts {
    pub fn load() -> Result<Self, String> {
        let mut library = aero::text::Fonts::new();
        let mut family = |data: &'static [u8]| -> Result<Family, String> {
            let name = library.add(data).into_iter().next().ok_or("no font in the data")?;
            Ok(Family { name, em_per_size: em_per_size(data)? })
        };
        Ok(Self { faces: Faces(Arc::new([family(TEXT)?, family(DISPLAY)?])), library, kept: HashMap::new() })
    }

    pub fn layout(&mut self, face: Face, text: &str, size: f32) -> &TextLayout {
        layout(&mut self.kept, &mut self.library, &self.faces, (face, text, size))
    }

    /// Drops the layouts this frame did not use.
    pub fn end_frame(&mut self) {
        self.kept.retain(|_, kept| std::mem::replace(&mut kept.used, false));
        self.library.next_frame();
    }

    pub fn faces(&self) -> Faces {
        self.faces.clone()
    }

    /// The fonts themselves, for a theme to draw with.
    pub fn library(&mut self) -> &mut aero::text::Fonts {
        &mut self.library
    }

    /// Draws `text` with its top left corner at `at`.
    pub fn draw(&mut self, draw: &mut Draw, (face, text, size): (Face, &str, f32), at: Vec2, color: Color) {
        let laid = layout(&mut self.kept, &mut self.library, &self.faces, (face, text, size));
        draw.text_colored(&mut self.library, laid, at, color);
    }

    /// Where each cluster of `text` ends, and how wide the text is up to there.
    fn advances(&mut self, face: Face, text: &str, size: f32) -> Vec<(usize, f32)> {
        self.layout(face, text, size).clusters().map(|c| (c.bytes.end, c.x + c.advance)).collect()
    }

    pub fn width(&mut self, face: Face, text: &str, size: f32) -> f32 {
        self.advances(face, text, size).last().map_or(0.0, |&(_, w)| w)
    }

    /// `text`, cut with an ellipsis if wider than `max`.
    pub fn fit<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> Cow<'a, str> {
        let advances = self.advances(face, text, size);
        if advances.last().is_none_or(|&(_, w)| w <= max) {
            return Cow::Borrowed(text);
        }
        let room = max - self.width(face, "…", size);
        let end = advances.iter().take_while(|&&(_, w)| w <= room).last().map_or(0, |&(end, _)| end);
        Cow::Owned(format!("{}…", text[..end].trim_end()))
    }

    /// The end of `text` that fits in `max`.
    pub fn tail<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> &'a str {
        let advances = self.advances(face, text, size);
        let total = advances.last().map_or(0.0, |&(_, w)| w);
        let starts = std::iter::once((0, 0.0)).chain(advances.iter().copied());
        starts
            .filter(|&(start, _)| start < text.len())
            .find(|&(_, x)| total - x <= max)
            .map_or("", |(start, _)| &text[start..])
    }

    /// The start of `text` that fits in `max`.
    pub fn head<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> &'a str {
        let advances = self.advances(face, text, size);
        let end = advances.iter().take_while(|&&(_, w)| w <= max).last().map_or(0, |&(end, _)| end);
        &text[..end]
    }

    /// The gap between clusters nearest to `x`, as a byte index.
    pub fn index_at(&mut self, face: Face, text: &str, size: f32, x: f32) -> usize {
        let mut before = (0, 0.0);
        for (end, width) in self.advances(face, text, size) {
            if x < f32::midpoint(before.1, width) {
                break;
            }
            before = (end, width);
        }
        before.0
    }

    /// `text` in lines no wider than `max`, broken between words when it can be, keeping its line breaks.
    pub fn wrap<'a>(&mut self, face: Face, text: &'a str, size: f32, max: f32) -> Vec<&'a str> {
        let mut lines = Vec::new();
        for mut rest in text.split('\n') {
            loop {
                let (line, after) = rest.split_at(self.line_end(face, rest, size, max));
                lines.push(line);
                rest = after;
                if rest.is_empty() {
                    break;
                }
            }
        }
        lines
    }

    fn line_end(&mut self, face: Face, text: &str, size: f32, max: f32) -> usize {
        let first = text.chars().next().map_or(0, char::len_utf8);
        let advances = self.advances(face, text, size);
        let fits = advances.iter().take_while(|&&(_, w)| w <= max).last().map_or(first, |&(end, _)| end);
        if fits == text.len() {
            return fits;
        }
        text[..fits].rfind(' ').map_or(fits, |space| space + 1)
    }
}

fn layout<'a>(
    kept: &'a mut HashMap<u64, Kept>,
    library: &mut aero::text::Fonts,
    faces: &Faces,
    (face, text, size): (Face, &str, f32),
) -> &'a TextLayout {
    let mut hasher = std::hash::DefaultHasher::new();
    (face, size.to_bits(), text).hash(&mut hasher);
    let same = |k: &Kept| k.face == face && k.size.to_bits() == size.to_bits() && k.text == text;
    let entry = kept.entry(hasher.finish());
    let kept = match entry {
        Entry::Occupied(found) if same(found.get()) => found.into_mut(),
        entry => {
            let style = faces.style(face, size);
            let made = Kept { face, size, text: text.to_owned(), layout: library.layout(text, &style), used: false };
            match entry {
                Entry::Occupied(mut taken) => {
                    taken.insert(made);
                    taken.into_mut()
                }
                Entry::Vacant(free) => free.insert(made),
            }
        }
    };
    kept.used = true;
    &kept.layout
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
        Fonts::load().expect("bundled fonts")
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
    fn the_tail_is_the_end_that_fits() {
        assert_eq!(fonts().tail(Face::Text, "Salon", 20.0, 500.0), "Salon");
        let long = "abcdefghij".repeat(10);
        let end = fonts().tail(Face::Text, &long, 20.0, 200.0);
        assert!(long.ends_with(end) && end.len() < long.len() && !end.is_empty());
        assert!(width(end, 20.0) <= 200.0);
        assert_eq!(fonts().tail(Face::Text, "Pas la place", 20.0, 1.0), "");
    }

    #[test]
    fn the_head_is_the_start_that_fits() {
        assert_eq!(fonts().head(Face::Text, "Salon", 20.0, 500.0), "Salon");
        let long = "abcdefghij".repeat(10);
        let start = fonts().head(Face::Text, &long, 20.0, 200.0);
        assert!(long.starts_with(start) && start.len() < long.len() && !start.is_empty());
        assert!(width(start, 20.0) <= 200.0);
        assert_eq!(fonts().head(Face::Text, "Pas la place", 20.0, 1.0), "");
    }

    #[test]
    fn a_point_falls_on_the_nearest_gap_between_characters() {
        let text = "héWi";
        let gaps = [0, 1, 3, 4, 5].map(|end| (end, width(&text[..end], 20.0)));
        for (end, x) in gaps {
            assert_eq!(fonts().index_at(Face::Text, text, 20.0, x + 0.4), end);
            assert_eq!(fonts().index_at(Face::Text, text, 20.0, x - 0.4), end);
        }
        assert_eq!(fonts().index_at(Face::Text, text, 20.0, -50.0), 0);
        assert_eq!(fonts().index_at(Face::Text, text, 20.0, 5000.0), text.len());
        assert_eq!(fonts().index_at(Face::Text, "", 20.0, 10.0), 0);
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
