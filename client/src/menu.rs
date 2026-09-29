use notan::prelude::*;

use crate::state::{AuthForm, Screen, Settings, State};
use crate::ui::{Rect, SharpText, Stepper, View};
use crate::{http, theme};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    Play,
    Friends,
    Settings,
    Logout,
}

impl MenuItem {
    fn label(self) -> &'static str {
        match self {
            Self::Play => "Jouer",
            Self::Friends => "Amis",
            Self::Settings => "Paramètres",
            Self::Logout => "Déconnexion",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Play => theme::bar::GREEN,
            Self::Friends => theme::bar::BLUE,
            Self::Settings => theme::bar::YELLOW,
            Self::Logout => theme::bar::RED,
        }
    }
}

const ROW_H: f32 = 64.0;

/// The menu's rows, stacked with no gap under the title.
fn menu_rows(view: View, logged_in: bool) -> impl Iterator<Item = (MenuItem, Rect)> {
    let items: &[MenuItem] = if logged_in {
        &[MenuItem::Play, MenuItem::Friends, MenuItem::Settings, MenuItem::Logout]
    } else {
        &[MenuItem::Play, MenuItem::Settings]
    };
    let top = view.h / 2.0 - 60.0;
    items
        .iter()
        .enumerate()
        .map(move |(i, &item)| (item, Rect::at(0.0, top + i as f32 * ROW_H, view.w, ROW_H)))
}

const AVATAR: f32 = 76.0;

fn avatar_rect(ww: f32) -> Rect {
    Rect::at(ww - 70.0 - AVATAR / 2.0, 70.0 - AVATAR / 2.0, AVATAR, AVATAR)
}

fn outdated_btn(ww: f32) -> Rect {
    let avatar = avatar_rect(ww);
    let (w, h) = (190.0, 50.0);
    Rect::at(avatar.x - 20.0 - w, avatar.y + (avatar.h - h) / 2.0, w, h)
}

pub fn update_menu(state: &mut State) {
    let view = state.ui.view();
    let ww = view.w;
    let logged_in = state.auth.is_some();

    let clicked = menu_rows(view, logged_in).find(|&(_, row)| state.ui.clicked(row));
    match clicked.map(|(item, _)| item) {
        Some(MenuItem::Play) => start_play(state),
        Some(MenuItem::Friends) => {
            crate::friends::enter_friends(state);
            state.screen = Screen::Friends;
        }
        Some(MenuItem::Settings) => state.screen = Screen::Settings,
        Some(MenuItem::Logout) => do_logout(state),
        None => {}
    }

    if state.outdated && state.ui.clicked(outdated_btn(ww)) {
        crate::update::apply();
    }

    if logged_in && state.ui.clicked(avatar_rect(ww)) {
        crate::profile::enter_profile(state);
        state.screen = Screen::Profile;
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
    let mut draw = state.ui.screen_canvas(gfx);

    draw.sharp_text(&state.fonts.display, "Rouillo")
        .position(ww / 2.0, wh / 2.0 - 140.0)
        .size(theme::size::HERO)
        .h_align_center()
        .v_align_middle()
        .color(pal.title);

    for (item, row) in menu_rows(state.ui.view(), state.auth.is_some()) {
        state
            .ui
            .menu_bar(&mut draw, &state.fonts, row, item.label(), item.color());
    }
    if state.outdated {
        state
            .ui
            .button(&mut draw, &state.fonts, outdated_btn(ww), "Mettre à jour");
    }

    if let Some(auth) = &state.auth {
        let avatar = avatar_rect(ww);
        state.ui.avatar(&mut draw, &state.fonts, avatar, &auth.username);
        draw.sharp_text(&state.fonts.text, &auth.username)
            .position(avatar.x + avatar.w / 2.0, avatar.y + avatar.h + 16.0)
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
    let mut draw = state.ui.screen_canvas(gfx);

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
