use super::Rect;
use crate::theme;

/// How much of a row or column a part takes: a fixed length, or a share of
/// what the fixed parts and the gaps leave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Px(f32),
    Fill(f32),
}

pub const FILL: Size = Size::Fill(1.0);

fn split<const N: usize>(length: f32, gap: f32, sizes: [Size; N]) -> [(f32, f32); N] {
    let fixed: f32 = sizes
        .iter()
        .map(|s| match s {
            Size::Px(px) => *px,
            Size::Fill(_) => 0.0,
        })
        .sum();
    let weights: f32 = sizes
        .iter()
        .map(|s| match s {
            Size::Fill(w) => *w,
            Size::Px(_) => 0.0,
        })
        .sum();
    let gaps = gap * N.saturating_sub(1) as f32;
    let spare = (length - fixed - gaps).max(0.0);
    let mut at = 0.0;
    sizes.map(|s| {
        let len = match s {
            Size::Px(px) => px,
            Size::Fill(w) if weights > 0.0 => spare * w / weights,
            Size::Fill(_) => 0.0,
        };
        let part = (at, len);
        at += len + gap;
        part
    })
}

impl Rect {
    /// `w` wide, centered on `cx`, from `y` down: a form's field or button.
    pub const fn centered_on(cx: f32, y: f32, w: f32, h: f32) -> Self {
        Self::at(cx - w / 2.0, y, w, h)
    }

    /// Stacked parts, top to bottom.
    pub fn rows<const N: usize>(self, gap: f32, sizes: [Size; N]) -> [Self; N] {
        split(self.h, gap, sizes).map(|(at, h)| Self::at(self.x, self.y + at, self.w, h))
    }

    /// Parts side by side, left to right.
    pub fn columns<const N: usize>(self, gap: f32, sizes: [Size; N]) -> [Self; N] {
        split(self.w, gap, sizes).map(|(at, w)| Self::at(self.x + at, self.y, w, self.h))
    }

    /// The `i`th of rows `h` high from the top, `gap` apart: a list's row.
    pub fn nth_row(self, i: usize, h: f32, gap: f32) -> Self {
        Self::at(self.x, self.y + i as f32 * (h + gap), self.w, h)
    }

    /// How many rows `h` high, `gap` apart, fit.
    pub fn rows_fitting(self, h: f32, gap: f32) -> usize {
        ((self.h + gap) / (h + gap)).floor().max(0.0) as usize
    }

    pub fn top(self, h: f32) -> Self {
        Self::at(self.x, self.y, self.w, h)
    }

    pub fn bottom_band(self, h: f32) -> Self {
        Self::at(self.x, self.y + self.h - h, self.w, h)
    }

    pub fn left(self, w: f32) -> Self {
        Self::at(self.x, self.y, w, self.h)
    }

    pub fn right(self, w: f32) -> Self {
        Self::at(self.x + self.w - w, self.y, w, self.h)
    }

    /// Everything below the first `h`.
    pub fn below(self, h: f32) -> Self {
        Self::at(self.x, self.y + h, self.w, (self.h - h).max(0.0))
    }

    /// Everything above the last `h`.
    pub fn above(self, h: f32) -> Self {
        Self::at(self.x, self.y, self.w, (self.h - h).max(0.0))
    }

    pub fn inset(self, by: f32) -> Self {
        self.pad(by, by, by, by)
    }

    pub fn pad(self, top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self::at(
            self.x + left,
            self.y + top,
            (self.w - left - right).max(0.0),
            (self.h - top - bottom).max(0.0),
        )
    }

    /// A `w` by `h` box in the middle, no larger than this one.
    pub fn centered(self, w: f32, h: f32) -> Self {
        let (w, h) = (w.min(self.w), h.min(self.h));
        Self::at(self.x + (self.w - w) / 2.0, self.y + (self.h - h) / 2.0, w, h)
    }

    /// At most `w` wide, kept in the middle.
    pub fn max_width(self, w: f32) -> Self {
        self.centered(w, self.h)
    }

    /// `h` high, centered vertically: a button in a band.
    pub fn middle(self, h: f32) -> Self {
        self.centered(self.w, h)
    }
}

/// The parts every screen with a header shares: the header band, the
/// space under it, and the bottom-left button (Retour) under that.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub header: Rect,
    pub body: Rect,
    pub back: Rect,
}

pub const BACK_W: f32 = 200.0;
pub const BACK_H: f32 = 54.0;
const BACK_MARGIN: f32 = 40.0;
const BACK_BOTTOM: f32 = 26.0;
const BODY_TOP: f32 = 30.0;

impl Frame {
    pub fn of(view: super::View) -> Self {
        let screen = Rect::at(0.0, 0.0, view.w, view.h);
        let header = screen.top(theme::HEADER_H);
        let back = Rect::at(BACK_MARGIN, view.h - BACK_BOTTOM - BACK_H, BACK_W, BACK_H);
        let body = Rect::at(
            0.0,
            header.bottom() + BODY_TOP,
            view.w,
            (back.y - header.bottom() - 2.0 * BODY_TOP).max(0.0),
        );
        Self { header, body, back }
    }

    /// The header's area right of the title, inset from the screen's edge.
    pub fn header_tools(self, title_w: f32) -> Rect {
        self.header.pad(0.0, 60.0, 0.0, title_w)
    }
}

/// A list filling a screen's body: a centered card, rows from its top and a
/// pager at its bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ListCard {
    pub card: Rect,
    row_h: f32,
}

const LIST_MAX_W: f32 = 840.0;
const LIST_PAD: f32 = 12.0;
const LIST_GAP: f32 = 6.0;

impl ListCard {
    /// `row_h` is a row's pitch, gap included.
    pub fn new(view: super::View, row_h: f32) -> Self {
        Self {
            card: Frame::of(view).body.pad(0.0, 20.0, 0.0, 20.0).max_width(LIST_MAX_W),
            row_h,
        }
    }

    fn list(self) -> Rect {
        self.card
            .pad(LIST_PAD, LIST_PAD, super::PAGER_H + LIST_PAD + LIST_GAP, LIST_PAD)
    }

    pub fn per_page(self) -> usize {
        self.list().rows_fitting(self.row_h - LIST_GAP, LIST_GAP).max(1)
    }

    pub fn row(self, i: usize) -> Rect {
        self.list().nth_row(i, self.row_h - LIST_GAP, LIST_GAP)
    }

    pub fn pager(self) -> Rect {
        self.card.bottom_band(super::PAGER_H)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::View;

    fn close(a: Rect, b: Rect) -> bool {
        [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)]
            .iter()
            .all(|(p, q)| (p - q).abs() < 1e-3)
    }

    #[test]
    fn fixed_parts_keep_their_size_and_fills_share_the_rest() {
        let r = Rect::at(10.0, 20.0, 300.0, 100.0);
        let [a, b, c] = r.columns(10.0, [Size::Px(50.0), FILL, Size::Fill(2.0)]);
        assert!(close(a, Rect::at(10.0, 20.0, 50.0, 100.0)));
        let share = (300.0 - 50.0 - 20.0) / 3.0;
        assert!(close(b, Rect::at(70.0, 20.0, share, 100.0)));
        assert!(close(c, Rect::at(80.0 + share, 20.0, 2.0 * share, 100.0)));
        assert!((c.x + c.w - (r.x + r.w)).abs() < 1e-3, "the last part ends at the edge");
    }

    #[test]
    fn rows_stack_with_their_gaps() {
        let [top, rest] = Rect::at(0.0, 0.0, 100.0, 100.0).rows(4.0, [Size::Px(30.0), FILL]);
        assert!(close(top, Rect::at(0.0, 0.0, 100.0, 30.0)));
        assert!(close(rest, Rect::at(0.0, 34.0, 100.0, 66.0)));
    }

    #[test]
    fn too_little_room_shrinks_fills_to_nothing_not_below() {
        let [a, b] = Rect::at(0.0, 0.0, 40.0, 10.0).columns(0.0, [Size::Px(60.0), FILL]);
        assert_eq!(a.w, 60.0);
        assert_eq!(b.w, 0.0);
    }

    #[test]
    fn list_rows_and_how_many_fit() {
        let card = Rect::at(0.0, 100.0, 200.0, 170.0);
        assert!(close(card.nth_row(2, 50.0, 6.0), Rect::at(0.0, 212.0, 200.0, 50.0)));
        assert_eq!(card.rows_fitting(50.0, 6.0), 3);
        assert_eq!(card.rows_fitting(500.0, 6.0), 0);
    }

    #[test]
    fn centered_boxes_never_overflow() {
        let r = Rect::at(0.0, 0.0, 100.0, 50.0);
        assert!(close(r.centered(40.0, 20.0), Rect::at(30.0, 15.0, 40.0, 20.0)));
        assert!(close(r.centered(400.0, 20.0), Rect::at(0.0, 15.0, 100.0, 20.0)));
        assert!(close(r.max_width(60.0), Rect::at(20.0, 0.0, 60.0, 50.0)));
    }

    #[test]
    fn bands_and_insets() {
        let r = Rect::at(0.0, 0.0, 100.0, 80.0);
        assert!(close(r.bottom_band(20.0), Rect::at(0.0, 60.0, 100.0, 20.0)));
        assert!(close(r.below(20.0), Rect::at(0.0, 20.0, 100.0, 60.0)));
        assert!(close(r.above(20.0), Rect::at(0.0, 0.0, 100.0, 60.0)));
        assert!(close(r.right(30.0), Rect::at(70.0, 0.0, 30.0, 80.0)));
        assert!(close(r.inset(10.0), Rect::at(10.0, 10.0, 80.0, 60.0)));
    }

    #[test]
    fn the_frame_keeps_the_back_button_where_screens_had_it() {
        let view = View::default();
        let frame = Frame::of(view);
        assert!(close(frame.back, Rect::at(40.0, view.h - 80.0, 200.0, 54.0)));
        assert_eq!(frame.header.h, theme::HEADER_H);
        assert!(frame.body.y > frame.header.bottom() && frame.body.bottom() < frame.back.y);
    }
}
