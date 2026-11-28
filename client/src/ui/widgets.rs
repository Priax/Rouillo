//! The game's widgets on Aero's interface: drawn by the theme, with a sound when the pointer comes on one and
//! another when it is clicked.

use std::hash::Hash;

use aero::draw::Draw;
use aero::edit::{EditResponse, TextField};
use aero::geom::{Color, Rect as Area};
use aero::image::Image;
use aero::math::vec2;
use aero::text::TextStyle;
use aero::ui::{Response, Role, Sense, Ui};
use aero::widgets::{control, Button, Link, Tab};

use super::button::paint_icon;
use super::controls::paint_row;
use super::stepper::{paint_labelled, paint_switch, paint_value, tile_label};
use super::{bar, portrait, Face, Gui, Icon, Label, Modal, Persona, Rect, Stepper};
use crate::theme;

fn sounds(response: Response) -> Response {
    if response.entered {
        crate::audio::play_ui_hover();
    }
    if response.pressed.is_some() {
        crate::audio::play_ui_click();
    }
    response
}

/// `key` names it among the widgets of its scope. Returns whether it was clicked.
pub fn button<'a>(ui: &mut Ui<'_>, key: impl Hash, rect: Rect, label: impl Into<Label<'a>>, enabled: bool) -> bool {
    let label = label.into();
    let response = match label {
        Label::Text(text) => Button::new(text).enabled(enabled).show(ui, key, rect),
        Label::Icon(icon) => {
            if icon == Icon::Busy {
                ui.cx.request_redraw();
            }
            let time = crate::connection::now_secs() as f32;
            let paint = |draw: &mut Draw, area: Area, color: Color| {
                let center = area.center();
                paint_icon(draw, icon, (center.x, center.y), area.size().min_element(), color, time);
            };
            Button::painted(&paint, label.id()).enabled(enabled).show(ui, key, rect)
        }
    };
    sounds(response).clicked
}

/// Returns whether it was clicked.
pub fn tab(ui: &mut Ui<'_>, rect: Rect, text: &str, selected: bool) -> bool {
    sounds(Tab::new(text, selected).show(ui, text, rect)).clicked
}

/// Text that leads somewhere. Returns whether it was clicked.
pub fn link(ui: &mut Ui<'_>, key: impl Hash, rect: Rect, text: &str, enabled: bool) -> bool {
    sounds(Link::new(text).enabled(enabled).show(ui, key, rect)).clicked
}

/// A slanted bar across `row`, as the menus stack them. Returns whether it was clicked.
pub fn menu_bar(ui: &mut Ui<'_>, gui: &mut Gui, row: Rect, (label, color): (&str, Color), enabled: bool) -> bool {
    let hovered = ui.hovered(ui.id(label));
    let (response, visual) = control(ui, label, bar::reach(row, hovered), enabled, Role::Button, label);
    bar::paint(ui.draw, &mut ui.cx.fonts, (&gui.stage.palette(), gui.stage.time()), row, (label, color), visual);
    sounds(response).clicked
}

/// A list row that lights up under the pointer; the caller draws what it holds. Returns whether it was clicked.
pub fn row(ui: &mut Ui<'_>, gui: &Gui, key: impl Hash, rect: Rect, index: usize) -> bool {
    let (response, visual) = control(ui, key, rect, true, Role::Button, "");
    paint_row(ui.draw, &gui.stage.palette(), rect, index, visual);
    sounds(response).clicked
}

/// The player's round portrait, which grows a little under the pointer. Returns whether it was clicked.
pub fn avatar(ui: &mut Ui<'_>, gui: &mut Gui, rect: Rect, name: &str, picture: Option<&Image>) -> bool {
    let (response, visual) = control(ui, ("avatar", name), rect, true, Role::Button, name);
    let center = rect.center().into();
    let radius = rect.w().min(rect.h()) / 2.0 * (1.0 + 0.05 * visual.hover);
    let who = Persona { name, glow: visual.hover, picture };
    portrait(ui.draw, &gui.stage.palette(), &mut ui.cx.fonts, center, radius, &who);
    sounds(response).clicked
}

/// A labelled value with a button on each side to step it. Returns the step asked: -1, 1, or 0.
pub fn stepper(ui: &mut Ui<'_>, gui: &mut Gui, key: impl Hash + Copy, s: Stepper, (label, value): (&str, &str)) -> i32 {
    paint_labelled(ui.draw, &mut ui.cx.fonts, &gui.stage.palette(), s, (label, value));
    let less = button(ui, (key, "minus"), s.minus, Icon::Minus, true);
    let more = button(ui, (key, "plus"), s.plus, Icon::Plus, true);
    i32::from(more) - i32::from(less)
}

/// A value between a minus and a plus button across `band`, the buttons only when `editable`. Returns the step
/// asked: -1, 1, or 0.
pub fn stepper_band(ui: &mut Ui<'_>, key: impl Hash + Copy, band: Rect, value: &str, editable: bool) -> i32 {
    let s = Stepper::across(band);
    paint_value(ui.draw, &mut ui.cx.fonts, s, value);
    if !editable {
        return 0;
    }
    let less = button(ui, (key, "minus"), s.minus, Icon::Minus, true);
    let more = button(ui, (key, "plus"), s.plus, Icon::Plus, true);
    i32::from(more) - i32::from(less)
}

/// A settings tile: its name, then its value between the buttons that step it. Returns the step asked.
pub fn stepper_tile(
    ui: &mut Ui<'_>,
    gui: &mut Gui,
    key: impl Hash + Copy,
    tile: Rect,
    (label, value): (&str, &str),
    editable: bool,
) -> i32 {
    tile_label(ui.draw, &mut ui.cx.fonts, &gui.stage.palette(), tile, label);
    stepper_band(ui, key, Stepper::tile_band(tile), value, editable)
}

/// A settings tile whose value is a button, for a setting changed somewhere else. Returns whether it was clicked.
pub fn button_tile(ui: &mut Ui<'_>, gui: &mut Gui, key: impl Hash, tile: Rect, (label, value): (&str, &str)) -> bool {
    tile_label(ui.draw, &mut ui.cx.fonts, &gui.stage.palette(), tile, label);
    button(ui, key, Stepper::tile_band(tile), value, true)
}

/// A settings tile for a yes or no: its name, then a switch that flips when clicked, if `editable`. Returns whether it
/// was clicked.
pub fn switch_tile(
    ui: &mut Ui<'_>,
    gui: &mut Gui,
    key: impl Hash,
    tile: Rect,
    (label, on): (&str, bool),
    editable: bool,
) -> bool {
    let pal = gui.stage.palette();
    tile_label(ui.draw, &mut ui.cx.fonts, &pal, tile, label);
    let band = Stepper::tile_band(tile);
    let (response, visual) = control(ui, key, band, editable, Role::Button, label);
    paint_switch(ui.draw, &mut ui.cx.fonts, &pal, band, on, visual);
    sounds(response).clicked
}

/// A dialog over the screen, `inside` drawing its body: nothing under it takes a click, and a click beside its card or
/// on its cross dismisses it. Returns what `inside` did, and whether it was dismissed.
pub fn modal<R>(
    ui: &mut Ui<'_>,
    gui: &mut Gui,
    (modal, title): (Modal, &str),
    inside: impl FnOnce(&mut Ui<'_>, &mut Gui) -> R,
) -> (R, bool) {
    ui.modal(|ui| {
        let view = gui.stage.view();
        let screen = view.rect();
        let beside = ui.interact(ui.id("modal scrim"), screen, Sense::CLICK).clicked;
        modal.paint(ui.draw, &mut ui.cx.fonts, &gui.stage.palette(), (view, title));
        ui.interact(ui.id("modal card"), modal.card, Sense::NONE);
        let closed = button(ui, "modal close", modal.close(), Icon::Close, true);
        (inside(ui, gui), beside || closed)
    })
}

const FIELD_PADDING: f32 = 12.0;
const AREA_TEXT: f32 = theme::size::LABEL;
const AREA_LINE_H: f32 = 26.0;

/// A field's options, to which [`text_field`] adds the game's look.
pub fn field<'a>() -> TextField<'a> {
    TextField::new(TextStyle::new())
}

/// The height of a field of `lines` lines.
pub const fn area_height(lines: usize) -> f32 {
    2.0 * FIELD_PADDING + lines as f32 * AREA_LINE_H
}

/// A field to type in, on the frame [`super::Look::field`] draws: `field` holds its options. `key` names it among the widgets.
pub fn text_field(
    ui: &mut Ui<'_>,
    gui: &mut Gui,
    key: impl Hash + Copy,
    rect: Rect,
    field: TextField<'_>,
    text: &mut String,
) -> EditResponse {
    let pal = gui.stage.palette();
    let faces = super::faces();
    let (style, padding) = if field.multiline {
        let style = faces.style(Face::Text, AREA_TEXT);
        let line = AREA_LINE_H / style.size;
        (style.line_height(line).max_width(rect.w() - 2.0 * FIELD_PADDING), vec2(FIELD_PADDING, FIELD_PADDING))
    } else {
        let size = (rect.h() * 0.45).clamp(16.0, 24.0);
        (faces.style(Face::Text, size), vec2(FIELD_PADDING, (rect.h() - size) / 2.0))
    };
    field
        .style(style.color(pal.text))
        .padding(padding)
        .caret(pal.text)
        .selection(pal.accent.with_alpha(0.35))
        .placeholder_color(pal.text_muted)
        .show(ui, key, rect, text)
}

/// Gives the field `key` the keyboard, without what was pressed this frame: the Enter that opened it types nothing.
pub fn focus_field(ui: &mut Ui<'_>, key: impl Hash) {
    let id = ui.id(key);
    ui.focus(id);
    let _ = ui.take_keyboard(id);
}
