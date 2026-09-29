use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http;
use crate::state::{
    ApiFriendsResponse, ApiMatchEntry, ApiUserProfile, FriendEntry, FriendshipStatus, OtherProfileData, ProfileCore,
    ProfileData, ProfileEditField, Screen, State,
};
use crate::theme::{self, Palette};
use crate::ui::{
    self, divider, list_row, portrait, text_field, Face, Field, Fonts, Pill, Rect, SharpText, Status, Ui, View,
};

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
            if let Some(data) = http::json::<ApiUserProfile>(&resp) {
                core.username = data.username;
                core.elo = data.elo;
                core.bio = data.bio;
                core.favorite_music = data.favorite_music;
                core.total_matches = data.total_matches;
                core.wins = data.wins;
                core.all_time_max_chain = data.all_time_max_chain;
                core.total_nuisance_sent = data.total_nuisance_sent;
            }
            Some(ProfileLoad::Loaded)
        }
        Ok(_) => Some(ProfileLoad::HttpError),
        Err(_) => Some(ProfileLoad::NetworkError),
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
/// Room between the cards and the buttons, for a status line.
const STATUS_GAP: f32 = 56.0;

fn draw_header(draw: &mut Draw, ui: &Ui, fonts: &Fonts, core: &ProfileCore) {
    let pal = ui.palette();
    let view = ui.view();
    ui.header_band(draw, Rect::at(0.0, 0.0, view.w, PROFILE_HEADER_H));
    let radius = 48.0;
    let (px, py) = (60.0 + radius, PROFILE_HEADER_H / 2.0);
    portrait(draw, &pal, fonts, (px, py), radius, &core.username, 0.0);
    let text_x = px + radius + 28.0;
    let name = fonts.fit(
        Face::Display,
        &core.username,
        theme::size::TITLE,
        view.w - text_x - 60.0,
    );
    draw.sharp_text(&fonts.display, &name)
        .position(text_x, py - 16.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);
    let elo = format!("ELO {}", core.elo);
    let rating = Pill {
        text: &elo,
        color: theme::GOLD,
        size: theme::size::BODY,
    };
    rating.draw(draw, fonts, (text_x, py + 30.0));
}

/// The two cards under the header: statistics on the left, the match
/// history on the right, down to the button row.
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

    let winrate = if core.total_matches > 0 {
        core.wins * 100 / core.total_matches
    } else {
        0
    };
    let stats = [
        ("Matchs", core.total_matches.to_string()),
        ("Victoires", core.wins.to_string()),
        ("Winrate", format!("{winrate}%")),
        ("Max chain", core.all_time_max_chain.to_string()),
        ("Nuisance", core.total_nuisance_sent.to_string()),
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
    let about = [
        ("Bio", core.bio.as_deref(), pal.text),
        ("Musique", core.favorite_music.as_deref(), pal.accent),
    ];
    let room = card.w - 2.0 * CARD_PAD;
    for (label, value, color) in about {
        let Some(value) = value.filter(|v| !v.is_empty()) else {
            continue;
        };
        draw.sharp_text(&fonts.text, label)
            .position(label_x, info_y)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(pal.text_dim);
        draw.sharp_text(&fonts.text, &fonts.fit(Face::Text, value, theme::size::BODY, room))
            .position(label_x, info_y + 22.0)
            .size(theme::size::BODY)
            .v_align_middle()
            .color(color);
        info_y += 54.0;
    }
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
    let user_id = auth.user_id.clone();
    let token = auth.token.clone();

    let profile_slot = http::new_slot();
    let history_slot = http::new_slot();

    http::get(
        http::api_url(&format!("users/{user_id}")),
        Some(token.clone()),
        Arc::clone(&profile_slot),
    );
    http::get(
        http::api_url(&format!("users/{user_id}/matches?limit=10")),
        Some(token),
        Arc::clone(&history_slot),
    );

    state.profile = Some(ProfileData {
        core: ProfileCore::loading(user_id, auth.username.clone(), auth.elo, profile_slot, history_slot),
        editing: false,
        edit_bio: String::new(),
        edit_music: String::new(),
        edit_focused: ProfileEditField::Bio,
        edit_pending: None,
        edit_status: Status::Empty,
    });
}

fn poll_edit(state: &mut State) {
    let Some(p) = state.profile.as_mut() else { return };
    let Some(result) = http::take(&mut p.edit_pending) else {
        return;
    };
    match result {
        Ok(resp) if resp.status == 200 => {
            #[derive(serde::Deserialize)]
            struct PatchResp {
                bio: Option<String>,
                favorite_music: Option<String>,
            }
            if let Some(data) = http::json::<PatchResp>(&resp) {
                p.core.bio = data.bio;
                p.core.favorite_music = data.favorite_music;
                p.editing = false;
                p.edit_bio.clear();
                p.edit_music.clear();
                p.edit_status.clear();
            }
        }
        Ok(resp) => p.edit_status = Status::error(http::error_message(&resp)),
        Err(e) => p.edit_status = Status::error(format!("Erreur réseau: {e}")),
    }
}

fn save_profile(state: &mut State) {
    if state.profile.as_ref().is_none_or(|p| p.edit_pending.is_some()) {
        return;
    }
    let bio = state
        .profile
        .as_ref()
        .map(|p| p.edit_bio.trim().to_owned())
        .unwrap_or_default();
    let music = state
        .profile
        .as_ref()
        .map(|p| p.edit_music.trim().to_owned())
        .unwrap_or_default();
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let body = serde_json::json!({
        "bio": if bio.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(bio) },
        "favorite_music": if music.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(music) },
    })
    .to_string();
    let slot = http::new_slot();
    http::patch_json(http::api_url("me"), body, token, Arc::clone(&slot));
    if let Some(p) = state.profile.as_mut() {
        p.edit_pending = Some(slot);
        p.edit_status.clear();
    }
}

fn edit_boxes(cx: f32) -> (Rect, Rect) {
    (
        Rect::at(cx - 300.0, 215.0, 600.0, 46.0),
        Rect::at(cx - 300.0, 305.0, 600.0, 46.0),
    )
}

fn edit_buttons(cx: f32) -> (Rect, Rect) {
    (
        Rect::at(cx - 220.0, 390.0, 200.0, 54.0),
        Rect::at(cx + 20.0, 390.0, 200.0, 54.0),
    )
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
    poll_edit(state);

    let (ww, wh) = state.ui.view().size();
    let cx = ww / 2.0;
    let editing = state.profile.as_ref().is_some_and(|p| p.editing);

    if editing {
        if state.backspace.fired() {
            if let Some(p) = state.profile.as_mut() {
                match p.edit_focused {
                    ProfileEditField::Bio => {
                        p.edit_bio.pop();
                    }
                    ProfileEditField::Music => {
                        p.edit_music.pop();
                    }
                }
            }
        }
        if app.keyboard.was_pressed(KeyCode::Tab) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_focused = match p.edit_focused {
                    ProfileEditField::Bio => ProfileEditField::Music,
                    ProfileEditField::Music => ProfileEditField::Bio,
                };
            }
        }
        let (bio_box, music_box) = edit_boxes(cx);
        if state.ui.clicked(bio_box) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_focused = ProfileEditField::Bio;
            }
        }
        if state.ui.clicked(music_box) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_focused = ProfileEditField::Music;
            }
        }
        let (save_btn, cancel_btn) = edit_buttons(cx);
        if (state.ui.clicked(save_btn) || app.keyboard.was_pressed(KeyCode::Enter))
            && state.profile.as_ref().is_some_and(|p| p.edit_pending.is_none())
        {
            save_profile(state);
        }
        if state.ui.clicked(cancel_btn) || app.keyboard.was_pressed(KeyCode::Escape) {
            if let Some(p) = state.profile.as_mut() {
                p.editing = false;
                p.edit_status.clear();
            }
        }
    } else {
        let (back_btn, edit_btn, logout_btn) = own_buttons(cx, wh);

        if state.ui.clicked(back_btn) || app.keyboard.was_pressed(KeyCode::Escape) {
            state.profile = None;
            state.screen = Screen::Menu;
            return;
        }
        if state.ui.clicked(edit_btn) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_bio = p.core.bio.clone().unwrap_or_default();
                p.edit_music = p.core.favorite_music.clone().unwrap_or_default();
                p.edit_focused = ProfileEditField::Bio;
                p.editing = true;
                p.edit_status.clear();
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
        gfx.render(&draw);
        return;
    };

    draw_header(&mut draw, &state.ui, &state.fonts, &profile.core);

    if profile.editing {
        draw_edit_form(&state.ui, &mut draw, &state.fonts, profile, cx);
    } else {
        draw_stats_panel(&mut draw, &state.ui, &state.fonts, &profile.core, false);
        draw_history_panel(&mut draw, &state.ui, &state.fonts, &profile.core, true);

        let (back_btn, edit_btn, logout_btn) = own_buttons(cx, wh);
        state.ui.button(&mut draw, &state.fonts, back_btn, "Retour");
        state.ui.button(&mut draw, &state.fonts, edit_btn, "Modifier");
        state.ui.button(&mut draw, &state.fonts, logout_btn, "Déconnexion");
    }

    gfx.render(&draw);
}

fn draw_edit_form(ui: &crate::ui::Ui, draw: &mut Draw, fonts: &Fonts, profile: &ProfileData, cx: f32) {
    let pal = ui.palette();
    let (bio_box, music_box) = edit_boxes(cx);
    let fields = [
        (
            bio_box,
            "Bio",
            "Ta bio (max 500 caractères)",
            &profile.edit_bio,
            ProfileEditField::Bio,
        ),
        (
            music_box,
            "Musique préférée",
            "Ta musique préférée (max 200 caractères)",
            &profile.edit_music,
            ProfileEditField::Music,
        ),
    ];
    for (rect, label, placeholder, value, which) in fields {
        draw.sharp_text(&fonts.text, label)
            .position(rect.x, rect.y - 15.0)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(pal.text_dim);
        let field = Field {
            placeholder,
            value,
            focused: profile.edit_focused == which,
            secret: false,
        };
        text_field(draw, &pal, fonts, rect, &field);
    }

    let (save_btn, cancel_btn) = edit_buttons(cx);
    let saving = profile.edit_pending.is_some();
    ui.button_enabled(
        draw,
        fonts,
        save_btn,
        if saving { "Sauvegarde..." } else { "Enregistrer" },
        !saving,
    );
    ui.button(draw, fonts, cancel_btn, "Annuler");

    if let Some((msg, color)) = profile.edit_status.shown(&pal) {
        draw.sharp_text(&fonts.text, msg)
            .position(cx, 465.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

pub fn enter_other_profile(state: &mut State, user_id: String, username: String, prev: Screen) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let profile_slot = http::new_slot();
    let history_slot = http::new_slot();

    http::get(
        http::api_url(&format!("users/{user_id}")),
        token.clone(),
        Arc::clone(&profile_slot),
    );
    http::get(
        http::api_url(&format!("users/{user_id}/matches?limit=8")),
        token.clone(),
        Arc::clone(&history_slot),
    );

    let (friendship, friendship_check_slot) = if let Some(f) = state.friends.as_ref() {
        (friendship_with(&f.friends, &f.sent, &f.received, &user_id), None)
    } else if token.is_some() {
        let slot = http::new_slot();
        http::get(http::api_url("friends"), token, Arc::clone(&slot));
        (FriendshipStatus::Unknown, Some(slot))
    } else {
        (FriendshipStatus::Unknown, None)
    };

    state.other_profile = Some(OtherProfileData {
        core: ProfileCore::loading(user_id, username, 0, profile_slot, history_slot),
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
                p.core.username = "Inconnu".to_owned();
            }
            Some(ProfileLoad::NetworkError) => {
                p.load_failed = true;
                p.core.username = "Erreur réseau".to_owned();
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
        gfx.render(&draw);
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

    gfx.render(&draw);
}
