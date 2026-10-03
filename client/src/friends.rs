use std::collections::HashMap;
use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::state::{ApiFriendsResponse, FriendEntry, FriendsData, Screen, State, UserSearchEntry};
use crate::ui::{
    self, divider, field_clicked, list_row, page_count, text_field, Face, Field, Fonts, Pager, Pill, Rect, SharpText,
    Status, TextInput, Ui,
};
use crate::{http, theme};

const FIELD_H: f32 = 44.0;
const RESULTS_TOP: f32 = 128.0;
const RESULT_ROW_H: f32 = 44.0;
const MAX_SEARCH_RESULTS: usize = 4;
const ADD_STATUS_Y: f32 = 330.0;
const COLUMNS_TOP: f32 = 344.0;
const CARD_TITLE_H: f32 = 50.0;
const LIST_Y: f32 = COLUMNS_TOP + CARD_TITLE_H + 6.0;
const ROW_H: f32 = 48.0;
const MAX_ROWS: usize = 5;
const CARD_ROWS: usize = MAX_ROWS + 1;
const COL_W: f32 = 360.0;
const COL_GAP: f32 = 30.0;
const ACTION_STATUS_Y: f32 = 708.0;
const SMALL_BTN_H: f32 = 32.0;

fn col_x(ww: f32, col: usize) -> f32 {
    let total = 3.0 * COL_W + 2.0 * COL_GAP;
    let margin = (ww - total) / 2.0;
    margin + col as f32 * (COL_W + COL_GAP)
}

fn column_card(ww: f32, col: usize) -> Rect {
    let bottom = LIST_Y + CARD_ROWS as f32 * ROW_H + 10.0;
    Rect::at(col_x(ww, col), COLUMNS_TOP, COL_W, bottom - COLUMNS_TOP)
}

fn list_row_rect(ww: f32, col: usize, row: usize) -> Rect {
    let card = column_card(ww, col);
    Rect::at(card.x + 8.0, LIST_Y + row as f32 * ROW_H, card.w - 16.0, ROW_H - 4.0)
}

fn pager_area(ww: f32, col: usize) -> Rect {
    let card = column_card(ww, col);
    Rect::at(card.x, LIST_Y + MAX_ROWS as f32 * ROW_H, card.w, ROW_H)
}

fn lists(f: &FriendsData) -> [&[FriendEntry]; 3] {
    [&f.friends, &f.received, &f.sent]
}

fn row_button(row: Rect, from_right: f32, w: f32) -> Rect {
    Rect::at(
        row.x + row.w - from_right - w,
        row.y + (row.h - SMALL_BTN_H) / 2.0,
        w,
        SMALL_BTN_H,
    )
}

fn search_submit_btn(ww: f32) -> Rect {
    Rect::at(
        ww - 60.0 - 160.0,
        (theme::HEADER_H - FIELD_H) / 2.0 + 12.0,
        160.0,
        FIELD_H,
    )
}

fn search_box(ww: f32) -> Rect {
    let submit = search_submit_btn(ww);
    Rect::at(submit.x - 12.0 - 430.0, submit.y, 430.0, FIELD_H)
}

fn results_card(ww: f32) -> Rect {
    Rect::at(
        ww / 2.0 - 420.0,
        RESULTS_TOP,
        840.0,
        16.0 + MAX_SEARCH_RESULTS as f32 * RESULT_ROW_H,
    )
}

fn result_row(ww: f32, row: usize) -> Rect {
    let card = results_card(ww);
    Rect::at(
        card.x + 8.0,
        card.y + 8.0 + row as f32 * RESULT_ROW_H,
        card.w - 16.0,
        RESULT_ROW_H - 4.0,
    )
}

fn result_add_btn(ww: f32, row: usize) -> Rect {
    row_button(result_row(ww, row), 128.0, 120.0)
}

fn result_view_btn(ww: f32, row: usize) -> Rect {
    row_button(result_row(ww, row), 4.0, 116.0)
}

fn remove_btn(ww: f32, col: usize, row: usize) -> Rect {
    row_button(list_row_rect(ww, col, row), 6.0, 110.0)
}

fn watch_btn(ww: f32, row: usize) -> Rect {
    row_button(list_row_rect(ww, 0, row), 122.0, 110.0)
}

type Pictures = HashMap<String, Texture>;

fn pictures<'a>(gfx: &mut Graphics, state: &State, urls: impl Iterator<Item = &'a Option<String>>) -> Pictures {
    urls.flatten()
        .filter_map(|url| Some((url.clone(), state.images.get(gfx, url)?)))
        .collect()
}

fn avatar(
    draw: &mut Draw,
    ui: &Ui,
    fonts: &Fonts,
    (x, cy): (f32, f32),
    name: &str,
    url: Option<&String>,
    pics: &Pictures,
) {
    let who = ui::Persona {
        name,
        glow: 0.0,
        picture: url.and_then(|u| pics.get(u)),
    };
    ui::portrait(draw, &ui.palette(), fonts, (x + AVATAR_R, cy), AVATAR_R, &who);
}

const AVATAR_R: f32 = 15.0;
const AVATAR_ROOM: f32 = 2.0 * AVATAR_R + 10.0;

fn name_zone(ww: f32, col: usize, row: usize) -> Rect {
    let rect = list_row_rect(ww, col, row);
    let first_button = [watch_btn(ww, row), accept_btn(ww, row), remove_btn(ww, 2, row)][col];
    Rect::at(rect.x, rect.y, first_button.x - rect.x - 8.0, rect.h)
}

fn accept_btn(ww: f32, row: usize) -> Rect {
    row_button(list_row_rect(ww, 1, row), 122.0, 116.0)
}

fn reject_btn(ww: f32, row: usize) -> Rect {
    row_button(list_row_rect(ww, 1, row), 6.0, 110.0)
}

fn confirm_yes_btn(ww: f32, row: usize) -> Rect {
    row_button(list_row_rect(ww, 0, row), 62.0, 52.0)
}

fn confirm_no_btn(ww: f32, row: usize) -> Rect {
    row_button(list_row_rect(ww, 0, row), 6.0, 52.0)
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
        search_input: TextInput::default(),
        search_results: Vec::new(),
        search_slot: None,
        search_status: Status::Empty,
        add_pending: None,
        add_status: Status::Empty,
        confirm_remove: None,
        action_pending: None,
        action_status: Status::Empty,
        pages: Default::default(),
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
    http::get(
        http::api_url(&format!("users/search?q={}", http::encode_query(&q))),
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
        Ok(resp) => f.search_status = Status::error(http::error_message(&resp)),
        Err(e) => f.search_status = Status::error(http::network_error(&e)),
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
            f.add_status = Status::error(http::network_error(&e));
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
            f.action_status = Status::error(http::network_error(&e));
            false
        }
    };
    if refresh {
        refresh_list(state);
    }
}

enum FriendAction {
    Watch(String),
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

    let paging = state
        .friends
        .as_ref()
        .is_some_and(|f| f.pages.iter().any(Pager::focused));
    if let Some(f) = state.friends.as_mut() {
        if !paging {
            f.search_input.edit(&state.keys);
        }
        field_clicked(&state.ui, &state.fonts, search_box(ww), &mut f.search_input);
        for col in 0..3 {
            let pages = page_count(lists(f)[col].len(), MAX_ROWS);
            f.pages[col].clamp(pages);
            f.pages[col].update(app, &state.ui, &state.fonts, &state.keys, pager_area(ww, col), pages);
        }
    }

    if state.ui.clicked(search_submit_btn(ww)) || (!paging && app.keyboard.was_pressed(KeyCode::Enter)) {
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
    if let Some((uid, uname)) = clicked_profile(f, &state.ui, ww).or_else(|| clicked_listed(f, &state.ui, ww)) {
        crate::profile::enter_other_profile(state, uid, uname, Screen::Friends);
        state.screen = Screen::OtherProfile;
        return;
    }
    let add = clicked_add(f, &state.ui, ww);
    let action = clicked_action(f, &state.ui, ww);
    if let Some(uid) = add {
        send_add_request(state, &uid);
    }
    match action {
        Some(FriendAction::Watch(user_id)) => watch(state, user_id),
        Some(action) => apply_action(state, action),
        None => {}
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

fn clicked_listed(f: &FriendsData, ui: &Ui, ww: f32) -> Option<(String, String)> {
    if f.list_slot.is_some() {
        return None;
    }
    lists(f).into_iter().enumerate().find_map(|(col, list)| {
        f.pages[col]
            .shown(list, MAX_ROWS)
            .find(|&(i, _)| ui.clicked(name_zone(ww, col, i)))
            .map(|(_, e)| (e.user_id.clone(), e.username.clone()))
    })
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
    // While the lists reload they are not drawn, so neither are their buttons.
    if f.action_pending.is_some() || f.list_slot.is_some() {
        return None;
    }
    if let Some(confirm_id) = &f.confirm_remove {
        let (row, _) = f.pages[0]
            .shown(&f.friends, MAX_ROWS)
            .find(|(_, e)| &e.user_id == confirm_id)?;
        if ui.clicked(confirm_yes_btn(ww, row)) {
            return Some(FriendAction::ConfirmRemove(confirm_id.clone()));
        }
        if ui.clicked(confirm_no_btn(ww, row)) {
            return Some(FriendAction::CancelRemove);
        }
        return None;
    }
    for (i, e) in f.pages[0].shown(&f.friends, MAX_ROWS) {
        if ui.clicked(remove_btn(ww, 0, i)) {
            return Some(FriendAction::StartRemove(e.user_id.clone()));
        }
        if ui.clicked(watch_btn(ww, i)) {
            return Some(FriendAction::Watch(e.user_id.clone()));
        }
    }
    for (i, e) in f.pages[1].shown(&f.received, MAX_ROWS) {
        if ui.clicked(accept_btn(ww, i)) {
            return Some(FriendAction::Accept(e.user_id.clone()));
        }
        if ui.clicked(reject_btn(ww, i)) {
            return Some(FriendAction::Reject(e.user_id.clone()));
        }
    }
    for (i, e) in f.pages[2].shown(&f.sent, MAX_ROWS) {
        if ui.clicked(remove_btn(ww, 2, i)) {
            return Some(FriendAction::Cancel(e.user_id.clone()));
        }
    }
    None
}

/// Watches a friend's game: the server says where it is, or that there is none.
fn watch(state: &mut State, user_id: String) {
    state.notice.clear();
    if state.conn.is_live() {
        state.conn.send(&shared::ClientMessage::WatchFriend { user_id });
    } else {
        state.pending_watch = Some(user_id);
        state.conn.connect(crate::connection::now_secs());
        state.rooms.clear();
    }
    state.screen = Screen::RoomBrowser;
}

fn apply_action(state: &mut State, action: FriendAction) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let Some(f) = state.friends.as_mut() else {
        return;
    };
    match action {
        FriendAction::Watch(_) => {}
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
    let pal = state.ui.palette();
    let (ww, wh) = state.ui.view().size();

    let mut draw = state.ui.screen_canvas(gfx);
    state.ui.header_band(&mut draw, Rect::at(0.0, 0.0, ww, theme::HEADER_H));
    draw.sharp_text(&state.fonts.display, "Amis")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);

    if let Some(f) = &state.friends {
        let urls = lists(f).into_iter().flatten().map(|e| &e.avatar_url);
        let pics = pictures(gfx, state, urls.chain(f.search_results.iter().map(|e| &e.avatar_url)));
        draw_search(&mut draw, &state.ui, &state.fonts, f, (ww, &pics));
        draw_lists(&mut draw, &state.ui, &state.fonts, f, (ww, &pics));

        state.ui.button(&mut draw, &state.fonts, back_btn(wh), "Retour");
        let label = if f.list_slot.is_some() { "..." } else { "Rafraîchir" };
        state
            .ui
            .button_enabled(&mut draw, &state.fonts, refresh_btn(wh), label, can_refresh(f));
    }
    state.ui.render(gfx, &draw);
}

fn draw_search(draw: &mut Draw, ui: &Ui, fonts: &Fonts, f: &FriendsData, (ww, pics): (f32, &Pictures)) {
    let pal = ui.palette();
    let field = search_box(ww);
    draw.sharp_text(&fonts.text, "Rechercher un ami")
        .position(field.x, field.y - 14.0)
        .size(theme::size::SMALL)
        .v_align_middle()
        .color(pal.text_dim);

    let searching = f.search_slot.is_some();
    let search = Field {
        placeholder: "Pseudo ou UUID",
        input: &f.search_input,
        focused: true,
    };
    text_field(draw, ui, fonts, field, &search);
    ui.button_enabled(
        draw,
        fonts,
        search_submit_btn(ww),
        if searching { "..." } else { "Rechercher" },
        !searching,
    );

    let status = f.search_status.shown(&pal);
    if f.search_results.is_empty() && status.is_none() {
        return;
    }
    let card = results_card(ww);
    ui::card(draw, &pal, card);
    if let (true, Some((msg, color))) = (f.search_results.is_empty(), status) {
        draw.sharp_text(&fonts.text, msg)
            .position(card.x + card.w / 2.0, card.y + card.h / 2.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
    let adding = f.add_pending.is_some();
    for (i, e) in f.search_results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
        let row = result_row(ww, i);
        let mid = row.y + row.h / 2.0;
        list_row(draw, &pal, row, i);
        avatar(
            draw,
            ui,
            fonts,
            (row.x + 10.0, mid),
            &e.username,
            e.avatar_url.as_ref(),
            pics,
        );
        draw.sharp_text(&fonts.text, &e.username)
            .position(row.x + 10.0 + AVATAR_ROOM, mid)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(pal.text);
        let elo = format!("ELO {}", e.elo);
        let rating = Pill {
            text: &elo,
            color: theme::GOLD,
            size: theme::size::SMALL,
        };
        rating.draw(draw, fonts, (row.x + 260.0, mid));
        if !f.friends.iter().any(|fr| fr.user_id == e.user_id) {
            ui.button_enabled(draw, fonts, result_add_btn(ww, i), "Ajouter", !adding);
        }
        ui.button(draw, fonts, result_view_btn(ww, i), "Profil");
    }

    if let Some((msg, color)) = f.add_status.shown(&pal) {
        draw.sharp_text(&fonts.text, msg)
            .position(ww / 2.0, ADD_STATUS_Y)
            .size(theme::size::BODY)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

fn draw_lists(draw: &mut Draw, ui: &Ui, fonts: &Fonts, f: &FriendsData, (ww, pics): (f32, &Pictures)) {
    let pal = ui.palette();
    let columns = [
        ("Amis", &f.friends, "Aucun ami pour l'instant"),
        ("Demandes reçues", &f.received, "Aucune demande reçue"),
        ("Envoyées", &f.sent, "Aucune demande envoyée"),
    ];
    for (col, (title, list, _)) in columns.iter().enumerate() {
        let card = column_card(ww, col);
        ui::card(draw, &pal, card);
        let mid = card.y + CARD_TITLE_H / 2.0;
        draw.sharp_text(&fonts.display, title)
            .position(card.x + 16.0, mid)
            .size(theme::size::EMPHASIS)
            .v_align_middle()
            .color(pal.text);
        let count = list.len().to_string();
        let tally = Pill {
            text: &count,
            color: pal.accent,
            size: theme::size::SMALL,
        };
        tally.draw(draw, fonts, (card.x + card.w - 16.0 - tally.width(fonts), mid));
        divider(draw, &pal, card.x + 16.0, card.y + CARD_TITLE_H, card.w - 32.0);
    }

    if f.list_slot.is_some() {
        draw.sharp_text(&fonts.text, "Chargement...")
            .position(ww / 2.0, LIST_Y + 24.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
    } else {
        let busy = f.action_pending.is_some();
        let confirm = f.confirm_remove.as_deref();
        draw_col(
            draw,
            ui,
            fonts,
            (ww, 0),
            (columns[0].1, &f.pages[0], pics),
            columns[0].2,
            |draw, fonts, entry, i| {
                if confirm == Some(entry.user_id.as_str()) {
                    ui.button_enabled(draw, fonts, confirm_yes_btn(ww, i), "Oui", !busy);
                    ui.button(draw, fonts, confirm_no_btn(ww, i), "Non");
                } else {
                    let active = !busy && confirm.is_none();
                    ui.button_enabled(draw, fonts, watch_btn(ww, i), "Regarder", confirm.is_none());
                    ui.button_enabled(draw, fonts, remove_btn(ww, 0, i), "Retirer", active);
                }
            },
        );
        draw_col(
            draw,
            ui,
            fonts,
            (ww, 1),
            (columns[1].1, &f.pages[1], pics),
            columns[1].2,
            |draw, fonts, _, i| {
                ui.button_enabled(draw, fonts, accept_btn(ww, i), "Accepter", !busy);
                ui.button_enabled(draw, fonts, reject_btn(ww, i), "Refuser", !busy);
            },
        );
        draw_col(
            draw,
            ui,
            fonts,
            (ww, 2),
            (columns[2].1, &f.pages[2], pics),
            columns[2].2,
            |draw, fonts, _, i| {
                ui.button_enabled(draw, fonts, remove_btn(ww, 2, i), "Annuler", !busy);
            },
        );
    }

    for (col, list) in lists(f).into_iter().enumerate() {
        let pages = page_count(list.len(), MAX_ROWS);
        if pages > 1 && f.list_slot.is_none() {
            f.pages[col].draw(draw, ui, fonts, pager_area(ww, col), pages);
        }
    }

    if let Some((msg, color)) = f.action_status.shown(&pal) {
        draw.sharp_text(&fonts.text, msg)
            .position(ww / 2.0, ACTION_STATUS_Y)
            .size(theme::size::SMALL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

fn draw_col<F>(
    draw: &mut Draw,
    ui: &Ui,
    fonts: &Fonts,
    (ww, col): (f32, usize),
    (list, pager, pics): (&[FriendEntry], &Pager, &Pictures),
    empty_msg: &str,
    draw_buttons: F,
) where
    F: Fn(&mut Draw, &Fonts, &FriendEntry, usize),
{
    let pal = &ui.palette();
    if list.is_empty() {
        let card = column_card(ww, col);
        draw.sharp_text(&fonts.text, empty_msg)
            .position(card.x + card.w / 2.0, LIST_Y + 24.0)
            .size(theme::size::SMALL)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
        return;
    }
    let room = name_zone(ww, col, 0).w - 12.0 - AVATAR_ROOM;
    for (i, e) in pager.shown(list, MAX_ROWS) {
        let row = list_row_rect(ww, col, i);
        list_row(draw, pal, row, i);
        ui.row(draw, name_zone(ww, col, i), i, &format!("friend:{col}:{}", e.user_id));
        let mid = row.y + row.h / 2.0;
        avatar(
            draw,
            ui,
            fonts,
            (row.x + 8.0, mid),
            &e.username,
            e.avatar_url.as_ref(),
            pics,
        );
        let text_x = row.x + 8.0 + AVATAR_ROOM;
        draw.sharp_text(
            &fonts.text,
            &fonts.fit(Face::Text, &e.username, theme::size::BODY, room),
        )
        .position(text_x, mid - 8.0)
        .size(theme::size::BODY)
        .v_align_middle()
        .color(pal.text);
        draw.sharp_text(&fonts.text, &format!("ELO {}", e.elo))
            .position(text_x, mid + 10.0)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(theme::GOLD);
        draw_buttons(draw, fonts, e, i);
    }
}
