use notan::draw::*;
use notan::prelude::*;

use crate::http;
use crate::menu::{draw_text_box, Btn};
use crate::state::{ApiFriendsResponse, FriendEntry, FriendsData, Screen, State, UserSearchEntry};

const SEARCH_Y: f32 = 112.0;
const SEARCH_RESULT_Y: f32 = 168.0;
const RESULT_ROW_H: f32 = 44.0;
const MAX_SEARCH_RESULTS: usize = 4;

const ADD_ERROR_Y: f32 = SEARCH_RESULT_Y + RESULT_ROW_H * MAX_SEARCH_RESULTS as f32 + 6.0;

const COL_HEADER_Y: f32 = ADD_ERROR_Y + 28.0;
const LIST_Y: f32 = COL_HEADER_Y + 44.0;
const ROW_H: f32 = 50.0;
const MAX_ROWS: usize = 6;
const COL_W: f32 = 360.0;
const COL_GAP: f32 = 30.0;

fn col_x(ww: f32, col: usize) -> f32 {
    let total = 3.0 * COL_W + 2.0 * COL_GAP;
    let margin = (ww - total) / 2.0;
    margin + col as f32 * (COL_W + COL_GAP)
}

fn search_submit_btn(ww: f32) -> Btn {
    Btn {
        x: ww / 2.0 + 155.0,
        y: SEARCH_Y,
        w: 160.0,
        h: 44.0,
    }
}

fn result_add_btn(ww: f32, row: usize) -> Btn {
    Btn {
        x: ww / 2.0 + 110.0,
        y: SEARCH_RESULT_Y + row as f32 * RESULT_ROW_H + 5.0,
        w: 120.0,
        h: 34.0,
    }
}

fn result_view_btn(ww: f32, row: usize) -> Btn {
    Btn {
        x: ww / 2.0 + 235.0,
        y: SEARCH_RESULT_Y + row as f32 * RESULT_ROW_H + 5.0,
        w: 115.0,
        h: 34.0,
    }
}

fn remove_btn(ww: f32, col: usize, row: usize) -> Btn {
    Btn {
        x: col_x(ww, col) + COL_W - 114.0,
        y: LIST_Y + row as f32 * ROW_H + 8.0,
        w: 110.0,
        h: 34.0,
    }
}

fn accept_btn(ww: f32, row: usize) -> Btn {
    Btn {
        x: col_x(ww, 1) + COL_W - 238.0,
        y: LIST_Y + row as f32 * ROW_H + 8.0,
        w: 120.0,
        h: 34.0,
    }
}

fn reject_btn(ww: f32, row: usize) -> Btn {
    Btn {
        x: col_x(ww, 1) + COL_W - 114.0,
        y: LIST_Y + row as f32 * ROW_H + 8.0,
        w: 110.0,
        h: 34.0,
    }
}

fn confirm_yes_btn(ww: f32, row: usize) -> Btn {
    Btn {
        x: col_x(ww, 0) + COL_W - 112.0,
        y: LIST_Y + row as f32 * ROW_H + 8.0,
        w: 52.0,
        h: 34.0,
    }
}

fn confirm_no_btn(ww: f32, row: usize) -> Btn {
    Btn {
        x: col_x(ww, 0) + COL_W - 56.0,
        y: LIST_Y + row as f32 * ROW_H + 8.0,
        w: 52.0,
        h: 34.0,
    }
}

fn back_btn(wh: f32) -> Btn {
    Btn {
        x: 40.0,
        y: wh - 80.0,
        w: 200.0,
        h: 54.0,
    }
}

fn refresh_btn(wh: f32) -> Btn {
    Btn {
        x: 254.0,
        y: wh - 80.0,
        w: 160.0,
        h: 54.0,
    }
}

pub fn enter_friends(state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, slot.clone());
    state.friends = Some(FriendsData {
        friends: Vec::new(),
        sent: Vec::new(),
        received: Vec::new(),
        list_slot: Some(slot),
        search_input: String::new(),
        search_results: Vec::new(),
        search_slot: None,
        search_error: String::new(),
        add_pending: None,
        add_error: String::new(),
        add_success: false,
        confirm_remove: None,
        action_pending: None,
        action_error: String::new(),
    });
}

fn refresh_list(state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, slot.clone());
    if let Some(f) = state.friends.as_mut() {
        f.list_slot = Some(slot);
    }
}

fn looks_like_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn try_search(state: &mut State) {
    let busy = state.friends.as_ref().map(|f| f.search_slot.is_some()).unwrap_or(true);
    if busy {
        return;
    }
    let q = state
        .friends
        .as_ref()
        .map(|f| f.search_input.trim().to_owned())
        .unwrap_or_default();
    if q.len() < 2 {
        if let Some(f) = state.friends.as_mut() {
            f.search_error = "Saisir au moins 2 caractères.".to_owned();
            f.search_results.clear();
        }
        return;
    }
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    let encoded_q = q.replace(' ', "%20");
    http::get(
        http::api_url(&format!("users/search?q={encoded_q}")),
        token,
        slot.clone(),
    );
    if let Some(f) = state.friends.as_mut() {
        f.search_slot = Some(slot);
        f.search_error.clear();
        f.search_results.clear();
    }
}

fn send_add_request(state: &mut State, user_id: &str) {
    let busy = state.friends.as_ref().map(|f| f.add_pending.is_some()).unwrap_or(true);
    if busy {
        return;
    }
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let body = serde_json::json!({ "user_id": user_id }).to_string();
    let slot = http::new_slot();
    http::post_json(http::api_url("friends"), body, token, slot.clone());
    if let Some(f) = state.friends.as_mut() {
        f.add_pending = Some(slot);
        f.add_error.clear();
    }
}

fn poll_list(state: &mut State) {
    let result = state
        .friends
        .as_ref()
        .and_then(|f| f.list_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(f) = state.friends.as_mut() {
        f.list_slot = None;
    }
    if let Ok(resp) = result {
        if let Some(text) = resp.text() {
            if let Ok(data) = serde_json::from_str::<ApiFriendsResponse>(text) {
                if let Some(f) = state.friends.as_mut() {
                    f.friends = data.friends;
                    f.sent = data.sent;
                    f.received = data.received;
                }
            }
        }
    }
}

fn poll_search(state: &mut State) {
    let result = state
        .friends
        .as_ref()
        .and_then(|f| f.search_slot.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(f) = state.friends.as_mut() {
        f.search_slot = None;
    }
    match result {
        Ok(resp) if resp.status == 200 => {
            if let Some(text) = resp.text() {
                if let Ok(entries) = serde_json::from_str::<Vec<UserSearchEntry>>(text) {
                    if let Some(f) = state.friends.as_mut() {
                        if entries.is_empty() {
                            f.search_error = "Aucun résultat.".to_owned();
                        }
                        f.search_results = entries;
                    }
                }
            }
        }
        Ok(resp) => {
            if let Some(f) = state.friends.as_mut() {
                f.search_error = format!("Erreur {}", resp.status);
            }
        }
        Err(e) => {
            if let Some(f) = state.friends.as_mut() {
                f.search_error = format!("Erreur réseau: {e}");
            }
        }
    }
}

fn poll_add(state: &mut State) {
    let result = state
        .friends
        .as_ref()
        .and_then(|f| f.add_pending.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(f) = state.friends.as_mut() {
        f.add_pending = None;
    }
    match result {
        Ok(resp) if resp.status == 201 => {
            if let Some(f) = state.friends.as_mut() {
                f.add_error = "Demande envoyée !".to_owned();
                f.add_success = true;
            }
            refresh_list(state);
        }
        Ok(resp) => {
            let msg = serde_json::from_str::<serde_json::Value>(resp.text().unwrap_or_default())
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("Erreur {}", resp.status));
            if let Some(f) = state.friends.as_mut() {
                f.add_error = msg;
                f.add_success = false;
            }
        }
        Err(e) => {
            if let Some(f) = state.friends.as_mut() {
                f.add_error = format!("Erreur réseau: {e}");
                f.add_success = false;
            }
        }
    }
}

fn poll_action(state: &mut State) {
    let result = state
        .friends
        .as_ref()
        .and_then(|f| f.action_pending.as_ref())
        .and_then(http::poll);
    let Some(result) = result else { return };
    if let Some(f) = state.friends.as_mut() {
        f.action_pending = None;
    }
    match result {
        Ok(resp) if resp.status < 300 => {
            if let Some(f) = state.friends.as_mut() {
                f.action_error.clear();
            }
            refresh_list(state);
        }
        Ok(resp) => {
            let msg = serde_json::from_str::<serde_json::Value>(resp.text().unwrap_or_default())
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("Erreur {}", resp.status));
            if let Some(f) = state.friends.as_mut() {
                f.action_error = msg;
            }
            refresh_list(state);
        }
        Err(e) => {
            if let Some(f) = state.friends.as_mut() {
                f.action_error = format!("Erreur réseau: {e}");
            }
        }
    }
}

enum FriendAction {
    StartRemove(String),
    ConfirmRemove(String),
    CancelRemove,
    Accept(String),
    Reject(String),
    Cancel(String),
}

pub fn update_friends(app: &mut App, state: &mut State) {
    poll_list(state);
    poll_search(state);
    poll_add(state);
    poll_action(state);

    let ww = app.window().width() as f32;
    let wh = app.window().height() as f32;

    if app.keyboard.was_pressed(KeyCode::Backspace) {
        if let Some(f) = state.friends.as_mut() {
            f.search_input.pop();
        }
    }

    if search_submit_btn(ww).clicked(app) || app.keyboard.was_pressed(KeyCode::Enter) {
        let q = state
            .friends
            .as_ref()
            .map(|f| f.search_input.trim().to_owned())
            .unwrap_or_default();
        if looks_like_uuid(&q) {
            crate::other_profile::enter_other_profile(state, q, "...".to_owned(), Screen::Friends);
            state.screen = Screen::OtherProfile;
            return;
        }
        try_search(state);
    }

    if back_btn(wh).clicked(app) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.friends = None;
        state.screen = Screen::Menu;
        return;
    }

    let can_refresh = state
        .friends
        .as_ref()
        .map(|f| f.list_slot.is_none() && f.action_pending.is_none())
        .unwrap_or(false);
    if can_refresh && refresh_btn(wh).clicked(app) {
        refresh_list(state);
    }

    let view_target: Option<(String, String)> = 'detect: {
        let Some(f) = &state.friends else {
            break 'detect None;
        };
        for (i, e) in f.search_results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
            if result_view_btn(ww, i).clicked(app) {
                break 'detect Some((e.user_id.clone(), e.username.clone()));
            }
        }
        None
    };
    if let Some((uid, uname)) = view_target {
        crate::other_profile::enter_other_profile(state, uid, uname, Screen::Friends);
        state.screen = Screen::OtherProfile;
        return;
    }

    let add_target: Option<String> = 'detect: {
        let Some(f) = &state.friends else {
            break 'detect None;
        };
        if f.add_pending.is_some() {
            break 'detect None;
        }
        for (i, e) in f.search_results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
            if result_add_btn(ww, i).clicked(app) && !f.friends.iter().any(|fr| fr.user_id == e.user_id) {
                break 'detect Some(e.user_id.clone());
            }
        }
        None
    };
    if let Some(uid) = add_target {
        send_add_request(state, &uid);
    }

    let action: Option<FriendAction> = 'detect: {
        let Some(f) = &state.friends else {
            break 'detect None;
        };
        if f.action_pending.is_some() {
            break 'detect None;
        }

        if let Some(confirm_id) = &f.confirm_remove {
            let confirm_row = f
                .friends
                .iter()
                .take(MAX_ROWS)
                .enumerate()
                .find(|(_, e)| &e.user_id == confirm_id)
                .map(|(i, _)| i);
            if let Some(row) = confirm_row {
                if confirm_yes_btn(ww, row).clicked(app) {
                    break 'detect Some(FriendAction::ConfirmRemove(confirm_id.clone()));
                }
                if confirm_no_btn(ww, row).clicked(app) {
                    break 'detect Some(FriendAction::CancelRemove);
                }
            }
            break 'detect None;
        }

        for (i, e) in f.friends.iter().take(MAX_ROWS).enumerate() {
            if remove_btn(ww, 0, i).clicked(app) {
                break 'detect Some(FriendAction::StartRemove(e.user_id.clone()));
            }
        }
        for (i, e) in f.received.iter().take(MAX_ROWS).enumerate() {
            if accept_btn(ww, i).clicked(app) {
                break 'detect Some(FriendAction::Accept(e.user_id.clone()));
            }
            if reject_btn(ww, i).clicked(app) {
                break 'detect Some(FriendAction::Reject(e.user_id.clone()));
            }
        }
        for (i, e) in f.sent.iter().take(MAX_ROWS).enumerate() {
            if remove_btn(ww, 2, i).clicked(app) {
                break 'detect Some(FriendAction::Cancel(e.user_id.clone()));
            }
        }
        None
    };

    if let Some(action) = action {
        match action {
            FriendAction::StartRemove(id) => {
                if let Some(f) = state.friends.as_mut() {
                    f.confirm_remove = Some(id);
                }
            }
            FriendAction::CancelRemove => {
                if let Some(f) = state.friends.as_mut() {
                    f.confirm_remove = None;
                }
            }
            FriendAction::ConfirmRemove(id) | FriendAction::Reject(id) | FriendAction::Cancel(id) => {
                let token = state.auth.as_ref().map(|a| a.token.clone());
                let slot = http::new_slot();
                http::delete_req(http::api_url(&format!("friends/{id}")), token, slot.clone());
                if let Some(f) = state.friends.as_mut() {
                    f.action_pending = Some(slot);
                    f.action_error.clear();
                    f.confirm_remove = None;
                }
            }
            FriendAction::Accept(id) => {
                let token = state.auth.as_ref().map(|a| a.token.clone());
                let slot = http::new_slot();
                http::post_empty(http::api_url(&format!("friends/{id}/accept")), token, slot.clone());
                if let Some(f) = state.friends.as_mut() {
                    f.action_pending = Some(slot);
                    f.action_error.clear();
                }
            }
        }
    }
}

pub fn draw_friends(app: &mut App, gfx: &mut Graphics, state: &State) {
    let ww = app.window().width() as f32;
    let wh = app.window().height() as f32;
    let cx = ww / 2.0;

    let mut draw = gfx.create_draw();
    draw.clear(Color::from_rgb(0.05, 0.05, 0.08));

    draw.text(&state.font, "Amis")
        .position(cx, 58.0)
        .size(52.0)
        .h_align_center()
        .v_align_middle()
        .color(Color::from_rgb(0.9, 0.7, 1.0));

    let f = match &state.friends {
        Some(f) => f,
        None => {
            gfx.render(&draw);
            return;
        }
    };

    draw.text(&state.font, "Rechercher un ami :")
        .position(ww / 2.0 - 280.0, 96.0)
        .size(17.0)
        .v_align_middle()
        .color(Color::from_rgb(0.65, 0.65, 0.80));

    let searching = f.search_slot.is_some();
    draw_text_box(
        &mut draw,
        &state.font,
        ww / 2.0 - 280.0,
        SEARCH_Y,
        430.0,
        44.0,
        "Pseudo ou UUID",
        &f.search_input,
        true,
    );
    search_submit_btn(ww).draw_styled(
        &mut draw,
        app,
        &state.font,
        if searching { "..." } else { "Rechercher" },
        !searching,
    );

    if !f.search_results.is_empty() {
        for (i, e) in f.search_results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
            let y = SEARCH_RESULT_Y + i as f32 * RESULT_ROW_H;
            let bg = if i % 2 == 0 {
                Color::from_rgba(0.12, 0.12, 0.20, 0.9)
            } else {
                Color::from_rgba(0.09, 0.09, 0.15, 0.9)
            };
            draw.rect((ww / 2.0 - 280.0, y), (600.0, RESULT_ROW_H - 2.0)).color(bg);
            draw.text(&state.font, &e.username)
                .position(ww / 2.0 - 268.0, y + RESULT_ROW_H / 2.0)
                .size(20.0)
                .v_align_middle()
                .color(Color::WHITE);
            draw.text(&state.font, &format!("ELO {}", e.elo))
                .position(ww / 2.0 - 110.0, y + RESULT_ROW_H / 2.0)
                .size(17.0)
                .v_align_middle()
                .color(Color::YELLOW);
            let adding = f.add_pending.is_some();
            let already_friend = f.friends.iter().any(|fr| fr.user_id == e.user_id);
            if !already_friend {
                result_add_btn(ww, i).draw_styled(&mut draw, app, &state.font, "Ajouter", !adding);
            }
            result_view_btn(ww, i).draw(&mut draw, app, &state.font, "Profil");
        }
    } else if !f.search_error.is_empty() {
        let color = if f.search_error.starts_with("Erreur") {
            Color::from_rgb(0.9, 0.4, 0.4)
        } else {
            Color::GRAY
        };
        draw.text(&state.font, &f.search_error)
            .position(ww / 2.0 - 280.0, SEARCH_RESULT_Y + 16.0)
            .size(18.0)
            .v_align_middle()
            .color(color);
    }

    if !f.add_error.is_empty() {
        draw.text(&state.font, &f.add_error)
            .position(cx, ADD_ERROR_Y)
            .size(17.0)
            .h_align_center()
            .v_align_middle()
            .color(if f.add_success {
                Color::from_rgb(0.4, 0.9, 0.4)
            } else {
                Color::from_rgb(0.9, 0.4, 0.4)
            });
    }

    draw.rect((40.0, COL_HEADER_Y - 12.0), (ww - 80.0, 1.0))
        .color(Color::from_rgb(0.2, 0.2, 0.3));

    let headers = ["Amis", "Demandes reçues", "Envoyées"];
    for (col, header) in headers.iter().enumerate() {
        let hx = col_x(ww, col) + COL_W / 2.0;
        draw.text(&state.font, header)
            .position(hx, COL_HEADER_Y)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.7, 0.7, 0.9));
        draw.rect((col_x(ww, col), COL_HEADER_Y + 12.0), (COL_W, 1.0))
            .color(Color::from_rgb(0.3, 0.3, 0.45));
    }

    let loading = f.list_slot.is_some();
    let busy = f.action_pending.is_some();

    if loading {
        draw.text(&state.font, "Chargement...")
            .position(cx, LIST_Y + 16.0)
            .size(20.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::GRAY);
    } else {
        let confirm = f.confirm_remove.clone();

        draw_col(
            &mut draw,
            app,
            &state.font,
            ww,
            0,
            &f.friends,
            busy,
            "Aucun ami pour l'instant",
            |draw, app, font, entry, i, busy| {
                if confirm.as_deref() == Some(entry.user_id.as_str()) {
                    confirm_yes_btn(ww, i).draw_styled(draw, app, font, "Oui", !busy);
                    confirm_no_btn(ww, i).draw(draw, app, font, "Non");
                } else {
                    let active = !busy && confirm.is_none();
                    remove_btn(ww, 0, i).draw_styled(draw, app, font, "Retirer", active);
                }
            },
        );

        draw_col(
            &mut draw,
            app,
            &state.font,
            ww,
            1,
            &f.received,
            busy,
            "Aucune demande reçue",
            |draw, app, font, _, i, busy| {
                accept_btn(ww, i).draw_styled(draw, app, font, "Accepter", !busy);
                reject_btn(ww, i).draw_styled(draw, app, font, "Refuser", !busy);
            },
        );

        draw_col(
            &mut draw,
            app,
            &state.font,
            ww,
            2,
            &f.sent,
            busy,
            "Aucune demande envoyée",
            |draw, app, font, _, i, busy| {
                remove_btn(ww, 2, i).draw_styled(draw, app, font, "Annuler", !busy);
            },
        );
    }

    if !f.action_error.is_empty() {
        draw.text(&state.font, &f.action_error)
            .position(cx, LIST_Y + MAX_ROWS as f32 * ROW_H + 18.0)
            .size(16.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.9, 0.4, 0.4));
    }

    back_btn(wh).draw(&mut draw, app, &state.font, "Retour");
    let loading = f.list_slot.is_some();
    refresh_btn(wh).draw_styled(
        &mut draw,
        app,
        &state.font,
        if loading { "..." } else { "Rafraîchir" },
        !loading,
    );
    gfx.render(&draw);
}

fn draw_col<F>(
    draw: &mut Draw,
    app: &App,
    font: &crate::Font,
    ww: f32,
    col: usize,
    list: &[FriendEntry],
    busy: bool,
    empty_msg: &str,
    draw_buttons: F,
) where
    F: Fn(&mut Draw, &App, &crate::Font, &FriendEntry, usize, bool),
{
    if list.is_empty() {
        draw.text(font, empty_msg)
            .position(col_x(ww, col) + COL_W / 2.0, LIST_Y + 18.0)
            .size(16.0)
            .h_align_center()
            .v_align_middle()
            .color(Color::from_rgb(0.4, 0.4, 0.5));
        return;
    }
    for (i, e) in list.iter().take(MAX_ROWS).enumerate() {
        let x = col_x(ww, col);
        let y = LIST_Y + i as f32 * ROW_H;
        let bg = if i % 2 == 0 {
            Color::from_rgba(0.12, 0.12, 0.18, 0.8)
        } else {
            Color::from_rgba(0.09, 0.09, 0.14, 0.8)
        };
        draw.rect((x, y), (COL_W, ROW_H - 3.0)).color(bg);
        draw.text(font, &e.username)
            .position(x + 10.0, y + ROW_H / 2.0 - 8.0)
            .size(19.0)
            .v_align_middle()
            .color(Color::WHITE);
        draw.text(font, &format!("ELO {}", e.elo))
            .position(x + 10.0, y + ROW_H / 2.0 + 10.0)
            .size(14.0)
            .v_align_middle()
            .color(Color::YELLOW);
        draw_buttons(draw, app, font, e, i, busy);
    }
}
