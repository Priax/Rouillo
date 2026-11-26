use std::sync::Arc;

use crate::connection::now_secs;
use crate::nav::{Nav, Transition};
use crate::state::{
    ApiMeResponse, AuthField, AuthForm, AuthInfo, AuthMode, Screen, Startup, State, MAX_PASSWORD_CHARS,
};
use crate::ui::{self, widgets, Face, Gui, Rect, Status, View, FILL};
use crate::{http, theme};

const STARTUP_RETRY_SECS: f64 = 5.0;

struct AuthLayout {
    cx: f32,
    base_y: f32,
    card: Rect,
    tabs: [Rect; 2],
    username: Rect,
    password: Rect,
    submit: Rect,
    guest: Rect,
}

/// The title and form are 520 high. When the window is lower, as when a
/// phone's keyboard takes half of it, the focused field is kept in the middle.
fn auth_layout(view: View, focused: AuthField) -> AuthLayout {
    let (ww, wh) = view.size();
    let cx = ww / 2.0;
    let field_y = match focused {
        AuthField::Username => 60.0,
        AuthField::Password => 140.0,
    };
    let base_y = if wh >= 560.0 { wh / 2.0 - 160.0 } else { wh / 2.0 - field_y - 25.0 };
    let field = |y: f32| Rect::centered_on(cx, y, 420.0, 50.0);

    AuthLayout {
        cx,
        base_y,
        card: Rect::centered_on(cx, base_y - 40.0, 520.0, 440.0),
        tabs: Rect::centered_on(cx, base_y - 10.0, 460.0, 50.0).columns(20.0, [FILL, FILL]),
        username: field(base_y + 60.0),
        password: field(base_y + 140.0),
        submit: Rect::centered_on(cx, base_y + 220.0, 360.0, 56.0),
        guest: Rect::centered_on(cx, base_y + 296.0, 360.0, 50.0),
    }
}

fn submit(form: &mut AuthForm) {
    if form.pending.is_some() {
        return;
    }

    let username = form.username.trim().to_owned();
    let password = form.password.to_owned();
    if username.is_empty() || password.is_empty() {
        form.status = Status::error("Remplissez tous les champs.");
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

/// Whether the answer to the form just signed the player in.
fn poll_auth(state: &mut State) -> bool {
    let Some(result) = http::take_json::<AuthInfo>(&mut state.auth_form.pending) else {
        return false;
    };
    match result {
        Ok(r) => {
            crate::state::save_token(&r.token);
            state.auth = Some(r);
            state.auth_form = AuthForm::default();
            crate::solo::signed_in(state);
            return true;
        }
        Err(msg) => state.auth_form.status = Status::error(msg),
    }

    false
}

pub fn check_stored_session(startup: &mut Startup) {
    startup.retry_at = None;
    let Some(token) = crate::state::load_stored_token() else {
        return;
    };

    let slot = http::new_slot();
    http::get(http::api_url("me"), Some(token), Arc::clone(&slot));
    startup.check = Some(slot);
}

pub fn poll_startup_check(state: &mut State, nav: &mut Nav) {
    let waiting = state.auth.is_none() && matches!(nav.current(), Screen::Title | Screen::Auth);
    if waiting && state.startup.retry_at.is_some_and(|t| now_secs() >= t) {
        check_stored_session(&mut state.startup);
    }

    let Some(result) = http::take(&mut state.startup.check) else {
        return;
    };
    match result {
        Ok(resp) if resp.status == 200 => {
            let me = http::json::<ApiMeResponse>(&resp).filter(|_| waiting);
            if let (Some(me), Some(token)) = (me, crate::state::load_stored_token()) {
                state.auth = Some(AuthInfo {
                    token,
                    user_id: me.id,
                    username: me.username,
                    elo: me.elo,
                    avatar_url: me.avatar_url,
                });
                state.auth_form.status.clear();
                nav.apply(Transition::To(Screen::Menu));
                crate::solo::signed_in(state);
            }
        }
        Ok(resp) if resp.status == 401 => {
            crate::state::clear_stored_token();
            if matches!(nav.current(), Screen::Title) {
                nav.apply(Transition::To(Screen::Auth));
            }
        }
        _ => {
            state.startup.retry_at = Some(now_secs() + STARTUP_RETRY_SECS);
            state.auth_form.status = Status::info("Serveur injoignable, nouvel essai dans quelques secondes...");
        }
    }
}

pub fn update_auth(state: &mut State) -> Transition {
    if poll_auth(state) {
        return Transition::To(Screen::Menu);
    }
    Transition::Stay
}

pub fn show(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, state: &mut State) -> Transition {
    let pal = gui.ui.palette();
    let layout = auth_layout(gui.ui.view(), state.auth_form.focused);
    let AuthLayout { cx, base_y, .. } = layout;
    let form = &state.auth_form;
    let modes = [("Connexion", AuthMode::Login), ("Inscription", AuthMode::Register)];
    let loading = form.pending.is_some();
    let submit_label = match (loading, form.mode) {
        (true, _) => "Chargement...",
        (false, AuthMode::Login) => "Se connecter",
        (false, AuthMode::Register) => "S'inscrire",
    };

    let mut canvas = gui.ui.screen_canvas(ui);
    let ui = &mut *canvas;
    gui.fonts
        .write(ui.draw, Face::Display, "Rouillo")
        .center(cx, base_y - 110.0)
        .size(theme::size::HERO)
        .color(pal.title);
    ui::card(ui.draw, &pal, layout.card);
    let mut chosen = None;
    for (&tab, (label, mode)) in layout.tabs.iter().zip(modes) {
        if widgets::tab(ui, tab, label, form.mode == mode) {
            chosen = Some(mode);
        }
    }
    if !ui.cx.wants_keyboard() {
        widgets::focus_field(ui, form.focused);
    }
    let form = &mut state.auth_form;
    let name = widgets::field().placeholder("Nom d'utilisateur").max_chars(shared::MAX_USERNAME_CHARS);
    let name = widgets::text_field(ui, gui, AuthField::Username, layout.username, name, &mut form.username);
    let password = widgets::field().placeholder("Mot de passe").max_chars(MAX_PASSWORD_CHARS).masked(true);
    let password = widgets::text_field(ui, gui, AuthField::Password, layout.password, password, &mut form.password);
    if name.response.focused {
        form.focused = AuthField::Username;
    } else if password.response.focused {
        form.focused = AuthField::Password;
    }
    let submitted =
        widgets::button(ui, "submit", layout.submit, submit_label, !loading) || name.submitted || password.submitted;
    let guest = widgets::button(ui, "guest", layout.guest, "Jouer en invité", true);
    if let Some((msg, color)) = form.status.shown(&pal) {
        gui.fonts.write(ui.draw, Face::Text, msg).center(cx, base_y + 370.0).size(theme::size::LABEL).color(color);
    }

    if let Some(mode) = chosen {
        form.mode = mode;
        form.status.clear();
    }
    if submitted {
        submit(form);
    }
    if guest {
        state.auth = None;
        state.auth_form = AuthForm::default();
        return Transition::To(Screen::Menu);
    }
    Transition::Stay
}
