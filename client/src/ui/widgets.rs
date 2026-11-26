//! The game's widgets on Aero's interface: drawn by the theme, with a sound when the pointer comes on one and
//! another when it is clicked.

use aero::draw::Draw;
use aero::geom::{Color, Rect as Area};
use aero::ui::{Response, Ui};
use aero::widgets::{Button, Tab};

use super::button::paint_icon;
use super::{Icon, Label, Rect};

fn sounds(response: Response) -> Response {
    if response.entered {
        crate::audio::play_ui_hover();
    }
    if response.pressed.is_some() {
        crate::audio::play_ui_click();
    }
    response
}

/// Returns whether it was clicked.
pub fn button<'a>(ui: &mut Ui<'_>, rect: Rect, label: impl Into<Label<'a>>, enabled: bool) -> bool {
    let label = label.into();
    let response = match label {
        Label::Text(text) => Button::new(text).enabled(enabled).show(ui, text, rect.into()),
        Label::Icon(icon) => {
            if icon == Icon::Busy {
                ui.cx.request_redraw();
            }
            let time = crate::connection::now_secs() as f32;
            let paint = |draw: &mut Draw, area: Area, color: Color| {
                let center = area.center();
                paint_icon(draw, icon, (center.x, center.y), area.size().min_element(), color, time);
            };
            Button::painted(&paint, label.id()).enabled(enabled).show(ui, label.id(), rect.into())
        }
    };
    sounds(response).clicked
}

/// Returns whether it was clicked.
pub fn tab(ui: &mut Ui<'_>, rect: Rect, text: &str, selected: bool) -> bool {
    sounds(Tab::new(text, selected).show(ui, text, rect.into())).clicked
}
