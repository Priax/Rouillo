use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;
use shared::{RoomSetting, Rules, BOARD_COLUMNS, BOARD_ROWS};

use crate::draw::{draw_empty_well, FRAME_PAD};
use crate::ui::{Fonts, Icon, Modal, Rect, SharpText, Ui, View};
use crate::{config, theme};

const CARD_W: f32 = 600.0;
const BUTTON: f32 = 44.0;
const GAP: f32 = 8.0;
const AXIS_GAP: f32 = 14.0;
const Y_LABEL: f32 = 44.0;
const X_LABEL: f32 = 40.0;
const TOP: f32 = 20.0;
const ARROW_ROOM: f32 = 12.0;
const FOOTER_H: f32 = 76.0;
/// How far, in cells, each axis runs past the largest board.
const OVERHANG: f32 = 0.6;
const TICK: f32 = 5.0;
const DASH: f32 = 8.0;

pub enum Action {
    Close,
    Adjust(Vec<(RoomSetting, i32)>),
}

fn max_of(range: &std::ops::RangeInclusive<u8>) -> f32 {
    f32::from(*range.end())
}

/// The board grows up and right from the corner of two axes. Each axis has
/// its buttons beside its middle, where they stay while the board grows.
struct Layout {
    modal: Modal,
    cell: f32,
    /// The bottom left corner of the well.
    origin: (f32, f32),
    /// Where the axes cross.
    corner: (f32, f32),
    x_end: f32,
    y_end: f32,
    /// Minus then plus, for each axis.
    columns: [Rect; 2],
    rows: [Rect; 2],
    standard: Rect,
}

impl Layout {
    fn new(view: View) -> Self {
        let screen = Rect::at(0.0, 0.0, view.w, view.h);
        let modal = Modal {
            card: screen.pad(40.0, 0.0, 40.0, 0.0).max_width(CARD_W),
        };
        let plot = modal.body().pad(0.0, 0.0, FOOTER_H, 0.0);
        let frame = FRAME_PAD / config::CELL_SIZE;
        let (max_columns, max_rows) = (max_of(&BOARD_COLUMNS), max_of(&BOARD_ROWS));
        let (cells_w, cells_h) = (max_columns + 2.0 * frame + OVERHANG, max_rows + 2.0 * frame + OVERHANG);
        let (fixed_w, fixed_h) = (
            BUTTON + GAP + Y_LABEL + AXIS_GAP + ARROW_ROOM,
            TOP + AXIS_GAP + X_LABEL + GAP + BUTTON,
        );
        let cell = ((plot.w - fixed_w) / cells_w).min((plot.h - fixed_h) / cells_h);
        let (total_w, total_h) = (fixed_w + cells_w * cell, fixed_h + cells_h * cell);
        let x0 = plot.x + (plot.w - total_w) / 2.0;
        let y0 = plot.y + (plot.h - total_h) / 2.0;
        let corner = (x0 + BUTTON + GAP + Y_LABEL, y0 + total_h - BUTTON - GAP - X_LABEL);
        let origin = (corner.0 + AXIS_GAP + frame * cell, corner.1 - AXIS_GAP - frame * cell);

        let mid_x = origin.0 + max_columns * cell / 2.0;
        let below = corner.1 + X_LABEL + GAP;
        let mid_y = origin.1 - max_rows * cell / 2.0;
        Self {
            modal,
            cell,
            origin,
            corner,
            x_end: origin.0 + (max_columns + frame + OVERHANG) * cell,
            y_end: origin.1 - (max_rows + frame + OVERHANG) * cell,
            columns: [
                Rect::at(mid_x - GAP / 2.0 - BUTTON, below, BUTTON, BUTTON),
                Rect::at(mid_x + GAP / 2.0, below, BUTTON, BUTTON),
            ],
            rows: [
                Rect::at(x0, mid_y + GAP / 2.0, BUTTON, BUTTON),
                Rect::at(x0, mid_y - GAP / 2.0 - BUTTON, BUTTON, BUTTON),
            ],
            standard: modal.body().bottom_band(BUTTON).centered(260.0, BUTTON),
        }
    }

    /// The well's area on screen for `columns` by `rows` cells.
    fn well(&self, columns: u8, rows: u8) -> Rect {
        let (w, h) = (f32::from(columns) * self.cell, f32::from(rows) * self.cell);
        Rect::at(self.origin.0, self.origin.1 - h, w, h)
    }
}

/// A board's size as its tile shows it.
pub fn label(rules: &Rules) -> String {
    format!("{} × {}", rules.columns, rules.rows)
}

pub fn is_standard(rules: &Rules) -> bool {
    let standard = Rules::default();
    (rules.columns, rules.rows) == (standard.columns, standard.rows)
}

fn steps_to(setting: RoomSetting, from: u8, to: u8) -> impl Iterator<Item = (RoomSetting, i32)> {
    let dir = (i32::from(to) - i32::from(from)).signum();
    std::iter::repeat_n((setting, dir), usize::from(from.abs_diff(to)))
}

pub fn update(app: &App, ui: &Ui, rules: &Rules, editable: bool) -> Option<Action> {
    let layout = Layout::new(ui.view());
    let keys = &app.keyboard;
    if layout.modal.dismissed(ui) || keys.was_pressed(KeyCode::Escape) {
        return Some(Action::Close);
    }
    if !editable {
        return None;
    }
    let pressed = |rect: Rect, key: KeyCode| ui.clicked(rect) || keys.was_pressed(key);
    let step = [
        (RoomSetting::Columns, -1, pressed(layout.columns[0], KeyCode::ArrowLeft)),
        (RoomSetting::Columns, 1, pressed(layout.columns[1], KeyCode::ArrowRight)),
        (RoomSetting::Rows, -1, pressed(layout.rows[0], KeyCode::ArrowDown)),
        (RoomSetting::Rows, 1, pressed(layout.rows[1], KeyCode::ArrowUp)),
    ]
    .into_iter()
    .find(|&(_, _, pressed)| pressed);
    if let Some((setting, dir, _)) = step {
        return Some(Action::Adjust(vec![(setting, dir)]));
    }
    let standard = Rules::default();
    if !is_standard(rules) && ui.clicked(layout.standard) {
        let steps = steps_to(RoomSetting::Columns, rules.columns, standard.columns)
            .chain(steps_to(RoomSetting::Rows, rules.rows, standard.rows))
            .collect();
        return Some(Action::Adjust(steps));
    }
    None
}

pub fn draw(draw: &mut Draw, ui: &Ui, fonts: &Fonts, rules: &Rules, editable: bool) {
    let pal = ui.palette();
    let layout = Layout::new(ui.view());
    layout.modal.draw(draw, ui, fonts, "Taille du plateau");

    let well = layout.well(rules.columns, rules.rows);
    draw_empty_well(draw, rules, well, ui.time() as f32);

    let standard = Rules::default();
    if !is_standard(rules) {
        let outline = layout.well(standard.columns, standard.rows);
        let color = pal.text_dim.with_alpha(0.6);
        dashed_rect(draw, outline, color);
        draw.sharp_text(&fonts.text, "standard")
            .position(outline.x + 4.0, outline.y - 12.0)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(color);
    }

    draw_axes(draw, ui, fonts, &layout, rules);

    if editable {
        for (rect, icon) in layout
            .columns
            .into_iter()
            .chain(layout.rows)
            .zip([Icon::Minus, Icon::Plus].repeat(2))
        {
            ui.button(draw, fonts, rect, icon);
        }
        ui.button_enabled(draw, fonts, layout.standard, "Taille standard", !is_standard(rules));
    } else {
        let (x, y) = layout.standard.center();
        draw.sharp_text(&fonts.text, "Seul l'hôte peut changer la taille.")
            .position(x, y)
            .size(theme::size::BODY)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_dim);
    }
}

fn draw_axes(draw: &mut Draw, ui: &Ui, fonts: &Fonts, layout: &Layout, rules: &Rules) {
    let pal = ui.palette();
    let (cx, cy) = layout.corner;
    let (x_end, y_end) = (layout.x_end, layout.y_end);
    let well = layout.well(rules.columns, rules.rows);
    let (right, top) = (well.x + well.w, well.y);

    for (from, to, color) in [
        ((cx, cy), (x_end, cy), pal.border),
        ((cx, cy), (cx, y_end), pal.border),
        ((well.x, cy), (right, cy), pal.accent),
        ((cx, well.y + well.h), (cx, top), pal.accent),
    ] {
        draw.line(from, to).width(3.0).color(color);
    }
    let head = 7.0;
    draw.triangle((x_end + head, cy), (x_end - head, cy - head), (x_end - head, cy + head))
        .color(pal.border);
    draw.triangle((cx, y_end - head), (cx - head, y_end + head), (cx + head, y_end + head))
        .color(pal.border);

    for c in 0..=*BOARD_COLUMNS.end() {
        let x = well.x + f32::from(c) * layout.cell;
        draw.line((x, cy - TICK), (x, cy + TICK)).width(2.0).color(pal.border);
    }
    for r in 0..=*BOARD_ROWS.end() {
        let y = well.y + well.h - f32::from(r) * layout.cell;
        draw.line((cx - TICK, y), (cx + TICK, y)).width(2.0).color(pal.border);
    }

    draw.sharp_text(&fonts.display, &rules.columns.to_string())
        .position(right, cy + TICK + 4.0)
        .size(theme::size::HEADING)
        .h_align_center()
        .color(theme::GOLD);
    draw.sharp_text(&fonts.display, &rules.rows.to_string())
        .position(cx - TICK - 8.0, top)
        .size(theme::size::HEADING)
        .h_align_right()
        .v_align_middle()
        .color(theme::GOLD);
}

fn dashed_rect(draw: &mut Draw, r: Rect, color: Color) {
    let corners = r.corners();
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        let dashes = (length / (2.0 * DASH)).ceil() as usize;
        for d in 0..dashes {
            let t0 = d as f32 * 2.0 * DASH / length;
            let t1 = ((d as f32 * 2.0 + 1.0) * DASH / length).min(1.0);
            let at = |t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            draw.line(at(t0), at(t1)).width(2.0).color(color);
        }
    }
}
