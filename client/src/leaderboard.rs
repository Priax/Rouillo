use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http::{self, HttpSlot};
use crate::state::{Screen, State};
use crate::theme;
use crate::ui::{self, Face, Pager, Pill, Rect, SharpText, Status, View, PAGER_H};

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

pub fn enter(state: &mut State) {
    state.leaderboard = Some(Leaderboard::default());
    refresh(state);
    state.screen = Screen::Leaderboard;
}

fn refresh(state: &mut State) {
    let me = state.auth.as_ref().map(|a| a.user_id.clone());
    let Some(board) = state.leaderboard.as_mut() else {
        return;
    };
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

fn list_card(view: View) -> Rect {
    let w = (view.w - 40.0).min(840.0);
    Rect::at(
        (view.w - w) / 2.0,
        theme::HEADER_H + 30.0,
        w,
        view.h - theme::HEADER_H - 30.0 - 110.0,
    )
}

fn per_page(view: View) -> usize {
    ((list_card(view).h - 24.0 - PAGER_H) / ROW_H).floor().max(1.0) as usize
}

fn row_rect(view: View, i: usize) -> Rect {
    let card = list_card(view);
    Rect::at(
        card.x + 12.0,
        card.y + 12.0 + i as f32 * ROW_H,
        card.w - 24.0,
        ROW_H - 6.0,
    )
}

fn pager_area(view: View) -> Rect {
    let card = list_card(view);
    Rect::at(card.x, card.y + card.h - PAGER_H, card.w, PAGER_H)
}

fn back_btn(view: View) -> Rect {
    Rect::at(40.0, view.h - 80.0, 200.0, 54.0)
}

fn refresh_btn(view: View) -> Rect {
    Rect::at(view.w - 60.0 - 190.0, (theme::HEADER_H - 44.0) / 2.0, 190.0, 44.0)
}

pub fn update(app: &App, state: &mut State) {
    let me = state.auth.as_ref().map(|a| a.user_id.clone());
    let view = state.ui.view();
    let Some(board) = state.leaderboard.as_mut() else {
        state.screen = Screen::Menu;
        return;
    };
    poll(board, me.as_deref());
    let pages = ui::page_count(board.top.len(), per_page(view));
    board.pager.clamp(pages);
    board
        .pager
        .update(app, &state.ui, &state.fonts, &state.keys, pager_area(view), pages);

    let clicked = board
        .pager
        .shown(&board.top, per_page(view))
        .find(|&(i, _)| state.ui.clicked(row_rect(view, i)))
        .map(|(_, e)| (e.user_id.clone(), e.username.clone()));
    if let Some((user_id, username)) = clicked {
        if Some(&user_id) == me.as_ref() {
            crate::profile::enter_profile(state, Screen::Leaderboard);
            state.screen = Screen::Profile;
        } else {
            crate::profile::enter_other_profile(state, user_id, username, Screen::Leaderboard);
            state.screen = Screen::OtherProfile;
        }
        return;
    }

    let loading = board.slot.is_some();
    if !loading && state.ui.clicked(refresh_btn(view)) {
        refresh(state);
    } else if state.ui.clicked(back_btn(view)) || app.keyboard.was_pressed(KeyCode::Escape) {
        state.leaderboard = None;
        state.screen = Screen::Menu;
    }
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
    state.ui.row(draw, row, i, &format!("rank:{}", e.user_id));
    let mid = row.y + row.h / 2.0;
    let who = ui::Persona {
        name: &e.username,
        glow: 0.0,
        picture,
    };
    ui::portrait(draw, &pal, fonts, (row.x + 100.0 + AVATAR_R, mid), AVATAR_R, &who);
    draw.sharp_text(&fonts.display, &format!("#{}", e.rank))
        .position(row.x + 18.0, mid)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(rank_color(e.rank, pal.text_muted));

    let elo = format!("ELO {}", e.elo);
    let record = format!("{} V / {} séries", e.series_won, e.series);
    let mut pills = Vec::new();
    if mine {
        pills.push(Pill {
            text: "Vous",
            color: pal.accent,
            size: theme::size::SMALL,
        });
    }
    pills.push(Pill {
        text: &record,
        color: pal.text_dim,
        size: theme::size::SMALL,
    });
    pills.push(Pill {
        text: &elo,
        color: theme::GOLD,
        size: theme::size::SMALL,
    });
    let pills_start = ui::pills_ending_at(draw, fonts, &pills, row.x + row.w - 16.0, mid);
    let name_x = row.x + 100.0 + 2.0 * AVATAR_R + 12.0;
    let name = fonts.fit(
        Face::Display,
        &e.username,
        theme::size::EMPHASIS,
        pills_start - 20.0 - name_x,
    );
    draw.sharp_text(&fonts.display, &name)
        .position(name_x, mid)
        .size(theme::size::EMPHASIS)
        .v_align_middle()
        .color(pal.text);
}

pub fn draw(gfx: &mut Graphics, state: &State) {
    let pal = state.ui.palette();
    let view = state.ui.view();
    let mut draw = state.ui.screen_canvas(gfx);
    state
        .ui
        .header_band(&mut draw, Rect::at(0.0, 0.0, view.w, theme::HEADER_H));
    draw.sharp_text(&state.fonts.display, "Classement")
        .position(60.0, theme::HEADER_H / 2.0)
        .size(theme::size::TITLE)
        .v_align_middle()
        .color(pal.text);

    let card = list_card(view);
    ui::card(&mut draw, &pal, card);
    let Some(board) = &state.leaderboard else {
        state.ui.render(gfx, &draw);
        return;
    };
    let me = state.auth.as_ref().map(|a| a.user_id.as_str());
    let empty = if board.slot.is_some() {
        Some("Chargement...")
    } else if board.top.is_empty() && board.status.shown(&pal).is_none() {
        Some("Personne n'a encore joué en classé.")
    } else {
        None
    };
    if let Some(text) = empty {
        draw.sharp_text(&state.fonts.text, text)
            .position(card.x + card.w / 2.0, card.y + 60.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(pal.text_muted);
    } else {
        for (i, e) in board.pager.shown(&board.top, per_page(view)) {
            let picture = state.images.get_opt(gfx, e.avatar_url.as_deref());
            let mine = Some(e.user_id.as_str()) == me;
            draw_row(&mut draw, state, (row_rect(view, i), i), e, mine, picture.as_ref());
        }
        let pages = ui::page_count(board.top.len(), per_page(view));
        if pages > 1 {
            board
                .pager
                .draw(&mut draw, &state.ui, &state.fonts, pager_area(view), pages);
        }
    }

    let loading = board.slot.is_some();
    state.ui.button_enabled(
        &mut draw,
        &state.fonts,
        refresh_btn(view),
        if loading { "..." } else { "Rafraîchir" },
        !loading,
    );
    let back = back_btn(view);
    state.ui.button(&mut draw, &state.fonts, back, "Retour");

    let footer = match (&board.me, me, loading) {
        (_, _, true) => None,
        (Some(e), _, _) if e.rank > SIZE => Some((format!("Votre rang: #{} (ELO {})", e.rank, e.elo), pal.text)),
        (None, Some(_), _) => Some((
            "Jouez une série classée pour apparaître au classement.".to_owned(),
            pal.text_dim,
        )),
        _ => None,
    };
    let footer = board
        .status
        .shown(&pal)
        .map(|(msg, color)| (msg.to_owned(), color))
        .or(footer);
    if let Some((msg, color)) = footer {
        draw.sharp_text(&state.fonts.text, &msg)
            .position(view.w / 2.0, back.y + back.h / 2.0)
            .size(theme::size::EMPHASIS)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }

    state.ui.render(gfx, &draw);
}
