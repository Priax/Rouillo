use std::sync::Arc;

use notan::draw::{CreateDraw, Draw, DrawShapes, DrawTextSection};
use notan::prelude::*;

use crate::state::{
    ApiFriendsResponse, ApiMatchEntry, ApiUserProfile, FriendEntry, FriendshipStatus, OtherProfileData, ProfileCore,
    ProfileData, ProfileEditField, Screen, State,
};
use crate::ui::{text_field, Field, Rect, Status};
use crate::{http, theme};

const HISTORY_Y: f32 = 175.0;
const STATS_Y: f32 = 207.0;

pub(crate) fn truncate_display(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max_chars).collect();
        t.push('…');
        t
    }
}

pub(crate) fn panels(ww: f32) -> (f32, f32, f32, f32) {
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

fn draw_header(draw: &mut Draw, font: &crate::Font, cx: f32, core: &ProfileCore) {
    draw.text(font, &core.username)
        .position(cx, 90.0)
        .size(52.0)
        .h_align_center()
        .v_align_middle()
        .color(theme::TITLE);
    draw.text(font, &format!("ELO {}", core.elo))
        .position(cx, 140.0)
        .size(26.0)
        .h_align_center()
        .v_align_middle()
        .color(theme::GOLD);
}

fn draw_stats_panel(
    draw: &mut Draw,
    font: &crate::Font,
    ui_font: &crate::Font,
    core: &ProfileCore,
    ww: f32,
    load_failed: bool,
) {
    let (left_x, left_w, _, _) = panels(ww);
    let label_x = left_x + 10.0;
    let val_x = left_x + left_w - 10.0;

    if load_failed {
        draw.text(font, "Profil introuvable.")
            .position(left_x + left_w / 2.0, STATS_Y + 40.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(theme::DANGER);
        return;
    }
    if core.profile_slot.is_some() {
        draw.text(font, "Chargement des stats...")
            .position(left_x + left_w / 2.0, STATS_Y + 40.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_MUTED);
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
    for (i, (label, val)) in stats.iter().enumerate() {
        let y = STATS_Y + i as f32 * 30.0;
        draw.text(font, label)
            .position(label_x, y)
            .size(20.0)
            .v_align_middle()
            .color(theme::TEXT_MUTED);
        draw.text(font, val)
            .position(val_x, y)
            .size(20.0)
            .h_align_right()
            .v_align_middle()
            .color(theme::TEXT);
    }

    let sep_y = STATS_Y + stats.len() as f32 * 30.0 + 14.0;
    draw.rect((label_x, sep_y), (left_w - 20.0, 1.0)).color(theme::DIVIDER);

    let mut info_y = sep_y + 22.0;
    if let Some(bio) = &core.bio {
        if !bio.is_empty() {
            draw.text(font, "Bio")
                .position(label_x, info_y)
                .size(16.0)
                .v_align_middle()
                .color(theme::TEXT_DIM);
            info_y += 22.0;
            draw.text(ui_font, &truncate_display(bio, 55))
                .position(label_x, info_y)
                .size(17.0)
                .v_align_middle()
                .color(theme::TEXT);
            info_y += 28.0;
        }
    }
    if let Some(music) = &core.favorite_music {
        if !music.is_empty() {
            draw.text(ui_font, &format!("♪  {}", truncate_display(music, 45)))
                .position(label_x, info_y)
                .size(17.0)
                .v_align_middle()
                .color(theme::ACCENT);
        }
    }
}

fn draw_history_panel(draw: &mut Draw, app: &App, font: &crate::Font, core: &ProfileCore, ww: f32, clickable: bool) {
    let (_, _, right_x, right_w) = panels(ww);
    draw.text(font, "Derniers matchs")
        .position(right_x + right_w / 2.0, HISTORY_Y)
        .size(22.0)
        .h_align_center()
        .v_align_middle()
        .color(theme::TEXT_DIM);
    draw.rect((right_x, HISTORY_Y + 14.0), (right_w, 1.0))
        .color(theme::DIVIDER);

    if core.history_slot.is_some() {
        draw.text(font, "Chargement...")
            .position(right_x + right_w / 2.0, HISTORY_Y + 48.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_MUTED);
    } else if core.match_history.is_empty() {
        draw.text(font, "Aucun match pour l'instant.")
            .position(right_x + right_w / 2.0, HISTORY_Y + 48.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_MUTED);
    } else {
        for (i, m) in core.match_history.iter().enumerate().take(7) {
            draw_match_row(
                draw,
                app,
                font,
                history_row(right_x, right_w, i),
                m,
                &core.user_id,
                clickable,
            );
        }
    }
}

fn history_row(right_x: f32, right_w: f32, i: usize) -> Rect {
    Rect::at(right_x, HISTORY_Y + 12.0 + i as f32 * 48.0, right_w, 42.0)
}

fn opponent_zone(row: Rect) -> Rect {
    Rect::at(row.x + row.w * 0.17, row.y, row.w * 0.32, row.h)
}

fn history_row_zone(ww: f32, i: usize) -> Rect {
    let (_, _, right_x, right_w) = panels(ww);
    opponent_zone(history_row(right_x, right_w, i))
}

fn draw_match_row(
    draw: &mut Draw,
    app: &App,
    font: &crate::Font,
    row: Rect,
    m: &ApiMatchEntry,
    viewed_id: &str,
    clickable: bool,
) {
    let (row_x, row_w) = (row.x, row.w);
    let y = row.y + 20.0;
    let i_am_p1 = m.player1.user_id.as_deref() == Some(viewed_id);
    let won = m.winner_slot == if i_am_p1 { 1i16 } else { 2i16 };
    let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
    let me = if i_am_p1 { &m.player1 } else { &m.player2 };
    let opp_name = opp.username.as_deref().unwrap_or("Invité");

    draw.rect((row.x, row.y), (row.w, row.h))
        .color(if won { theme::SUCCESS_BG } else { theme::DANGER_BG });

    let result_color = if won { theme::SUCCESS } else { theme::DANGER };
    draw.text(font, if won { "VICTOIRE" } else { "DEFAITE" })
        .position(row_x + row_w * 0.09, y)
        .size(16.0)
        .h_align_center()
        .v_align_middle()
        .color(result_color);

    let opp_hover = clickable && opp.user_id.is_some() && opponent_zone(row).contains(app.mouse.x, app.mouse.y);
    draw.text(font, &format!("vs {opp_name}"))
        .position(row_x + row_w * 0.33, y)
        .size(17.0)
        .h_align_center()
        .v_align_middle()
        .color(if opp_hover { theme::LINK } else { theme::TEXT });

    draw.text(font, &format!("Chain x{}  Nuis {}", me.max_chain, me.nuisance_sent))
        .position(row_x + row_w * 0.65, y)
        .size(15.0)
        .h_align_center()
        .v_align_middle()
        .color(theme::GOLD);

    let secs = m.duration_secs as u32;
    draw.text(font, &format!("{}:{:02}", secs / 60, secs % 60))
        .position(row_x + row_w * 0.88, y)
        .size(15.0)
        .h_align_center()
        .v_align_middle()
        .color(theme::TEXT_MUTED);
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

    let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
    let cx = ww / 2.0;
    let editing = state.profile.as_ref().is_some_and(|p| p.editing);

    if editing {
        if app.keyboard.was_pressed(KeyCode::Backspace) {
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
        if bio_box.clicked(app) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_focused = ProfileEditField::Bio;
            }
        }
        if music_box.clicked(app) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_focused = ProfileEditField::Music;
            }
        }
        let (save_btn, cancel_btn) = edit_buttons(cx);
        if (save_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Enter))
            && state.profile.as_ref().is_some_and(|p| p.edit_pending.is_none())
        {
            save_profile(state);
        }
        if cancel_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
            if let Some(p) = state.profile.as_mut() {
                p.editing = false;
                p.edit_status.clear();
            }
        }
    } else {
        let (back_btn, edit_btn, logout_btn) = own_buttons(cx, wh);

        if back_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
            state.profile = None;
            state.screen = Screen::Menu;
            return;
        }
        if edit_btn.clicked(app) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_bio = p.core.bio.clone().unwrap_or_default();
                p.edit_music = p.core.favorite_music.clone().unwrap_or_default();
                p.edit_focused = ProfileEditField::Bio;
                p.editing = true;
                p.edit_status.clear();
            }
        }
        if logout_btn.clicked(app) {
            crate::menu::do_logout(state);
            return;
        }

        let opp_target: Option<(String, String)> = 'find: {
            let Some(profile) = &state.profile else {
                break 'find None;
            };
            let my_id = profile.core.user_id.as_str();
            for (i, m) in profile.core.match_history.iter().enumerate().take(7) {
                let i_am_p1 = m.player1.user_id.as_deref() == Some(my_id);
                let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
                if let (Some(opp_id), Some(opp_name)) = (&opp.user_id, &opp.username) {
                    if history_row_zone(ww, i).clicked(app) {
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

pub fn draw_profile(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
    let cx = ww / 2.0;
    let mut draw = gfx.create_draw();
    draw.clear(theme::BACKGROUND);

    let Some(profile) = &state.profile else {
        gfx.render(&draw);
        return;
    };

    draw_header(&mut draw, &state.font, cx, &profile.core);

    if profile.editing {
        draw_edit_form(&state.ui, &mut draw, &state.font, &state.ui_font, profile, cx);
    } else {
        draw_stats_panel(&mut draw, &state.font, &state.ui_font, &profile.core, ww, false);
        draw_history_panel(&mut draw, app, &state.font, &profile.core, ww, true);

        let (back_btn, edit_btn, logout_btn) = own_buttons(cx, wh);
        state.ui.button(&mut draw, &state.font, back_btn, "Retour");
        state.ui.button(&mut draw, &state.font, edit_btn, "Modifier");
        state.ui.button(&mut draw, &state.font, logout_btn, "Déconnexion");
    }

    gfx.render(&draw);
}

fn draw_edit_form(
    ui: &crate::ui::Ui,
    draw: &mut Draw,
    font: &crate::Font,
    ui_font: &crate::Font,
    profile: &ProfileData,
    cx: f32,
) {
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
        draw.text(font, label)
            .position(rect.x, rect.y - 15.0)
            .size(20.0)
            .v_align_middle()
            .color(theme::TEXT_DIM);
        let field = Field {
            placeholder,
            value,
            focused: profile.edit_focused == which,
            secret: false,
        };
        text_field(draw, ui_font, rect, &field);
    }

    let (save_btn, cancel_btn) = edit_buttons(cx);
    let saving = profile.edit_pending.is_some();
    ui.button_enabled(
        draw,
        font,
        save_btn,
        if saving { "Sauvegarde..." } else { "Enregistrer" },
        !saving,
    );
    ui.button(draw, font, cancel_btn, "Annuler");

    if let Some((msg, color)) = profile.edit_status.shown() {
        draw.text(font, msg)
            .position(cx, 465.0)
            .size(20.0)
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

    let ww = app.window().width() as f32;
    let wh = app.window().height() as f32;
    let cx = ww / 2.0;
    let (add_btn, back_btn) = other_buttons(cx, wh);

    let sending = state.other_profile.as_ref().is_some_and(|p| p.friend_slot.is_some());
    let can_add = matches!(
        state.other_profile.as_ref().map(|p| (p.friendship, p.load_failed)),
        Some((FriendshipStatus::Unknown | FriendshipStatus::NotFriends, false))
    );
    if add_btn.clicked(app) && !sending && can_add {
        add_friend(state);
    }

    if back_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
        let prev = state.other_profile.as_ref().map_or(Screen::Menu, |p| p.prev_screen);
        state.other_profile = None;
        state.screen = prev;
    }
}

pub fn draw_other_profile(app: &mut App, gfx: &mut Graphics, state: &State) {
    let ww = app.window().width() as f32;
    let wh = app.window().height() as f32;
    let cx = ww / 2.0;

    let mut draw = gfx.create_draw();
    draw.clear(theme::BACKGROUND);

    let Some(p) = &state.other_profile else {
        gfx.render(&draw);
        return;
    };

    draw_header(&mut draw, &state.font, cx, &p.core);
    draw_stats_panel(&mut draw, &state.font, &state.ui_font, &p.core, ww, p.load_failed);
    draw_history_panel(&mut draw, app, &state.font, &p.core, ww, false);

    let btn_y = button_row_y(wh);
    let sending = p.friend_slot.is_some();
    let (add_btn, back_btn) = other_buttons(cx, wh);

    let can_add = !p.load_failed && p.friendship == FriendshipStatus::NotFriends;
    if can_add {
        let label = if sending { "Envoi..." } else { "Ajouter en ami" };
        state
            .ui
            .button_enabled(&mut draw, &state.font, add_btn, label, !sending);
    }
    state.ui.button(&mut draw, &state.font, back_btn, "Retour");

    if let Some((msg, color)) = p.friend_status.shown() {
        draw.text(&state.font, msg)
            .position(cx, btn_y - 50.0)
            .size(19.0)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    gfx.render(&draw);
}
