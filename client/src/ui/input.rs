use std::borrow::Cow;
use std::cell::Cell;
use std::ops::Deref;

use super::EditKeys;

#[derive(Default)]
pub struct TextInput {
    text: String,
    caret: usize,
    masked: bool,
    scroll: Cell<usize>,
}

impl Deref for TextInput {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl From<String> for TextInput {
    fn from(text: String) -> Self {
        Self {
            caret: text.len(),
            text,
            ..Self::default()
        }
    }
}

impl TextInput {
    pub fn masked() -> Self {
        Self {
            masked: true,
            ..Self::default()
        }
    }

    pub const fn caret(&self) -> usize {
        self.caret
    }

    pub fn set_caret(&mut self, at: usize) {
        self.caret = self.text.floor_char_boundary(at);
    }

    pub fn around_caret(&self) -> (&str, &str) {
        self.text.split_at(self.caret)
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.caret, c);
        self.caret += c.len_utf8();
    }

    pub fn erase(&mut self) {
        if self.left() {
            self.text.remove(self.caret);
        }
    }

    pub fn left(&mut self) -> bool {
        let before = self.text[..self.caret].chars().next_back();
        self.caret -= before.map_or(0, char::len_utf8);
        before.is_some()
    }

    pub fn right(&mut self) {
        self.caret += self.text[self.caret..].chars().next().map_or(0, char::len_utf8);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    pub fn edit(&mut self, keys: &EditKeys) {
        if keys.erase.fired() {
            self.erase();
        }
        if keys.left.fired() {
            self.left();
        }
        if keys.right.fired() {
            self.right();
        }
    }

    /// The text as drawn and the caret's place in it: a masked input shows
    /// one star per character, so its offsets are not those of the text.
    pub(super) fn shown(&self) -> (Cow<'_, str>, usize) {
        if self.masked {
            let before = self.text[..self.caret].chars().count();
            (Cow::Owned("*".repeat(self.text.chars().count())), before)
        } else {
            (Cow::Borrowed(&self.text), self.caret)
        }
    }

    pub(super) fn set_caret_shown(&mut self, at: usize) {
        self.caret = if self.masked {
            self.text.char_indices().nth(at).map_or(self.text.len(), |(i, _)| i)
        } else {
            self.text.floor_char_boundary(at)
        };
    }

    pub(super) fn scroll(&self) -> usize {
        self.scroll.get()
    }

    pub(super) fn set_scroll(&self, scroll: usize) {
        self.scroll.set(scroll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> TextInput {
        TextInput::from(text.to_owned())
    }

    #[test]
    fn typing_and_erasing_happen_at_the_caret() {
        let mut field = input("hé!");
        assert_eq!(field.caret(), 4, "a loaded text is edited from its end");
        field.left();
        field.left();
        field.insert('y');
        assert_eq!(&*field, "hyé!");
        field.erase();
        field.erase();
        assert_eq!((&*field, field.caret()), ("é!", 0));
        field.erase();
        assert_eq!(&*field, "é!", "nothing before the caret to erase");
    }

    #[test]
    fn the_caret_stops_at_both_ends_and_between_characters() {
        let mut field = input("é!");
        field.right();
        assert_eq!(field.caret(), 3);
        for _ in 0..3 {
            field.left();
        }
        assert_eq!(field.caret(), 0);
        field.right();
        assert_eq!(field.caret(), 2, "one character, not one byte");
        field.set_caret(1);
        assert_eq!(field.caret(), 0);
        field.set_caret(99);
        assert_eq!(field.caret(), 3);
        field.clear();
        assert_eq!((&*field, field.caret()), ("", 0));
    }

    #[test]
    fn a_masked_input_shows_stars_and_places_its_caret_among_them() {
        let mut field = TextInput::masked();
        for c in "pé€".chars() {
            field.insert(c);
        }
        field.left();
        assert_eq!(field.shown(), (Cow::Borrowed("***"), 2));
        field.set_caret_shown(1);
        assert_eq!(field.around_caret(), ("p", "é€"));
        field.set_caret_shown(7);
        assert_eq!(field.caret(), field.len());
    }
}
