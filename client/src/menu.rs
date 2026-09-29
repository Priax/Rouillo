use notan::draw::DrawShapes;
use notan::prelude::*;

use crate::state::{AuthForm, Screen, Settings, State};
use crate::ui::{Rect, SharpText, Stepper, Ui, View};
use crate::{http, theme};

struct MenuLayout {
    play: Rect,
    settings: Rect,
    friends: Option<Rect>,
    logout: Option<Rect>,
}

fn menu_layout(win_w: f32, win_h: f32, logged_in: bool) -> MenuLayout {
    let w = 280.0;
    let h = 70.0;
    let x = (win_w - w) / 2.0;
    let cy = win_h / 2.0;
    let (friends, logout) = if logged_in {
        (
            Some(Rect::at(x, cy + 160.0, w, 56.0)),
            Some(Rect::at(x, cy + 240.0, w, 56.0)),
        )
    } else {
        (None, None)
    };
    MenuLayout {
        play: Rect::at(x, cy - 20.0, w, h),
        settings: Rect::at(x, cy + 70.0, w, h),
        friends,
        logout,
    }
}

fn avatar_pos(ww: f32) -> (f32, f32, f32) {
    (ww - 70.0, 70.0, 38.0)
}

fn outdated_btn(ww: f32) -> Rect {
    let (acx, acy, ar) = avatar_pos(ww);
    let (w, h) = (190.0, 50.0);
    Rect::at(acx - ar - 20.0 - w, acy - h / 2.0, w, h)
}

fn avatar_hovered(ui: &Ui, cx: f32, cy: f32, r: f32) -> bool {
    let (mx, my) = ui.mouse();
    let (dx, dy) = (mx - cx, my - cy);
    dx * dx + dy * dy <= r * r
}

fn avatar_clicked(ui: &Ui, cx: f32, cy: f32, r: f32) -> bool {
    ui.pressed() && avatar_hovered(ui, cx, cy, r)
}

pub fn update_menu(state: &mut State) {
    let (ww, wh) = state.ui.view().size();
    let logged_in = state.auth.is_some();
    let layout = menu_layout(ww, wh, logged_in);

    if state.ui.clicked(layout.play) {
        start_play(state);
    } else if state.ui.clicked(layout.settings) {
        state.screen = Screen::Settings;
    }

    if let Some(btn) = layout.friends {
        if state.ui.clicked(btn) {
            crate::friends::enter_friends(state);
            state.screen = Screen::Friends;
        }
    }

    if let Some(btn) = layout.logout {
        if state.ui.clicked(btn) {
            do_logout(state);
        }
    }

    if state.outdated && state.ui.clicked(outdated_btn(ww)) {
        crate::update::apply();
    }

    if logged_in {
        let (acx, acy, ar) = avatar_pos(ww);
        if avatar_clicked(&state.ui, acx, acy, ar) {
            crate::profile::enter_profile(state);
            state.screen = Screen::Profile;
        }
    }
}

pub fn do_logout(state: &mut State) {
    if let Some(auth) = &state.auth {
        http::post_empty(http::api_url("logout"), Some(auth.token.clone()), http::new_slot());
    }
    crate::state::clear_stored_token();
    state.auth = None;
    state.auth_form = AuthForm::default();
    state.friends = None;
    state.profile = None;
    state.other_profile = None;
    state.screen = Screen::Auth;
}

pub fn draw_menu(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let (ww, wh) = state.ui.view().size();
    let mut draw = state.ui.canvas(gfx);
    draw.clear(pal.background);

    draw.sharp_text(&state.fonts.display, "Rouillo")
        .position(ww / 2.0, wh / 2.0 - 140.0)
        .size(theme::size::HERO)
        .h_align_center()
        .v_align_middle()
        .color(pal.title);

    let logged_in = state.auth.is_some();
    let layout = menu_layout(ww, wh, logged_in);
    state.ui.button(&mut draw, &state.fonts, layout.play, "Jouer");
    if state.outdated {
        state
            .ui
            .button(&mut draw, &state.fonts, outdated_btn(ww), "Mettre à jour");
    }
    state.ui.button(&mut draw, &state.fonts, layout.settings, "Paramètres");
    if let Some(btn) = layout.friends {
        state.ui.button(&mut draw, &state.fonts, btn, "Amis");
    }
    if let Some(btn) = layout.logout {
        state.ui.button(&mut draw, &state.fonts, btn, "Déconnexion");
    }

    if let Some(auth) = &state.auth {
        let (acx, acy, ar) = avatar_pos(ww);
        let hover = avatar_hovered(&state.ui, acx, acy, ar);
        let fill = if hover { pal.avatar_hover } else { pal.avatar };
        draw.circle(ar).position(acx, acy).color(fill);
        draw.circle(ar).position(acx, acy).stroke(2.0).color(pal.accent);
        let initial: String = auth
            .username
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        draw.sharp_text(&state.fonts.display, &initial)
            .position(acx, acy)
            .size(theme::size::HEADING)
            .h_align_center()
            .v_align_middle()
            .color(pal.text);
        draw.sharp_text(&state.fonts.text, &auth.username)
            .position(acx, acy + ar + 16.0)
            .size(theme::size::BODY)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_dim);
    }

    if let Some((msg, color)) = state.notice.shown(&pal) {
        draw.sharp_text(&state.fonts.text, msg)
            .position(ww / 2.0, wh - 60.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    gfx.render(&draw);
}

struct SettingsLayout {
    steppers: [Stepper; Settings::COUNT],
    back: Rect,
}

fn settings_layout(view: View) -> SettingsLayout {
    let (win_w, win_h) = view.size();
    let row_h = 70.0;
    let first_y = win_h / 2.0 - (Settings::COUNT as f32 * row_h) / 2.0;
    let center_x = win_w / 2.0;
    let steppers = std::array::from_fn(|i| Stepper::at(center_x + 20.0, first_y + i as f32 * row_h));

    SettingsLayout {
        steppers,
        back: Rect::at(
            center_x - 100.0,
            first_y + Settings::COUNT as f32 * row_h + 40.0,
            200.0,
            60.0,
        ),
    }
}

pub fn update_settings(app: &mut App, state: &mut State) {
    let layout = settings_layout(state.ui.view());
    for (i, stepper) in layout.steppers.into_iter().enumerate() {
        if state.ui.clicked(stepper.minus) {
            state.settings.adjust(i, -1);
        }
        if state.ui.clicked(stepper.plus) {
            state.settings.adjust(i, 1);
        }
    }
    if state.ui.clicked(layout.back) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.screen = Screen::Menu;
    }
}

pub fn draw_settings(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let (ww, wh) = state.ui.view().size();
    let mut draw = state.ui.canvas(gfx);
    draw.clear(pal.background);

    draw.sharp_text(&state.fonts.display, "SETTINGS")
        .position(ww / 2.0, wh / 2.0 - 170.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(pal.text);

    let layout = settings_layout(state.ui.view());
    for (i, stepper) in layout.steppers.into_iter().enumerate() {
        let value = format!("{:.0} ms", state.settings.value(i) * 1000.0);
        state
            .ui
            .stepper(&mut draw, &state.fonts, stepper, Settings::label(i), &value, true);
    }

    state.ui.button(&mut draw, &state.fonts, layout.back, "Back");
    gfx.render(&draw);
}

fn start_play(state: &mut State) {
    state.conn.connect(crate::connection::now_secs());
    state.rooms.clear();
    state.notice.clear();
    state.screen = Screen::RoomBrowser;
}
