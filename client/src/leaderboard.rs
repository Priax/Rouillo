use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http::{self, HttpSlot};
use crate::nav::Transition;
use crate::state::{Screen, State};
use crate::theme;
use crate::ui::{self, Face, Frame, Label, ListCard, Pager, Pill, Rect, SharpText, Status, View};

const SIZE: i64 = 100;
const ROW_H: f32 = 56.0;
const AVATAR_R: f32 = 17.0;

const SILVER: Color = Color::from_rgb(0.78, 0.8, 0.84);
const BRONZE: Color = Color::from_rgb(0.85, 0.55, 0.3);

#[derive(serde::Deserialize, Clone)]
pub struct Entry {
    rank: i64,
    user_id: String,
    username: String,
    elo: i32,
    series: i64,
    series_won: i64,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Default)]
pub struct Leaderboard {
    top: Vec<Entry>,
    me: Option<Entry>,
    slot: Option<HttpSlot>,
    status: Status,
    pager: Pager,
}

pub fn enter(state: &State) -> Transition {
    let mut board = Box::<Leaderboard>::default();
    refresh(&mut board, state);

    Transition::To(Screen::Leaderboard(board))
}

fn refresh(board: &mut Leaderboard, state: &State) {
    let me = state.auth.as_ref().map(|a| a.user_id.clone());
    let url = match me {
        Some(id) => http::api_url(&format!("leaderboard?me={id}")),
        None => http::api_url("leaderboard"),
    };

    let slot = http::new_slot();
    http::get(url, None, Arc::clone(&slot));
    board.slot = Some(slot);
    board.status.clear();
}

fn poll(board: &mut Leaderboard, me: Option<&str>) {
    let Some(result) = http::take_json::<Vec<Entry>>(&mut board.slot) else {
        return;
    };
    match result {
        Ok(rows) => {
            board.me = rows.iter().find(|e| Some(e.user_id.as_str()) == me).cloned();
            board.top = rows.into_iter().filter(|e| e.rank <= SIZE).collect();
        }
        Err(msg) => board.status = Status::error(msg),
    }
}

fn list(view: View) -> ListCard {
    ListCard::new(view, ROW_H)
}

fn back_btn(view: View) -> Rect {
    Frame::of(view).back
}

fn refresh_btn(view: View) -> Rect {
    Frame::of(view).header_tools(0.0).right(190.0).middle(44.0)
}

pub fn update(app: &App, state: &mut State, board: &mut Leaderboard) -> Transition {
    let me = state.auth.as_ref().map(|a| a.user_id.clone());
    let list = list(state.ui.view());
    let view = state.ui.view();

    poll(board, me.as_deref());
    let pages = ui::page_count(board.top.len(), list.per_page());
    board.pager.clamp(pages);
    board.pager.update(app, &state.ui, &state.fonts, &state.input.keys, list.pager(), pages);

    let clicked = board
        .pager
        .shown(&board.top, list.per_page())
        .find(|&(i, _)| state.ui.clicked(list.row(i)))
        .map(|(_, e)| (e.user_id.clone(), e.username.clone()));
    if let Some((user_id, username)) = clicked {
        if Some(&user_id) == me.as_ref() {
            return crate::profile::enter_profile(state);
        }
        return crate::profile::enter_other_profile(state, user_id, username, None);
    }

    let loading = board.slot.is_some();
    if !loading && state.ui.clicked(refresh_btn(view)) {
        refresh(board, state);
    } else if state.ui.clicked(back_btn(view)) || app.keyboard.was_pressed(KeyCode::Escape) {
        return Transition::To(Screen::Menu);
    }

    Transition::Stay
}

fn rank_color(rank: i64, fallback: Color) -> Color {
    match rank {
        1 => theme::GOLD,
        2 => SILVER,
        3 => BRONZE,
        _ => fallback,
    }
}

fn draw_row(draw: &mut Draw, state: &State, (row, i): (Rect, usize), e: &Entry, mine: bool, picture: Option<&Texture>) {
    let pal = state.ui.palette();
    let fonts = &state.fonts;
    let mid = row.y + row.h / 2.0;
    let who = ui::Persona { name: &e.username, glow: 0.0, picture };
    let name_x = row.x + 100.0 + 2.0 * AVATAR_R + 12.0;
    let elo = format!("ELO {}", e.elo);
    let record = format!("{} V / {} séries", e.series_won, e.series);
    let mut pills = Vec::new();
    if mine {
        pills.push(Pill::small("Vous", pal.accent));
    }
    pills.push(Pill::small(&record, pal.text_dim));
    pills.push(Pill::small(&elo, theme::GOLD));

    state.ui.row(draw, row, i, &format!("rank:{}", e.user_id));
    ui::portrait(draw, &pal, fonts, (row.x + 100.0 + AVATAR_R, mid), AVATAR_R, &who);
    draw.sharp_text(&fonts.display, &format!("#{}", e.rank))
        .left(row.x + 18.0, mid)
        .size(theme::size::EMPHASIS)
        .color(rank_color(e.rank, pal.text_muted));
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, row.x + row.w - 16.0, mid);
    let name = fonts.fit(Face::Display, &e.username, theme::size::EMPHASIS, pills_start - 20.0 - name_x);
    draw.sharp_text(&fonts.display, &name).left(name_x, mid).size(theme::size::EMPHASIS).color(pal.text);
}

pub fn draw(gfx: &mut Graphics, state: &State, board: &Leaderboard) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let list = list(view);
    let card = list.card;
    let me = state.auth.as_ref().map(|a| a.user_id.as_str());
    let loading = board.slot.is_some();
    let back = back_btn(view);
    let refresh = Label::busy_or(loading, "Rafraîchir");
    let footer = match (&board.me, me, loading) {
        (_, _, true) => None,
        (Some(e), _, _) if e.rank > SIZE => Some((format!("Votre rang: #{} (ELO {})", e.rank, e.elo), pal.text)),
        (None, Some(_), _) => Some(("Jouez une série classée pour apparaître au classement.".to_owned(), pal.text_dim)),
        _ => None,
    };
    let footer = board.status.shown(&pal).map(|(msg, color)| (msg.to_owned(), color)).or(footer);
    let empty = if board.slot.is_some() {
        Some("Chargement...")
    } else if board.top.is_empty() && board.status.shown(&pal).is_none() {
        Some("Personne n'a encore joué en classé.")
    } else {
        None
    };

    let mut draw = state.ui.screen_canvas(gfx);
    state.ui.page_header(&mut draw, &state.fonts, "Classement");
    ui::card(&mut draw, &pal, card);
    if let Some(text) = empty {
        draw.sharp_text(&state.fonts.text, text)
            .center(card.x + card.w / 2.0, card.y + 60.0)
            .size(theme::size::EMPHASIS)
            .color(pal.text_muted);
    } else {
        for (i, e) in board.pager.shown(&board.top, list.per_page()) {
            let picture = state.images.get_opt(gfx, e.avatar_url.as_deref());
            let mine = Some(e.user_id.as_str()) == me;
            draw_row(&mut draw, state, (list.row(i), i), e, mine, picture.as_ref());
        }
        let pages = ui::page_count(board.top.len(), list.per_page());
        if pages > 1 {
            board.pager.draw(&mut draw, &state.ui, &state.fonts, list.pager(), pages);
        }
    }

    state.ui.button_enabled(&mut draw, &state.fonts, refresh_btn(view), refresh, !loading);
    state.ui.button(&mut draw, &state.fonts, back, "Retour");
    if let Some((msg, color)) = footer {
        draw.sharp_text(&state.fonts.text, &msg)
            .center(view.w / 2.0, back.y + back.h / 2.0)
            .size(theme::size::EMPHASIS)
            .color(color);
    }

    state.ui.render(gfx, &draw);
}
