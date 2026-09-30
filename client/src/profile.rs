use std::borrow::Cow;
use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http;
use crate::profile_edit::{self, EditForm};
use crate::state::{
    ApiFriendsResponse, ApiMatchEntry, ApiUserProfile, FriendEntry, FriendshipStatus, OtherProfileData, ProfileCore,
    ProfileData, Screen, State,
};
use crate::theme::{self, Palette};
use crate::ui::{self, divider, list_row, portrait, Face, Fonts, Pill, Rect, SharpText, Status, Ui, View};

const ABOUT_LINE_H: f32 = 24.0;
const ABOUT_BLOCK_H: f32 = 54.0;

fn panels(ww: f32) -> (f32, f32, f32, f32) {
    let left_x = 40.0;
    let left_w = (ww * 0.36).max(280.0);
    let right_x = left_x + left_w + 30.0;
    let right_w = (ww - right_x - 20.0).max(200.0);
    (left_x, left_w, right_x, right_w)
}

fn button_row_y(wh: f32) -> f32 {
    (wh - 90.0).max(590.0)
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

fn load_core(user_id: String, username: String, elo: i32, token: Option<String>, matches: u32) -> ProfileCore {
    let profile_slot = http::new_slot();
    let history_slot = http::new_slot();
    http::get(
        http::api_url(&format!("users/{user_id}")),
        token.clone(),
        Arc::clone(&profile_slot),
    );
    http::get(
        http::api_url(&format!("users/{user_id}/matches?limit={matches}")),
        token,
        Arc::clone(&history_slot),
    );
    ProfileCore {
        user_id,
        info: ApiUserProfile {
            username,
            elo,
            ..ApiUserProfile::default()
        },
        match_history: Vec::new(),
        profile_slot: Some(profile_slot),
        history_slot: Some(history_slot),
    }
}

fn poll_core_history(core: &mut ProfileCore) {
    let Some(Ok(resp)) = http::take(&mut core.history_slot) else {
        return;
    };
    if let Some(matches) = http::json::<Vec<ApiMatchEntry>>(&resp) {
        core.match_history = matches;
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
const HISTORY_ROWS: usize = 7;
const HISTORY_ROW_H: f32 = 48.0;
const STATUS_GAP: f32 = 56.0;

fn draw_header(draw: &mut Draw, ui: &Ui, fonts: &Fonts, core: &ProfileCore) {
    let pal = ui.palette();
    let view = ui.view();
    ui.header_band(draw, Rect::at(0.0, 0.0, view.w, PROFILE_HEADER_H));
    let radius = 48.0;
    let (px, py) = (60.0 + radius, PROFILE_HEADER_H / 2.0);
    portrait(draw, &pal, fonts, (px, py), radius, &core.info.username, 0.0);
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

fn cards(view: View) -> (Rect, Rect) {
    let (left_x, left_w, right_x, right_w) = panels(view.w);
    let h = button_row_y(view.h) - STATUS_GAP - CARD_TOP;
    (
        Rect::at(left_x, CARD_TOP, left_w, h),
        Rect::at(right_x, CARD_TOP, right_w, h),
    )
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

    let winrate = if core.info.total_matches > 0 {
        core.info.wins * 100 / core.info.total_matches
    } else {
        0
    };
    let stats = [
        ("Matchs", core.info.total_matches.to_string()),
        ("Victoires", core.info.wins.to_string()),
        ("Winrate", format!("{winrate}%")),
        ("Max chain", core.info.all_time_max_chain.to_string()),
        ("Nuisance", core.info.total_nuisance_sent.to_string()),
    ];
    let (label_x, value_x) = (card.x + CARD_PAD, card.x + card.w - CARD_PAD);
    let first = card.y + CARD_TITLE_H + 30.0;
    for (i, (label, value)) in stats.iter().enumerate() {
        let y = first + i as f32 * 32.0;
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

    let sep_y = first + stats.len() as f32 * 32.0 - 6.0;
    divider(draw, &pal, label_x, sep_y, card.w - 2.0 * CARD_PAD);

    let mut info_y = sep_y + 24.0;
    let room = card.w - 2.0 * CARD_PAD;
    let bio = core.info.bio.as_deref().filter(|v| !v.is_empty());
    let music = core.info.favorite_music.as_deref().filter(|v| !v.is_empty());
    let mut about = |label: &str, lines: &[Cow<str>], color: Color| {
        draw.sharp_text(&fonts.text, label)
            .position(label_x, info_y)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(pal.text_dim);
        for (i, line) in lines.iter().enumerate() {
            draw.sharp_text(&fonts.text, line)
                .position(label_x, info_y + 22.0 + i as f32 * ABOUT_LINE_H)
                .size(theme::size::BODY)
                .v_align_middle()
                .color(color);
        }
        info_y += ABOUT_BLOCK_H + (lines.len().max(1) - 1) as f32 * ABOUT_LINE_H;
    };
    if let Some(bio) = bio {
        let bottom = card.y + card.h - CARD_PAD - if music.is_some() { ABOUT_BLOCK_H } else { 0.0 };
        let max_lines = ((bottom - sep_y - 24.0 - 22.0) / ABOUT_LINE_H) as usize;
        about("Bio", &bio_lines(fonts, bio, room, max_lines.max(1)), pal.text);
    }
    if let Some(music) = music {
        about(
            "Musique",
            &[fonts.fit(Face::Text, music, theme::size::BODY, room)],
            pal.accent,
        );
    }
}

fn bio_lines<'a>(fonts: &Fonts, bio: &'a str, room: f32, max: usize) -> Vec<Cow<'a, str>> {
    let mut lines: Vec<Cow<str>> = fonts
        .wrap(Face::Text, bio, theme::size::BODY, room)
        .into_iter()
        .map(Cow::Borrowed)
        .collect();
    if lines.len() > max {
        lines.truncate(max);
        if let Some(last) = lines.last_mut() {
            let cut = format!("{}…", last.trim_end());
            *last = Cow::Owned(fonts.fit(Face::Text, &cut, theme::size::BODY, room).into_owned());
        }
    }
    lines
}

fn draw_history_panel(draw: &mut Draw, ui: &Ui, fonts: &Fonts, core: &ProfileCore, clickable: bool) {
    let pal = ui.palette();
    let (_, card) = cards(ui.view());
    ui::card(draw, &pal, card);
    card_title(draw, &pal, fonts, card, "Derniers matchs");

    if core.history_slot.is_some() {
        card_message(draw, fonts, card, "Chargement...", pal.text_muted);
    } else if core.match_history.is_empty() {
        card_message(draw, fonts, card, "Aucun match pour l'instant.", pal.text_muted);
    } else {
        for (i, m) in core.match_history.iter().enumerate().take(HISTORY_ROWS) {
            draw_match_row(draw, ui, fonts, (history_row(card, i), i), m, &core.user_id, clickable);
        }
    }
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
    Rect::at(row.x + row.w * 0.2, row.y, row.w * 0.3, row.h)
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
    let won = m.winner_slot == if i_am_p1 { 1i16 } else { 2i16 };
    let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
    let me = if i_am_p1 { &m.player1 } else { &m.player2 };
    let opp_name = opp.username.as_deref().unwrap_or("Invité");

    list_row(draw, &pal, row, index);

    let result = Pill {
        text: if won { "VICTOIRE" } else { "DÉFAITE" },
        color: if won { theme::SUCCESS } else { theme::DANGER },
        size: theme::size::SMALL,
    };
    result.draw(draw, fonts, (row.x + row.w * 0.1 - result.width(fonts) / 2.0, mid));

    let linked = clickable && opp.user_id.is_some();
    ui.link(draw, fonts, opponent_zone(row), &format!("vs {opp_name}"), linked);

    draw.sharp_text(
        &fonts.text,
        &format!("Chain x{}  Nuis {}", me.max_chain, me.nuisance_sent),
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

pub fn enter_profile(state: &mut State) {
    let Some(auth) = &state.auth else {
        return;
    };
    let core = load_core(
        auth.user_id.clone(),
        auth.username.clone(),
        auth.elo,
        Some(auth.token.clone()),
        10,
    );
    state.profile = Some(ProfileData { core, edit: None });
}

fn own_buttons(cx: f32, wh: f32) -> (Rect, Rect, Rect) {
    let y = button_row_y(wh);
    (
        Rect::at(cx - 390.0, y, 200.0, 54.0),
        Rect::at(cx - 100.0, y, 200.0, 54.0),
        Rect::at(cx + 190.0, y, 200.0, 54.0),
    )
}

pub fn update_profile(app: &mut App, state: &mut State) {
    if let Some(p) = state.profile.as_mut() {
        poll_core_profile(&mut p.core);
        poll_core_history(&mut p.core);
    }
    profile_edit::poll(state);

    let (ww, wh) = state.ui.view().size();
    let cx = ww / 2.0;

    if state.profile.as_ref().is_some_and(|p| p.edit.is_some()) {
        profile_edit::update(app, state);
    } else {
        let (back_btn, edit_btn, logout_btn) = own_buttons(cx, wh);

        if state.ui.clicked(back_btn) || app.keyboard.was_pressed(KeyCode::Escape) {
            state.profile = None;
            state.screen = Screen::Menu;
            return;
        }
        if state.ui.clicked(edit_btn) {
            if let Some(p) = state.profile.as_mut() {
                p.edit = Some(EditForm::open(&p.core.info));
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
            for (i, m) in profile.core.match_history.iter().enumerate().take(HISTORY_ROWS) {
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

pub fn draw_profile(gfx: &mut Graphics, state: &State) {
    let (ww, wh) = state.ui.view().size();
    let cx = ww / 2.0;
    let mut draw = state.ui.screen_canvas(gfx);

    let Some(profile) = &state.profile else {
        state.ui.render(gfx, &draw);
        return;
    };

    draw_header(&mut draw, &state.ui, &state.fonts, &profile.core);

    if let Some(form) = &profile.edit {
        profile_edit::draw(&state.ui, &mut draw, &state.fonts, form, cx);
    } else {
        draw_stats_panel(&mut draw, &state.ui, &state.fonts, &profile.core, false);
        draw_history_panel(&mut draw, &state.ui, &state.fonts, &profile.core, true);

        let (back_btn, edit_btn, logout_btn) = own_buttons(cx, wh);
        state.ui.button(&mut draw, &state.fonts, back_btn, "Retour");
        state.ui.button(&mut draw, &state.fonts, edit_btn, "Modifier");
        state.ui.button(&mut draw, &state.fonts, logout_btn, "Déconnexion");
    }

    state.ui.render(gfx, &draw);
}

pub fn enter_other_profile(state: &mut State, user_id: String, username: String, prev: Screen) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let core = load_core(user_id, username, 0, token.clone(), 8);

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
    let Some(Ok(resp)) = http::take(&mut p.friendship_check_slot) else {
        return;
    };
    if let Some(data) = http::json::<ApiFriendsResponse>(&resp) {
        p.friendship = friendship_with(&data.friends, &data.sent, &data.received, &p.core.user_id);
    }
}

fn poll_friend(state: &mut State) {
    let Some(p) = state.other_profile.as_mut() else { return };
    let Some(result) = http::take(&mut p.friend_slot) else {
        return;
    };
    match result {
        Ok(resp) if resp.status == 201 => {
            p.friend_status = Status::success("Demande envoyée !");
            p.friendship = FriendshipStatus::RequestSent;
        }
        Ok(resp) => {
            p.friend_status = Status::error(http::error_message(&resp));
        }
        Err(e) => {
            p.friend_status = Status::error(format!("Erreur réseau: {e}"));
        }
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

fn other_buttons(cx: f32, wh: f32) -> (Rect, Rect) {
    let y = button_row_y(wh);
    (
        Rect::at(cx - 110.0, y, 220.0, 54.0),
        Rect::at(cx + 140.0, y, 200.0, 54.0),
    )
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
        poll_core_history(&mut p.core);
    }
    poll_friend(state);

    let ww = state.ui.view().w;
    let wh = state.ui.view().h;
    let cx = ww / 2.0;
    let (add_btn, back_btn) = other_buttons(cx, wh);

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
    let ww = state.ui.view().w;
    let wh = state.ui.view().h;
    let cx = ww / 2.0;

    let mut draw = state.ui.screen_canvas(gfx);

    let Some(p) = &state.other_profile else {
        state.ui.render(gfx, &draw);
        return;
    };

    draw_header(&mut draw, &state.ui, &state.fonts, &p.core);
    draw_stats_panel(&mut draw, &state.ui, &state.fonts, &p.core, p.load_failed);
    draw_history_panel(&mut draw, &state.ui, &state.fonts, &p.core, false);

    let btn_y = button_row_y(wh);
    let sending = p.friend_slot.is_some();
    let (add_btn, back_btn) = other_buttons(cx, wh);

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

    state.ui.render(gfx, &draw);
}
