use notan::draw::*;
use notan::prelude::*;

use crate::http;
use crate::menu::Btn;
use crate::state::{ApiAuthResponse, ApiMeResponse, AuthField, AuthForm, AuthInfo, AuthMode, Screen, State};

fn win_w(app: &mut App) -> f32 {
    app.window().width() as f32
}
fn win_h(app: &mut App) -> f32 {
    app.window().height() as f32
}

fn input_box(cx: f32, y: f32) -> (f32, f32, f32, f32) {
    let w = 420.0;
    let h = 50.0;
    (cx - w / 2.0, y, w, h)
}

fn submit(form: &mut AuthForm) {
    if form.pending.is_some() {
        return;
    }
    let username = form.username.trim().to_owned();
    let password = form.password.clone();
    if username.is_empty() || password.is_empty() {
        form.error = "Remplis tous les champs.".to_owned();
        return;
    }
    let body = serde_json::json!({ "username": username, "password": password }).to_string();
    let slot = http::new_slot();
    let path = match form.mode {
        AuthMode::Login => "login",
        AuthMode::Register => "register",
    };
    http::post_json(http::api_url(path), body, None, slot.clone());
    form.error.clear();
    form.pending = Some(slot);
}

// Called every frame. Processes pending HTTP response and updates state accordingly.
fn poll_auth(state: &mut State) {
    let result = state.auth_form.pending.as_ref().and_then(http::poll);
    let Some(result) = result else { return };
    state.auth_form.pending = None;

    match result {
        Err(e) => {
            state.auth_form.error = format!("Erreur réseau: {e}");
        }
        Ok(resp) => {
            let text = resp.text().unwrap_or_default();
            if resp.status == 200 || resp.status == 201 {
                match serde_json::from_str::<ApiAuthResponse>(text) {
                    Ok(r) => {
                        crate::state::save_token(&r.token);
                        state.auth = Some(AuthInfo {
                            token: r.token,
                            user_id: r.user_id,
                            username: r.username,
                            elo: r.elo,
                        });
                        state.auth_form = AuthForm::default();
                        state.screen = Screen::Menu;
                    }
                    Err(_) => {
                        state.auth_form.error = "Réponse serveur invalide.".to_owned();
                    }
                }
            } else {
                // Try to extract "error" field from JSON body, else use status text.
                let msg = serde_json::from_str::<serde_json::Value>(text)
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("Erreur {}", resp.status));
                state.auth_form.error = msg;
            }
        }
    }
}

pub fn poll_startup_check(state: &mut State) {
    let result = state.startup_check.as_ref().and_then(http::poll);
    let Some(result) = result else { return };
    state.startup_check = None;

    match result {
        Ok(resp) if resp.status == 200 => {
            let text = resp.text().unwrap_or_default();
            if let Ok(me) = serde_json::from_str::<ApiMeResponse>(text) {
                if let Some(token) = crate::state::load_stored_token() {
                    state.auth = Some(AuthInfo {
                        token,
                        user_id: me.id,
                        username: me.username,
                        elo: me.elo,
                    });
                    state.screen = Screen::Menu;
                }
            }
        }
        _ => {
            crate::state::clear_stored_token();
        }
    }
}

pub fn update_auth(app: &mut App, state: &mut State) {
    poll_auth(state);

    let (ww, wh) = (win_w(app), win_h(app));
    let cx = ww / 2.0;
    let base_y = wh / 2.0 - 160.0;

    if app.keyboard.was_pressed(KeyCode::Backspace) {
        match state.auth_form.focused {
            AuthField::Username => {
                state.auth_form.username.pop();
            }
            AuthField::Password => {
                state.auth_form.password.pop();
            }
        }
    }

    if app.keyboard.was_pressed(KeyCode::Tab) {
        state.auth_form.focused = match state.auth_form.focused {
            AuthField::Username => AuthField::Password,
            AuthField::Password => AuthField::Username,
        };
    }

    let login_tab = Btn {
        x: cx - 230.0,
        y: base_y - 10.0,
        w: 220.0,
        h: 50.0,
    };
    let register_tab = Btn {
        x: cx + 10.0,
        y: base_y - 10.0,
        w: 220.0,
        h: 50.0,
    };
    if login_tab.clicked(app) {
        state.auth_form.mode = AuthMode::Login;
        state.auth_form.error.clear();
    }
    if register_tab.clicked(app) {
        state.auth_form.mode = AuthMode::Register;
        state.auth_form.error.clear();
    }

    let (ux, uy, uw, uh) = input_box(cx, base_y + 60.0);
    let username_box = Btn {
        x: ux,
        y: uy,
        w: uw,
        h: uh,
    };
    if username_box.clicked(app) {
        state.auth_form.focused = AuthField::Username;
    }

    let (px, py, pw, ph) = input_box(cx, base_y + 140.0);
    let password_box = Btn {
        x: px,
        y: py,
        w: pw,
        h: ph,
    };
    if password_box.clicked(app) {
        state.auth_form.focused = AuthField::Password;
    }

    let submit_btn = Btn {
        x: cx - 180.0,
        y: base_y + 220.0,
        w: 360.0,
        h: 56.0,
    };
    if (submit_btn.clicked(app) || app.keyboard.was_pressed(KeyCode::Enter)) && state.auth_form.pending.is_none() {
        submit(&mut state.auth_form);
    }

    let guest_btn = Btn {
        x: cx - 180.0,
        y: base_y + 296.0,
        w: 360.0,
        h: 50.0,
    };
    if guest_btn.clicked(app) {
        state.auth = None;
        state.auth_form = AuthForm::default();
        state.screen = Screen::Menu;
    }
}

pub fn draw_auth(app: &mut App, gfx: &mut Graphics, state: &State) {
    let (ww, wh) = (app.window().width() as f32, app.window().height() as f32);
    let cx = ww / 2.0;
    let base_y = wh / 2.0 - 160.0;

    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    draw.text(&state.font, "ROUILLO")
        .position(cx, base_y - 80.0)
        .size(64.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.9, 0.7, 1.0));

    let modes = [("Connexion", AuthMode::Login), ("Inscription", AuthMode::Register)];
    for (i, (label, mode)) in modes.iter().enumerate() {
        let x = cx - 230.0 + i as f32 * 240.0;
        let active = state.auth_form.mode == *mode;
        let bg = if active {
            Color::from_rgb(0.28, 0.30, 0.42)
        } else {
            Color::from_rgb(0.12, 0.12, 0.18)
        };
        let border = if active {
            Color::from_rgb(0.6, 0.5, 0.9)
        } else {
            Color::from_rgb(0.3, 0.3, 0.4)
        };
        draw.rect((x, base_y - 10.0), (220.0, 50.0)).color(bg);
        draw.rect((x, base_y - 10.0), (220.0, 50.0)).stroke(2.0).color(border);
        draw.text(&state.font, label)
            .position(x + 110.0, base_y + 15.0)
            .size(24.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::WHITE);
    }

    draw_input_field(
        &mut draw,
        &state.font,
        cx,
        base_y + 60.0,
        "Nom d'utilisateur",
        &state.auth_form.username,
        false,
        state.auth_form.focused == AuthField::Username,
    );
    draw_input_field(
        &mut draw,
        &state.font,
        cx,
        base_y + 140.0,
        "Mot de passe",
        &state.auth_form.password,
        true,
        state.auth_form.focused == AuthField::Password,
    );

    let loading = state.auth_form.pending.is_some();
    let submit_label = if loading {
        "Chargement..."
    } else {
        match state.auth_form.mode {
            AuthMode::Login => "Se connecter",
            AuthMode::Register => "S'inscrire",
        }
    };
    let submit_btn = Btn {
        x: cx - 180.0,
        y: base_y + 220.0,
        w: 360.0,
        h: 56.0,
    };
    submit_btn.draw_styled(&mut draw, app, &state.font, submit_label, !loading);

    let guest_btn = Btn {
        x: cx - 180.0,
        y: base_y + 296.0,
        w: 360.0,
        h: 50.0,
    };
    guest_btn.draw(&mut draw, app, &state.font, "Jouer en invité");

    if !state.auth_form.error.is_empty() {
        draw.text(&state.font, &state.auth_form.error)
            .position(cx, base_y + 370.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.9, 0.3, 0.3));
    }

    gfx.render(&draw);
}

#[allow(clippy::too_many_arguments)]
fn draw_input_field(
    draw: &mut Draw,
    font: &crate::Font,
    cx: f32,
    y: f32,
    placeholder: &str,
    value: &str,
    secret: bool,
    focused: bool,
) {
    let w = 420.0;
    let h = 50.0;
    let x = cx - w / 2.0;

    let border_color = if focused {
        Color::from_rgb(0.6, 0.5, 0.9)
    } else {
        Color::from_rgb(0.35, 0.37, 0.5)
    };

    draw.rect((x, y), (w, h)).color(Color::from_rgb(0.12, 0.13, 0.20));
    draw.rect((x, y), (w, h)).stroke(2.0).color(border_color);

    let (shown, col) = if value.is_empty() {
        (placeholder.to_owned(), Color::from_rgb(0.45, 0.45, 0.55))
    } else if secret {
        ("*".repeat(value.chars().count()), Color::WHITE)
    } else {
        (value.to_owned(), Color::WHITE)
    };

    draw.text(font, &shown)
        .position(x + 14.0, y + h / 2.0)
        .size(24.0)
        .v_align_middle()
        .color(col);
}
