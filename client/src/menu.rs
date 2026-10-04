use notan::draw::DrawShapes;
use notan::prelude::*;

use crate::state::{AuthForm, Screen, Settings, State};
use crate::ui::{Rect, SettingsPanel, SharpText, Status, View};
use crate::{http, theme};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    Play,
    Solo,
    Ranked,
    Leaderboard,
    Help,
    Multiplayer,
    Friends,
    Settings,
    Logout,
    Back,
}

impl MenuItem {
    fn label(self) -> &'static str {
        match self {
            Self::Play => "Jouer",
            Self::Solo => "Solo",
            Self::Ranked => "Classé",
            Self::Leaderboard => "Classement",
            Self::Help => "Aide",
            Self::Multiplayer => "Multijoueur",
            Self::Friends => "Amis",
            Self::Settings => "Paramètres",
            Self::Logout => "Déconnexion",
            Self::Back => "Retour",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Play | Self::Multiplayer => theme::bar::GREEN,
            Self::Solo | Self::Help => theme::bar::PURPLE,
            Self::Ranked | Self::Leaderboard => theme::bar::ORANGE,
            Self::Friends => theme::bar::BLUE,
            Self::Settings => theme::bar::YELLOW,
            Self::Logout | Self::Back => theme::bar::RED,
        }
    }
}

fn items(screen: Screen, logged_in: bool) -> &'static [MenuItem] {
    match (screen, logged_in) {
        (Screen::PlayMenu, true) => &[MenuItem::Solo, MenuItem::Ranked, MenuItem::Multiplayer, MenuItem::Back],
        (Screen::PlayMenu, false) => &[MenuItem::Solo, MenuItem::Multiplayer, MenuItem::Back],
        (_, true) => &[
            MenuItem::Play,
            MenuItem::Leaderboard,
            MenuItem::Friends,
            MenuItem::Help,
            MenuItem::Settings,
            MenuItem::Logout,
        ],
        (_, false) => &[
            MenuItem::Play,
            MenuItem::Leaderboard,
            MenuItem::Help,
            MenuItem::Settings,
        ],
    }
}

const ROW_H: f32 = 64.0;

/// Where the rows start: a little below the middle, higher on a short screen
/// so the last one stays on it.
fn menu_top(view: View, rows: usize) -> f32 {
    (view.h / 2.0 - 60.0).min(view.h - rows as f32 * ROW_H - 16.0)
}

/// The menu's rows, stacked with no gap under the title.
fn menu_rows(view: View, items: &'static [MenuItem]) -> impl Iterator<Item = (MenuItem, Rect)> {
    let top = menu_top(view, items.len());
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

pub fn update_menu(app: &App, state: &mut State) {
    let view = state.ui.view();
    let ww = view.w;
    let logged_in = state.auth.is_some();

    let clicked = menu_rows(view, items(state.screen, logged_in)).find(|&(_, row)| state.ui.bar_clicked(row));
    match clicked.map(|(item, _)| item) {
        Some(MenuItem::Play) => {
            state.notice.clear();
            state.screen = Screen::PlayMenu;
        }
        Some(MenuItem::Multiplayer | MenuItem::Ranked) if state.too_old => {
            state.notice = Status::error("Mettez d'abord le jeu à jour, avec le bouton en haut à droite.");
        }
        Some(MenuItem::Multiplayer) => start_play(state),
        Some(MenuItem::Ranked) => crate::ranked::enter(state),
        Some(MenuItem::Leaderboard) => crate::leaderboard::enter(state),
        Some(MenuItem::Help) => crate::help::enter(state),
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
        Some(MenuItem::Back) => state.screen = Screen::Menu,
        None => {
            if state.screen == Screen::PlayMenu && app.keyboard.was_pressed(KeyCode::Escape) {
                state.screen = Screen::Menu;
            }
        }
    }

    if state.outdated && state.ui.clicked(outdated_btn(ww)) {
        crate::update::apply();
    }

    let profile_pad = state.pads.pressed(crate::pads::Pad::North);
    if logged_in && (state.ui.clicked(avatar_rect(ww)) || profile_pad) {
        crate::profile::enter_profile(state, Screen::Menu);
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
    state.conn.disconnect();
    state.room = None;
    clear_auth(state);
    state.auth_form = AuthForm::default();
    state.screen = Screen::Auth;
}

pub fn clear_auth(state: &mut State) {
    crate::state::clear_stored_token();
    state.auth = None;
    state.solo_best = crate::state::load_best_score();
    state.solo_best_slot = None;
    state.friends = None;
    state.profile = None;
    state.other_profile = None;
}

pub fn draw_menu(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let (ww, wh) = state.ui.view().size();
    let mut draw = state.ui.screen_canvas(gfx);

    draw.sharp_text(&state.fonts.display, "Rouillo")
        .position(
            ww / 2.0,
            menu_top(state.ui.view(), items(state.screen, state.auth.is_some()).len()) - 80.0,
        )
        .size(theme::size::HERO)
        .h_align_center()
        .v_align_middle()
        .color(pal.title);

    for (item, row) in menu_rows(state.ui.view(), items(state.screen, state.auth.is_some())) {
        state
            .ui
            .menu_bar(&mut draw, &state.fonts, row, item.label(), item.color());
    }
    if state.outdated {
        let btn = outdated_btn(ww);
        state.ui.button(&mut draw, &state.fonts, btn, "Mettre à jour");
        if let Some(text) = crate::update::progress() {
            draw.sharp_text(&state.fonts.text, &text)
                .position(btn.x + btn.w, btn.y + btn.h + 18.0)
                .size(theme::size::SMALL)
                .h_align_right()
                .v_align_middle()
                .color(pal.text_dim);
        }
    }

    if let Some(auth) = &state.auth {
        let avatar = avatar_rect(ww);
        let picture = state.images.get_opt(gfx, auth.avatar_url.as_deref());
        state
            .ui
            .avatar(&mut draw, &state.fonts, avatar, &auth.username, picture.as_ref());
        if state.ui.pad_active() {
            let (x, y) = (avatar.x + 6.0, avatar.y + avatar.h - 6.0);
            draw.circle(14.0).position(x, y).color(theme::GOLD);
            draw.sharp_text(&state.fonts.display, crate::pads::Pad::North.label())
                .position(x, y)
                .size(theme::size::LABEL)
                .h_align_center()
                .v_align_middle()
                .color(Color::BLACK);
        }
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
    let capturing = state.bindings_panel.capturing();
    if state
        .bindings_panel
        .update(app, &state.ui, &state.pads, &mut state.controls.bindings)
    {
        state.controls.bindings.save();
    }
    if capturing {
        return;
    }
    let panel = settings_panel(view);
    let before = (state.settings.das_delay, state.settings.das_speed);
    for i in 0..Settings::COUNT {
        if state.ui.clicked(panel.stepper(i).minus) {
            state.settings.adjust(i, -1);
        }
        if state.ui.clicked(panel.stepper(i).plus) {
            state.settings.adjust(i, 1);
        }
    }
    if (state.settings.das_delay, state.settings.das_speed) != before {
        state.settings.save();
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

    state
        .bindings_panel
        .draw(&mut draw, &state.ui, &state.fonts, &state.controls.bindings);
    state.ui.button(&mut draw, &state.fonts, settings_back(view), "Retour");
    state.ui.render(gfx, &draw);
}

fn start_play(state: &mut State) {
    state.conn.connect(crate::connection::now_secs());
    state.rooms.clear();
    state.notice.clear();
    state.screen = Screen::RoomBrowser;
}
