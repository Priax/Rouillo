use notan::draw::*;
use notan::prelude::*;

use crate::http;
use crate::menu::Btn;
use crate::state::{
    ApiFriendsResponse, ApiMatchEntry, ApiUserProfile, FriendshipStatus, OtherProfileData, Screen, State,
};

pub fn enter_other_profile(state: &mut State, user_id: String, username: String, prev: Screen) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let profile_slot = http::new_slot();
    let history_slot = http::new_slot();

    http::get(
        http::api_url(&format!("users/{user_id}")),
        token.clone(),
        profile_slot.clone(),
    );
    http::get(
        http::api_url(&format!("users/{user_id}/matches?limit=8")),
        token.clone(),
        history_slot.clone(),
    );

    let (friendship, friendship_check_slot) = if let Some(f) = state.friends.as_ref() {
        let status = if f.friends.iter().any(|e| e.user_id == user_id) {
            FriendshipStatus::Friends
        } else if f.sent.iter().any(|e| e.user_id == user_id) {
            FriendshipStatus::RequestSent
        } else if f.received.iter().any(|e| e.user_id == user_id) {
            FriendshipStatus::RequestReceived
        } else {
            FriendshipStatus::NotFriends
        };
        (status, None)
    } else if token.is_some() {
        let slot = http::new_slot();
        http::get(http::api_url("friends"), token, slot.clone());
        (FriendshipStatus::Unknown, Some(slot))
    } else {
        (FriendshipStatus::Unknown, None)
    };

    state.other_profile = Some(OtherProfileData {
        user_id,
        username,
        elo: 0,
        bio: None,
        favorite_music: None,
        total_matches: 0,
        wins: 0,
        all_time_max_chain: 0,
        total_nuisance_sent: 0,
        match_history: Vec::new(),
        profile_slot: Some(profile_slot),
        history_slot: Some(history_slot),
        friendship_check_slot,
        load_failed: false,
        friendship,
        friend_slot: None,
        friend_msg: String::new(),
        friend_success: false,
        prev_screen: prev,
    });
}

fn poll_friendship_check(state: &mut State) {
    let result = state
        .other_profile
        .as_ref()
        .and_then(|p| p.friendship_check_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    let user_id = state
        .other_profile
        .as_ref()
        .map(|p| p.user_id.clone())
        .unwrap_or_default();
    if let Some(p) = state.other_profile.as_mut() {
        p.friendship_check_slot = None;
    }
    if let Ok(resp) = result {
        if let Some(text) = resp.text() {
            if let Ok(data) = serde_json::from_str::<ApiFriendsResponse>(text) {
                let status = if data.friends.iter().any(|e| e.user_id == user_id) {
                    FriendshipStatus::Friends
                } else if data.sent.iter().any(|e| e.user_id == user_id) {
                    FriendshipStatus::RequestSent
                } else if data.received.iter().any(|e| e.user_id == user_id) {
                    FriendshipStatus::RequestReceived
                } else {
                    FriendshipStatus::NotFriends
                };
                if let Some(p) = state.other_profile.as_mut() {
                    p.friendship = status;
                }
            }
        }
    }
}

fn poll_profile(state: &mut State) {
    let result = state
        .other_profile
        .as_ref()
        .and_then(|p| p.profile_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(p) = state.other_profile.as_mut() {
        p.profile_slot = None;
    }
    match result {
        Ok(resp) if resp.status == 200 => {
            if let Some(text) = resp.text() {
                if let Ok(data) = serde_json::from_str::<ApiUserProfile>(text) {
                    if let Some(p) = state.other_profile.as_mut() {
                        p.username = data.username;
                        p.elo = data.elo;
                        p.bio = data.bio;
                        p.favorite_music = data.favorite_music;
                        p.total_matches = data.total_matches;
                        p.wins = data.wins;
                        p.all_time_max_chain = data.all_time_max_chain;
                        p.total_nuisance_sent = data.total_nuisance_sent;
                    }
                }
            }
        }
        Ok(_) => {
            if let Some(p) = state.other_profile.as_mut() {
                p.load_failed = true;
                p.username = "Inconnu".to_owned();
            }
        }
        Err(_) => {
            if let Some(p) = state.other_profile.as_mut() {
                p.load_failed = true;
                p.username = "Erreur réseau".to_owned();
            }
        }
    }
}

fn poll_history(state: &mut State) {
    let result = state
        .other_profile
        .as_ref()
        .and_then(|p| p.history_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(p) = state.other_profile.as_mut() {
        p.history_slot = None;
    }
    if let Ok(resp) = result {
        if let Some(text) = resp.text() {
            if let Ok(matches) = serde_json::from_str::<Vec<ApiMatchEntry>>(text) {
                if let Some(p) = state.other_profile.as_mut() {
                    p.match_history = matches;
                }
            }
        }
    }
}

fn poll_friend(state: &mut State) {
    let result = state
        .other_profile
        .as_ref()
        .and_then(|p| p.friend_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(p) = state.other_profile.as_mut() {
        p.friend_slot = None;
    }
    match result {
        Ok(resp) if resp.status == 201 => {
            if let Some(p) = state.other_profile.as_mut() {
                p.friend_msg = "Demande envoyée !".to_owned();
                p.friend_success = true;
                p.friendship = FriendshipStatus::RequestSent;
            }
        }
        Ok(resp) => {
            let msg = serde_json::from_str::<serde_json::Value>(resp.text().unwrap_or_default())
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("Erreur {}", resp.status));
            if let Some(p) = state.other_profile.as_mut() {
                p.friend_msg = msg;
                p.friend_success = false;
            }
        }
        Err(e) => {
            if let Some(p) = state.other_profile.as_mut() {
                p.friend_msg = format!("Erreur réseau: {e}");
                p.friend_success = false;
            }
        }
    }
}

fn add_friend(state: &mut State) {
    if state
        .other_profile
        .as_ref()
        .map(|p| p.friend_slot.is_some())
        .unwrap_or(true)
    {
        return;
    }
    let (user_id, token) = (
        state
            .other_profile
            .as_ref()
            .map(|p| p.user_id.clone())
            .unwrap_or_default(),
        state.auth.as_ref().map(|a| a.token.clone()),
    );
    let body = serde_json::json!({ "user_id": user_id }).to_string();
    let slot = http::new_slot();
    http::post_json(http::api_url("friends"), body, token, slot.clone());
    if let Some(p) = state.other_profile.as_mut() {
        p.friend_slot = Some(slot);
        p.friend_msg.clear();
    }
}

pub fn update_other_profile(app: &mut App, state: &mut State) {
    poll_friendship_check(state);
    poll_profile(state);
    poll_history(state);
    poll_friend(state);

    let ww = app.window().width() as f32;
    let wh = app.window().height() as f32;
    let cx = ww / 2.0;

    let btn_y = (wh - 90.0).max(590.0);
    let add_btn = Btn {
        x: cx - 110.0,
        y: btn_y,
        w: 220.0,
        h: 54.0,
    };
    let back_btn = Btn {
        x: cx + 140.0,
        y: btn_y,
        w: 200.0,
        h: 54.0,
    };

    let sending = state
        .other_profile
        .as_ref()
        .map(|p| p.friend_slot.is_some())
        .unwrap_or(false);
    let can_add = matches!(
        state.other_profile.as_ref().map(|p| (p.friendship, p.load_failed)),
        Some((FriendshipStatus::Unknown, false) | (FriendshipStatus::NotFriends, false))
    );
    if add_btn.clicked(app) && !sending && can_add {
        add_friend(state);
    }

    if back_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
        let prev = state
            .other_profile
            .as_ref()
            .map(|p| p.prev_screen)
            .unwrap_or(Screen::Menu);
        state.other_profile = None;
        state.screen = prev;
    }
}

pub fn draw_other_profile(app: &mut App, gfx: &mut Graphics, state: &State) {
    let ww = app.window().width() as f32;
    let wh = app.window().height() as f32;
    let cx = ww / 2.0;

    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    let p = match &state.other_profile {
        Some(p) => p,
        None => {
            gfx.render(&draw);
            return;
        }
    };

    draw.text(&state.font, &p.username)
        .position(cx, 90.0)
        .size(52.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.9, 0.7, 1.0));
    draw.text(&state.font, &format!("ELO {}", p.elo))
        .position(cx, 140.0)
        .size(26.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::YELLOW);

    let (left_x, left_w, right_x, right_w) = crate::profile::panels(ww);

    const STATS_Y: f32 = 207.0;
    let label_x = left_x + 10.0;
    let val_x = left_x + left_w - 10.0;

    if p.load_failed {
        draw.text(&state.font, "Profil introuvable.")
            .position(left_x + left_w / 2.0, STATS_Y + 40.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.9, 0.4, 0.4));
    } else if p.profile_slot.is_some() {
        draw.text(&state.font, "Chargement des stats...")
            .position(left_x + left_w / 2.0, STATS_Y + 40.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else {
        let winrate = if p.total_matches > 0 {
            p.wins * 100 / p.total_matches
        } else {
            0
        };
        let stats = [
            ("Matchs", p.total_matches.to_string()),
            ("Victoires", p.wins.to_string()),
            ("Winrate", format!("{winrate}%")),
            ("Max chain", p.all_time_max_chain.to_string()),
            ("Nuisance", p.total_nuisance_sent.to_string()),
        ];
        for (i, (label, val)) in stats.iter().enumerate() {
            let y = STATS_Y + i as f32 * 30.0;
            draw.text(&state.font, label)
                .position(label_x, y)
                .size(20.0)
                .v_align_middle()
                .color(Color::GRAY);
            draw.text(&state.font, val)
                .position(val_x, y)
                .size(20.0)
                .h_align_right()
                .v_align_middle()
                .color(Color::WHITE);
        }

        let sep_y = STATS_Y + stats.len() as f32 * 30.0 + 14.0;
        draw.rect((label_x, sep_y), (left_w - 20.0, 1.0))
            .color(Color::from_rgb(0.25, 0.25, 0.35));

        let mut info_y = sep_y + 22.0;
        if let Some(bio) = &p.bio {
            if !bio.is_empty() {
                draw.text(&state.font, "Bio")
                    .position(label_x, info_y)
                    .size(16.0)
                    .v_align_middle()
                    .color(Color::from_rgb(0.55, 0.55, 0.7));
                info_y += 22.0;
                draw.text(&state.ui_font, &crate::profile::truncate_display(bio, 55))
                    .position(label_x, info_y)
                    .size(17.0)
                    .v_align_middle()
                    .color(Color::from_rgb(0.85, 0.85, 0.95));
                info_y += 28.0;
            }
        }
        if let Some(music) = &p.favorite_music {
            if !music.is_empty() {
                draw.text(
                    &state.ui_font,
                    &format!("♪  {}", crate::profile::truncate_display(music, 45)),
                )
                .position(label_x, info_y)
                .size(17.0)
                .v_align_middle()
                .color(Color::from_rgb(0.6, 0.85, 0.65));
            }
        }
    }

    const HISTORY_Y: f32 = 175.0;
    draw.text(&state.font, "Derniers matchs")
        .position(right_x + right_w / 2.0, HISTORY_Y)
        .size(22.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.7, 0.7, 0.9));
    draw.rect((right_x, HISTORY_Y + 14.0), (right_w, 1.0))
        .color(Color::from_rgb(0.25, 0.25, 0.4));

    if p.history_slot.is_some() {
        draw.text(&state.font, "Chargement...")
            .position(right_x + right_w / 2.0, HISTORY_Y + 48.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else if p.match_history.is_empty() {
        draw.text(&state.font, "Aucun match pour l'instant.")
            .position(right_x + right_w / 2.0, HISTORY_Y + 48.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else {
        for (i, m) in p.match_history.iter().enumerate().take(7) {
            let row_y = HISTORY_Y + 32.0 + i as f32 * 48.0;
            crate::profile::draw_match_row(
                &mut draw,
                app,
                &state.font,
                right_x,
                right_w,
                row_y,
                m,
                &p.user_id,
                false,
            );
        }
    }

    let btn_y = (wh - 90.0).max(590.0);
    let sending = p.friend_slot.is_some();
    let add_btn = Btn {
        x: cx - 110.0,
        y: btn_y,
        w: 220.0,
        h: 54.0,
    };
    let back_btn = Btn {
        x: cx + 140.0,
        y: btn_y,
        w: 200.0,
        h: 54.0,
    };

    let can_add = !p.load_failed && p.friendship == FriendshipStatus::NotFriends;
    if can_add {
        let label = if sending { "Envoi..." } else { "Ajouter en ami" };
        add_btn.draw_styled(&mut draw, app, &state.font, label, !sending);
    }
    back_btn.draw(&mut draw, app, &state.font, "Retour");

    if !p.friend_msg.is_empty() {
        draw.text(&state.font, &p.friend_msg)
            .position(cx, btn_y - 50.0)
            .size(19.0)
            .h_align_center()
            .v_align_middle()
            .color(if p.friend_success {
                Color::from_rgb(0.4, 0.9, 0.4)
            } else {
                Color::from_rgb(0.9, 0.4, 0.4)
            });
    }

    gfx.render(&draw);
}
