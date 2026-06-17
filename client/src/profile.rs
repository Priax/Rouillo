use notan::draw::*;
use notan::prelude::*;

use crate::http;
use crate::menu::Btn;
use crate::state::{ApiMatchEntry, ApiUserProfile, AuthForm, ProfileData, Screen, State};

fn win_w(app: &mut App) -> f32 {
    app.window().width() as f32
}
fn win_h(app: &mut App) -> f32 {
    app.window().height() as f32
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
        total_matches: 0,
        wins: 0,
        all_time_max_chain: 0,
        total_nuisance_sent: 0,
        match_history: Vec::new(),
        profile_slot: Some(profile_slot),
        history_slot: Some(history_slot),
    });
}

fn poll_profile(state: &mut State) {
    let profile = match state.profile.as_mut() {
        Some(p) => p,
        None => return,
    };

    if let Some(result) = profile.profile_slot.as_ref().and_then(http::poll) {
        profile.profile_slot = None;
        if let Ok(resp) = result {
            if let Some(text) = resp.text() {
                if let Ok(p) = serde_json::from_str::<ApiUserProfile>(text) {
                    profile.username = p.username;
                    profile.elo = p.elo;
                    profile.total_matches = p.total_matches;
                    profile.wins = p.wins;
                    profile.all_time_max_chain = p.all_time_max_chain;
                    profile.total_nuisance_sent = p.total_nuisance_sent;
                }
            }
        }
    }

    if let Some(result) = profile.history_slot.as_ref().and_then(http::poll) {
        profile.history_slot = None;
        if let Ok(resp) = result {
            if let Some(text) = resp.text() {
                if let Ok(matches) = serde_json::from_str::<Vec<ApiMatchEntry>>(text) {
                    profile.match_history = matches;
                }
            }
        }
    }
}

pub fn update_profile(app: &mut App, state: &mut State) {
    poll_profile(state);

    let (ww, wh) = (win_w(app), win_h(app));
    let cx = ww / 2.0;

    let back_btn = Btn {
        x: cx - 270.0,
        y: wh - 90.0,
        w: 240.0,
        h: 54.0,
    };
    let logout_btn = Btn {
        x: cx + 30.0,
        y: wh - 90.0,
        w: 240.0,
        h: 54.0,
    };

    if back_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.profile = None;
        state.screen = Screen::Menu;
    }

    if logout_btn.clicked(app) {
        crate::state::clear_stored_token();
        state.auth = None;
        state.auth_form = AuthForm::default();
        state.profile = None;
        state.screen = Screen::Auth;
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
        .position(cx, 60.0)
        .size(52.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.9, 0.7, 1.0));
    draw.text(&state.font, &format!("ELO {}", profile.elo))
        .position(cx, 112.0)
        .size(26.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::YELLOW);

    let loading_stats = profile.profile_slot.is_some();
    if loading_stats {
        draw.text(&state.font, "Chargement des stats...")
            .position(cx, 170.0)
            .size(22.0)
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
        let col_w = ww / stats.len() as f32;
        for (i, (label, val)) in stats.iter().enumerate() {
            let x = col_w * (i as f32 + 0.5);
            draw.text(&state.font, val)
                .position(x, 158.0)
                .size(30.0)
                .h_align_center()
                .v_align_middle()
                .color(Color::WHITE);
            draw.text(&state.font, label)
                .position(x, 188.0)
                .size(18.0)
                .h_align_center()
                .v_align_middle()
                .color(Color::GRAY);
        }
    }

    let my_id = &profile.user_id;
    let history_y = 230.0;
    draw.text(&state.font, "Derniers matchs")
        .position(cx, history_y)
        .size(24.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.7, 0.7, 0.9));

    let loading_history = profile.history_slot.is_some();
    if loading_history {
        draw.text(&state.font, "Chargement...")
            .position(cx, history_y + 40.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else if profile.match_history.is_empty() {
        draw.text(&state.font, "Aucun match pour l'instant.")
            .position(cx, history_y + 40.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else {
        for (i, m) in profile.match_history.iter().enumerate().take(8) {
            let row_y = history_y + 40.0 + i as f32 * 48.0;
            draw_match_row(&mut draw, &state.font, ww, row_y, m, my_id);
        }
    }

    let back_btn = Btn {
        x: cx - 270.0,
        y: wh - 90.0,
        w: 240.0,
        h: 54.0,
    };
    let logout_btn = Btn {
        x: cx + 30.0,
        y: wh - 90.0,
        w: 240.0,
        h: 54.0,
    };
    back_btn.draw(&mut draw, app, &state.font, "Retour");
    logout_btn.draw(&mut draw, app, &state.font, "Déconnexion");

    gfx.render(&draw);
}

fn draw_match_row(draw: &mut Draw, font: &crate::Font, ww: f32, y: f32, m: &ApiMatchEntry, my_id: &str) {
    let row_x = ww / 2.0 - 450.0;
    let row_w = 900.0;

    let i_am_p1 = m.player1.user_id.as_deref() == Some(my_id);
    let my_slot = if i_am_p1 { 1i16 } else { 2i16 };
    let won = m.winner_slot == my_slot;

    let opp = if i_am_p1 { &m.player2 } else { &m.player1 };
    let me = if i_am_p1 { &m.player1 } else { &m.player2 };
    let opp_name = opp.username.as_deref().unwrap_or("Invité");

    let bg = if won {
        Color::from_rgba(0.1, 0.3, 0.1, 0.6)
    } else {
        Color::from_rgba(0.3, 0.1, 0.1, 0.6)
    };
    draw.rect((row_x, y - 20.0), (row_w, 42.0)).color(bg);

    let result_label = if won { "VICTOIRE" } else { "DEFAITE" };
    let result_color = if won {
        Color::from_rgb(0.4, 0.9, 0.4)
    } else {
        Color::from_rgb(0.9, 0.4, 0.4)
    };
    draw.text(font, result_label)
        .position(row_x + 90.0, y)
        .size(18.0)
        .h_align_center()
        .v_align_middle()
        .color(result_color);

    draw.text(font, &format!("vs {opp_name}"))
        .position(row_x + 300.0, y)
        .size(20.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::WHITE);

    draw.text(font, &format!("Chain x{}  Nuis {}", me.max_chain, me.nuisance_sent))
        .position(row_x + 570.0, y)
        .size(18.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::YELLOW);

    let secs = m.duration_secs as u32;
    draw.text(font, &format!("{}:{:02}", secs / 60, secs % 60))
        .position(row_x + 780.0, y)
        .size(18.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::GRAY);
}
