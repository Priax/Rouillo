//! The game's widgets on Aero's interface: drawn by the theme, with a sound when the pointer comes on one and
//! another when it is clicked.

use std::hash::Hash;

use aero::draw::Draw;
use aero::geom::{Color, Rect as Area};
use aero::image::Image;
use aero::ui::{Response, Role, Ui};
use aero::widgets::{control, Button, Tab};

use super::button::paint_icon;
use super::{bar, portrait, Gui, Icon, Label, Persona, Rect};

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

/// The player's round portrait, which grows a little under the pointer. Returns whether it was clicked.
pub fn avatar(ui: &mut Ui<'_>, gui: &mut Gui, rect: Rect, name: &str, picture: Option<&Image>) -> bool {
    let (response, visual) = control(ui, ("avatar", name), rect.into(), true, Role::Button, name);
    let center = rect.center();
    let radius = rect.w.min(rect.h) / 2.0 * (1.0 + 0.05 * visual.hover);
    let who = Persona { name, glow: visual.hover, picture };
    portrait(ui.draw, &gui.ui.palette(), &mut gui.fonts, center, radius, &who);
    sounds(response).clicked
}
