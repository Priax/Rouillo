use std::sync::Arc;

use aero::draw::Draw;
use aero::geom::Color;
use aero::image::Image;

use crate::account::{self, AccountForm, Outcome};
use crate::history::History;
use crate::nav::Transition;
use crate::profile_edit::{self, EditForm};
use crate::state::{
    ApiFriendsResponse, ApiMatchEntry, ApiUserProfile, FriendEntry, FriendsData, FriendshipStatus, OtherProfileData,
    ProfileCore, ProfileData, Screen, State,
};
use crate::theme::{self, Palette};
use crate::tick::Tick;
use crate::ui::{
    self, divider, list_row, portrait, px, widgets, Face, Fonts, Gui, Icon, Pill, Rect, Status, Typeset, View, FILL,
    PAGER_H,
};
use crate::{http, profile_about};

const PROFILE_HEADER_H: f32 = 150.0;
const CARD_TOP: f32 = 180.0;
const CARD_PAD: f32 = 20.0;
const CARD_TITLE_H: f32 = 56.0;
const HISTORY_ROW_H: f32 = 48.0;
const STATUS_GAP: f32 = 56.0;
const STAT_ROWS: usize = 4;
const STAT_ROW_H: f32 = 32.0;

const BUTTON_H: f32 = 54.0;
const BUTTON_STEP: f32 = 64.0;
const STATS_CARD_H: f32 = 340.0;
/// Below this width, a row shows neither the statistics nor the length.
const WIDE_ROW: f32 = 620.0;

/// The bottom buttons sit near the bottom of the window, never below it: on a
/// short screen the cards shrink, down to what the four stats need.
fn button_row_y(wh: f32) -> f32 {
    let lowest_card_bottom = CARD_TOP + CARD_TITLE_H + 30.0 + STAT_ROWS as f32 * STAT_ROW_H + 16.0;
    (wh - 90.0).max(lowest_card_bottom + STATUS_GAP)
}

enum ProfileLoad {
    Loaded,
    HttpError,
    NetworkError,
}

fn poll_core_profile(core: &mut ProfileCore) -> Option<ProfileLoad> {
    match http::take(&mut core.profile_slot)? {
        Ok(resp) if resp.status == 200 => {
            if let Some(info) = http::json::<ApiUserProfile>(&resp) {
                core.info = info;
            }
            Some(ProfileLoad::Loaded)
        }
        Ok(_) => Some(ProfileLoad::HttpError),
        Err(_) => Some(ProfileLoad::NetworkError),
    }
}

fn load_core(user_id: String, username: String, elo: i32, token: Option<String>) -> ProfileCore {
    let profile_slot = http::get_user(&user_id, token.clone());
    ProfileCore {
        history: History::load(user_id.clone(), token),
        user_id,
        info: ApiUserProfile { username, elo, ..ApiUserProfile::default() },
        about_open: false,
        profile_slot: Some(profile_slot),
        replay_status: Status::Empty,
        favorite_slot: None,
    }
}

/// The match list's size, and the answer to a favorite; the clicks are in [`show_history_panel`].
fn update_history(gui: &Gui, core: &mut ProfileCore) {
    core.history.fit(history_rows(gui.ui.view()));
    match http::take_done(&mut core.favorite_slot) {
        Some(Ok(())) => core.history.fetch(),
        Some(Err(e)) => core.replay_status = Status::error(e),
        None => {}
    }
}

/// What a click in the match list asks for.
enum HistoryClick {
    Replay(String, u8),
    Favorite(String, bool),
    Opponent(String, String),
}

fn act_on_history(state: &mut State, core: &mut ProfileCore, click: Option<HistoryClick>) -> Transition {
    match click {
        Some(HistoryClick::Replay(id, slot)) => {
            core.replay_status.clear();
            crate::replay::open(state, &id, slot);
        }
        Some(HistoryClick::Favorite(id, on)) => {
            if let (Some(auth), None) = (state.auth.as_ref(), &core.favorite_slot) {
                core.replay_status = Status::Empty;
                core.favorite_slot = Some(crate::replay::set_favorite(&id, on, auth.token.clone()));
            }
        }
        Some(HistoryClick::Opponent(user_id, username)) => return enter_other_profile(state, user_id, username, None),
        None => {}
    }
    Transition::Stay
}

fn friendship_with(
    friends: &[FriendEntry],
    sent: &[FriendEntry],
    received: &[FriendEntry],
    id: &str,
) -> FriendshipStatus {
    if friends.iter().any(|e| e.user_id == id) {
        FriendshipStatus::Friends
    } else if sent.iter().any(|e| e.user_id == id) {
        FriendshipStatus::RequestSent
    } else if received.iter().any(|e| e.user_id == id) {
        FriendshipStatus::RequestReceived
    } else {
        FriendshipStatus::NotFriends
    }
}

/// Draws `picture` over the whole of `r`, cropped to keep its proportions.
fn cover(draw: &mut Draw, picture: &Image, r: Rect) {
    let (tw, th) = picture.size();
    let (tw, th) = (tw as f32, th as f32);
    let scale = (r.w() / tw).max(r.h() / th);
    let (sw, sh) = (r.w() / scale, r.h() / scale);
    draw.image(picture, r).crop(Rect::at((tw - sw) / 2.0, (th - sh) / 2.0, sw, sh));
}

fn draw_header(draw: &mut Draw, fonts: &mut Fonts, gui: &mut Gui, core: &ProfileCore) {
    let ui = &mut gui.ui;
    let pal = ui.palette();
    let view = ui.view();
    let banner = gui.images.get_opt(core.info.banner_url.as_deref());
    let avatar = gui.images.get_opt(core.info.avatar_url.as_deref());
    let radius = 48.0;
    let (px, py) = (60.0 + radius, PROFILE_HEADER_H / 2.0);
    let who = ui::Persona { name: &core.info.username, glow: 0.0, picture: avatar.as_ref() };
    let text_x = px + radius + 28.0;
    let name = fonts.fit(Face::Display, &core.info.username, theme::size::TITLE, view.w - text_x - 60.0);
    let elo = format!("ELO {}", core.info.elo);
    let rating = Pill { text: &elo, color: theme::GOLD, size: theme::size::BODY };

    ui.header_band(draw, Rect::at(0.0, 0.0, view.w, PROFILE_HEADER_H));
    if let Some(banner) = banner {
        let r = Rect::at(0.0, 0.0, view.w, PROFILE_HEADER_H - 2.0);
        cover(draw, &banner, r);
        draw.rect(r).color(Color::BLACK.with_alpha(0.35));
    }
    portrait(draw, &pal, fonts, (px, py), radius, &who);
    fonts.write(draw, Face::Display, &name).left(text_x, py - 16.0).size(theme::size::TITLE).color(pal.text);
    rating.draw(draw, fonts, (text_x, py + 30.0));
}

/// Where the bottom buttons start: one row in landscape, two upright.
fn buttons_top(view: View) -> f32 {
    if view.portrait() {
        view.h - 90.0 - BUTTON_STEP
    } else {
        button_row_y(view.h)
    }
}

/// The stats card and the match list: side by side, or stacked upright.
fn cards(view: View) -> (Rect, Rect) {
    let area = Rect::at(0.0, CARD_TOP, view.w, buttons_top(view) - STATUS_GAP - CARD_TOP);

    if view.portrait() {
        let [stats, list] = area.inset([0.0, 20.0, 0.0, 20.0]).rows(20.0, [px(STATS_CARD_H), FILL]);
        return (stats, list);
    }

    let stats_w = (view.w * 0.36).max(280.0);
    let [stats, list] = area.inset([0.0, 20.0, 0.0, 40.0]).columns(30.0, [px(stats_w), FILL]);
    (stats, list)
}

/// The bottom buttons, one per width: in one row, or two to a row upright.
fn bottom_buttons<const N: usize>(view: View, widths: [f32; N]) -> [Rect; N] {
    let top = buttons_top(view);
    let cx = view.w / 2.0;

    if view.portrait() {
        let w = (view.w - 60.0) / 2.0;
        return std::array::from_fn(|i| {
            let row = (i / 2) as f32;
            let x = if N - i == 1 && i % 2 == 0 { cx - w / 2.0 } else { 20.0 + (i % 2) as f32 * (w + 20.0) };
            Rect::at(x, top + row * BUTTON_STEP, w, BUTTON_H)
        });
    }

    let gap = 30.0;
    let total: f32 = widths.iter().sum::<f32>() + gap * (N - 1) as f32;
    Rect::centered_on(cx, top, total, BUTTON_H).columns(gap, widths.map(px))
}

fn card_title(draw: &mut Draw, pal: &Palette, fonts: &mut Fonts, card: Rect, title: &str) {
    fonts
        .write(draw, Face::Display, title)
        .left(card.x() + CARD_PAD, card.y() + CARD_TITLE_H / 2.0)
        .size(theme::size::EMPHASIS)
        .color(pal.text_dim);
    divider(draw, pal, card.x() + CARD_PAD, card.y() + CARD_TITLE_H, card.w() - 2.0 * CARD_PAD);
}

fn card_message(draw: &mut Draw, fonts: &mut Fonts, card: Rect, text: &str, color: Color) {
    fonts
        .write(draw, Face::Text, text)
        .center(card.x() + card.w() / 2.0, card.y() + CARD_TITLE_H + 40.0)
        .size(theme::size::LABEL)
        .color(color);
}

/// Returns whether the link to the whole bio was clicked.
fn show_stats_panel(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, core: &ProfileCore, load_failed: bool) -> bool {
    let pal = gui.ui.palette();
    let (card, _) = cards(gui.ui.view());
    let fonts = &mut ui.cx.fonts;
    let draw = &mut *ui.draw;

    ui::card(draw, &pal, card);
    card_title(draw, &pal, fonts, card, "Statistiques");

    if load_failed {
        card_message(draw, fonts, card, "Profil introuvable.", theme::DANGER);
        return false;
    }
    if core.profile_slot.is_some() {
        card_message(draw, fonts, card, "Chargement des stats...", pal.text_muted);
        return false;
    }

    let info = &core.info;
    let stats: [_; STAT_ROWS] = [
        ("Amical (parties)", win_rate(info.casual_wins, info.casual_matches)),
        ("Classé (séries)", win_rate(info.ranked_series_won, info.ranked_series)),
        ("Meilleure chaîne", info.all_time_max_chain.to_string()),
        ("Nuisances envoyées", info.total_nuisance_sent.to_string()),
    ];
    let (label_x, value_x) = (card.x() + CARD_PAD, card.x() + card.w() - CARD_PAD);
    let first = card.y() + CARD_TITLE_H + 30.0;
    let area = about_area(card);
    let summary = profile_about::summary(fonts, &core.info, area, 1);

    for (i, (label, value)) in stats.iter().enumerate() {
        let y = first + i as f32 * STAT_ROW_H;
        fonts.write(draw, Face::Text, label).left(label_x, y).size(theme::size::LABEL).color(pal.text_muted);
        fonts.write(draw, Face::Display, value).right(value_x, y).size(theme::size::EMPHASIS).color(pal.text);
    }

    divider(draw, &pal, label_x, stats_end(card), card.w() - 2.0 * CARD_PAD);
    profile_about::draw_summary(draw, &gui.ui, fonts, &summary, area);
    summary.cut && profile_about::show_more_link(ui, area)
}

fn win_rate(wins: i64, played: i64) -> String {
    if played == 0 {
        return "-".to_string();
    }
    format!("{wins} / {played} ({}%)", wins * 100 / played)
}

fn stats_end(card: Rect) -> f32 {
    card.y() + CARD_TITLE_H + 30.0 + STAT_ROWS as f32 * STAT_ROW_H - 6.0
}

fn about_area(card: Rect) -> Rect {
    let top = stats_end(card) + 24.0;
    Rect::at(card.x() + CARD_PAD, top, card.w() - 2.0 * CARD_PAD, card.y() + card.h() - CARD_PAD - top)
}

/// Closes the whole bio on Escape. Returns whether it is open, which leaves the rest of the screen alone.
fn update_about(tick: &Tick, core: &mut ProfileCore) -> bool {
    if core.about_open && tick.back() {
        core.about_open = false;
        return true;
    }
    core.about_open
}

/// `own`: the viewed player's own profile, whose favorites and opponents can be clicked.
fn show_history_panel(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    core: &mut ProfileCore,
    own: bool,
) -> Option<HistoryClick> {
    let pal = gui.ui.palette();
    let view = gui.ui.view();
    let (_, card) = cards(view);
    let fonts = &mut ui.cx.fonts;

    ui::card(ui.draw, &pal, card);
    card_title(ui.draw, &pal, fonts, card, "Matchs");
    if let Some((msg, color)) = core.replay_status.shown(&pal) {
        fonts
            .write(ui.draw, Face::Text, msg)
            .right(card.x() + card.w() - CARD_PAD, card.y() + CARD_TITLE_H / 2.0)
            .size(theme::size::SMALL)
            .color(color);
    }

    let mut click = None;
    if core.history.slot.is_some() && core.history.entries.is_empty() {
        card_message(ui.draw, fonts, card, "Chargement...", pal.text_muted);
    } else if core.history.entries.is_empty() {
        card_message(ui.draw, fonts, card, "Aucun match pour l'instant.", pal.text_muted);
    } else {
        for (i, m) in core.history.entries.iter().enumerate().take(core.history.per_page) {
            if let Some(c) = show_match_row(ui, gui, (history_row(card, i), i), m, (&core.user_id, own)) {
                click = Some(c);
            }
        }
    }
    if core.info.total_matches > core.history.per_page as i64 {
        core.history.show_pager(ui, gui, pager_area(view), core.info.total_matches);
    }
    click
}
/// How many match rows fit above the pager.
fn history_rows(view: View) -> usize {
    let (_, card) = cards(view);
    let room = card.h() - CARD_TITLE_H - 12.0 - PAGER_H;

    (room / HISTORY_ROW_H).max(1.0) as usize
}

fn pager_area(view: View) -> Rect {
    let (_, card) = cards(view);
    Rect::at(card.x(), card.y() + card.h() - PAGER_H, card.w(), PAGER_H)
}

fn history_row(card: Rect, i: usize) -> Rect {
    Rect::at(
        card.x() + CARD_PAD / 2.0,
        card.y() + CARD_TITLE_H + 12.0 + i as f32 * HISTORY_ROW_H,
        card.w() - CARD_PAD,
        HISTORY_ROW_H - 4.0,
    )
}

fn favorite_zone(row: Rect) -> Rect {
    let margin = if row.w() < WIDE_ROW { 4.0 } else { CARD_PAD + 44.0 };
    Rect::at(row.x() + row.w() - margin - 38.0, row.y() + 5.0, 38.0, row.h() - 10.0)
}

fn replay_zone(row: Rect) -> Rect {
    let star = favorite_zone(row);
    Rect::at(star.x() - 6.0 - 96.0, star.y(), 96.0, star.h())
}

fn opponent_zone(row: Rect) -> Rect {
    Rect::at(row.x() + row.w() * 0.33, row.y(), row.w() * 0.25, row.h())
}

fn show_match_row(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    (row, index): (Rect, usize),
    m: &ApiMatchEntry,
    (viewed_id, own): (&str, bool),
) -> Option<HistoryClick> {
    let me = m.me(viewed_id)?;

    let pal = gui.ui.palette();
    let fonts = &mut ui.cx.fonts;
    let mid = row.y() + row.h() / 2.0;
    let opp = m.opponent(viewed_id);
    let place = format!("{}e", me.place);
    let (text, color) = if me.won {
        ("VICTOIRE", theme::SUCCESS)
    } else if m.drawn() {
        ("ÉGALITÉ", pal.text_dim)
    } else if opp.is_some() {
        ("DÉFAITE", theme::DANGER)
    } else {
        (place.as_str(), theme::DANGER)
    };
    let result = Pill::small(text, color);
    let label = match opp {
        Some(opp) => format!("vs {}", opp.username.as_deref().unwrap_or("Invité")),
        None => format!("{} joueurs", m.players.len()),
    };
    let opponent = opp.and_then(|o| Some((o.user_id.clone()?, o.username.clone()?))).filter(|_| own);
    let wide = row.w() >= WIDE_ROW;
    let star = if m.favorite { Icon::Star } else { Icon::StarOutline };
    let secs = m.duration_secs as u32;

    list_row(ui.draw, &pal, row, index);
    let left = row.x() + row.w() * 0.1 - result.width(fonts) / 2.0;
    result.draw(ui.draw, fonts, (left, mid));
    if m.ranked {
        Pill::small("CLASSÉ", theme::bar::ORANGE).draw(ui.draw, fonts, (row.x() + row.w() * 0.2, mid));
    }
    if wide {
        fonts
            .write(ui.draw, Face::Text, &format!("Chaîne x{}  Nuis. {}", me.max_chain, me.nuisance_sent))
            .right(replay_zone(row).x() - 14.0, mid)
            .size(theme::size::SMALL)
            .color(theme::GOLD);
        fonts
            .write(ui.draw, Face::Text, &format!("{}:{:02}", secs / 60, secs % 60))
            .right(row.x() + row.w() - CARD_PAD, mid)
            .size(theme::size::SMALL)
            .color(pal.text_muted);
    }

    let mut click = None;
    if widgets::link(ui, ("opponent", &m.id), opponent_zone(row), &label, opponent.is_some()) {
        click = opponent.map(|(user_id, username)| HistoryClick::Opponent(user_id, username));
    }
    if m.has_replay && widgets::button(ui, ("replay", &m.id), replay_zone(row), "Revoir", true) {
        click = Some(HistoryClick::Replay(m.id.clone(), u8::try_from(me.slot).unwrap_or(1)));
    }
    if own && (m.has_replay || m.favorite) && widgets::button(ui, ("favorite", &m.id), favorite_zone(row), star, true) {
        click = Some(HistoryClick::Favorite(m.id.clone(), !m.favorite));
    }
    click
}

pub fn enter_profile(state: &State) -> Transition {
    let Some(auth) = &state.auth else {
        return Transition::Stay;
    };
    let core = load_core(auth.user_id.clone(), auth.username.clone(), auth.elo, Some(auth.token.clone()));

    Transition::Push(Screen::Profile(Box::new(ProfileData { core, edit: None, account: None })))
}

fn own_buttons(view: View) -> [Rect; 4] {
    bottom_buttons(view, [200.0; 4])
}

/// What comes back and the keys; the clicks are in [`show_profile`].
pub fn update_profile(tick: &Tick, gui: &mut Gui, state: &mut State, p: &mut ProfileData) -> Transition {
    poll_core_profile(&mut p.core);
    p.core.history.poll();
    profile_edit::poll(state, p);

    if p.edit.is_some() {
        profile_edit::update(tick, p);
        return Transition::Stay;
    }
    if p.account.is_some() {
        return update_account(tick, state, p);
    }
    if update_about(tick, &mut p.core) {
        return Transition::Stay;
    }
    update_history(gui, &mut p.core);
    if tick.back() {
        return Transition::Pop;
    }
    Transition::Stay
}

fn update_account(tick: &Tick, state: &mut State, p: &mut ProfileData) -> Transition {
    let Some(form) = p.account.as_mut() else {
        return Transition::Stay;
    };
    let outcome = account::update(tick, form);
    account_outcome(state, p, outcome)
}

fn account_outcome(state: &mut State, p: &mut ProfileData, outcome: Outcome) -> Transition {
    match outcome {
        Outcome::Stay => {}
        Outcome::Close => p.account = None,
        Outcome::LoggedOut(msg) => {
            let gone = crate::menu::forget_session(state);
            state.auth_form.status = Status::success(msg);
            return gone;
        }
        Outcome::Renamed(name) => {
            p.core.info.username.clone_from(&name);
            if let Some(auth) = state.auth.as_mut() {
                auth.username = name;
            }
        }
    }
    Transition::Stay
}

pub fn show_profile(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State, p: &mut ProfileData) -> Transition {
    let [back_btn, edit_btn, account_btn, logout_btn] = own_buttons(gui.ui.view());
    let token = state.auth.as_ref().map(|a| a.token.clone());

    let mut canvas = gui.ui.screen_canvas(ui);
    let ui = &mut *canvas;
    draw_header(ui.draw, &mut ui.cx.fonts, gui, &p.core);
    if p.edit.is_some() {
        profile_edit::show(ui, gui, token, p);
        return Transition::Stay;
    }
    if let Some(form) = p.account.as_mut() {
        let outcome = account::show(ui, gui, form, token);
        return account_outcome(state, p, outcome);
    }

    let more = show_stats_panel(ui, gui, &p.core, false);
    let click = show_history_panel(ui, gui, &mut p.core, true);
    let back = widgets::button(ui, "back", back_btn, "Retour", true);
    let edit = widgets::button(ui, "edit", edit_btn, "Modifier", true);
    let account = widgets::button(ui, "account", account_btn, "Compte", true);
    let logout = widgets::button(ui, "logout", logout_btn, "Déconnexion", true);
    if p.core.about_open && profile_about::show_overlay(ui, gui, &p.core.info) {
        p.core.about_open = false;
    }

    if more {
        p.core.about_open = true;
    } else if back {
        return Transition::Pop;
    } else if edit {
        p.edit = Some(EditForm::open(&p.core.info));
    } else if account {
        p.account = Some(AccountForm::open());
    } else if logout {
        return crate::menu::do_logout(state);
    }
    act_on_history(state, &mut p.core, click)
}
/// Opens a player's profile. `friends`, when at hand, spares asking whether
/// they are one.
pub fn enter_other_profile(
    state: &State,
    user_id: String,
    username: String,
    friends: Option<&FriendsData>,
) -> Transition {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let core = load_core(user_id, username, 0, token.clone());

    let (friendship, friendship_check_slot) = if let Some(f) = friends {
        (friendship_with(&f.friends, &f.sent, &f.received, &core.user_id), None)
    } else if token.is_some() {
        let slot = http::new_slot();
        http::get(http::api_url("friends"), token, Arc::clone(&slot));
        (FriendshipStatus::Unknown, Some(slot))
    } else {
        (FriendshipStatus::Unknown, None)
    };

    Transition::Push(Screen::OtherProfile(Box::new(OtherProfileData {
        core,
        friendship_check_slot,
        load_failed: false,
        friendship,
        friend_slot: None,
        friend_status: Status::Empty,
    })))
}

fn poll_friendship_check(p: &mut OtherProfileData) {
    if let Some(Ok(data)) = http::take_json::<ApiFriendsResponse>(&mut p.friendship_check_slot) {
        p.friendship = friendship_with(&data.friends, &data.sent, &data.received, &p.core.user_id);
    }
}

fn poll_friend(p: &mut OtherProfileData) {
    let Some(result) = http::take_done(&mut p.friend_slot) else {
        return;
    };
    match result {
        Ok(()) => {
            p.friend_status = Status::success("Demande envoyée !");
            p.friendship = FriendshipStatus::RequestSent;
        }
        Err(msg) => p.friend_status = Status::error(msg),
    }
}

fn add_friend(state: &State, p: &mut OtherProfileData) {
    if p.friend_slot.is_some() {
        return;
    }

    let token = state.auth.as_ref().map(|a| a.token.clone());
    let body = serde_json::json!({ "user_id": p.core.user_id }).to_string();
    let slot = http::new_slot();
    http::post_json(http::api_url("friends"), body, token, Arc::clone(&slot));
    p.friend_slot = Some(slot);
    p.friend_status.clear();
}

fn other_buttons(view: View) -> (Rect, Rect) {
    let [add, back] = bottom_buttons(view, [220.0, 200.0]);
    (add, back)
}

/// What comes back and the keys; the clicks are in [`show_other_profile`].
pub fn update_other_profile(tick: &Tick, gui: &Gui, p: &mut OtherProfileData) -> Transition {
    poll_friendship_check(p);
    match poll_core_profile(&mut p.core) {
        Some(ProfileLoad::HttpError) => {
            p.load_failed = true;
            p.core.info.username = "Inconnu".to_owned();
        }
        Some(ProfileLoad::NetworkError) => {
            p.load_failed = true;
            p.core.info.username = "Erreur réseau".to_owned();
        }
        _ => {}
    }
    p.core.history.poll();
    poll_friend(p);
    if update_about(tick, &mut p.core) {
        return Transition::Stay;
    }
    update_history(gui, &mut p.core);
    if tick.back() {
        return Transition::Pop;
    }
    Transition::Stay
}

pub fn show_other_profile(
    ui: &mut aero::ui::Ui<'_>,
    gui: &mut Gui,
    state: &mut State,
    p: &mut OtherProfileData,
) -> Transition {
    let pal = gui.ui.palette();
    let cx = gui.ui.view().w / 2.0;
    let btn_y = buttons_top(gui.ui.view());
    let sending = p.friend_slot.is_some();
    let (add_btn, back_btn) = other_buttons(gui.ui.view());
    let can_add = !p.load_failed && p.friendship == FriendshipStatus::NotFriends;
    let label = if sending { "Envoi..." } else { "Ajouter en ami" };

    let mut canvas = gui.ui.screen_canvas(ui);
    let ui = &mut *canvas;
    draw_header(ui.draw, &mut ui.cx.fonts, gui, &p.core);
    let more = show_stats_panel(ui, gui, &p.core, p.load_failed);
    let click = show_history_panel(ui, gui, &mut p.core, false);
    let add = can_add && widgets::button(ui, "add", add_btn, label, !sending);
    let back = widgets::button(ui, "back", back_btn, "Retour", true);
    if let Some((msg, color)) = p.friend_status.shown(&pal) {
        ui.cx
            .fonts
            .write(ui.draw, Face::Text, msg)
            .center(cx, btn_y - STATUS_GAP / 2.0)
            .size(theme::size::BODY)
            .color(color);
    }
    if p.core.about_open && profile_about::show_overlay(ui, gui, &p.core.info) {
        p.core.about_open = false;
    }

    if more {
        p.core.about_open = true;
    } else if add {
        add_friend(state, p);
    } else if back {
        return Transition::Pop;
    }
    act_on_history(state, &mut p.core, click)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bottom_buttons_stay_on_a_short_screen() {
        for wh in [560.0, 600.0, 680.0, 800.0] {
            let y = super::button_row_y(wh);
            assert!(y + 54.0 <= wh, "{wh}: buttons end at {}", y + 54.0);
        }
    }
}
