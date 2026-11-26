//! The game's widgets on Aero's interface: drawn by the theme, with a sound when the pointer comes on one and
//! another when it is clicked.

use std::hash::Hash;

use aero::draw::Draw;
use aero::geom::{Color, Rect as Area};
use aero::image::Image;
use aero::ui::{Response, Role, Sense, Ui};
use aero::widgets::{control, Button, Tab};

use super::button::paint_icon;
use super::controls::paint_row;
use super::stepper::{paint_labelled, paint_switch, paint_value, tile_label};
use super::{bar, portrait, Gui, Icon, Label, Modal, Persona, Rect, Stepper};

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
        Label::Text(text) => Button::new(text).enabled(enabled).show(ui, key, rect.into()),
        Label::Icon(icon) => {
            if icon == Icon::Busy {
                ui.cx.request_redraw();
            }
            let time = crate::connection::now_secs() as f32;
            let paint = |draw: &mut Draw, area: Area, color: Color| {
                let center = area.center();
                paint_icon(draw, icon, (center.x, center.y), area.size().min_element(), color, time);
            };
            Button::painted(&paint, label.id()).enabled(enabled).show(ui, key, rect.into())
        }
    };
    sounds(response).clicked
}

/// Returns whether it was clicked.
pub fn tab(ui: &mut Ui<'_>, rect: Rect, text: &str, selected: bool) -> bool {
    sounds(Tab::new(text, selected).show(ui, text, rect.into())).clicked
}

/// A slanted bar across `row`, as the menus stack them. Returns whether it was clicked.
pub fn menu_bar(ui: &mut Ui<'_>, gui: &mut Gui, row: Rect, (label, color): (&str, Color), enabled: bool) -> bool {
    let hovered = ui.hovered(ui.id(label));
    let (response, visual) = control(ui, label, bar::reach(row, hovered).into(), enabled, Role::Button, label);
    bar::paint(ui.draw, &mut gui.fonts, (&gui.ui.palette(), gui.ui.time()), row, (label, color), visual);
    sounds(response).clicked
}

/// A list row that lights up under the pointer; the caller draws what it holds. Returns whether it was clicked.
pub fn row(ui: &mut Ui<'_>, gui: &Gui, key: impl Hash, rect: Rect, index: usize) -> bool {
    let (response, visual) = control(ui, key, rect.into(), true, Role::Button, "");
    paint_row(ui.draw, &gui.ui.palette(), rect, index, visual);
    sounds(response).clicked
}

/// The player's round portrait, which grows a little under the pointer. Returns whether it was clicked.
pub fn avatar(ui: &mut Ui<'_>, gui: &mut Gui, rect: Rect, name: &str, picture: Option<&Image>) -> bool {
    let (response, visual) = control(ui, ("avatar", name), rect.into(), true, Role::Button, name);
    let center = rect.center();
    let radius = rect.w.min(rect.h) / 2.0 * (1.0 + 0.05 * visual.hover);
    let who = Persona { name, glow: visual.hover, picture };
    portrait(ui.draw, &gui.ui.palette(), &mut gui.fonts, center, radius, &who);
    sounds(response).clicked
}

/// A labelled value with a button on each side to step it. Returns the step asked: -1, 1, or 0.
pub fn stepper(ui: &mut Ui<'_>, gui: &mut Gui, key: impl Hash + Copy, s: Stepper, (label, value): (&str, &str)) -> i32 {
    paint_labelled(ui.draw, &mut gui.fonts, &gui.ui.palette(), s, (label, value));
    let less = button(ui, (key, "minus"), s.minus, Icon::Minus, true);
    let more = button(ui, (key, "plus"), s.plus, Icon::Plus, true);
    i32::from(more) - i32::from(less)
}

/// A value between a minus and a plus button across `band`, the buttons only when `editable`. Returns the step
/// asked: -1, 1, or 0.
pub fn stepper_band(
    ui: &mut Ui<'_>,
    gui: &mut Gui,
    key: impl Hash + Copy,
    band: Rect,
    value: &str,
    editable: bool,
) -> i32 {
    let s = Stepper::across(band);
    paint_value(ui.draw, &mut gui.fonts, s, value);
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
    tile_label(ui.draw, &mut gui.fonts, &gui.ui.palette(), tile, label);
    stepper_band(ui, gui, key, Stepper::tile_band(tile), value, editable)
}

/// A settings tile whose value is a button, for a setting changed somewhere else. Returns whether it was clicked.
pub fn button_tile(ui: &mut Ui<'_>, gui: &mut Gui, key: impl Hash, tile: Rect, (label, value): (&str, &str)) -> bool {
    tile_label(ui.draw, &mut gui.fonts, &gui.ui.palette(), tile, label);
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
    let pal = gui.ui.palette();
    tile_label(ui.draw, &mut gui.fonts, &pal, tile, label);
    let band = Stepper::tile_band(tile);
    let (response, visual) = control(ui, key, band.into(), editable, Role::Button, label);
    paint_switch(ui.draw, &mut gui.fonts, &pal, band, on, visual);
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
        let view = gui.ui.view();
        let screen = Rect::at(0.0, 0.0, view.w, view.h);
        let beside = ui.interact(ui.id("modal scrim"), screen.into(), Sense::CLICK).clicked;
        modal.paint(ui.draw, &mut gui.fonts, &gui.ui.palette(), (view, title));
        ui.interact(ui.id("modal card"), modal.card.into(), Sense::NONE);
        let closed = button(ui, "modal close", modal.close(), Icon::Close, true);
        (inside(ui, gui), beside || closed)
    })
}
