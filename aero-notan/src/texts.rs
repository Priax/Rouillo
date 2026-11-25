//! Text laid out once and drawn every frame after: notan's drawing names text anew each frame, parley lays it out.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use aero::text::{Fonts, TextLayout, TextStyle};

use crate::graphics::Font;

/// Layouts by what shapes them, kept while drawn every frame.
#[derive(Default)]
pub(crate) struct Texts {
    kept: HashMap<u64, Kept>,
}

struct Kept {
    font: Font,
    size: f32,
    max_width: Option<f32>,
    text: String,
    layout: TextLayout,
    used: bool,
}

impl Texts {
    pub fn layout(
        &mut self,
        fonts: &mut Fonts,
        font: &Font,
        size: f32,
        max_width: Option<f32>,
        text: &str,
    ) -> &TextLayout {
        let mut hasher = std::hash::DefaultHasher::new();
        (font.family(), size.to_bits(), max_width.map(f32::to_bits), text).hash(&mut hasher);
        let same = |k: &Kept| k.font == *font && k.size == size && k.max_width == max_width && k.text == text;
        let kept = match self.kept.entry(hasher.finish()) {
            std::collections::hash_map::Entry::Occupied(entry) if same(entry.get()) => entry.into_mut(),
            entry => {
                let style = TextStyle::new().family(font.family()).size(font.em(size)).line_height(font.line_height());
                let style = max_width.map_or(style, |width| style.max_width(width));
                let layout = fonts.layout(text, &style);
                let kept = Kept { font: font.clone(), size, max_width, text: text.to_owned(), layout, used: false };
                match entry {
                    std::collections::hash_map::Entry::Occupied(mut taken) => {
                        taken.insert(kept);
                        taken.into_mut()
                    }
                    std::collections::hash_map::Entry::Vacant(free) => free.insert(kept),
                }
            }
        };
        kept.used = true;
        &kept.layout
    }

    /// Drops what this frame did not draw.
    pub fn end_frame(&mut self) {
        self.kept.retain(|_, kept| std::mem::replace(&mut kept.used, false));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.kept.len()
    }
}

#[cfg(test)]
mod tests {
    use aero::text::Fonts;

    use super::Texts;
    use crate::graphics::Font;

    #[test]
    fn layouts_are_kept_while_drawn_every_frame() {
        let mut fonts = Fonts::new();
        let font =
            Font::add(&mut fonts, include_bytes!("../../aero/assets/fonts/Nunito-ExtraBold.ttf")).expect("a font");
        let mut texts = Texts::default();
        let width = texts.layout(&mut fonts, &font, 20.0, None, "Rouillo").size().x;
        texts.layout(&mut fonts, &font, 20.0, None, "Jouer");
        texts.end_frame();
        assert_eq!(texts.len(), 2);
        assert_eq!(texts.layout(&mut fonts, &font, 20.0, None, "Rouillo").size().x, width, "the same text, kept");
        assert!(texts.layout(&mut fonts, &font, 30.0, None, "Rouillo").size().x > width, "another size, laid anew");
        texts.end_frame();
        assert_eq!(texts.len(), 2, "what was not drawn this frame went");
        texts.end_frame();
        assert_eq!(texts.len(), 0);
    }
}
