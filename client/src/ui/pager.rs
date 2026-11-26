use std::hash::Hash;

use aero::draw::Draw;
use aero::input::KeyCode;

use super::{field_clicked, text_field, widgets, EditKeys, Face, Field, Fonts, Gui, Icon, Rect, TextInput, Ui};
use crate::theme;
use crate::tick::Tick;

const PAGE_DIGITS: usize = 4;
const BTN_W: f32 = 44.0;
const BTN_H: f32 = 36.0;
const FIELD_W: f32 = 70.0;
const GAP: f32 = 8.0;
/// The height of the pager row under a list.
pub const PAGER_H: f32 = 52.0;
const OF_W: f32 = 60.0;

/// Page arrows, first and last page, and a field to jump to a page number.
pub struct Pager {
    page: usize,
    input: TextInput,
    focused: bool,
}

struct Layout {
    first: Rect,
    prev: Rect,
    field: Rect,
    of_x: f32,
    next: Rect,
    last: Rect,
}

impl Layout {
    fn new(area: Rect) -> Self {
        let total = 4.0 * BTN_W + FIELD_W + OF_W + 6.0 * GAP;
        let x0 = area.x + (area.w - total) / 2.0;
        let y = area.y + (area.h - BTN_H) / 2.0;
        let at = |x: f32, w: f32| Rect::at(x, y, w, BTN_H);
        let field_x = x0 + 2.0 * (BTN_W + GAP);
        let next_x = field_x + FIELD_W + GAP + OF_W + GAP;
        Self {
            first: at(x0, BTN_W),
            prev: at(x0 + BTN_W + GAP, BTN_W),
            field: at(field_x, FIELD_W),
            of_x: field_x + FIELD_W + GAP,
            next: at(next_x, BTN_W),
            last: at(next_x + BTN_W + GAP, BTN_W),
        }
    }
}

fn page_input(page: usize) -> TextInput {
    TextInput::from(page.to_string()).max_chars(PAGE_DIGITS).only(|c| c.is_ascii_digit())
}

pub fn page_count(items: usize, per_page: usize) -> usize {
    items.div_ceil(per_page.max(1)).max(1)
}

impl Default for Pager {
    fn default() -> Self {
        Self { page: 0, input: page_input(1), focused: false }
    }
}

impl Pager {
    pub const fn page(&self) -> usize {
        self.page
    }

    pub const fn focused(&self) -> bool {
        self.focused
    }

    /// The items of the current page, numbered from the top of the page.
    pub fn shown<'a, T>(&self, items: &'a [T], per_page: usize) -> impl Iterator<Item = (usize, &'a T)> {
        items.iter().skip(self.page * per_page).take(per_page).enumerate()
    }

    pub fn typing(&mut self) -> Option<&mut TextInput> {
        self.focused.then_some(&mut self.input)
    }

    fn set(&mut self, page: usize) {
        self.page = page;
        self.input = page_input(page + 1);
    }

    /// Keeps the page within `pages`, for a list that shrank.
    pub fn clamp(&mut self, pages: usize) {
        let page = self.page.min(pages.max(1) - 1);
        if page != self.page {
            self.set(page);
        }
    }

    /// Moves to `page`, kept within `pages`; returns whether it changed.
    fn go(&mut self, page: usize, pages: usize) -> bool {
        let page = page.min(pages.max(1) - 1);
        let changed = page != self.page;
        self.set(page);
        changed
    }

    /// The page each button leads to: first, previous, next, last.
    fn targets(&self, pages: usize) -> [usize; 4] {
        [0, self.page.saturating_sub(1), self.page + 1, pages.max(1) - 1]
    }

    /// The page field: clicked into or away from, typed in, Enter to go. Returns whether the page changed.
    pub fn update_keys(
        &mut self,
        tick: &Tick,
        ui: &Ui,
        fonts: &mut Fonts,
        keys: &EditKeys,
        area: Rect,
        pages: usize,
    ) -> bool {
        let l = Layout::new(area);
        if field_clicked(ui, fonts, l.field, &mut self.input) {
            self.focused = true;
        } else if ui.clicked(Rect::at(0.0, 0.0, ui.view().w, ui.view().h)) {
            self.focused = false;
        }
        if !self.focused {
            return false;
        }
        self.input.edit(keys);
        if !tick.pressed(KeyCode::Enter) {
            return false;
        }
        self.focused = false;
        let page = self.input.parse::<usize>().unwrap_or(1).max(1) - 1;
        self.go(page, pages)
    }

    /// The field and the buttons of the screens not on Aero yet. Returns whether the page changed.
    pub fn update(
        &mut self,
        tick: &Tick,
        ui: &Ui,
        fonts: &mut Fonts,
        keys: &EditKeys,
        area: Rect,
        pages: usize,
    ) -> bool {
        if self.update_keys(tick, ui, fonts, keys, area, pages) {
            return true;
        }
        let l = Layout::new(area);
        let targets = self.targets(pages);
        match [l.first, l.prev, l.next, l.last].into_iter().zip(targets).find(|&(rect, _)| ui.clicked(rect)) {
            Some((_, page)) => self.go(page, pages),
            None => false,
        }
    }

    pub fn draw(&self, draw: &mut Draw, ui: &mut Ui, fonts: &mut Fonts, area: Rect, pages: usize) {
        let pages = pages.max(1);
        let l = Layout::new(area);
        let back = self.page > 0;
        let forward = self.page + 1 < pages;
        ui.button_enabled(draw, fonts, l.first, Icon::First, back);
        ui.button_enabled(draw, fonts, l.prev, Icon::Previous, back);
        self.paint_field(draw, ui, fonts, (&l, pages));
        ui.button_enabled(draw, fonts, l.next, Icon::Next, forward);
        ui.button_enabled(draw, fonts, l.last, Icon::Last, forward);
    }

    fn paint_field(&self, draw: &mut Draw, ui: &mut Ui, fonts: &mut Fonts, (l, pages): (&Layout, usize)) {
        let field = Field { placeholder: "", input: &self.input, focused: self.focused };
        text_field(draw, ui, fonts, l.field, &field);
        fonts
            .write(draw, Face::Text, &format!("/ {pages}"))
            .left(l.of_x, l.field.y + l.field.h / 2.0)
            .size(theme::size::LABEL)
            .color(ui.palette().text_dim);
    }

    /// Draws the pager on Aero's interface and moves to the page its buttons ask; `key` names it among the widgets.
    /// Returns whether the page changed.
    pub fn show(
        &mut self,
        ui: &mut aero::ui::Ui<'_>,
        gui: &mut Gui,
        key: impl Hash + Copy,
        area: Rect,
        pages: usize,
    ) -> bool {
        let pages = pages.max(1);
        let l = Layout::new(area);
        let (back, forward) = (self.page > 0, self.page + 1 < pages);
        let buttons = [
            (l.first, Icon::First, back),
            (l.prev, Icon::Previous, back),
            (l.next, Icon::Next, forward),
            (l.last, Icon::Last, forward),
        ];
        let targets = self.targets(pages);
        let mut wanted = None;
        for (i, ((rect, icon, enabled), page)) in buttons.into_iter().zip(targets).enumerate() {
            if widgets::button(ui, (key, i), rect, icon, enabled) {
                wanted = Some(page);
            }
        }
        self.paint_field(ui.draw, &mut gui.ui, &mut gui.fonts, (&l, pages));
        wanted.is_some_and(|page| self.go(page, pages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_page_still_counts_and_an_empty_list_has_one_page() {
        assert_eq!(page_count(0, 7), 1);
        assert_eq!(page_count(7, 7), 1);
        assert_eq!(page_count(8, 7), 2);
        assert_eq!(page_count(15, 7), 3);
        assert_eq!(page_count(3, 0), 3, "a page holds at least one item");
    }

    #[test]
    fn a_shrinking_list_pulls_the_page_back() {
        let mut pager = Pager::default();
        pager.set(4);
        pager.clamp(3);
        assert_eq!(pager.page(), 2);
        pager.clamp(5);
        assert_eq!(pager.page(), 2, "a growing list keeps the page");
    }
}
