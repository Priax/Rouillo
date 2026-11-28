pub use aero::layout::{px, Size, FILL};

use super::Rect;
use crate::theme;

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
        let screen = view.rect();
        let header = screen.top_strip(theme::HEADER_H);
        let back = Rect::at(BACK_MARGIN, view.h - BACK_BOTTOM - BACK_H, BACK_W, BACK_H);
        let body =
            Rect::at(0.0, header.bottom() + BODY_TOP, view.w, (back.y() - header.bottom() - 2.0 * BODY_TOP).max(0.0));
        Self { header, body, back }
    }

    /// The header's area right of the title, inset from the screen's edge.
    pub fn header_tools(self, title_w: f32) -> Rect {
        self.header.inset([0.0, 60.0, 0.0, title_w])
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
        Self { card: Frame::of(view).body.inset([0.0, 20.0, 0.0, 20.0]).max_width(LIST_MAX_W), row_h }
    }

    fn list(self) -> Rect {
        self.card.inset([LIST_PAD, LIST_PAD, super::PAGER_H + LIST_PAD + LIST_GAP, LIST_PAD])
    }

    pub fn per_page(self) -> usize {
        self.list().rows_fitting(self.row_h - LIST_GAP, LIST_GAP).max(1)
    }

    pub fn row(self, i: usize) -> Rect {
        self.list().nth_row(i, self.row_h - LIST_GAP, LIST_GAP)
    }

    pub fn pager(self) -> Rect {
        self.card.bottom_strip(super::PAGER_H)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::View;

    fn close(a: Rect, b: Rect) -> bool {
        [(a.x(), b.x()), (a.y(), b.y()), (a.w(), b.w()), (a.h(), b.h())].iter().all(|(p, q)| (p - q).abs() < 1e-3)
    }

    #[test]
    fn fixed_parts_keep_their_size_and_fills_share_the_rest() {
        let r = Rect::at(10.0, 20.0, 300.0, 100.0);
        let [a, b, c] = r.columns(10.0, [px(50.0), FILL, Size::fill(2.0)]);
        assert!(close(a, Rect::at(10.0, 20.0, 50.0, 100.0)));
        let share = (300.0 - 50.0 - 20.0) / 3.0;
        assert!(close(b, Rect::at(70.0, 20.0, share, 100.0)));
        assert!(close(c, Rect::at(80.0 + share, 20.0, 2.0 * share, 100.0)));
        assert!((c.x() + c.w() - (r.x() + r.w())).abs() < 1e-3, "the last part ends at the edge");
    }

    #[test]
    fn rows_stack_with_their_gaps() {
        let [top, rest] = Rect::at(0.0, 0.0, 100.0, 100.0).rows(4.0, [px(30.0), FILL]);
        assert!(close(top, Rect::at(0.0, 0.0, 100.0, 30.0)));
        assert!(close(rest, Rect::at(0.0, 34.0, 100.0, 66.0)));
    }

    #[test]
    fn too_little_room_shrinks_fills_to_nothing_not_below() {
        let [a, b] = Rect::at(0.0, 0.0, 40.0, 10.0).columns(0.0, [px(60.0), FILL]);
        assert_eq!(a.w(), 60.0);
        assert_eq!(b.w(), 0.0);
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
        assert!(close(r.bottom_strip(20.0), Rect::at(0.0, 60.0, 100.0, 20.0)));
        assert!(close(r.below(20.0), Rect::at(0.0, 20.0, 100.0, 60.0)));
        assert!(close(r.above(20.0), Rect::at(0.0, 0.0, 100.0, 60.0)));
        assert!(close(r.right_strip(30.0), Rect::at(70.0, 0.0, 30.0, 80.0)));
        assert!(close(r.inset(10.0), Rect::at(10.0, 10.0, 80.0, 60.0)));
    }

    #[test]
    fn the_frame_keeps_the_back_button_where_screens_had_it() {
        let view = View::default();
        let frame = Frame::of(view);
        assert!(close(frame.back, Rect::at(40.0, view.h - 80.0, 200.0, 54.0)));
        assert_eq!(frame.header.h(), theme::HEADER_H);
        assert!(frame.body.y() > frame.header.bottom() && frame.body.bottom() < frame.back.y());
    }
}
