use notan::draw::{CreateDraw, DrawShapes, DrawTextSection};
use notan::prelude::*;

use crate::state::{AuthForm, Screen, Settings, State};
use crate::ui::Rect;
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

fn avatar_hovered(app: &App, cx: f32, cy: f32, r: f32) -> bool {
    let dx = app.mouse.x - cx;
    let dy = app.mouse.y - cy;
    dx * dx + dy * dy <= r * r
}

fn avatar_clicked(app: &App, cx: f32, cy: f32, r: f32) -> bool {
    avatar_hovered(app, cx, cy, r) && app.mouse.left_was_pressed()
}

pub fn update_menu(app: &mut App, state: &mut State) {
    let (ww, wh) = (win_w(app), win_h(app));
    let logged_in = state.auth.is_some();
    let layout = menu_layout(ww, wh, logged_in);

    if layout.play.clicked(app) {
        start_play(state);
    } else if layout.settings.clicked(app) {
        state.screen = Screen::Settings;
    }

    if let Some(btn) = layout.friends {
        if btn.clicked(app) {
            crate::friends::enter_friends(state);
            state.screen = Screen::Friends;
        }
    }

    if let Some(btn) = layout.logout {
        if btn.clicked(app) {
            do_logout(state);
        }
    }

    if state.outdated && outdated_btn(ww).clicked(app) {
        crate::update::apply();
    }

    if logged_in {
        let (acx, acy, ar) = avatar_pos(ww);
        if avatar_clicked(app, acx, acy, ar) {
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

pub fn draw_menu(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (ww, wh) = (win_w(app), win_h(app));
    let mut draw = gfx.create_draw();
    draw.clear(theme::BACKGROUND);

    draw.text(&state.font, "Rouillo")
        .position(ww / 2.0, wh / 2.0 - 140.0)
        .size(80.0)
        .h_align_center()
        .v_align_middle()
        .color(theme::TITLE);

    let logged_in = state.auth.is_some();
    let layout = menu_layout(ww, wh, logged_in);
    state.ui.button(&mut draw, &state.font, layout.play, "Jouer");
    if state.outdated {
        state
            .ui
            .button(&mut draw, &state.font, outdated_btn(ww), "Mettre à jour");
    }
    state.ui.button(&mut draw, &state.font, layout.settings, "Paramètres");
    if let Some(btn) = layout.friends {
        state.ui.button(&mut draw, &state.font, btn, "Amis");
    }
    if let Some(btn) = layout.logout {
        state.ui.button(&mut draw, &state.font, btn, "Déconnexion");
    }

    if let Some(auth) = &state.auth {
        let (acx, acy, ar) = avatar_pos(ww);
        let hover = avatar_hovered(app, acx, acy, ar);
        let fill = if hover { theme::AVATAR_HOVER } else { theme::AVATAR };
        draw.circle(ar).position(acx, acy).color(fill);
        draw.circle(ar).position(acx, acy).stroke(2.0).color(theme::ACCENT);
        let initial: String = auth
            .username
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        draw.text(&state.font, &initial)
            .position(acx, acy)
            .size(30.0)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT);
        draw.text(&state.font, &auth.username)
            .position(acx, acy + ar + 16.0)
            .size(17.0)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_DIM);
    }

    if let Some((msg, color)) = state.notice.shown() {
        draw.text(&state.font, msg)
            .position(ww / 2.0, wh - 60.0)
            .size(22.0)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    gfx.render(&draw);
}

struct SettingsLayout {
    minus: [Rect; Settings::COUNT],
    plus: [Rect; Settings::COUNT],
    back: Rect,
}

fn settings_layout(win_w: f32, win_h: f32) -> SettingsLayout {
    let row_h = 70.0;
    let btn = 50.0;
    let first_y = win_h / 2.0 - (Settings::COUNT as f32 * row_h) / 2.0;
    let center_x = win_w / 2.0;
    let minus_x = center_x + 60.0;
    let plus_x = center_x + 200.0;

    let mut minus = [Rect::at(0.0, 0.0, btn, btn); Settings::COUNT];
    let mut plus = minus;
    for i in 0..Settings::COUNT {
        let y = first_y + i as f32 * row_h;
        minus[i] = Rect::at(minus_x, y, btn, btn);
        plus[i] = Rect::at(plus_x, y, btn, btn);
    }

    SettingsLayout {
        minus,
        plus,
        back: Rect::at(
            center_x - 100.0,
            first_y + Settings::COUNT as f32 * row_h + 40.0,
            200.0,
            60.0,
        ),
    }
}

pub fn update_settings(app: &mut App, state: &mut State) {
    let layout = settings_layout(win_w(app), win_h(app));
    for i in 0..Settings::COUNT {
        if layout.minus[i].clicked(app) {
            state.settings.adjust(i, -1);
        }
        if layout.plus[i].clicked(app) {
            state.settings.adjust(i, 1);
        }
    }
    if layout.back.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.screen = Screen::Menu;
    }
}

pub fn draw_settings(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (ww, wh) = (win_w(app), win_h(app));
    let mut draw = gfx.create_draw();
    draw.clear(theme::BACKGROUND);

    draw.text(&state.font, "SETTINGS")
        .position(ww / 2.0, wh / 2.0 - 170.0)
        .size(50.0)
        .h_align_center()
        .v_align_middle()
        .color(theme::TEXT);

    let layout = settings_layout(ww, wh);
    let center_x = ww / 2.0;
    for i in 0..Settings::COUNT {
        let y = layout.minus[i].y;
        let mid = y + layout.minus[i].h / 2.0;
        draw.text(&state.font, Settings::label(i))
            .position(center_x - 100.0, mid)
            .size(24.0)
            .h_align_right()
            .v_align_middle()
            .color(theme::TEXT_DIM);
        state.ui.button(&mut draw, &state.font, layout.minus[i], "-");
        state.ui.button(&mut draw, &state.font, layout.plus[i], "+");
        draw.text(&state.font, &format!("{:.0} ms", state.settings.value(i) * 1000.0))
            .position(center_x + 130.0, mid)
            .size(22.0)
            .h_align_center()
            .v_align_middle()
            .color(theme::GOLD);
    }

    state.ui.button(&mut draw, &state.font, layout.back, "Back");
    gfx.render(&draw);
}

fn start_play(state: &mut State) {
    state.conn.connect(crate::connection::now_secs());
    state.rooms.clear();
    state.notice.clear();
    state.screen = Screen::RoomBrowser;
}

fn win_w(app: &mut App) -> f32 {
    app.window().width() as f32
}

fn win_h(app: &mut App) -> f32 {
    app.window().height() as f32
}
