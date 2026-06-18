use notan::draw::*;
use notan::prelude::*;

use crate::http;
use crate::menu::{draw_text_box, Btn};
use crate::state::{ApiMatchEntry, ApiUserProfile, ProfileData, ProfileEditField, Screen, State};

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

pub fn enter_profile(state: &mut State) {
    let auth = match &state.auth {
        Some(a) => a,
        None => return,
    };
    let user_id = auth.user_id.clone();
    let token = auth.token.clone();

    let profile_slot = http::new_slot();
    let history_slot = http::new_slot();

    http::get(
        http::api_url(&format!("users/{user_id}")),
        Some(token.clone()),
        profile_slot.clone(),
    );
    http::get(
        http::api_url(&format!("users/{user_id}/matches?limit=10")),
        Some(token),
        history_slot.clone(),
    );

    state.profile = Some(ProfileData {
        user_id,
        username: auth.username.clone(),
        elo: auth.elo,
        bio: None,
        favorite_music: None,
        total_matches: 0,
        wins: 0,
        all_time_max_chain: 0,
        total_nuisance_sent: 0,
        match_history: Vec::new(),
        profile_slot: Some(profile_slot),
        history_slot: Some(history_slot),
        editing: false,
        edit_bio: String::new(),
        edit_music: String::new(),
        edit_focused: ProfileEditField::Bio,
        edit_pending: None,
        edit_error: String::new(),
    });
}

fn poll_profile(state: &mut State) {
    let result = state
        .profile
        .as_ref()
        .and_then(|p| p.profile_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(p) = state.profile.as_mut() {
        p.profile_slot = None;
    }
    if let Ok(resp) = result {
        if let Some(text) = resp.text() {
            if let Ok(data) = serde_json::from_str::<ApiUserProfile>(text) {
                if let Some(p) = state.profile.as_mut() {
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
}

fn poll_history(state: &mut State) {
    let result = state
        .profile
        .as_ref()
        .and_then(|p| p.history_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(p) = state.profile.as_mut() {
        p.history_slot = None;
    }
    if let Ok(resp) = result {
        if let Some(text) = resp.text() {
            if let Ok(matches) = serde_json::from_str::<Vec<ApiMatchEntry>>(text) {
                if let Some(p) = state.profile.as_mut() {
                    p.match_history = matches;
                }
            }
        }
    }
}

fn poll_edit(state: &mut State) {
    let result = state
        .profile
        .as_ref()
        .and_then(|p| p.edit_pending.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(p) = state.profile.as_mut() {
        p.edit_pending = None;
    }

    match result {
        Ok(resp) if resp.status == 200 => {
            #[derive(serde::Deserialize)]
            struct PatchResp {
                bio: Option<String>,
                favorite_music: Option<String>,
            }
            if let Some(text) = resp.text() {
                if let Ok(data) = serde_json::from_str::<PatchResp>(text) {
                    if let Some(p) = state.profile.as_mut() {
                        p.bio = data.bio;
                        p.favorite_music = data.favorite_music;
                        p.editing = false;
                        p.edit_bio.clear();
                        p.edit_music.clear();
                        p.edit_error.clear();
                    }
                }
            }
        }
        Ok(resp) => {
            let msg = serde_json::from_str::<serde_json::Value>(resp.text().unwrap_or_default())
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("Erreur {}", resp.status));
            if let Some(p) = state.profile.as_mut() {
                p.edit_error = msg;
            }
        }
        Err(e) => {
            if let Some(p) = state.profile.as_mut() {
                p.edit_error = format!("Erreur réseau: {e}");
            }
        }
    }
}

fn save_profile(state: &mut State) {
    if state.profile.as_ref().map(|p| p.edit_pending.is_some()).unwrap_or(true) {
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
    http::patch_json(http::api_url("me"), body, token, slot.clone());
    if let Some(p) = state.profile.as_mut() {
        p.edit_pending = Some(slot);
        p.edit_error.clear();
    }
}

pub fn update_profile(app: &mut App, state: &mut State) {
    poll_profile(state);
    poll_history(state);
    poll_edit(state);

    let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
    let cx = ww / 2.0;
    let editing = state.profile.as_ref().map(|p| p.editing).unwrap_or(false);

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
        let bio_box = Btn {
            x: cx - 300.0,
            y: 220.0,
            w: 600.0,
            h: 46.0,
        };
        let music_box = Btn {
            x: cx - 300.0,
            y: 310.0,
            w: 600.0,
            h: 46.0,
        };
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
        let save_btn = Btn {
            x: cx - 220.0,
            y: 390.0,
            w: 200.0,
            h: 54.0,
        };
        let cancel_btn = Btn {
            x: cx + 20.0,
            y: 390.0,
            w: 200.0,
            h: 54.0,
        };
        if (save_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Enter))
            && state
                .profile
                .as_ref()
                .map(|p| p.edit_pending.is_none())
                .unwrap_or(false)
        {
            save_profile(state);
        }
        if cancel_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
            if let Some(p) = state.profile.as_mut() {
                p.editing = false;
                p.edit_error.clear();
            }
        }
    } else {
        let btn_y = (wh - 90.0).max(590.0);
        let back_btn = Btn {
            x: cx - 390.0,
            y: btn_y,
            w: 200.0,
            h: 54.0,
        };
        let edit_btn = Btn {
            x: cx - 100.0,
            y: btn_y,
            w: 200.0,
            h: 54.0,
        };
        let logout_btn = Btn {
            x: cx + 190.0,
            y: btn_y,
            w: 200.0,
            h: 54.0,
        };

        if back_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
            state.profile = None;
            state.screen = Screen::Menu;
            return;
        }
        if edit_btn.clicked(app) {
            if let Some(p) = state.profile.as_mut() {
                p.edit_bio = p.bio.clone().unwrap_or_default();
                p.edit_music = p.favorite_music.clone().unwrap_or_default();
                p.edit_focused = ProfileEditField::Bio;
                p.editing = true;
                p.edit_error.clear();
            }
        }
        if logout_btn.clicked(app) {
            crate::menu::do_logout(state);
            return;
        }

        // Click on opponent name in match history
        let my_id = state.profile.as_ref().map(|p| p.user_id.clone()).unwrap_or_default();
        let (_, _, right_x, right_w) = panels(ww);
        const HISTORY_Y: f32 = 175.0;

        let opp_target: Option<(String, String)> = 'find: {
            let Some(profile) = &state.profile else {
                break 'find None;
            };
            for (i, m) in profile.match_history.iter().enumerate().take(7) {
                let row_y = HISTORY_Y + 32.0 + i as f32 * 48.0;
                let i_am_p1 = m.player1.user_id.as_deref() == Some(my_id.as_str());
                let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
                if let (Some(opp_id), Some(opp_name)) = (&opp.user_id, &opp.username) {
                    let zone = Btn {
                        x: right_x + right_w * 0.17,
                        y: row_y - 20.0,
                        w: right_w * 0.32,
                        h: 42.0,
                    };
                    if zone.clicked(app) {
                        break 'find Some((opp_id.clone(), opp_name.clone()));
                    }
                }
            }
            None
        };
        if let Some((uid, uname)) = opp_target {
            crate::other_profile::enter_other_profile(state, uid, uname, Screen::Profile);
            state.screen = Screen::OtherProfile;
        }
    }
}

pub fn draw_profile(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
    let cx = ww / 2.0;
    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    let profile = match &state.profile {
        Some(p) => p,
        None => {
            gfx.render(&draw);
            return;
        }
    };

    draw.text(&state.font, &profile.username)
        .position(cx, 90.0)
        .size(52.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.9, 0.7, 1.0));
    draw.text(&state.font, &format!("ELO {}", profile.elo))
        .position(cx, 140.0)
        .size(26.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::YELLOW);

    if profile.editing {
        draw_edit_form(app, &mut draw, &state.font, &state.ui_font, profile, cx);
    } else {
        draw_profile_view(app, &mut draw, &state.font, &state.ui_font, profile, ww, wh);
    }

    gfx.render(&draw);
}

fn draw_edit_form(
    app: &App,
    draw: &mut Draw,
    font: &crate::Font,
    ui_font: &crate::Font,
    profile: &ProfileData,
    cx: f32,
) {
    draw.text(font, "Bio")
        .position(cx - 300.0, 200.0)
        .size(20.0)
        .v_align_middle()
        .color(Color::from_rgb(0.7, 0.7, 0.9));
    draw_text_box(
        draw,
        ui_font,
        cx - 300.0,
        215.0,
        600.0,
        46.0,
        "Ta bio (max 500 caractères)",
        &profile.edit_bio,
        profile.edit_focused == ProfileEditField::Bio,
    );

    draw.text(font, "Musique préférée")
        .position(cx - 300.0, 290.0)
        .size(20.0)
        .v_align_middle()
        .color(Color::from_rgb(0.7, 0.7, 0.9));
    draw_text_box(
        draw,
        ui_font,
        cx - 300.0,
        305.0,
        600.0,
        46.0,
        "Ta musique préférée (max 200 caractères)",
        &profile.edit_music,
        profile.edit_focused == ProfileEditField::Music,
    );

    let save_btn = Btn {
        x: cx - 220.0,
        y: 390.0,
        w: 200.0,
        h: 54.0,
    };
    let cancel_btn = Btn {
        x: cx + 20.0,
        y: 390.0,
        w: 200.0,
        h: 54.0,
    };
    let saving = profile.edit_pending.is_some();
    save_btn.draw_styled(
        draw,
        app,
        font,
        if saving { "Sauvegarde..." } else { "Enregistrer" },
        !saving,
    );
    cancel_btn.draw(draw, app, font, "Annuler");

    if !profile.edit_error.is_empty() {
        draw.text(font, &profile.edit_error)
            .position(cx, 465.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.9, 0.3, 0.3));
    }
}

fn draw_profile_view(
    app: &App,
    draw: &mut Draw,
    font: &crate::Font,
    ui_font: &crate::Font,
    profile: &ProfileData,
    ww: f32,
    wh: f32,
) {
    let cx = ww / 2.0;
    let (left_x, left_w, right_x, right_w) = panels(ww);

    const STATS_Y: f32 = 207.0;
    let label_x = left_x + 10.0;
    let val_x = left_x + left_w - 10.0;

    if profile.profile_slot.is_some() {
        draw.text(font, "Chargement des stats...")
            .position(left_x + left_w / 2.0, STATS_Y + 40.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else {
        let winrate = if profile.total_matches > 0 {
            profile.wins * 100 / profile.total_matches
        } else {
            0
        };
        let stats = [
            ("Matchs", profile.total_matches.to_string()),
            ("Victoires", profile.wins.to_string()),
            ("Winrate", format!("{winrate}%")),
            ("Max chain", profile.all_time_max_chain.to_string()),
            ("Nuisance", profile.total_nuisance_sent.to_string()),
        ];
        for (i, (label, val)) in stats.iter().enumerate() {
            let y = STATS_Y + i as f32 * 30.0;
            draw.text(font, label)
                .position(label_x, y)
                .size(20.0)
                .v_align_middle()
                .color(Color::GRAY);
            draw.text(font, val)
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
        if let Some(bio) = &profile.bio {
            if !bio.is_empty() {
                draw.text(font, "Bio")
                    .position(label_x, info_y)
                    .size(16.0)
                    .v_align_middle()
                    .color(Color::from_rgb(0.55, 0.55, 0.7));
                info_y += 22.0;
                draw.text(ui_font, &truncate_display(bio, 55))
                    .position(label_x, info_y)
                    .size(17.0)
                    .v_align_middle()
                    .color(Color::from_rgb(0.85, 0.85, 0.95));
                info_y += 28.0;
            }
        }
        if let Some(music) = &profile.favorite_music {
            if !music.is_empty() {
                draw.text(ui_font, &format!("♪  {}", truncate_display(music, 45)))
                    .position(label_x, info_y)
                    .size(17.0)
                    .v_align_middle()
                    .color(Color::from_rgb(0.6, 0.85, 0.65));
            }
        }
    }

    const HISTORY_Y: f32 = 175.0;
    draw.text(font, "Derniers matchs")
        .position(right_x + right_w / 2.0, HISTORY_Y)
        .size(22.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.7, 0.7, 0.9));
    draw.rect((right_x, HISTORY_Y + 14.0), (right_w, 1.0))
        .color(Color::from_rgb(0.25, 0.25, 0.4));

    if profile.history_slot.is_some() {
        draw.text(font, "Chargement...")
            .position(right_x + right_w / 2.0, HISTORY_Y + 48.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else if profile.match_history.is_empty() {
        draw.text(font, "Aucun match pour l'instant.")
            .position(right_x + right_w / 2.0, HISTORY_Y + 48.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else {
        for (i, m) in profile.match_history.iter().enumerate().take(7) {
            let row_y = HISTORY_Y + 32.0 + i as f32 * 48.0;
            draw_match_row(draw, app, font, right_x, right_w, row_y, m, &profile.user_id, true);
        }
    }

    let btn_y = (wh - 90.0).max(590.0);
    let back_btn = Btn {
        x: cx - 390.0,
        y: btn_y,
        w: 200.0,
        h: 54.0,
    };
    let edit_btn = Btn {
        x: cx - 100.0,
        y: btn_y,
        w: 200.0,
        h: 54.0,
    };
    let logout_btn = Btn {
        x: cx + 190.0,
        y: btn_y,
        w: 200.0,
        h: 54.0,
    };
    back_btn.draw(draw, app, font, "Retour");
    edit_btn.draw(draw, app, font, "Modifier");
    logout_btn.draw(draw, app, font, "Déconnexion");
}

pub(crate) fn draw_match_row(
    draw: &mut Draw,
    app: &App,
    font: &crate::Font,
    row_x: f32,
    row_w: f32,
    y: f32,
    m: &ApiMatchEntry,
    viewed_id: &str,
    clickable: bool,
) {
    let i_am_p1 = m.player1.user_id.as_deref() == Some(viewed_id);
    let won = m.winner_slot == if i_am_p1 { 1i16 } else { 2i16 };
    let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
    let me = if i_am_p1 { &m.player1 } else { &m.player2 };
    let opp_name = opp.username.as_deref().unwrap_or("Invité");

    draw.rect((row_x, y - 20.0), (row_w, 42.0)).color(if won {
        Color::from_rgba(0.1, 0.3, 0.1, 0.6)
    } else {
        Color::from_rgba(0.3, 0.1, 0.1, 0.6)
    });

    let result_color = if won {
        Color::from_rgb(0.4, 0.9, 0.4)
    } else {
        Color::from_rgb(0.9, 0.4, 0.4)
    };
    draw.text(font, if won { "VICTOIRE" } else { "DEFAITE" })
        .position(row_x + row_w * 0.09, y)
        .size(16.0)
        .h_align_center()
        .v_align_middle()
        .color(result_color);

    let opp_has_id = opp.user_id.is_some();
    let zone_x = row_x + row_w * 0.17;
    let zone_w = row_w * 0.32;
    let opp_hover = clickable
        && opp_has_id
        && app.mouse.x >= zone_x
        && app.mouse.x <= zone_x + zone_w
        && app.mouse.y >= y - 20.0
        && app.mouse.y <= y + 22.0;
    draw.text(font, &format!("vs {opp_name}"))
        .position(row_x + row_w * 0.33, y)
        .size(17.0)
        .h_align_center()
        .v_align_middle()
        .color(if opp_hover {
            Color::from_rgb(0.6, 0.8, 1.0)
        } else {
            Color::WHITE
        });

    draw.text(font, &format!("Chain x{}  Nuis {}", me.max_chain, me.nuisance_sent))
        .position(row_x + row_w * 0.65, y)
        .size(15.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::YELLOW);

    let secs = m.duration_secs as u32;
    draw.text(font, &format!("{}:{:02}", secs / 60, secs % 60))
        .position(row_x + row_w * 0.88, y)
        .size(15.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::GRAY);
}
