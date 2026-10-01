use notan::draw::Draw;
use notan::prelude::*;

use super::{field_clicked, text_field, EditKeys, Field, Fonts, Rect, SharpText, TextInput, Ui};
use crate::theme;

const PAGE_DIGITS: usize = 4;
const BTN_W: f32 = 44.0;
const BTN_H: f32 = 36.0;
const FIELD_W: f32 = 70.0;
const GAP: f32 = 8.0;
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

pub fn page_count(items: usize, per_page: usize) -> usize {
    items.div_ceil(per_page.max(1)).max(1)
}

impl Default for Pager {
    fn default() -> Self {
        Self {
            page: 0,
            input: "1".to_string().into(),
            focused: false,
        }
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

    pub fn type_char(&mut self, c: char) {
        if self.focused && c.is_ascii_digit() && self.input.len() < PAGE_DIGITS {
            self.input.insert(c);
        }
    }

    fn set(&mut self, page: usize) {
        self.page = page;
        self.input = (page + 1).to_string().into();
    }

    /// Keeps the page within `pages`, for a list that shrank.
    pub fn clamp(&mut self, pages: usize) {
        let page = self.page.min(pages.max(1) - 1);
        if page != self.page {
            self.set(page);
        }
    }

    /// Returns whether the page changed.
    pub fn update(&mut self, app: &App, ui: &Ui, fonts: &Fonts, keys: &EditKeys, area: Rect, pages: usize) -> bool {
        let pages = pages.max(1);
        let l = Layout::new(area);
        if field_clicked(ui, fonts, l.field, &mut self.input) {
            self.focused = true;
        } else if ui.clicked(Rect::at(0.0, 0.0, ui.view().w, ui.view().h)) {
            self.focused = false;
        }
        let wanted = if self.focused {
            self.input.edit(keys);
            app.keyboard.was_pressed(KeyCode::Enter).then(|| {
                self.focused = false;
                self.input.parse::<usize>().unwrap_or(1).max(1) - 1
            })
        } else {
            None
        };
        let wanted = wanted.or_else(|| {
            if ui.clicked(l.first) {
                Some(0)
            } else if ui.clicked(l.prev) {
                Some(self.page.saturating_sub(1))
            } else if ui.clicked(l.next) {
                Some(self.page + 1)
            } else if ui.clicked(l.last) {
                Some(pages - 1)
            } else {
                None
            }
        });
        let Some(page) = wanted.map(|p| p.min(pages - 1)) else {
            return false;
        };
        let changed = page != self.page;
        self.set(page);
        changed
    }

    pub fn draw(&self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, area: Rect, pages: usize) {
        let pages = pages.max(1);
        let l = Layout::new(area);
        let back = self.page > 0;
        let forward = self.page + 1 < pages;
        ui.button_enabled(draw, fonts, l.first, "<<", back);
        ui.button_enabled(draw, fonts, l.prev, "<", back);
        let field = Field {
            placeholder: "",
            input: &self.input,
            focused: self.focused,
        };
        text_field(draw, ui, fonts, l.field, &field);
        draw.sharp_text(&fonts.text, &format!("/ {pages}"))
            .position(l.of_x, l.field.y + l.field.h / 2.0)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(ui.palette().text_dim);
        ui.button_enabled(draw, fonts, l.next, ">", forward);
        ui.button_enabled(draw, fonts, l.last, ">>", forward);
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
