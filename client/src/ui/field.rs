use std::borrow::Cow;

use notan::draw::{Draw, DrawShapes};

use super::fonts::Metrics;
use super::{EditKeys, Face, Fonts, Rect, SharpText, TextInput, Ui};
use crate::theme::{self, Palette};

pub struct Field<'a> {
    pub placeholder: &'a str,
    pub input: &'a TextInput,
    pub focused: bool,
}

const PADDING: f32 = 12.0;
const CARET_W: f32 = 2.0;
const CARET_GAP: f32 = 5.0;
const AREA_TEXT: f32 = theme::size::LABEL;
const AREA_LINE_H: f32 = 26.0;

fn frame(draw: &mut Draw, pal: &Palette, r: Rect, focused: bool) {
    let border = if focused { pal.accent } else { pal.border };
    draw.rect((r.x, r.y), (r.w, r.h))
        .corner_radius(theme::RADIUS)
        .color(pal.surface);
    draw.rect((r.x, r.y), (r.w, r.h))
        .corner_radius(theme::RADIUS)
        .stroke(2.0)
        .color(border);
}

fn caret(draw: &mut Draw, ui: &Ui, input: &TextInput, x: f32, mid: f32, size: f32) {
    if ui.caret_on(input, input.caret()) {
        let h = size * 1.2;
        draw.rect((x - CARET_W / 2.0, mid - h / 2.0), (CARET_W, h))
            .color(ui.palette().text);
    }
}

fn room(r: Rect) -> f32 {
    r.w - 2.0 * PADDING - CARET_W - 1.0
}

struct Line<'a> {
    text: Cow<'a, str>,
    from: usize,
    caret: usize,
    size: f32,
}

impl Line<'_> {
    fn visible(&self) -> &str {
        &self.text[self.from..]
    }
}

fn line<'a>(fonts: &Metrics, r: Rect, input: &'a TextInput) -> Line<'a> {
    let size = (r.h * 0.45).clamp(16.0, 24.0);
    let (text, caret) = input.shown();
    let fitting = |t: &str| t.len() - fonts.tail(Face::Text, t, size, room(r)).len();
    let from = text
        .floor_char_boundary(input.scroll().min(fitting(&text)))
        .clamp(fitting(&text[..caret]), caret);
    input.set_scroll(from);
    Line {
        text,
        from,
        caret,
        size,
    }
}

pub fn text_field(draw: &mut Draw, ui: &Ui, fonts: &Fonts, r: Rect, field: &Field) {
    let pal = ui.palette();
    frame(draw, &pal, r, field.focused);

    let (x, mid) = (r.x + PADDING, r.y + r.h / 2.0);
    let line = line(fonts, r, field.input);
    let (text, color) = if line.text.is_empty() {
        (field.placeholder, pal.text_muted)
    } else {
        (fonts.head(Face::Text, line.visible(), line.size, room(r)), pal.text)
    };
    draw.sharp_text(&fonts.text, text)
        .position(x + placeholder_shift(field), mid)
        .size(line.size)
        .v_align_middle()
        .color(color);
    if field.focused {
        let before = &line.text[line.from..line.caret];
        let caret_x = x + fonts.width(Face::Text, before, line.size);
        caret(draw, ui, field.input, caret_x, mid, line.size);
    }
}

pub fn field_clicked(ui: &Ui, fonts: &Fonts, r: Rect, input: &mut TextInput) -> bool {
    let Some((x, _)) = ui.click_in(r) else {
        return false;
    };
    let line = line(fonts, r, input);
    let at = line.from + fonts.index_at(Face::Text, line.visible(), line.size, x - r.x - PADDING);
    input.set_caret_shown(at);
    true
}

fn placeholder_shift(field: &Field) -> f32 {
    if field.focused && field.input.is_empty() {
        CARET_W + CARET_GAP
    } else {
        0.0
    }
}

pub const fn area_height(lines: usize) -> f32 {
    2.0 * PADDING + lines as f32 * AREA_LINE_H
}

struct Row<'a> {
    text: &'a str,
    start: usize,
    wrapped: bool,
}

impl Row<'_> {
    fn at(&self, fonts: &Metrics, x: f32) -> usize {
        let last = self.text.chars().next_back().map_or(0, char::len_utf8);
        let end = self.text.len() - if self.wrapped { last } else { 0 };
        self.start + fonts.index_at(Face::Text, self.text, AREA_TEXT, x).min(end)
    }
}

struct Area<'a> {
    rows: Vec<Row<'a>>,
    first: usize,
    visible: usize,
    caret_row: usize,
}

impl Area<'_> {
    fn caret_x(&self, fonts: &Metrics, caret: usize) -> f32 {
        let row = &self.rows[self.caret_row];
        fonts.width(Face::Text, &row.text[..caret - row.start], AREA_TEXT)
    }
}

fn rows<'a>(fonts: &Metrics, text: &'a str, max: f32) -> Vec<Row<'a>> {
    let mut start = 0;
    fonts
        .wrap(Face::Text, text, AREA_TEXT, max)
        .into_iter()
        .map(|line| {
            let end = start + line.len();
            let wrapped = end < text.len() && !text[end..].starts_with('\n');
            let row = Row {
                text: line,
                start,
                wrapped,
            };
            start = end + usize::from(!wrapped);
            row
        })
        .collect()
}

fn area<'a>(fonts: &Metrics, r: Rect, input: &'a TextInput) -> Area<'a> {
    let rows = rows(fonts, input, room(r));
    let visible = (((r.h - 2.0 * PADDING) / AREA_LINE_H) as usize).max(1);
    let caret_row = rows.iter().rposition(|row| row.start <= input.caret()).unwrap_or(0);
    let first = input
        .scroll()
        .min(rows.len().saturating_sub(visible))
        .clamp((caret_row + 1).saturating_sub(visible), caret_row);
    input.set_scroll(first);
    Area {
        rows,
        first,
        visible,
        caret_row,
    }
}

pub fn text_area(draw: &mut Draw, ui: &Ui, fonts: &Fonts, r: Rect, field: &Field) {
    let pal = ui.palette();
    frame(draw, &pal, r, field.focused);

    let x = r.x + PADDING;
    let line_mid = |i: usize| r.y + PADDING + (i as f32 + 0.5) * AREA_LINE_H;
    let area = area(fonts, r, field.input);

    if field.input.is_empty() {
        draw.sharp_text(&fonts.text, field.placeholder)
            .position(x + placeholder_shift(field), line_mid(0))
            .size(AREA_TEXT)
            .v_align_middle()
            .color(pal.text_muted);
    }
    for (i, row) in area.rows.iter().skip(area.first).take(area.visible).enumerate() {
        draw.sharp_text(&fonts.text, row.text)
            .position(x, line_mid(i))
            .size(AREA_TEXT)
            .v_align_middle()
            .color(pal.text);
    }
    if field.focused {
        caret(
            draw,
            ui,
            field.input,
            x + area.caret_x(fonts, field.input.caret()),
            line_mid(area.caret_row - area.first),
            AREA_TEXT,
        );
    }
}

pub fn area_clicked(ui: &Ui, fonts: &Fonts, r: Rect, input: &mut TextInput) -> bool {
    let Some((x, y)) = ui.click_in(r) else {
        return false;
    };
    let area = area(fonts, r, input);
    let row = area.first + ((y - r.y - PADDING) / AREA_LINE_H) as usize;
    let at = area.rows[row.min(area.rows.len() - 1)].at(fonts, x - r.x - PADDING);
    input.set_caret(at);
    true
}

pub fn area_keys(fonts: &Fonts, r: Rect, input: &mut TextInput, keys: &EditKeys) {
    input.edit(keys);
    for (key, down) in [(&keys.up, false), (&keys.down, true)] {
        if key.fired() {
            let at = vertical_step(fonts, r, input, down);
            input.set_caret(at);
        }
    }
}

fn vertical_step(fonts: &Metrics, r: Rect, input: &TextInput, down: bool) -> usize {
    let area = area(fonts, r, input);
    let to = if down {
        area.caret_row + 1
    } else {
        area.caret_row.wrapping_sub(1)
    };
    match area.rows.get(to) {
        Some(row) => row.at(fonts, area.caret_x(fonts, input.caret())),
        None if down => input.len(),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: f32 = 500.0;
    const NARROW: f32 = 120.0;

    fn fonts() -> Metrics {
        Metrics::load().expect("bundled fonts")
    }

    fn input(text: &str, caret: usize) -> TextInput {
        let mut input = TextInput::from(text.to_owned());
        input.set_caret(caret);
        input
    }

    fn box_of(width: f32, lines: usize) -> Rect {
        Rect::at(0.0, 0.0, width + 2.0 * PADDING + CARET_W + 1.0, area_height(lines))
    }

    #[test]
    fn rows_know_where_they_start_in_the_text() {
        let fonts = fonts();
        let text = "un deux trois quatre\n\ncinq";
        let rows = rows(&fonts, text, NARROW);
        assert!(rows.len() > 3, "{} rows", rows.len());
        for row in &rows {
            assert_eq!(&text[row.start..row.start + row.text.len()], row.text);
        }
        let breaks: Vec<bool> = rows.iter().rev().take(3).map(|row| row.wrapped).collect();
        assert_eq!(breaks, [false; 3], "rows ended by a line break, or by the text");
        assert!(rows[0].wrapped);
        assert_eq!(rows[1].start, rows[0].text.len(), "a wrap takes no character");
    }

    #[test]
    fn the_caret_at_a_wrap_belongs_to_the_row_below() {
        let fonts = fonts();
        let text = "un deux trois quatre";
        let second = rows(&fonts, text, NARROW)[1].start;
        let at = input(text, second);
        let area = area(&fonts, box_of(NARROW, 6), &at);
        assert_eq!(area.caret_row, 1);
        assert!(area.caret_x(&fonts, second).abs() < f32::EPSILON);
        assert!(
            area.rows[0].at(&fonts, 5000.0) < second,
            "so the end of a wrapped row stops short of it"
        );
    }

    #[test]
    fn up_and_down_keep_the_column_and_run_to_the_ends() {
        let fonts = fonts();
        let r = box_of(WIDE, 6);
        let text = "abcdef\nab\nabcdef";
        let step = |caret: usize, down: bool| vertical_step(&fonts, r, &input(text, caret), down);
        assert_eq!(step(14, false), 9, "a shorter row is entered at its end");
        assert_eq!(step(8, true), 11);
        assert_eq!(step(8, false), 1);
        assert_eq!(step(3, false), 0);
        assert_eq!(step(11, true), text.len());
    }

    #[test]
    fn an_area_scrolls_only_to_keep_the_caret_in_sight() {
        let fonts = fonts();
        let r = box_of(WIDE, 2);
        let text = "a\nb\nc\nd\ne";
        let mut field = input(text, text.len());
        assert_eq!(area(&fonts, r, &field).first, 3, "the end of the text at first");
        field.set_caret(6);
        assert_eq!(area(&fonts, r, &field).first, 3, "the caret is still in sight");
        field.set_caret(2);
        assert_eq!(area(&fonts, r, &field).first, 1);
        field.set_caret(4);
        assert_eq!(area(&fonts, r, &field).first, 1);
        field.set_caret(8);
        assert_eq!(area(&fonts, r, &field).first, 3);
        let emptied = input("a", 1);
        emptied.set_scroll(3);
        assert_eq!(area(&fonts, r, &emptied).first, 0);
    }

    #[test]
    fn a_field_scrolls_only_to_keep_the_caret_in_sight() {
        let fonts = fonts();
        let r = box_of(NARROW, 1);
        let text = "abcdefghij".repeat(4);
        let mut field = input(&text, text.len());
        let end = line(&fonts, r, &field).from;
        assert!(end > 0, "the end of the text at first");
        field.set_caret(end + 2);
        assert_eq!(line(&fonts, r, &field).from, end, "the caret is still in sight");
        field.set_caret(3);
        assert_eq!(line(&fonts, r, &field).from, 3);
        field.set_caret(5);
        assert_eq!(line(&fonts, r, &field).from, 3);
        field.set_caret(text.len());
        assert_eq!(line(&fonts, r, &field).from, end);
    }
}
