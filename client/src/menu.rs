use notan::draw::DrawShapes;
use notan::prelude::*;

use crate::nav::Transition;
use crate::state::{AuthForm, Screen, Settings, State};
use crate::ui::{Frame, Rect, SettingsPanel, SharpText, Status, View};
use crate::{http, theme};

const ROW_H: f32 = 64.0;
const AVATAR: f32 = 76.0;

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

fn items(play: bool, logged_in: bool) -> &'static [MenuItem] {
    match (play, logged_in) {
        (true, true) => &[MenuItem::Solo, MenuItem::Ranked, MenuItem::Multiplayer, MenuItem::Back],
        (true, false) => &[MenuItem::Solo, MenuItem::Multiplayer, MenuItem::Back],
        (false, true) => &[
            MenuItem::Play,
            MenuItem::Leaderboard,
            MenuItem::Friends,
            MenuItem::Help,
            MenuItem::Settings,
            MenuItem::Logout,
        ],
        (false, false) => &[MenuItem::Play, MenuItem::Leaderboard, MenuItem::Help, MenuItem::Settings],
    }
}

/// Where the rows start: a little below the middle, higher on a short screen
/// so the last one stays on it.
fn menu_top(view: View, rows: usize) -> f32 {
    (view.h / 2.0 - 60.0).min(view.h - rows as f32 * ROW_H - 16.0)
}

/// The menu's rows, stacked with no gap under the title.
fn menu_rows(view: View, items: &'static [MenuItem]) -> impl Iterator<Item = (MenuItem, Rect)> {
    let top = menu_top(view, items.len());
    items.iter().enumerate().map(move |(i, &item)| (item, Rect::at(0.0, top, view.w, 0.0).nth_row(i, ROW_H, 0.0)))
}

fn avatar_rect(ww: f32) -> Rect {
    Rect::at(ww - 70.0 - AVATAR / 2.0, 70.0 - AVATAR / 2.0, AVATAR, AVATAR)
}

fn outdated_btn(ww: f32) -> Rect {
    let avatar = avatar_rect(ww);
    Rect::at(0.0, avatar.y, avatar.x - 20.0, avatar.h).right(190.0).middle(50.0)
}

pub fn update_menu(app: &App, state: &mut State, play: bool) -> Transition {
    let view = state.ui.view();
    let ww = view.w;
    let logged_in = state.auth.is_some();

    let clicked = menu_rows(view, items(play, logged_in)).find(|&(_, row)| state.ui.bar_clicked(row));
    match clicked.map(|(item, _)| item) {
        Some(MenuItem::Play) => {
            state.notice.clear();
            return Transition::To(Screen::PlayMenu);
        }
        Some(MenuItem::Multiplayer | MenuItem::Ranked) if state.net.too_old => {
            state.notice = Status::error("Mettez d'abord le jeu à jour, avec le bouton en haut à droite.");
        }
        Some(MenuItem::Multiplayer) => return start_play(state),
        Some(MenuItem::Ranked) => return crate::ranked::enter(state),
        Some(MenuItem::Leaderboard) => return crate::leaderboard::enter(state),
        Some(MenuItem::Help) => return crate::help::enter(),
        Some(MenuItem::Solo) => {
            state.notice.clear();
            return Transition::To(Screen::SoloSetup);
        }
        Some(MenuItem::Friends) => return crate::friends::enter_friends(state),
        Some(MenuItem::Settings) => return Transition::To(Screen::Settings),
        Some(MenuItem::Logout) => return do_logout(state),
        Some(MenuItem::Back) => return Transition::To(Screen::Menu),
        None => {
            if play && app.keyboard.was_pressed(KeyCode::Escape) {
                return Transition::To(Screen::Menu);
            }
        }
    }

    if state.outdated && state.ui.clicked(outdated_btn(ww)) {
        crate::update::apply();
    }

    let profile_pad = state.input.pads.pressed(crate::pads::Pad::North);
    if logged_in && (state.ui.clicked(avatar_rect(ww)) || profile_pad) {
        return crate::profile::enter_profile(state);
    }

    Transition::Stay
}

pub fn do_logout(state: &mut State) -> Transition {
    if let Some(auth) = &state.auth {
        http::post_empty(http::api_url("logout"), Some(auth.token.clone()), http::new_slot());
    }

    forget_session(state)
}

pub fn forget_session(state: &mut State) -> Transition {
    state.net.conn.disconnect();
    state.room = None;
    state.pending_invitation = None;
    clear_auth(state);
    state.auth_form = AuthForm::default();

    Transition::To(Screen::Auth)
}

pub fn clear_auth(state: &mut State) {
    crate::state::clear_stored_token();
    state.auth = None;
    state.solo.best = crate::state::load_best_score();
    state.solo.best_slot = None;
}

pub fn draw_menu(gfx: &mut Graphics, state: &State, play: bool) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let (ww, wh) = view.size();
    let items = items(play, state.auth.is_some());

    let mut draw = state.ui.screen_canvas(gfx);
    draw.sharp_text(&state.fonts.display, "Rouillo")
        .center(ww / 2.0, menu_top(view, items.len()) - 80.0)
        .size(theme::size::HERO)
        .color(pal.title);
    for (item, row) in menu_rows(view, items) {
        state.ui.menu_bar(&mut draw, &state.fonts, row, item.label(), item.color());
    }

    if state.outdated {
        let btn = outdated_btn(ww);
        state.ui.button(&mut draw, &state.fonts, btn, "Mettre à jour");
        if let Some((text, done)) = crate::update::progress() {
            let bar = Rect::at(btn.x, btn.y + btn.h + 10.0, btn.w, 6.0);
            let y = btn.y + btn.h + if done.is_some() { 30.0 } else { 18.0 };
            if let Some(done) = done {
                draw.rect((bar.x, bar.y), (bar.w, bar.h)).corner_radius(3.0).color(pal.text_dim.with_alpha(0.3));
                draw.rect((bar.x, bar.y), (bar.w * done, bar.h)).corner_radius(3.0).color(pal.title);
            }
            draw.sharp_text(&state.fonts.text, &text)
                .center(btn.x + btn.w / 2.0, y)
                .size(theme::size::SMALL)
                .color(pal.text_dim);
        }
    }

    if let Some(auth) = &state.auth {
        let avatar = avatar_rect(ww);
        let picture = state.images.get_opt(gfx, auth.avatar_url.as_deref());
        let (x, y) = (avatar.x + 6.0, avatar.y + avatar.h - 6.0);

        state.ui.avatar(&mut draw, &state.fonts, avatar, &auth.username, picture.as_ref());
        if state.ui.pad_active() {
            draw.circle(14.0).position(x, y).color(theme::GOLD);
            draw.sharp_text(&state.fonts.display, crate::pads::Pad::North.label())
                .center(x, y)
                .size(theme::size::LABEL)
                .color(Color::BLACK);
        }
        draw.sharp_text(&state.fonts.text, &auth.username)
            .center(avatar.x + avatar.w / 2.0, avatar.y + avatar.h + 16.0)
            .size(theme::size::BODY)
            .color(pal.text_dim);
    }

    if let Some((msg, color)) = state.notice.shown(&pal) {
        draw.sharp_text(&state.fonts.text, msg).center(ww / 2.0, wh - 60.0).size(theme::size::EMPHASIS).color(color);
    }

    crate::title::draw_version(&mut draw, &state.ui, &state.fonts);

    state.ui.render(gfx, &draw);
}

fn settings_panel(view: View) -> SettingsPanel {
    SettingsPanel::new(view, Settings::COUNT)
}

fn settings_back(view: View) -> Rect {
    Frame::of(view).back
}

pub fn update_settings(app: &mut App, state: &mut State) -> Transition {
    let view = state.ui.view();
    let panel = settings_panel(view);
    let capturing = state.bindings_panel.capturing();

    if state.bindings_panel.update(app, &state.ui, &state.input.pads, &mut state.input.controls.bindings) {
        state.input.controls.bindings.save();
    }
    if capturing {
        return Transition::Stay;
    }

    let before = state.settings;
    for i in 0..Settings::COUNT {
        if state.ui.clicked(panel.stepper(i).minus) {
            state.settings.adjust(i, -1);
        }
        if state.ui.clicked(panel.stepper(i).plus) {
            state.settings.adjust(i, 1);
        }
    }
    if state.settings != before {
        state.settings.save();
    }

    if state.ui.clicked(settings_back(view)) || app.keyboard.was_pressed(KeyCode::Escape) {
        return Transition::To(Screen::Menu);
    }

    Transition::Stay
}

pub fn draw_settings(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let panel = settings_panel(view);

    let mut draw = state.ui.screen_canvas(gfx);
    state.ui.page_header(&mut draw, &state.fonts, "Paramètres");
    panel.draw(&mut draw, &pal, &state.fonts, "Contrôles");
    for i in 0..Settings::COUNT {
        let value = format!("{:.0} ms", state.settings.value(i) * 1000.0);
        state.ui.stepper(&mut draw, &state.fonts, panel.stepper(i), Settings::label(i), &value, true);
    }

    state.bindings_panel.draw(&mut draw, &state.ui, &state.fonts, &state.input.controls.bindings);
    state.ui.button(&mut draw, &state.fonts, settings_back(view), "Retour");

    state.ui.render(gfx, &draw);
}

fn start_play(state: &mut State) -> Transition {
    state.net.conn.connect(crate::connection::now_secs());
    state.rooms.clear();
    state.notice.clear();

    Transition::To(Screen::RoomBrowser)
}
