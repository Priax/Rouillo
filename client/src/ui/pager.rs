use std::hash::Hash;

use super::{widgets, Face, Gui, Icon, Rect, Typeset};
use crate::theme;

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
    /// The page field's text, the page shown unless something else was typed.
    input: String,
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
        let x0 = area.x() + (area.w() - total) / 2.0;
        let y = area.y() + (area.h() - BTN_H) / 2.0;
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
        Self { page: 0, input: 1.to_string() }
    }
}

impl Pager {
    pub const fn page(&self) -> usize {
        self.page
    }

    /// The items of the current page, numbered from the top of the page.
    pub fn shown<'a, T>(&self, items: &'a [T], per_page: usize) -> impl Iterator<Item = (usize, &'a T)> {
        items.iter().skip(self.page * per_page).take(per_page).enumerate()
    }

    fn set(&mut self, page: usize) {
        self.page = page;
        self.input = (page + 1).to_string();
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
        let field = widgets::field().max_chars(PAGE_DIGITS).accepts(|c| c.is_ascii_digit());
        if widgets::text_field(ui, gui, (key, "page"), l.field, field, &mut self.input).submitted {
            ui.blur();
            wanted = Some(self.input.parse::<usize>().unwrap_or(1).max(1) - 1);
        }
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, &format!("/ {pages}"))
            .left(l.of_x, l.field.y() + l.field.h() / 2.0)
            .size(theme::size::LABEL)
            .color(gui.stage.palette().text_dim);
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
