use aero::geom::Color;
use aero::math::vec2;

use crate::nav::Transition;
use crate::state::{AuthForm, Screen, Settings, State};
use crate::tick::Tick;
use crate::ui::{widgets, Face, Frame, Gui, Rect, SettingsPanel, Status, Typeset, View};
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
    Rect::at(0.0, avatar.y(), avatar.x() - 20.0, avatar.h()).right_strip(190.0).middle(50.0)
}

/// What a menu does with the keys and the gamepad; its clicks are in [`show_menu`].
pub fn update_menu(tick: &Tick, state: &mut State, play: bool) -> Transition {
    if play && tick.back() {
        return Transition::To(Screen::Menu);
    }
    if state.auth.is_some() && state.input.pads.pressed(crate::pads::Pad::North) {
        return crate::profile::enter_profile(state);
    }
    Transition::Stay
}

fn choose(item: MenuItem, state: &mut State) -> Transition {
    match item {
        MenuItem::Play => {
            state.notice.clear();
            Transition::To(Screen::PlayMenu)
        }
        MenuItem::Multiplayer | MenuItem::Ranked if state.net.too_old => {
            state.notice = Status::error("Mettez d'abord le jeu à jour, avec le bouton en haut à droite.");
            Transition::Stay
        }
        MenuItem::Multiplayer => start_play(state),
        MenuItem::Ranked => crate::ranked::enter(state),
        MenuItem::Leaderboard => crate::leaderboard::enter(state),
        MenuItem::Help => crate::help::enter(),
        MenuItem::Solo => {
            state.notice.clear();
            Transition::To(Screen::SoloSetup)
        }
        MenuItem::Friends => crate::friends::enter_friends(state),
        MenuItem::Settings => Transition::To(Screen::Settings),
        MenuItem::Logout => do_logout(state),
        MenuItem::Back => Transition::To(Screen::Menu),
    }
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

pub fn show_menu(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State, play: bool) -> Transition {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let (ww, wh) = view.size();
    let items = items(play, state.auth.is_some());
    let mut chosen = None;

    let mut canvas = gui.ui.screen_canvas(ui);
    let ui = &mut *canvas;
    ui.cx
        .fonts
        .write(ui.draw, Face::Display, "Rouillo")
        .center(ww / 2.0, menu_top(view, items.len()) - 80.0)
        .size(theme::size::HERO)
        .color(pal.title);
    for (item, row) in menu_rows(view, items) {
        if widgets::menu_bar(ui, gui, row, (item.label(), item.color()), true) {
            chosen = Some(item);
        }
    }

    if state.outdated {
        let btn = outdated_btn(ww);
        if widgets::button(ui, "update", btn, "Mettre à jour", true) {
            crate::update::apply();
        }
        if let Some((text, done)) = crate::update::progress() {
            let bar = Rect::at(btn.x(), btn.y() + btn.h() + 10.0, btn.w(), 6.0);
            let y = btn.y() + btn.h() + if done.is_some() { 30.0 } else { 18.0 };
            if let Some(done) = done {
                ui.draw.rect(bar).radius(3.0).color(pal.text_dim.with_alpha(0.3));
                ui.draw.rect(Rect::at(bar.x(), bar.y(), bar.w() * done, bar.h())).radius(3.0).color(pal.title);
            }
            ui.cx
                .fonts
                .write(ui.draw, Face::Text, &text)
                .center(btn.x() + btn.w() / 2.0, y)
                .size(theme::size::SMALL)
                .color(pal.text_dim);
        }
    }

    let mut profile = false;
    if let Some(auth) = &state.auth {
        let avatar = avatar_rect(ww);
        let picture = gui.images.get_opt(auth.avatar_url.as_deref());
        let (x, y) = (avatar.x() + 6.0, avatar.y() + avatar.h() - 6.0);

        profile = widgets::avatar(ui, gui, avatar, &auth.username, picture.as_ref());
        if gui.ui.pad_active() {
            ui.draw.circle(vec2(x, y), 14.0).color(theme::GOLD);
            ui.cx
                .fonts
                .write(ui.draw, Face::Display, crate::pads::Pad::North.label())
                .center(x, y)
                .size(theme::size::LABEL)
                .color(Color::BLACK);
        }
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, &auth.username)
            .center(avatar.x() + avatar.w() / 2.0, avatar.y() + avatar.h() + 16.0)
            .size(theme::size::BODY)
            .color(pal.text_dim);
    }

    if let Some((msg, color)) = state.notice.shown(&pal) {
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, msg)
            .center(ww / 2.0, wh - 60.0)
            .size(theme::size::EMPHASIS)
            .color(color);
    }
    crate::title::draw_version(ui.draw, &gui.ui, &mut ui.cx.fonts);

    if let Some(item) = chosen {
        return choose(item, state);
    }
    if profile {
        return crate::profile::enter_profile(state);
    }
    Transition::Stay
}

fn settings_panel(view: View) -> SettingsPanel {
    SettingsPanel::new(view, Settings::COUNT)
}

fn settings_back(view: View) -> Rect {
    Frame::of(view).back
}

/// What the settings do with the keys and the gamepad; their clicks are in [`show_settings`].
pub fn update_settings(tick: &Tick, state: &mut State) -> Transition {
    let capturing = state.bindings_panel.capturing();
    if state.bindings_panel.update(tick, &state.input.pads, &mut state.input.controls.bindings) {
        state.input.controls.bindings.save();
    }
    if !capturing && tick.back() {
        return Transition::To(Screen::Menu);
    }
    Transition::Stay
}

pub fn show_settings(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let panel = settings_panel(view);

    let mut canvas = gui.ui.screen_canvas(ui);
    let ui = &mut *canvas;
    gui.ui.page_header(ui.draw, &mut ui.cx.fonts, "Paramètres");
    panel.draw(ui.draw, &pal, &mut ui.cx.fonts, "Contrôles");
    let before = state.settings;
    for i in 0..Settings::COUNT {
        let value = format!("{:.0} ms", state.settings.value(i) * 1000.0);
        let step = widgets::stepper(ui, gui, ("setting", i), panel.stepper(i), (Settings::label(i), &value));
        if step != 0 && !state.bindings_panel.capturing() {
            state.settings.adjust(i, step);
        }
    }
    if state.settings != before {
        state.settings.save();
    }

    if state.bindings_panel.show(ui, gui, &mut state.input.controls.bindings) {
        state.input.controls.bindings.save();
    }
    let back = widgets::button(ui, "back", settings_back(view), "Retour", true);
    if back && !state.bindings_panel.capturing() {
        return Transition::To(Screen::Menu);
    }
    Transition::Stay
}

fn start_play(state: &mut State) -> Transition {
    state.net.conn.connect(crate::connection::now_secs());
    state.rooms.clear();
    state.notice.clear();

    Transition::To(Screen::RoomBrowser)
}
