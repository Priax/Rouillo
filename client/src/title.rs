use notan::draw::Draw;
use notan::prelude::*;

use crate::state::{Screen, State};
use crate::theme;
use crate::ui::SharpText;

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

pub fn update_title(app: &App, state: &mut State) {
    let key = app.keyboard.pressed.iter().any(|k| !NOT_A_GESTURE.contains(k));
    if !key && !app.mouse.left_was_pressed() {
        return;
    }
    state.title_seen = true;
    crate::audio::play_ui_click();
    state.screen = if state.auth.is_some() {
        Screen::Menu
    } else {
        Screen::Auth
    };
}

pub fn draw_title(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let (ww, wh) = state.ui.view().size();
    let mut draw = state.ui.screen_canvas(gfx);

    draw.sharp_text(&state.fonts.display, "Rouillo")
        .position(ww / 2.0, wh / 2.0 - 60.0)
        .size(theme::size::HUGE)
        .h_align_center()
        .v_align_middle()
        .color(pal.title);

    let pulse = 0.6 + 0.4 * (state.ui.time() * PULSE_SPEED).sin() as f32;
    draw.sharp_text(&state.fonts.text, PROMPT)
        .position(ww / 2.0, wh / 2.0 + 90.0)
        .size(theme::size::EMPHASIS)
        .h_align_center()
        .v_align_middle()
        .color(pal.text_dim.with_alpha(pulse));

    draw_version(&mut draw, state);

    state.ui.render(gfx, &draw);
}

pub fn draw_version(draw: &mut Draw, state: &State) {
    let (ww, wh) = state.ui.view().size();
    draw.sharp_text(&state.fonts.text, concat!("v", env!("CARGO_PKG_VERSION")))
        .position(ww - 20.0, wh - 24.0)
        .size(theme::size::SMALL)
        .h_align_right()
        .v_align_middle()
        .color(state.ui.palette().text_dim);
}
