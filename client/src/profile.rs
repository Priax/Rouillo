use std::sync::Arc;

use notan::draw::{Draw, DrawImages, DrawShapes};
use notan::prelude::*;

use crate::account::{self, AccountForm, Outcome};
use crate::history::History;
use crate::profile_edit::{self, EditForm};
use crate::state::{
    ApiFriendsResponse, ApiMatchEntry, ApiUserProfile, FriendEntry, FriendshipStatus, OtherProfileData, ProfileCore,
    ProfileData, Screen, State,
};
use crate::theme::{self, Palette};
use crate::ui::{
    self, divider, list_row, portrait, Face, Fonts, Pill, Rect, SharpText, Status, TextInput, Ui, View, PAGER_H,
};
use crate::{http, profile_about};

fn panels(ww: f32) -> (f32, f32, f32, f32) {
    let left_x = 40.0;
    let left_w = (ww * 0.36).max(280.0);
    let right_x = left_x + left_w + 30.0;
    let right_w = (ww - right_x - 20.0).max(200.0);
    (left_x, left_w, right_x, right_w)
}

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

/// The field the keyboard types into: an open form's, or the match list's
/// page field.
pub fn typing(state: &mut State) -> Option<&mut TextInput> {
    if state.screen == Screen::OtherProfile {
        return state.other_profile.as_mut()?.core.history.typing();
    }
    let p = state.profile.as_mut()?;
    if let Some(form) = p.edit.as_mut() {
        Some(form.typing())
    } else if let Some(form) = p.account.as_mut() {
        form.typing()
    } else {
        p.core.history.typing()
    }
}

fn load_core(user_id: String, username: String, elo: i32, token: Option<String>) -> ProfileCore {
    let profile_slot = http::get_user(&user_id, token.clone());
    ProfileCore {
        history: History::load(user_id.clone(), token),
        user_id,
        info: ApiUserProfile {
            username,
            elo,
            ..ApiUserProfile::default()
        },
        about_open: false,
        profile_slot: Some(profile_slot),
    }
}

fn update_history(app: &App, state: &mut State, other: bool) {
    let area = pager_area(state.ui.view());
    let State {
        profile,
        other_profile,
        ui,
        fonts,
        keys,
        ..
    } = state;
    let core = if other {
        other_profile.as_mut().map(|p| &mut p.core)
    } else {
        profile.as_mut().map(|p| &mut p.core)
    };
    if let Some(core) = core {
        let total = core.info.total_matches;
        core.history
            .update(app, ui, fonts, keys, area, total, history_rows(ui.view()));
    }
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

const PROFILE_HEADER_H: f32 = 150.0;
const CARD_TOP: f32 = 180.0;
const CARD_PAD: f32 = 20.0;
const CARD_TITLE_H: f32 = 56.0;
const HISTORY_ROW_H: f32 = 48.0;
const STATUS_GAP: f32 = 56.0;
const STAT_ROWS: usize = 4;
const STAT_ROW_H: f32 = 32.0;

/// Draws `picture` over the whole of `r`, cropped to keep its proportions.
fn cover(draw: &mut Draw, picture: &Texture, r: Rect) {
    let (tw, th) = picture.size();
    let scale = (r.w / tw).max(r.h / th);
    let (sw, sh) = (r.w / scale, r.h / scale);
    draw.image(picture)
        .position(r.x, r.y)
        .size(r.w, r.h)
        .crop(((tw - sw) / 2.0, (th - sh) / 2.0), (sw, sh));
}

fn draw_header(draw: &mut Draw, gfx: &mut Graphics, state: &State, core: &ProfileCore) {
    let (ui, fonts) = (&state.ui, &state.fonts);
    let pal = ui.palette();
    let view = ui.view();
    ui.header_band(draw, Rect::at(0.0, 0.0, view.w, PROFILE_HEADER_H));
    if let Some(banner) = state.images.get_opt(gfx, core.info.banner_url.as_deref()) {
        let r = Rect::at(0.0, 0.0, view.w, PROFILE_HEADER_H - 2.0);
        cover(draw, &banner, r);
        draw.rect((r.x, r.y), (r.w, r.h)).color(Color::BLACK.with_alpha(0.35));
    }
    let radius = 48.0;
    let (px, py) = (60.0 + radius, PROFILE_HEADER_H / 2.0);
    let avatar = state.images.get_opt(gfx, core.info.avatar_url.as_deref());
    let who = ui::Persona {
        name: &core.info.username,
        glow: 0.0,
        picture: avatar.as_ref(),
    };
    portrait(draw, &pal, fonts, (px, py), radius, &who);
    let text_x = px + radius + 28.0;
    let name = fonts.fit(
        Face::Display,
        &core.info.username,
        theme::size::TITLE,
        view.w - text_x - 60.0,
    );
    draw.sharp_text(&fonts.display, &name)
        .position(text_x, py - 16.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);
    let elo = format!("ELO {}", core.info.elo);
    let rating = Pill {
        text: &elo,
        color: theme::GOLD,
        size: theme::size::BODY,
    };
    rating.draw(draw, fonts, (text_x, py + 30.0));
}

const BUTTON_H: f32 = 54.0;
const BUTTON_STEP: f32 = 64.0;
const STATS_CARD_H: f32 = 340.0;

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
    let bottom = buttons_top(view) - STATUS_GAP;
    if view.portrait() {
        let w = view.w - 40.0;
        let list_top = CARD_TOP + STATS_CARD_H + 20.0;
        return (
            Rect::at(20.0, CARD_TOP, w, STATS_CARD_H),
            Rect::at(20.0, list_top, w, bottom - list_top),
        );
    }
    let (left_x, left_w, right_x, right_w) = panels(view.w);
    let h = bottom - CARD_TOP;
    (
        Rect::at(left_x, CARD_TOP, left_w, h),
        Rect::at(right_x, CARD_TOP, right_w, h),
    )
}

/// The bottom buttons, one per width: in one row, or two to a row upright.
fn bottom_buttons<const N: usize>(view: View, widths: [f32; N]) -> [Rect; N] {
    let top = buttons_top(view);
    let cx = view.w / 2.0;
    if view.portrait() {
        let w = (view.w - 60.0) / 2.0;
        return std::array::from_fn(|i| {
            let row = (i / 2) as f32;
            let x = if N - i == 1 && i % 2 == 0 {
                cx - w / 2.0
            } else {
                20.0 + (i % 2) as f32 * (w + 20.0)
            };
            Rect::at(x, top + row * BUTTON_STEP, w, BUTTON_H)
        });
    }
    let gap = 30.0;
    let total: f32 = widths.iter().sum::<f32>() + gap * (N - 1) as f32;
    let mut x = cx - total / 2.0;
    widths.map(|w| {
        let r = Rect::at(x, top, w, BUTTON_H);
        x += w + gap;
        r
    })
}

fn card_title(draw: &mut Draw, pal: &Palette, fonts: &Fonts, card: Rect, title: &str) {
    draw.sharp_text(&fonts.display, title)
        .position(card.x + CARD_PAD, card.y + CARD_TITLE_H / 2.0)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(pal.text_dim);
    divider(
        draw,
        pal,
        card.x + CARD_PAD,
        card.y + CARD_TITLE_H,
        card.w - 2.0 * CARD_PAD,
    );
}

fn card_message(draw: &mut Draw, fonts: &Fonts, card: Rect, text: &str, color: Color) {
    draw.sharp_text(&fonts.text, text)
        .position(card.x + card.w / 2.0, card.y + CARD_TITLE_H + 40.0)
        .size(theme::size::LABEL)
        .h_align_center()
        .v_align_middle()
        .color(color);
}

fn draw_stats_panel(draw: &mut Draw, ui: &Ui, fonts: &Fonts, core: &ProfileCore, load_failed: bool) {
    let pal = ui.palette();
    let (card, _) = cards(ui.view());
    ui::card(draw, &pal, card);
    card_title(draw, &pal, fonts, card, "Statistiques");

    if load_failed {
        card_message(draw, fonts, card, "Profil introuvable.", theme::DANGER);
        return;
    }
    if core.profile_slot.is_some() {
        card_message(draw, fonts, card, "Chargement des stats...", pal.text_muted);
        return;
    }

    let info = &core.info;
    let stats: [_; STAT_ROWS] = [
        ("Amical (parties)", win_rate(info.casual_wins, info.casual_matches)),
        ("Classé (séries)", win_rate(info.ranked_series_won, info.ranked_series)),
        ("Meilleure chaîne", info.all_time_max_chain.to_string()),
        ("Nuisances envoyées", info.total_nuisance_sent.to_string()),
    ];
    let (label_x, value_x) = (card.x + CARD_PAD, card.x + card.w - CARD_PAD);
    let first = card.y + CARD_TITLE_H + 30.0;
    for (i, (label, value)) in stats.iter().enumerate() {
        let y = first + i as f32 * STAT_ROW_H;
        draw.sharp_text(&fonts.text, label)
            .position(label_x, y)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(pal.text_muted);
        draw.sharp_text(&fonts.display, value)
            .position(value_x, y)
            .size(theme::size::EMPHASIS)
            .h_align_right()
            .v_align_middle()
            .color(pal.text);
    }

    divider(draw, &pal, label_x, stats_end(card), card.w - 2.0 * CARD_PAD);

    let area = about_area(card);
    let summary = profile_about::summary(fonts, &core.info, area, 1);
    profile_about::draw_summary(draw, ui, fonts, &summary, area);
    if summary.cut {
        profile_about::draw_more_link(draw, ui, fonts, area);
    }
}

fn win_rate(wins: i64, played: i64) -> String {
    if played == 0 {
        return "-".to_string();
    }
    format!("{wins} / {played} ({}%)", wins * 100 / played)
}

fn draw_about_overlay(draw: &mut Draw, ui: &Ui, fonts: &Fonts, core: &ProfileCore) {
    if core.about_open {
        profile_about::draw_overlay(draw, ui, fonts, &core.info);
    }
}

fn stats_end(card: Rect) -> f32 {
    card.y + CARD_TITLE_H + 30.0 + STAT_ROWS as f32 * STAT_ROW_H - 6.0
}

fn about_area(card: Rect) -> Rect {
    let top = stats_end(card) + 24.0;
    Rect::at(
        card.x + CARD_PAD,
        top,
        card.w - 2.0 * CARD_PAD,
        card.y + card.h - CARD_PAD - top,
    )
}

fn update_about(app: &App, ui: &Ui, core: &mut ProfileCore) -> bool {
    if core.about_open {
        core.about_open = !profile_about::overlay_closed(app, ui);
        return true;
    }
    core.about_open = ui.pressed(profile_about::MORE);
    core.about_open
}

fn draw_history_panel(draw: &mut Draw, ui: &Ui, fonts: &Fonts, core: &ProfileCore, clickable: bool) {
    let pal = ui.palette();
    let (_, card) = cards(ui.view());
    ui::card(draw, &pal, card);
    card_title(draw, &pal, fonts, card, "Matchs");

    if core.history.slot.is_some() && core.history.entries.is_empty() {
        card_message(draw, fonts, card, "Chargement...", pal.text_muted);
    } else if core.history.entries.is_empty() {
        card_message(draw, fonts, card, "Aucun match pour l'instant.", pal.text_muted);
    } else {
        for (i, m) in core.history.entries.iter().enumerate().take(core.history.per_page) {
            draw_match_row(draw, ui, fonts, (history_row(card, i), i), m, &core.user_id, clickable);
        }
    }
    if core.info.total_matches > core.history.per_page as i64 {
        core.history
            .draw_pager(draw, ui, fonts, pager_area(ui.view()), core.info.total_matches);
    }
}

/// How many match rows fit above the pager.
fn history_rows(view: View) -> usize {
    let (_, card) = cards(view);
    let room = card.h - CARD_TITLE_H - 12.0 - PAGER_H;
    (room / HISTORY_ROW_H).max(1.0) as usize
}

fn pager_area(view: View) -> Rect {
    let (_, card) = cards(view);
    Rect::at(card.x, card.y + card.h - PAGER_H, card.w, PAGER_H)
}

fn history_row(card: Rect, i: usize) -> Rect {
    Rect::at(
        card.x + CARD_PAD / 2.0,
        card.y + CARD_TITLE_H + 12.0 + i as f32 * HISTORY_ROW_H,
        card.w - CARD_PAD,
        HISTORY_ROW_H - 4.0,
    )
}

fn opponent_zone(row: Rect) -> Rect {
    Rect::at(row.x + row.w * 0.33, row.y, row.w * 0.25, row.h)
}

fn history_row_zone(view: View, i: usize) -> Rect {
    let (_, card) = cards(view);
    opponent_zone(history_row(card, i))
}

fn draw_match_row(
    draw: &mut Draw,
    ui: &Ui,
    fonts: &Fonts,
    (row, index): (Rect, usize),
    m: &ApiMatchEntry,
    viewed_id: &str,
    clickable: bool,
) {
    let pal = ui.palette();
    let mid = row.y + row.h / 2.0;
    let i_am_p1 = m.player1.user_id.as_deref() == Some(viewed_id);
    let my_slot = if i_am_p1 { 1i16 } else { 2i16 };
    let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
    let me = if i_am_p1 { &m.player1 } else { &m.player2 };
    let opp_name = opp.username.as_deref().unwrap_or("Invité");

    list_row(draw, &pal, row, index);

    let (text, color) = match m.winner_slot {
        None => ("ÉGALITÉ", pal.text_dim),
        Some(w) if w == my_slot => ("VICTOIRE", theme::SUCCESS),
        Some(_) => ("DÉFAITE", theme::DANGER),
    };
    let result = Pill {
        text,
        color,
        size: theme::size::SMALL,
    };
    result.draw(draw, fonts, (row.x + row.w * 0.1 - result.width(fonts) / 2.0, mid));
    if m.ranked {
        let ranked = Pill {
            text: "CLASSÉ",
            color: theme::bar::ORANGE,
            size: theme::size::SMALL,
        };
        ranked.draw(draw, fonts, (row.x + row.w * 0.2, mid));
    }

    let linked = clickable && opp.user_id.is_some();
    ui.link(draw, fonts, opponent_zone(row), &format!("vs {opp_name}"), linked);

    draw.sharp_text(
        &fonts.text,
        &format!("Chaîne x{}  Nuis. {}", me.max_chain, me.nuisance_sent),
    )
    .position(row.x + row.w * 0.68, mid)
    .size(theme::size::SMALL)
    .h_align_center()
    .v_align_middle()
    .color(theme::GOLD);

    let secs = m.duration_secs as u32;
    draw.sharp_text(&fonts.text, &format!("{}:{:02}", secs / 60, secs % 60))
        .position(row.x + row.w - CARD_PAD, mid)
        .size(theme::size::SMALL)
        .h_align_right()
        .v_align_middle()
        .color(pal.text_muted);
}

pub fn enter_profile(state: &mut State, prev: Screen) {
    let Some(auth) = &state.auth else {
        return;
    };
    let core = load_core(
        auth.user_id.clone(),
        auth.username.clone(),
        auth.elo,
        Some(auth.token.clone()),
    );
    state.profile = Some(ProfileData {
        core,
        prev_screen: prev,
        edit: None,
        account: None,
    });
}

fn own_buttons(view: View) -> [Rect; 4] {
    bottom_buttons(view, [200.0; 4])
}

pub fn update_profile(app: &mut App, state: &mut State) {
    if let Some(p) = state.profile.as_mut() {
        poll_core_profile(&mut p.core);
        p.core.history.poll();
    }
    profile_edit::poll(state);

    if state.profile.as_ref().is_some_and(|p| p.edit.is_some()) {
        profile_edit::update(app, state);
    } else if state.profile.as_ref().is_some_and(|p| p.account.is_some()) {
        update_account(app, state);
    } else {
        if let Some(p) = state.profile.as_mut() {
            if update_about(app, &state.ui, &mut p.core) {
                return;
            }
        }
        update_history(app, state, false);
        let [back_btn, edit_btn, account_btn, logout_btn] = own_buttons(state.ui.view());

        if state.ui.clicked(back_btn) || app.keyboard.was_pressed(KeyCode::Escape) {
            let prev = state.profile.take().map_or(Screen::Menu, |p| p.prev_screen);
            state.screen = prev;
            return;
        }
        if state.ui.clicked(edit_btn) {
            if let Some(p) = state.profile.as_mut() {
                p.edit = Some(EditForm::open(&p.core.info));
            }
        }
        if state.ui.clicked(account_btn) {
            if let Some(p) = state.profile.as_mut() {
                p.account = Some(AccountForm::open());
            }
        }
        if state.ui.clicked(logout_btn) {
            crate::menu::do_logout(state);
            return;
        }

        let opp_target: Option<(String, String)> = 'find: {
            let Some(profile) = &state.profile else {
                break 'find None;
            };
            let my_id = profile.core.user_id.as_str();
            for (i, m) in profile
                .core
                .history
                .entries
                .iter()
                .enumerate()
                .take(profile.core.history.per_page)
            {
                let i_am_p1 = m.player1.user_id.as_deref() == Some(my_id);
                let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
                if let (Some(opp_id), Some(opp_name)) = (&opp.user_id, &opp.username) {
                    if state.ui.clicked(history_row_zone(state.ui.view(), i)) {
                        break 'find Some((opp_id.clone(), opp_name.clone()));
                    }
                }
            }
            None
        };
        if let Some((uid, uname)) = opp_target {
            enter_other_profile(state, uid, uname, Screen::Profile);
            state.screen = Screen::OtherProfile;
        }
    }
}

fn update_account(app: &App, state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let State {
        profile,
        ui,
        fonts,
        keys,
        ..
    } = state;
    let Some(form) = profile.as_mut().and_then(|p| p.account.as_mut()) else {
        return;
    };
    match account::update(app, ui, fonts, keys, form, token) {
        Outcome::Stay => {}
        Outcome::Close => {
            if let Some(p) = profile.as_mut() {
                p.account = None;
            }
        }
        Outcome::LoggedOut(msg) => {
            crate::menu::forget_session(state);
            state.auth_form.status = Status::success(msg);
        }
        Outcome::Renamed(name) => {
            if let Some(p) = profile.as_mut() {
                p.core.info.username.clone_from(&name);
            }
            if let Some(auth) = state.auth.as_mut() {
                auth.username = name;
            }
        }
    }
}

pub fn draw_profile(gfx: &mut Graphics, state: &State) {
    let cx = state.ui.view().w / 2.0;
    let mut draw = state.ui.screen_canvas(gfx);

    let Some(profile) = &state.profile else {
        state.ui.render(gfx, &draw);
        return;
    };

    draw_header(&mut draw, gfx, state, &profile.core);

    if let Some(form) = &profile.edit {
        profile_edit::draw(&state.ui, &mut draw, &state.fonts, form, &profile.core.info, cx);
    } else if let Some(form) = &profile.account {
        account::draw(&state.ui, &mut draw, &state.fonts, form, cx);
    } else {
        state.ui.set_input(!profile.core.about_open);
        draw_stats_panel(&mut draw, &state.ui, &state.fonts, &profile.core, false);
        draw_history_panel(&mut draw, &state.ui, &state.fonts, &profile.core, true);

        let [back_btn, edit_btn, account_btn, logout_btn] = own_buttons(state.ui.view());
        state.ui.button(&mut draw, &state.fonts, back_btn, "Retour");
        state.ui.button(&mut draw, &state.fonts, edit_btn, "Modifier");
        state.ui.button(&mut draw, &state.fonts, account_btn, "Compte");
        state.ui.button(&mut draw, &state.fonts, logout_btn, "Déconnexion");
        draw_about_overlay(&mut draw, &state.ui, &state.fonts, &profile.core);
    }

    state.ui.render(gfx, &draw);
}

pub fn enter_other_profile(state: &mut State, user_id: String, username: String, prev: Screen) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let core = load_core(user_id, username, 0, token.clone());

    let (friendship, friendship_check_slot) = if let Some(f) = state.friends.as_ref() {
        (friendship_with(&f.friends, &f.sent, &f.received, &core.user_id), None)
    } else if token.is_some() {
        let slot = http::new_slot();
        http::get(http::api_url("friends"), token, Arc::clone(&slot));
        (FriendshipStatus::Unknown, Some(slot))
    } else {
        (FriendshipStatus::Unknown, None)
    };

    state.other_profile = Some(OtherProfileData {
        core,
        friendship_check_slot,
        load_failed: false,
        friendship,
        friend_slot: None,
        friend_status: Status::Empty,
        prev_screen: prev,
    });
}

fn poll_friendship_check(state: &mut State) {
    let Some(p) = state.other_profile.as_mut() else { return };
    if let Some(Ok(data)) = http::take_json::<ApiFriendsResponse>(&mut p.friendship_check_slot) {
        p.friendship = friendship_with(&data.friends, &data.sent, &data.received, &p.core.user_id);
    }
}

fn poll_friend(state: &mut State) {
    let Some(p) = state.other_profile.as_mut() else { return };
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

fn add_friend(state: &mut State) {
    if state.other_profile.as_ref().is_none_or(|p| p.friend_slot.is_some()) {
        return;
    }
    let (user_id, token) = (
        state
            .other_profile
            .as_ref()
            .map(|p| p.core.user_id.clone())
            .unwrap_or_default(),
        state.auth.as_ref().map(|a| a.token.clone()),
    );
    let body = serde_json::json!({ "user_id": user_id }).to_string();
    let slot = http::new_slot();
    http::post_json(http::api_url("friends"), body, token, Arc::clone(&slot));
    if let Some(p) = state.other_profile.as_mut() {
        p.friend_slot = Some(slot);
        p.friend_status.clear();
    }
}

fn other_buttons(view: View) -> (Rect, Rect) {
    let [add, back] = bottom_buttons(view, [220.0, 200.0]);
    (add, back)
}

pub fn update_other_profile(app: &mut App, state: &mut State) {
    poll_friendship_check(state);
    if let Some(p) = state.other_profile.as_mut() {
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
    }
    poll_friend(state);
    if let Some(p) = state.other_profile.as_mut() {
        if update_about(app, &state.ui, &mut p.core) {
            return;
        }
    }

    update_history(app, state, true);
    let (add_btn, back_btn) = other_buttons(state.ui.view());

    let sending = state.other_profile.as_ref().is_some_and(|p| p.friend_slot.is_some());
    let can_add = matches!(
        state.other_profile.as_ref().map(|p| (p.friendship, p.load_failed)),
        Some((FriendshipStatus::Unknown | FriendshipStatus::NotFriends, false))
    );
    if state.ui.clicked(add_btn) && !sending && can_add {
        add_friend(state);
    }

    if state.ui.clicked(back_btn) || app.keyboard.was_pressed(KeyCode::Escape) {
        let prev = state.other_profile.as_ref().map_or(Screen::Menu, |p| p.prev_screen);
        state.other_profile = None;
        state.screen = prev;
    }
}

pub fn draw_other_profile(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let cx = state.ui.view().w / 2.0;

    let mut draw = state.ui.screen_canvas(gfx);

    let Some(p) = &state.other_profile else {
        state.ui.render(gfx, &draw);
        return;
    };

    draw_header(&mut draw, gfx, state, &p.core);
    state.ui.set_input(!p.core.about_open);
    draw_stats_panel(&mut draw, &state.ui, &state.fonts, &p.core, p.load_failed);
    draw_history_panel(&mut draw, &state.ui, &state.fonts, &p.core, false);

    let btn_y = buttons_top(state.ui.view());
    let sending = p.friend_slot.is_some();
    let (add_btn, back_btn) = other_buttons(state.ui.view());

    let can_add = !p.load_failed && p.friendship == FriendshipStatus::NotFriends;
    if can_add {
        let label = if sending { "Envoi..." } else { "Ajouter en ami" };
        state
            .ui
            .button_enabled(&mut draw, &state.fonts, add_btn, label, !sending);
    }
    state.ui.button(&mut draw, &state.fonts, back_btn, "Retour");

    if let Some((msg, color)) = p.friend_status.shown(&pal) {
        draw.sharp_text(&state.fonts.text, msg)
            .position(cx, btn_y - STATUS_GAP / 2.0)
            .size(theme::size::BODY)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
    draw_about_overlay(&mut draw, &state.ui, &state.fonts, &p.core);

    state.ui.render(gfx, &draw);
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
