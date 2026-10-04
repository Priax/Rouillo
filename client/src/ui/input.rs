use std::borrow::Cow;
use std::cell::Cell;
use std::ops::Deref;

use super::EditKeys;

const UNDO_STEPS: usize = 100;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Change {
    #[default]
    Other,
    Typing,
    Erasing,
}

/// What a field accepts from the keyboard, set where the field is made.
#[derive(Clone, Copy)]
struct Rules {
    max_chars: usize,
    accepts: fn(char) -> bool,
    lines: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            max_chars: usize::MAX,
            accepts: |_| true,
            lines: false,
        }
    }
}

#[derive(Default)]
pub struct TextInput {
    text: String,
    caret: usize,
    anchor: Option<usize>,
    pub(super) dragging: bool,
    masked: bool,
    rules: Rules,
    scroll: Cell<usize>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    last: Change,
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

    pub const fn max_chars(mut self, max: usize) -> Self {
        self.rules.max_chars = max;
        self
    }

    pub const fn only(mut self, accepts: fn(char) -> bool) -> Self {
        self.rules.accepts = accepts;
        self
    }

    /// Keeps line breaks, which a one-line field turns into spaces.
    pub const fn multiline(mut self) -> Self {
        self.rules.lines = true;
        self
    }

    /// Puts in what the player typed or pasted, as far as the field takes it:
    /// no control characters, no leading space in a visible field, and no
    /// more than its length.
    pub fn type_char(&mut self, c: char) {
        let c = match c {
            '\n' if self.rules.lines => '\n',
            '\t' | '\n' => ' ',
            c if c.is_control() => return,
            c => c,
        };
        if !(self.rules.accepts)(c) || (!self.masked && c.is_whitespace() && self.text.is_empty()) {
            return;
        }
        let replaced = self.selection().map_or(0, |(a, b)| self.text[a..b].chars().count());
        if self.text.chars().count() - replaced >= self.rules.max_chars {
            return;
        }
        self.insert(c);
    }

    pub const fn caret(&self) -> usize {
        self.caret
    }

    pub fn set_caret(&mut self, at: usize) {
        self.caret = self.text.floor_char_boundary(at);
        self.anchor = None;
        self.last = Change::Other;
    }

    /// Remembers the text before a change. Runs of typing or erasing make
    /// one step each, so undo takes back a word, not a letter.
    fn remember(&mut self, change: Change) {
        if change == Change::Other || change != self.last {
            if self.undo.len() == UNDO_STEPS {
                self.undo.remove(0);
            }
            self.undo.push((self.text.clone(), self.caret));
        }
        self.redo.clear();
        self.last = change;
    }

    fn restore(&mut self, undo: bool) {
        let (from, to) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let Some((text, caret)) = from.pop() else { return };
        to.push((std::mem::replace(&mut self.text, text), self.caret));
        self.caret = caret;
        self.anchor = None;
        self.last = Change::Other;
    }

    pub fn undo(&mut self) {
        self.restore(true);
    }

    pub fn redo(&mut self) {
        self.restore(false);
    }

    /// Moves the caret, keeping the other end of the selection where it was
    /// when `extend` (Shift held, or a drag), dropping the selection if not.
    pub fn move_caret(&mut self, at: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = self.text.floor_char_boundary(at);
        self.last = Change::Other;
    }

    /// The selected byte range, start first, when something is selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor.filter(|&a| a != self.caret)?;
        Some((anchor.min(self.caret), anchor.max(self.caret)))
    }

    /// The selection in the coordinates of the text as drawn.
    pub(super) fn shown_selection(&self) -> Option<(usize, usize)> {
        let (a, b) = self.selection()?;
        if self.masked {
            let stars = |i: usize| self.text[..i].chars().count();
            Some((stars(a), stars(b)))
        } else {
            Some((a, b))
        }
    }

    /// The selected text, never for a masked field.
    pub fn selected_text(&self) -> Option<&str> {
        let (a, b) = self.selection().filter(|_| !self.masked)?;
        Some(&self.text[a..b])
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(a..b, "");
        self.caret = a;
        self.anchor = None;
        true
    }

    pub fn around_caret(&self) -> (&str, &str) {
        self.text.split_at(self.caret)
    }

    pub fn insert(&mut self, c: char) {
        self.remember(if self.selection().is_some() {
            Change::Other
        } else {
            Change::Typing
        });
        self.last = Change::Typing;
        self.delete_selection();
        self.text.insert(self.caret, c);
        self.caret += c.len_utf8();
    }

    #[cfg(test)]
    pub fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            self.insert(c);
        }
    }

    pub fn erase(&mut self) {
        let erasing = if self.selection().is_some() {
            Change::Other
        } else {
            Change::Erasing
        };
        if self.selection().is_some() || self.caret > 0 {
            self.remember(erasing);
        }
        if !self.delete_selection() && self.back() {
            self.text.remove(self.caret);
        }
    }

    pub fn delete(&mut self) {
        if self.selection().is_some() || self.caret < self.text.len() {
            self.remember(Change::Other);
        }
        if !self.delete_selection() && self.caret < self.text.len() {
            self.text.remove(self.caret);
        }
    }

    fn back(&mut self) -> bool {
        let before = self.text[..self.caret].chars().next_back();
        self.caret -= before.map_or(0, char::len_utf8);
        before.is_some()
    }

    #[cfg(test)]
    fn left(&mut self) -> bool {
        self.anchor = None;
        self.last = Change::Other;
        self.back()
    }

    fn step(&mut self, forward: bool, extend: bool) {
        match self.selection() {
            Some((a, b)) if !extend => self.set_caret(if forward { b } else { a }),
            _ => {
                let at = if forward {
                    self.caret + self.text[self.caret..].chars().next().map_or(0, char::len_utf8)
                } else {
                    self.caret - self.text[..self.caret].chars().next_back().map_or(0, char::len_utf8)
                };
                self.move_caret(at, extend);
            }
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
        self.last = Change::Other;
    }

    /// Applies this frame's editing keys. Home and End go to the ends of the
    /// text; a multi-line area handles them itself, per row.
    pub fn edit(&mut self, keys: &EditKeys) {
        self.edit_line(keys);
        if keys.home {
            self.move_caret(0, keys.shift);
        }
        if keys.end {
            self.move_caret(self.text.len(), keys.shift);
        }
    }

    pub(super) fn edit_line(&mut self, keys: &EditKeys) {
        if keys.select_all {
            self.select_all();
        }
        if keys.copy || keys.cut {
            if let Some(text) = self.selected_text() {
                super::clipboard::set(text);
                if keys.cut {
                    self.remember(Change::Other);
                    self.delete_selection();
                }
            }
        }
        if keys.undo {
            self.undo();
        }
        if keys.redo {
            self.redo();
        }
        for _ in 0..keys.erases {
            self.erase();
        }
        if keys.delete.fired() {
            self.delete();
        }
        if keys.left.fired() {
            self.step(false, keys.shift);
        }
        if keys.right.fired() {
            self.step(true, keys.shift);
        }
        super::clipboard::publish(self.selected_text());
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

    pub(super) fn set_caret_shown(&mut self, at: usize, extend: bool) {
        let at = if self.masked {
            self.text.char_indices().nth(at).map_or(self.text.len(), |(i, _)| i)
        } else {
            at
        };
        self.move_caret(at, extend);
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
        field.step(true, false);
        assert_eq!(field.caret(), 3);
        for _ in 0..3 {
            field.left();
        }
        assert_eq!(field.caret(), 0);
        field.step(true, false);
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
        field.set_caret_shown(1, false);
        assert_eq!(field.around_caret(), ("p", "é€"));
        field.set_caret_shown(7, false);
        assert_eq!(field.caret(), field.len());
    }

    #[test]
    fn typing_replaces_the_selection_and_erasing_removes_it() {
        let mut field = input("bonjour");
        field.move_caret(3, false);
        field.move_caret(7, true);
        assert_eq!(field.selected_text(), Some("jour"));
        field.insert('s');
        assert_eq!((&*field, field.caret()), ("bons", 4));
        field.move_caret(0, true);
        field.erase();
        assert_eq!(&*field, "");
    }

    #[test]
    fn delete_removes_after_the_caret() {
        let mut field = input("aé!");
        field.set_caret(1);
        field.delete();
        assert_eq!(&*field, "a!");
        field.set_caret(2);
        field.delete();
        assert_eq!(&*field, "a!", "nothing after the end");
    }

    #[test]
    fn arrows_collapse_a_selection_or_grow_it_with_shift() {
        let mut field = input("abcd");
        field.set_caret(1);
        field.step(true, true);
        field.step(true, true);
        assert_eq!(field.selection(), Some((1, 3)));
        field.step(false, false);
        assert_eq!((field.selection(), field.caret()), (None, 1));
        field.select_all();
        field.step(true, false);
        assert_eq!((field.selection(), field.caret()), (None, 4));
    }

    #[test]
    fn undo_takes_back_a_run_of_typing_then_redo_brings_it_again() {
        let mut field = input("");
        field.insert_str("salut");
        field.left();
        field.insert_str("xy");
        field.erase();
        field.erase();
        field.erase();
        assert_eq!(&*field, "salt");
        field.undo();
        assert_eq!(&*field, "saluxyt", "the erasures were one step");
        field.undo();
        assert_eq!((&*field, field.caret()), ("salut", 4), "then the typing after the move");
        field.undo();
        assert_eq!(&*field, "");
        field.undo();
        assert_eq!(&*field, "", "nothing more to undo");
        field.redo();
        field.redo();
        assert_eq!(&*field, "saluxyt");
        field.insert('!');
        let typed = field.to_string();
        field.redo();
        assert_eq!(*field, *typed, "a new change drops what could be redone");
    }

    #[test]
    fn replacing_a_selection_is_its_own_step() {
        let mut field = input("");
        field.insert_str("abc");
        field.select_all();
        field.insert('z');
        field.insert('y');
        assert_eq!(&*field, "zy");
        field.undo();
        assert_eq!(&*field, "abc");
        field.select_all();
        field.erase();
        field.undo();
        assert_eq!(&*field, "abc");
    }

    #[test]
    fn a_field_takes_only_what_its_rules_allow() {
        let mut digits = TextInput::default().max_chars(3).only(|c| c.is_ascii_digit());
        for c in "1a2 34".chars() {
            digits.type_char(c);
        }
        assert_eq!(&*digits, "123");
        digits.select_all();
        digits.type_char('9');
        assert_eq!(&*digits, "9", "a full field still takes a replacement");

        let mut line = TextInput::default();
        for c in " a\tb\nc\u{7}".chars() {
            line.type_char(c);
        }
        assert_eq!(&*line, "a b c", "no leading space, breaks and tabs turn to spaces");
        let mut bio = TextInput::default().multiline();
        for c in "a\nb".chars() {
            bio.type_char(c);
        }
        assert_eq!(&*bio, "a\nb");
        let mut password = TextInput::masked();
        password.type_char(' ');
        assert_eq!(&*password, " ", "a password may start with a space");
    }

    #[test]
    fn a_masked_field_never_gives_its_text_away() {
        let mut field = TextInput::masked();
        field.insert_str("secret");
        field.select_all();
        assert_eq!(field.selection(), Some((0, 6)));
        assert_eq!(field.selected_text(), None);
        assert_eq!(field.shown_selection(), Some((0, 6)));
    }
}
