use aero::draw::Draw;
use aero::geom::Color;
use aero::input::KeyCode;
use aero::math::{vec2, Vec2};
use aero::path::Cap;
use shared::{RoomSetting, Rules, BOARD_COLUMNS, BOARD_ROWS};

use crate::draw::{draw_empty_well, FRAME_PAD};
use crate::tick::Tick;
use crate::ui::{widgets, Face, Fonts, Gui, Icon, Modal, Rect, Stage, Typeset, View};
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
        let screen = view.rect();
        let modal = Modal { card: screen.inset([40.0, 0.0, 40.0, 0.0]).max_width(CARD_W) };
        let plot = modal.body().inset([0.0, 0.0, FOOTER_H, 0.0]);
        let frame = FRAME_PAD / config::CELL_SIZE;
        let (max_columns, max_rows) = (max_of(&BOARD_COLUMNS), max_of(&BOARD_ROWS));
        let (cells_w, cells_h) = (max_columns + 2.0 * frame + OVERHANG, max_rows + 2.0 * frame + OVERHANG);
        let (fixed_w, fixed_h) =
            (BUTTON + GAP + Y_LABEL + AXIS_GAP + ARROW_ROOM, TOP + AXIS_GAP + X_LABEL + GAP + BUTTON);
        let cell = ((plot.w() - fixed_w) / cells_w).min((plot.h() - fixed_h) / cells_h);
        let (total_w, total_h) = (fixed_w + cells_w * cell, fixed_h + cells_h * cell);
        let x0 = plot.x() + (plot.w() - total_w) / 2.0;
        let y0 = plot.y() + (plot.h() - total_h) / 2.0;
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
            standard: modal.body().bottom_strip(BUTTON).centered(260.0, BUTTON),
        }
    }

    /// The axes' buttons, as [`STEPS`] orders them.
    const fn buttons(&self) -> [Rect; 4] {
        [self.columns[0], self.columns[1], self.rows[0], self.rows[1]]
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

/// Each axis's buttons, minus then plus, with their key: the same order as [`Layout::buttons`].
const STEPS: [(RoomSetting, i32, KeyCode); 4] = [
    (RoomSetting::Columns, -1, KeyCode::ArrowLeft),
    (RoomSetting::Columns, 1, KeyCode::ArrowRight),
    (RoomSetting::Rows, -1, KeyCode::ArrowDown),
    (RoomSetting::Rows, 1, KeyCode::ArrowUp),
];

fn to_standard(rules: &Rules) -> Action {
    let standard = Rules::default();
    let steps = steps_to(RoomSetting::Columns, rules.columns, standard.columns)
        .chain(steps_to(RoomSetting::Rows, rules.rows, standard.rows))
        .collect();
    Action::Adjust(steps)
}

/// What the keys do to the dialog: Escape closes it, the arrows step its size.
pub fn update_keys(tick: &Tick, editable: bool) -> Option<Action> {
    if tick.back() {
        return Some(Action::Close);
    }
    let (setting, dir, _) = STEPS.into_iter().filter(|_| editable).find(|&(_, _, key)| tick.pressed(key))?;
    Some(Action::Adjust(vec![(setting, dir)]))
}

/// The dialog, on Aero's interface, and what its buttons ask.
pub fn show(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, rules: &Rules, editable: bool) -> Option<Action> {
    let layout = Layout::new(gui.stage.view());
    let (action, dismissed) = widgets::modal(ui, gui, (layout.modal, "Taille du plateau"), |ui, gui| {
        paint(ui.draw, &gui.stage, &mut ui.cx.fonts, &layout, rules);
        if !editable {
            footnote(ui.draw, &gui.stage, &mut ui.cx.fonts, &layout);
            return None;
        }
        let mut step = None;
        for (i, ((setting, dir, _), rect)) in STEPS.into_iter().zip(layout.buttons()).enumerate() {
            let icon = if dir < 0 { Icon::Minus } else { Icon::Plus };
            if widgets::button(ui, ("size", i), rect, icon, true) {
                step = Some(Action::Adjust(vec![(setting, dir)]));
            }
        }
        let standard = widgets::button(ui, "standard", layout.standard, "Taille standard", !is_standard(rules));
        step.or_else(|| standard.then(|| to_standard(rules)))
    });
    if dismissed {
        Some(Action::Close)
    } else {
        action
    }
}

/// The well at its size, the standard one dashed behind it, and the axes.
fn paint(draw: &mut Draw, stage: &Stage, fonts: &mut Fonts, layout: &Layout, rules: &Rules) {
    let pal = stage.palette();
    let well = layout.well(rules.columns, rules.rows);
    let standard = Rules::default();
    let outline = layout.well(standard.columns, standard.rows);
    let dim = pal.text_dim.with_alpha(0.6);
    draw_empty_well(draw, rules, well, stage.time() as f32);
    if !is_standard(rules) {
        dashed_rect(draw, outline, dim);
        fonts
            .write(draw, Face::Text, "standard")
            .left(outline.x() + 4.0, outline.y() - 12.0)
            .size(theme::size::SMALL)
            .color(dim);
    }
    draw_axes(draw, stage, fonts, layout, rules);
}

fn footnote(draw: &mut Draw, stage: &Stage, fonts: &mut Fonts, layout: &Layout) {
    let (x, y) = layout.standard.center().into();
    fonts
        .write(draw, Face::Text, "Seul l'hôte peut changer la taille.")
        .center(x, y)
        .size(theme::size::BODY)
        .color(stage.palette().text_dim);
}

fn draw_axes(draw: &mut Draw, stage: &Stage, fonts: &mut Fonts, layout: &Layout, rules: &Rules) {
    let pal = stage.palette();
    let (cx, cy) = layout.corner;
    let (x_end, y_end) = (layout.x_end, layout.y_end);
    let well = layout.well(rules.columns, rules.rows);
    let (right, top) = (well.x() + well.w(), well.y());
    let head = 7.0;

    for (from, to, color) in [
        ((cx, cy), (x_end, cy), pal.border),
        ((cx, cy), (cx, y_end), pal.border),
        ((well.x(), cy), (right, cy), pal.accent),
        ((cx, well.y() + well.h()), (cx, top), pal.accent),
    ] {
        draw.line_with(Vec2::from(from), Vec2::from(to), 3.0, Cap::Butt).color(color);
    }
    draw.triangle(vec2(x_end + head, cy), vec2(x_end - head, cy - head), vec2(x_end - head, cy + head))
        .color(pal.border);
    draw.triangle(vec2(cx, y_end - head), vec2(cx - head, y_end + head), vec2(cx + head, y_end + head))
        .color(pal.border);

    for c in 0..=*BOARD_COLUMNS.end() {
        let x = well.x() + f32::from(c) * layout.cell;
        draw.line_with(vec2(x, cy - TICK), vec2(x, cy + TICK), 2.0, Cap::Butt).color(pal.border);
    }
    for r in 0..=*BOARD_ROWS.end() {
        let y = well.y() + well.h() - f32::from(r) * layout.cell;
        draw.line_with(vec2(cx - TICK, y), vec2(cx + TICK, y), 2.0, Cap::Butt).color(pal.border);
    }

    fonts
        .write(draw, Face::Display, &rules.columns.to_string())
        .position(right, cy + TICK + 4.0)
        .size(theme::size::HEADING)
        .h_align_center()
        .color(theme::GOLD);
    fonts
        .write(draw, Face::Display, &rules.rows.to_string())
        .right(cx - TICK - 8.0, top)
        .size(theme::size::HEADING)
        .color(theme::GOLD);
}

fn dashed_rect(draw: &mut Draw, r: Rect, color: Color) {
    let corners = r.corners().map(|c| (c.x, c.y));

    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        let dashes = (length / (2.0 * DASH)).ceil() as usize;
        let at = |t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        for d in 0..dashes {
            let t0 = d as f32 * 2.0 * DASH / length;
            let t1 = ((d as f32 * 2.0 + 1.0) * DASH / length).min(1.0);
            draw.line_with(Vec2::from(at(t0)), Vec2::from(at(t1)), 2.0, Cap::Butt).color(color);
        }
    }
}
