use aero::draw::Draw;
use aero::geom::Color;
use aero::math::vec2;

use crate::nav::Transition;
use crate::state::{AuthForm, Screen, Settings, State};
use crate::tick::Tick;
use crate::ui::{Face, Frame, Gui, Rect, SettingsPanel, Status, View};
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

pub fn update_menu(tick: &Tick, gui: &mut Gui, state: &mut State, play: bool) -> Transition {
    let view = gui.ui.view();
    let ww = view.w;
    let logged_in = state.auth.is_some();

    let clicked = menu_rows(view, items(play, logged_in)).find(|&(_, row)| gui.ui.bar_clicked(row));
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
            if play && tick.back() {
                return Transition::To(Screen::Menu);
            }
        }
    }

    if state.outdated && gui.ui.clicked(outdated_btn(ww)) {
        crate::update::apply();
    }

    let profile_pad = state.input.pads.pressed(crate::pads::Pad::North);
    if logged_in && (gui.ui.clicked(avatar_rect(ww)) || profile_pad) {
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

pub fn draw_menu(draw: &mut Draw, gui: &mut Gui, state: &State, play: bool) {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let (ww, wh) = view.size();
    let items = items(play, state.auth.is_some());

    let mut canvas = gui.ui.screen_canvas(draw);
    let draw = &mut *canvas;
    gui.fonts
        .write(draw, Face::Display, "Rouillo")
        .center(ww / 2.0, menu_top(view, items.len()) - 80.0)
        .size(theme::size::HERO)
        .color(pal.title);
    for (item, row) in menu_rows(view, items) {
        gui.ui.menu_bar(draw, &mut gui.fonts, row, item.label(), item.color());
    }

    if state.outdated {
        let btn = outdated_btn(ww);
        gui.ui.button(draw, &mut gui.fonts, btn, "Mettre à jour");
        if let Some((text, done)) = crate::update::progress() {
            let bar = Rect::at(btn.x, btn.y + btn.h + 10.0, btn.w, 6.0);
            let y = btn.y + btn.h + if done.is_some() { 30.0 } else { 18.0 };
            if let Some(done) = done {
                draw.rect(bar).radius(3.0).color(pal.text_dim.with_alpha(0.3));
                draw.rect(Rect::at(bar.x, bar.y, bar.w * done, bar.h)).radius(3.0).color(pal.title);
            }
            gui.fonts
                .write(draw, Face::Text, &text)
                .center(btn.x + btn.w / 2.0, y)
                .size(theme::size::SMALL)
                .color(pal.text_dim);
        }
    }

    if let Some(auth) = &state.auth {
        let avatar = avatar_rect(ww);
        let picture = gui.images.get_opt(auth.avatar_url.as_deref());
        let (x, y) = (avatar.x + 6.0, avatar.y + avatar.h - 6.0);

        gui.ui.avatar(draw, &mut gui.fonts, avatar, &auth.username, picture.as_ref());
        if gui.ui.pad_active() {
            draw.circle(vec2(x, y), 14.0).color(theme::GOLD);
            gui.fonts
                .write(draw, Face::Display, crate::pads::Pad::North.label())
                .center(x, y)
                .size(theme::size::LABEL)
                .color(Color::BLACK);
        }
        gui.fonts
            .write(draw, Face::Text, &auth.username)
            .center(avatar.x + avatar.w / 2.0, avatar.y + avatar.h + 16.0)
            .size(theme::size::BODY)
            .color(pal.text_dim);
    }

    if let Some((msg, color)) = state.notice.shown(&pal) {
        gui.fonts.write(draw, Face::Text, msg).center(ww / 2.0, wh - 60.0).size(theme::size::EMPHASIS).color(color);
    }

    crate::title::draw_version(draw, &gui.ui, &mut gui.fonts);
}

fn settings_panel(view: View) -> SettingsPanel {
    SettingsPanel::new(view, Settings::COUNT)
}

fn settings_back(view: View) -> Rect {
    Frame::of(view).back
}

pub fn update_settings(tick: &Tick, gui: &mut Gui, state: &mut State) -> Transition {
    let view = gui.ui.view();
    let panel = settings_panel(view);
    let capturing = state.bindings_panel.capturing();

    if state.bindings_panel.update(tick, &gui.ui, &state.input.pads, &mut state.input.controls.bindings) {
        state.input.controls.bindings.save();
    }
    if capturing {
        return Transition::Stay;
    }

    let before = state.settings;
    for i in 0..Settings::COUNT {
        if gui.ui.clicked(panel.stepper(i).minus) {
            state.settings.adjust(i, -1);
        }
        if gui.ui.clicked(panel.stepper(i).plus) {
            state.settings.adjust(i, 1);
        }
    }
    if state.settings != before {
        state.settings.save();
    }

    if gui.ui.clicked(settings_back(view)) || tick.back() {
        return Transition::To(Screen::Menu);
    }

    Transition::Stay
}

pub fn draw_settings(draw: &mut Draw, gui: &mut Gui, state: &State) {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let panel = settings_panel(view);

    let mut canvas = gui.ui.screen_canvas(draw);
    let draw = &mut *canvas;
    gui.ui.page_header(draw, &mut gui.fonts, "Paramètres");
    panel.draw(draw, &pal, &mut gui.fonts, "Contrôles");
    for i in 0..Settings::COUNT {
        let value = format!("{:.0} ms", state.settings.value(i) * 1000.0);
        gui.ui.stepper(draw, &mut gui.fonts, panel.stepper(i), Settings::label(i), &value, true);
    }

    state.bindings_panel.draw(draw, &mut gui.ui, &mut gui.fonts, &state.input.controls.bindings);
    gui.ui.button(draw, &mut gui.fonts, settings_back(view), "Retour");
}

fn start_play(state: &mut State) -> Transition {
    state.net.conn.connect(crate::connection::now_secs());
    state.rooms.clear();
    state.notice.clear();

    Transition::To(Screen::RoomBrowser)
}
