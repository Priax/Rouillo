use std::sync::Arc;

use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;

use crate::state::{ApiFriendsResponse, FriendEntry, FriendsData, Screen, State, UserSearchEntry};
use crate::ui::{text_field, Field, Fonts, Rect, SharpText, Status, Ui};
use crate::{http, theme};

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

fn search_box(ww: f32) -> Rect {
    Rect::at(ww / 2.0 - 280.0, SEARCH_Y, 430.0, 44.0)
}

fn search_submit_btn(ww: f32) -> Rect {
    Rect::at(ww / 2.0 + 155.0, SEARCH_Y, 160.0, 44.0)
}

fn result_add_btn(ww: f32, row: usize) -> Rect {
    Rect::at(
        ww / 2.0 + 110.0,
        SEARCH_RESULT_Y + row as f32 * RESULT_ROW_H + 5.0,
        120.0,
        34.0,
    )
}

fn result_view_btn(ww: f32, row: usize) -> Rect {
    Rect::at(
        ww / 2.0 + 235.0,
        SEARCH_RESULT_Y + row as f32 * RESULT_ROW_H + 5.0,
        115.0,
        34.0,
    )
}

fn remove_btn(ww: f32, col: usize, row: usize) -> Rect {
    Rect::at(
        col_x(ww, col) + COL_W - 114.0,
        LIST_Y + row as f32 * ROW_H + 8.0,
        110.0,
        34.0,
    )
}

fn accept_btn(ww: f32, row: usize) -> Rect {
    Rect::at(
        col_x(ww, 1) + COL_W - 238.0,
        LIST_Y + row as f32 * ROW_H + 8.0,
        120.0,
        34.0,
    )
}

fn reject_btn(ww: f32, row: usize) -> Rect {
    Rect::at(
        col_x(ww, 1) + COL_W - 114.0,
        LIST_Y + row as f32 * ROW_H + 8.0,
        110.0,
        34.0,
    )
}

fn confirm_yes_btn(ww: f32, row: usize) -> Rect {
    Rect::at(
        col_x(ww, 0) + COL_W - 112.0,
        LIST_Y + row as f32 * ROW_H + 8.0,
        52.0,
        34.0,
    )
}

fn confirm_no_btn(ww: f32, row: usize) -> Rect {
    Rect::at(
        col_x(ww, 0) + COL_W - 56.0,
        LIST_Y + row as f32 * ROW_H + 8.0,
        52.0,
        34.0,
    )
}

fn back_btn(wh: f32) -> Rect {
    Rect::at(40.0, wh - 80.0, 200.0, 54.0)
}

fn refresh_btn(wh: f32) -> Rect {
    Rect::at(254.0, wh - 80.0, 160.0, 54.0)
}

pub fn enter_friends(state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, Arc::clone(&slot));
    state.friends = Some(FriendsData {
        friends: Vec::new(),
        sent: Vec::new(),
        received: Vec::new(),
        list_slot: Some(slot),
        search_input: String::new(),
        search_results: Vec::new(),
        search_slot: None,
        search_status: Status::Empty,
        add_pending: None,
        add_status: Status::Empty,
        confirm_remove: None,
        action_pending: None,
        action_status: Status::Empty,
    });
}

fn refresh_list(state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let slot = http::new_slot();
    http::get(http::api_url("friends"), token, Arc::clone(&slot));
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
    let busy = state.friends.as_ref().is_none_or(|f| f.search_slot.is_some());
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
            f.search_status = Status::info("Saisir au moins 2 caractères.");
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
        Arc::clone(&slot),
    );
    if let Some(f) = state.friends.as_mut() {
        f.search_slot = Some(slot);
        f.search_status.clear();
        f.search_results.clear();
    }
}

fn send_add_request(state: &mut State, user_id: &str) {
    let busy = state.friends.as_ref().is_none_or(|f| f.add_pending.is_some());
    if busy {
        return;
    }
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let body = serde_json::json!({ "user_id": user_id }).to_string();
    let slot = http::new_slot();
    http::post_json(http::api_url("friends"), body, token, Arc::clone(&slot));
    if let Some(f) = state.friends.as_mut() {
        f.add_pending = Some(slot);
        f.add_status.clear();
    }
}

fn poll_list(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(Ok(resp)) = http::take(&mut f.list_slot) else {
        return;
    };
    let Some(data) = http::json::<ApiFriendsResponse>(&resp) else {
        return;
    };
    f.friends = data.friends;
    f.sent = data.sent;
    f.received = data.received;
}

fn poll_search(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(result) = http::take(&mut f.search_slot) else {
        return;
    };
    match result {
        Ok(resp) if resp.status == 200 => {
            if let Some(entries) = http::json::<Vec<UserSearchEntry>>(&resp) {
                if entries.is_empty() {
                    f.search_status = Status::info("Aucun résultat.");
                }
                f.search_results = entries;
            }
        }
        Ok(resp) => f.search_status = Status::error(format!("Erreur {}", resp.status)),
        Err(e) => f.search_status = Status::error(format!("Erreur réseau: {e}")),
    }
}

fn poll_add(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(result) = http::take(&mut f.add_pending) else {
        return;
    };
    let refresh = match result {
        Ok(resp) if resp.status == 201 => {
            f.add_status = Status::success("Demande envoyée !");
            true
        }
        Ok(resp) => {
            f.add_status = Status::error(http::error_message(&resp));
            false
        }
        Err(e) => {
            f.add_status = Status::error(format!("Erreur réseau: {e}"));
            false
        }
    };
    if refresh {
        refresh_list(state);
    }
}

fn poll_action(state: &mut State) {
    let Some(f) = state.friends.as_mut() else { return };
    let Some(result) = http::take(&mut f.action_pending) else {
        return;
    };
    let refresh = match result {
        Ok(resp) if resp.status < 300 => {
            f.action_status.clear();
            true
        }
        Ok(resp) => {
            f.action_status = Status::error(http::error_message(&resp));
            true
        }
        Err(e) => {
            f.action_status = Status::error(format!("Erreur réseau: {e}"));
            false
        }
    };
    if refresh {
        refresh_list(state);
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

    let ww = state.ui.view().w;
    let wh = state.ui.view().h;

    if state.backspace.fired() {
        if let Some(f) = state.friends.as_mut() {
            f.search_input.pop();
        }
    }

    if state.ui.clicked(search_submit_btn(ww)) || app.keyboard.was_pressed(KeyCode::Enter) {
        let q = state
            .friends
            .as_ref()
            .map(|f| f.search_input.trim().to_owned())
            .unwrap_or_default();
        if looks_like_uuid(&q) {
            crate::profile::enter_other_profile(state, q, "...".to_owned(), Screen::Friends);
            state.screen = Screen::OtherProfile;
            return;
        }
        try_search(state);
    }

    if state.ui.clicked(back_btn(wh)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.friends = None;
        state.screen = Screen::Menu;
        return;
    }

    if state.friends.as_ref().is_some_and(can_refresh) && state.ui.clicked(refresh_btn(wh)) {
        refresh_list(state);
    }

    let Some(f) = &state.friends else {
        return;
    };
    if let Some((uid, uname)) = clicked_profile(f, &state.ui, ww) {
        crate::profile::enter_other_profile(state, uid, uname, Screen::Friends);
        state.screen = Screen::OtherProfile;
        return;
    }
    let add = clicked_add(f, &state.ui, ww);
    let action = clicked_action(f, &state.ui, ww);
    if let Some(uid) = add {
        send_add_request(state, &uid);
    }
    if let Some(action) = action {
        apply_action(state, action);
    }
}

fn can_refresh(f: &FriendsData) -> bool {
    f.list_slot.is_none() && f.action_pending.is_none()
}

fn clicked_profile(f: &FriendsData, ui: &Ui, ww: f32) -> Option<(String, String)> {
    let (_, e) = f
        .search_results
        .iter()
        .take(MAX_SEARCH_RESULTS)
        .enumerate()
        .find(|&(i, _)| ui.clicked(result_view_btn(ww, i)))?;
    Some((e.user_id.clone(), e.username.clone()))
}

fn clicked_add(f: &FriendsData, ui: &Ui, ww: f32) -> Option<String> {
    if f.add_pending.is_some() {
        return None;
    }
    f.search_results
        .iter()
        .take(MAX_SEARCH_RESULTS)
        .enumerate()
        .find(|&(i, e)| ui.clicked(result_add_btn(ww, i)) && !f.friends.iter().any(|fr| fr.user_id == e.user_id))
        .map(|(_, e)| e.user_id.clone())
}

fn clicked_action(f: &FriendsData, ui: &Ui, ww: f32) -> Option<FriendAction> {
    if f.action_pending.is_some() {
        return None;
    }
    if let Some(confirm_id) = &f.confirm_remove {
        let row = f.friends.iter().take(MAX_ROWS).position(|e| &e.user_id == confirm_id)?;
        if ui.clicked(confirm_yes_btn(ww, row)) {
            return Some(FriendAction::ConfirmRemove(confirm_id.clone()));
        }
        if ui.clicked(confirm_no_btn(ww, row)) {
            return Some(FriendAction::CancelRemove);
        }
        return None;
    }
    for (i, e) in f.friends.iter().take(MAX_ROWS).enumerate() {
        if ui.clicked(remove_btn(ww, 0, i)) {
            return Some(FriendAction::StartRemove(e.user_id.clone()));
        }
    }
    for (i, e) in f.received.iter().take(MAX_ROWS).enumerate() {
        if ui.clicked(accept_btn(ww, i)) {
            return Some(FriendAction::Accept(e.user_id.clone()));
        }
        if ui.clicked(reject_btn(ww, i)) {
            return Some(FriendAction::Reject(e.user_id.clone()));
        }
    }
    for (i, e) in f.sent.iter().take(MAX_ROWS).enumerate() {
        if ui.clicked(remove_btn(ww, 2, i)) {
            return Some(FriendAction::Cancel(e.user_id.clone()));
        }
    }
    None
}

fn apply_action(state: &mut State, action: FriendAction) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let Some(f) = state.friends.as_mut() else {
        return;
    };
    match action {
        FriendAction::StartRemove(id) => f.confirm_remove = Some(id),
        FriendAction::CancelRemove => f.confirm_remove = None,
        FriendAction::ConfirmRemove(id) | FriendAction::Reject(id) | FriendAction::Cancel(id) => {
            let slot = http::new_slot();
            http::delete_req(http::api_url(&format!("friends/{id}")), token, Arc::clone(&slot));
            f.action_pending = Some(slot);
            f.action_status.clear();
            f.confirm_remove = None;
        }
        FriendAction::Accept(id) => {
            let slot = http::new_slot();
            http::post_empty(http::api_url(&format!("friends/{id}/accept")), token, Arc::clone(&slot));
            f.action_pending = Some(slot);
            f.action_status.clear();
        }
    }
}

pub fn draw_friends(gfx: &mut Graphics, state: &State) {
    let ww = state.ui.view().w;
    let wh = state.ui.view().h;

    let mut draw = state.ui.canvas(gfx);
    draw.clear(theme::BACKGROUND);
    draw.sharp_text(&state.fonts.display, "Amis")
        .position(ww / 2.0, 58.0)
        .size(theme::size::TITLE)
        .h_align_center()
        .v_align_middle()
        .color(theme::TITLE);

    if let Some(f) = &state.friends {
        draw_search(&mut draw, &state.ui, &state.fonts, f, ww);
        draw_lists(&mut draw, &state.ui, &state.fonts, f, ww);

        state.ui.button(&mut draw, &state.fonts, back_btn(wh), "Retour");
        let label = if f.list_slot.is_some() { "..." } else { "Rafraîchir" };
        state
            .ui
            .button_enabled(&mut draw, &state.fonts, refresh_btn(wh), label, can_refresh(f));
    }
    gfx.render(&draw);
}

fn draw_search(draw: &mut Draw, ui: &Ui, fonts: &Fonts, f: &FriendsData, ww: f32) {
    let field = search_box(ww);
    draw.sharp_text(&fonts.text, "Rechercher un ami:")
        .position(field.x, 96.0)
        .size(theme::size::BODY)
        .v_align_middle()
        .color(theme::TEXT_DIM);

    let searching = f.search_slot.is_some();
    let search = Field {
        placeholder: "Pseudo ou UUID",
        value: &f.search_input,
        focused: true,
        secret: false,
    };
    text_field(draw, fonts, field, &search);
    ui.button_enabled(
        draw,
        fonts,
        search_submit_btn(ww),
        if searching { "..." } else { "Rechercher" },
        !searching,
    );

    if f.search_results.is_empty() {
        if let Some((msg, color)) = f.search_status.shown() {
            draw.sharp_text(&fonts.text, msg)
                .position(field.x, SEARCH_RESULT_Y + 16.0)
                .size(theme::size::BODY)
                .v_align_middle()
                .color(color);
        }
    }
    let adding = f.add_pending.is_some();
    for (i, e) in f.search_results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
        let y = SEARCH_RESULT_Y + i as f32 * RESULT_ROW_H;
        let bg = if i % 2 == 0 { theme::SURFACE } else { theme::SURFACE_ALT };
        draw.rect((field.x, y), (600.0, RESULT_ROW_H - 2.0)).color(bg);
        draw.sharp_text(&fonts.text, &e.username)
            .position(field.x + 12.0, y + RESULT_ROW_H / 2.0)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(theme::TEXT);
        draw.sharp_text(&fonts.text, &format!("ELO {}", e.elo))
            .position(field.x + 170.0, y + RESULT_ROW_H / 2.0)
            .size(theme::size::BODY)
            .v_align_middle()
            .color(theme::GOLD);
        if !f.friends.iter().any(|fr| fr.user_id == e.user_id) {
            ui.button_enabled(draw, fonts, result_add_btn(ww, i), "Ajouter", !adding);
        }
        ui.button(draw, fonts, result_view_btn(ww, i), "Profil");
    }

    if let Some((msg, color)) = f.add_status.shown() {
        draw.sharp_text(&fonts.text, msg)
            .position(ww / 2.0, ADD_ERROR_Y)
            .size(theme::size::BODY)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

fn draw_lists(draw: &mut Draw, ui: &Ui, fonts: &Fonts, f: &FriendsData, ww: f32) {
    draw.rect((40.0, COL_HEADER_Y - 12.0), (ww - 80.0, 1.0))
        .color(theme::DIVIDER);
    let headers = ["Amis", "Demandes reçues", "Envoyées"];
    for (col, header) in headers.iter().enumerate() {
        draw.sharp_text(&fonts.text, header)
            .position(col_x(ww, col) + COL_W / 2.0, COL_HEADER_Y)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_DIM);
        draw.rect((col_x(ww, col), COL_HEADER_Y + 12.0), (COL_W, 1.0))
            .color(theme::DIVIDER);
    }

    if f.list_slot.is_some() {
        draw.sharp_text(&fonts.text, "Chargement...")
            .position(ww / 2.0, LIST_Y + 16.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_MUTED);
    } else {
        let busy = f.action_pending.is_some();
        let confirm = f.confirm_remove.as_deref();
        draw_col(
            draw,
            fonts,
            col_x(ww, 0),
            &f.friends,
            "Aucun ami pour l'instant",
            |draw, fonts, entry, i| {
                if confirm == Some(entry.user_id.as_str()) {
                    ui.button_enabled(draw, fonts, confirm_yes_btn(ww, i), "Oui", !busy);
                    ui.button(draw, fonts, confirm_no_btn(ww, i), "Non");
                } else {
                    let active = !busy && confirm.is_none();
                    ui.button_enabled(draw, fonts, remove_btn(ww, 0, i), "Retirer", active);
                }
            },
        );
        draw_col(
            draw,
            fonts,
            col_x(ww, 1),
            &f.received,
            "Aucune demande reçue",
            |draw, fonts, _, i| {
                ui.button_enabled(draw, fonts, accept_btn(ww, i), "Accepter", !busy);
                ui.button_enabled(draw, fonts, reject_btn(ww, i), "Refuser", !busy);
            },
        );
        draw_col(
            draw,
            fonts,
            col_x(ww, 2),
            &f.sent,
            "Aucune demande envoyée",
            |draw, fonts, _, i| {
                ui.button_enabled(draw, fonts, remove_btn(ww, 2, i), "Annuler", !busy);
            },
        );
    }

    if let Some((msg, color)) = f.action_status.shown() {
        draw.sharp_text(&fonts.text, msg)
            .position(ww / 2.0, LIST_Y + MAX_ROWS as f32 * ROW_H + 18.0)
            .size(theme::size::SMALL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

fn draw_col<F>(draw: &mut Draw, fonts: &Fonts, x: f32, list: &[FriendEntry], empty_msg: &str, draw_buttons: F)
where
    F: Fn(&mut Draw, &Fonts, &FriendEntry, usize),
{
    if list.is_empty() {
        draw.sharp_text(&fonts.text, empty_msg)
            .position(x + COL_W / 2.0, LIST_Y + 18.0)
            .size(theme::size::SMALL)
            .h_align_center()
            .v_align_middle()
            .color(theme::TEXT_MUTED);
        return;
    }
    for (i, e) in list.iter().take(MAX_ROWS).enumerate() {
        let y = LIST_Y + i as f32 * ROW_H;
        let bg = if i % 2 == 0 { theme::SURFACE } else { theme::SURFACE_ALT };
        draw.rect((x, y), (COL_W, ROW_H - 3.0)).color(bg);
        draw.sharp_text(&fonts.text, &e.username)
            .position(x + 10.0, y + ROW_H / 2.0 - 8.0)
            .size(theme::size::BODY)
            .v_align_middle()
            .color(theme::TEXT);
        draw.sharp_text(&fonts.text, &format!("ELO {}", e.elo))
            .position(x + 10.0, y + ROW_H / 2.0 + 10.0)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(theme::GOLD);
        draw_buttons(draw, fonts, e, i);
    }
}
