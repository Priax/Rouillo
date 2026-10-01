use notan::prelude::*;

use crate::state::{AuthForm, Screen, Settings, State};
use crate::ui::{Rect, SettingsPanel, SharpText, View};
use crate::{http, theme};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    Play,
    Solo,
    Friends,
    Settings,
    Logout,
}

impl MenuItem {
    fn label(self) -> &'static str {
        match self {
            Self::Play => "Multijoueur",
            Self::Solo => "Solo",
            Self::Friends => "Amis",
            Self::Settings => "Paramètres",
            Self::Logout => "Déconnexion",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Play => theme::bar::GREEN,
            Self::Solo => theme::bar::PURPLE,
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
        &[
            MenuItem::Solo,
            MenuItem::Play,
            MenuItem::Friends,
            MenuItem::Settings,
            MenuItem::Logout,
        ]
    } else {
        &[MenuItem::Solo, MenuItem::Play, MenuItem::Settings]
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

    let clicked = menu_rows(view, logged_in).find(|&(_, row)| state.ui.bar_clicked(row));
    match clicked.map(|(item, _)| item) {
        Some(MenuItem::Play) => start_play(state),
        Some(MenuItem::Solo) => {
            state.notice.clear();
            state.screen = Screen::SoloSetup;
        }
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
    forget_session(state);
}

pub fn forget_session(state: &mut State) {
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

    crate::title::draw_version(&mut draw, state);

    state.ui.render(gfx, &draw);
}

fn settings_panel(view: View) -> SettingsPanel {
    SettingsPanel::new(view, Settings::COUNT)
}

fn settings_back(view: View) -> Rect {
    Rect::at(40.0, view.h - 80.0, 200.0, 54.0)
}

pub fn update_settings(app: &mut App, state: &mut State) {
    let view = state.ui.view();
    let panel = settings_panel(view);
    for i in 0..Settings::COUNT {
        if state.ui.clicked(panel.stepper(i).minus) {
            state.settings.adjust(i, -1);
        }
        if state.ui.clicked(panel.stepper(i).plus) {
            state.settings.adjust(i, 1);
        }
    }
    if state.ui.clicked(settings_back(view)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.screen = Screen::Menu;
    }
}

pub fn draw_settings(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let mut draw = state.ui.screen_canvas(gfx);

    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    draw.sharp_text(&state.fonts.display, "Paramètres")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);

    let panel = settings_panel(view);
    panel.draw(&mut draw, &pal, &state.fonts, "Contrôles");
    for i in 0..Settings::COUNT {
        let value = format!("{:.0} ms", state.settings.value(i) * 1000.0);
        state.ui.stepper(
            &mut draw,
            &state.fonts,
            panel.stepper(i),
            Settings::label(i),
            &value,
            true,
        );
    }

    state.ui.button(&mut draw, &state.fonts, settings_back(view), "Retour");
    state.ui.render(gfx, &draw);
}

fn start_play(state: &mut State) {
    state.conn.connect(crate::connection::now_secs());
    state.rooms.clear();
    state.notice.clear();
    state.screen = Screen::RoomBrowser;
}
