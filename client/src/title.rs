use notan::draw::Draw;
use notan::prelude::*;

use crate::nav::Transition;
use crate::state::{Screen, State};
use crate::theme;
use crate::ui::{Fonts, Gui, SharpText, Ui};

const PROMPT: &str = "Appuyez sur une touche !!";
const PULSE_SPEED: f64 = 3.0;
pub const FADE: f32 = 1.2;

const NOT_A_GESTURE: [KeyCode; 12] = [
    KeyCode::Escape,
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
    KeyCode::CapsLock,
    KeyCode::Fn,
    KeyCode::FnLock,
];

pub fn update_title(app: &App, state: &mut State) -> Transition {
    let key = app.keyboard.pressed.iter().any(|k| !NOT_A_GESTURE.contains(k));
    if !key && !app.mouse.left_was_pressed() {
        return Transition::Stay;
    }

    state.startup.title_seen = true;
    crate::audio::play_ui_click();
    let checking = state.startup.check.is_some() || state.startup.retry_at.is_some();
    Transition::To(if state.auth.is_none() && checking { Screen::Auth } else { Screen::Menu })
}

pub fn draw_title(gfx: &mut Graphics, gui: &mut Gui) {
    let pal = gui.ui.palette();
    let (ww, wh) = gui.ui.view().size();
    let pulse = 0.6 + 0.4 * (gui.ui.time() * PULSE_SPEED).sin() as f32;

    let mut draw = gui.ui.screen_canvas(gfx);
    draw.sharp_text(&gui.fonts.display, "Rouillo")
        .center(ww / 2.0, wh / 2.0 - 60.0)
        .size(theme::size::HUGE)
        .color(pal.title);
    draw.sharp_text(&gui.fonts.text, PROMPT)
        .center(ww / 2.0, wh / 2.0 + 90.0)
        .size(theme::size::EMPHASIS)
        .color(pal.text_dim.with_alpha(pulse));
    draw_version(&mut draw, &gui.ui, &gui.fonts);

    gui.ui.render(gfx, &draw);
}

pub fn draw_version(draw: &mut Draw, ui: &Ui, fonts: &Fonts) {
    let (ww, wh) = ui.view().size();
    draw.sharp_text(&fonts.text, concat!("v", env!("CARGO_PKG_VERSION")))
        .right(ww - 20.0, wh - 24.0)
        .size(theme::size::SMALL)
        .color(ui.palette().text_dim);
}
