use notan::draw::Draw;
use notan::prelude::*;

use crate::nav::Transition;
use crate::state::{Screen, State};
use crate::theme;
use crate::ui::{Fonts, SharpText, Ui};

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
    Transition::To(if state.auth.is_none() && checking {
        Screen::Auth
    } else {
        Screen::Menu
    })
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

    draw_version(&mut draw, &state.ui, &state.fonts);

    state.ui.render(gfx, &draw);
}

pub fn draw_version(draw: &mut Draw, ui: &Ui, fonts: &Fonts) {
    let (ww, wh) = ui.view().size();
    draw.sharp_text(&fonts.text, concat!("v", env!("CARGO_PKG_VERSION")))
        .position(ww - 20.0, wh - 24.0)
        .size(theme::size::SMALL)
        .h_align_right()
        .v_align_middle()
        .color(ui.palette().text_dim);
}
