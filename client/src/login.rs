use std::sync::Arc;

use notan::prelude::*;

use crate::state::{ApiAuthResponse, ApiMeResponse, AuthField, AuthForm, AuthInfo, AuthMode, Screen, State};
use crate::ui::{text_field, Field, Rect, SharpText, Status, View};
use crate::{http, theme};

struct AuthLayout {
    cx: f32,
    base_y: f32,
    tabs: [Rect; 2],
    username: Rect,
    password: Rect,
    submit: Rect,
    guest: Rect,
}

fn auth_layout(view: View) -> AuthLayout {
    let (ww, wh) = view.size();
    let cx = ww / 2.0;
    let base_y = wh / 2.0 - 160.0;
    let field = |y: f32| Rect::at(cx - 210.0, y, 420.0, 50.0);
    AuthLayout {
        cx,
        base_y,
        tabs: [
            Rect::at(cx - 230.0, base_y - 10.0, 220.0, 50.0),
            Rect::at(cx + 10.0, base_y - 10.0, 220.0, 50.0),
        ],
        username: field(base_y + 60.0),
        password: field(base_y + 140.0),
        submit: Rect::at(cx - 180.0, base_y + 220.0, 360.0, 56.0),
        guest: Rect::at(cx - 180.0, base_y + 296.0, 360.0, 50.0),
    }
}

fn submit(form: &mut AuthForm) {
    if form.pending.is_some() {
        return;
    }
    let username = form.username.trim().to_owned();
    let password = form.password.clone();
    if username.is_empty() || password.is_empty() {
        form.status = Status::error("Remplis tous les champs.");
        return;
    }
    let body = serde_json::json!({ "username": username, "password": password }).to_string();
    let slot = http::new_slot();
    let path = match form.mode {
        AuthMode::Login => "login",
        AuthMode::Register => "register",
    };
    http::post_json(http::api_url(path), body, None, Arc::clone(&slot));
    form.status.clear();
    form.pending = Some(slot);
}

fn poll_auth(state: &mut State) {
    let Some(result) = http::take(&mut state.auth_form.pending) else {
        return;
    };
    match result {
        Err(e) => {
            state.auth_form.status = Status::error(format!("Erreur réseau: {e}"));
        }
        Ok(resp) => {
            if resp.status == 200 || resp.status == 201 {
                match http::json::<ApiAuthResponse>(&resp) {
                    Some(r) => {
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
                    None => {
                        state.auth_form.status = Status::error("Réponse serveur invalide.");
                    }
                }
            } else {
                state.auth_form.status = Status::error(http::error_message(&resp));
            }
        }
    }
}

pub fn poll_startup_check(state: &mut State) {
    let Some(result) = http::take(&mut state.startup_check) else {
        return;
    };
    match result {
        Ok(resp) if resp.status == 200 => {
            if let Some(me) = http::json::<ApiMeResponse>(&resp) {
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

    let layout = auth_layout(state.ui.view());

    if state.backspace.fired() {
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

    for (tab, mode) in layout.tabs.iter().zip([AuthMode::Login, AuthMode::Register]) {
        if state.ui.clicked(*tab) {
            state.auth_form.mode = mode;
            state.auth_form.status.clear();
        }
    }

    if state.ui.clicked(layout.username) {
        state.auth_form.focused = AuthField::Username;
    }
    if state.ui.clicked(layout.password) {
        state.auth_form.focused = AuthField::Password;
    }

    if (state.ui.clicked(layout.submit) || app.keyboard.was_pressed(KeyCode::Enter))
        && state.auth_form.pending.is_none()
    {
        submit(&mut state.auth_form);
    }

    if state.ui.clicked(layout.guest) {
        state.auth = None;
        state.auth_form = AuthForm::default();
        state.screen = Screen::Menu;
    }
}

pub fn draw_auth(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let layout = auth_layout(state.ui.view());
    let AuthLayout { cx, base_y, .. } = layout;

    let mut draw = state.ui.screen_canvas(gfx);

    draw.sharp_text(&state.fonts.display, "ROUILLO")
        .position(cx, base_y - 80.0)
        .size(theme::size::HERO)
        .h_align_center()
        .v_align_middle()
        .color(pal.title);

    let modes = [("Connexion", AuthMode::Login), ("Inscription", AuthMode::Register)];
    for (&tab, (label, mode)) in layout.tabs.iter().zip(modes) {
        let active = state.auth_form.mode == mode;
        state.ui.tab(&mut draw, &state.fonts, tab, label, active);
    }

    let form = &state.auth_form;
    let fields = [
        (
            layout.username,
            "Nom d'utilisateur",
            &form.username,
            AuthField::Username,
        ),
        (layout.password, "Mot de passe", &form.password, AuthField::Password),
    ];
    for (rect, placeholder, value, which) in fields {
        let field = Field {
            placeholder,
            value,
            focused: form.focused == which,
            secret: which == AuthField::Password,
        };
        text_field(&mut draw, &pal, &state.fonts, rect, &field);
    }

    let loading = state.auth_form.pending.is_some();
    let submit_label = if loading {
        "Chargement..."
    } else {
        match state.auth_form.mode {
            AuthMode::Login => "Se connecter",
            AuthMode::Register => "S'inscrire",
        }
    };
    state
        .ui
        .button_enabled(&mut draw, &state.fonts, layout.submit, submit_label, !loading);
    state
        .ui
        .button(&mut draw, &state.fonts, layout.guest, "Jouer en invité");

    if let Some((msg, color)) = state.auth_form.status.shown(&pal) {
        draw.sharp_text(&state.fonts.text, msg)
            .position(cx, base_y + 370.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    gfx.render(&draw);
}
