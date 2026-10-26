use notan::draw::Draw;
use notan::prelude::*;
use shared::cpu::{Difficulty, Style};
use shared::ClientMessage;

use crate::rooms::{step_team, team_look};
use crate::theme;
use crate::ui::{self, Fonts, Modal, Rect, Stepper, Ui, View};

const CARD_W: f32 = 520.0;
const REMOVE_W: f32 = 240.0;
const REMOVE_H: f32 = 48.0;
const TILES: usize = 3;

/// A lobby seat a CPU plays, as its host may change it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuSeat {
    /// From 1, as the server counts seats.
    pub slot: u8,
    pub difficulty: Difficulty,
    pub style: Style,
    pub team: Option<u8>,
}

pub enum Action {
    Close,
    Send(ClientMessage),
}

fn modal(view: View) -> Modal {
    let h = 70.0 + TILES as f32 * ui::TILE_H + REMOVE_H + 40.0;
    Modal {
        card: Rect::at(0.0, 0.0, view.w, view.h).centered(CARD_W, h),
    }
}

fn tile(modal: Modal, i: usize) -> Rect {
    modal.body().nth_row(i, ui::TILE_H, 0.0)
}

fn remove_button(modal: Modal) -> Rect {
    modal.body().bottom_band(REMOVE_H).centered(REMOVE_W, REMOVE_H)
}

impl CpuSeat {
    fn label(i: usize) -> &'static str {
        ["Difficulté", "Personnage", "Équipe"][i]
    }

    fn value(self, i: usize, pal: &theme::Palette) -> &'static str {
        match i {
            0 => self.difficulty.label(),
            1 => self.style.character(),
            _ => team_look(self.team, pal).0,
        }
    }

    fn stepped(self, i: usize, dir: i32) -> Self {
        match i {
            0 => Self {
                difficulty: ui::cycle(&Difficulty::ALL, self.difficulty, dir),
                ..self
            },
            1 => Self {
                style: ui::cycle(&Style::ALL, self.style, dir),
                ..self
            },
            _ => Self {
                team: step_team(self.team, dir),
                ..self
            },
        }
    }

    const fn message(self) -> ClientMessage {
        ClientMessage::SetCpu {
            slot: self.slot,
            difficulty: self.difficulty,
            style: self.style,
            team: self.team,
        }
    }
}

pub fn update(app: &App, ui: &Ui, seat: CpuSeat) -> Option<Action> {
    let modal = modal(ui.view());
    if modal.dismissed(ui) || app.keyboard.was_pressed(KeyCode::Escape) {
        return Some(Action::Close);
    }
    for i in 0..TILES {
        let stepper = Stepper::in_tile(tile(modal, i));
        for (rect, dir) in [(stepper.minus, -1), (stepper.plus, 1)] {
            if ui.clicked(rect) {
                return Some(Action::Send(seat.stepped(i, dir).message()));
            }
        }
    }
    if ui.clicked(remove_button(modal)) {
        return Some(Action::Send(ClientMessage::RemoveCpu { slot: seat.slot }));
    }
    None
}

pub fn draw(draw: &mut Draw, ui: &Ui, fonts: &Fonts, seat: CpuSeat) {
    let pal = ui.palette();
    let modal = modal(ui.view());
    modal.draw(draw, ui, fonts, "CPU");
    for i in 0..TILES {
        ui.stepper_tile(
            draw,
            fonts,
            tile(modal, i),
            CpuSeat::label(i),
            seat.value(i, &pal),
            true,
        );
    }
    ui.button(draw, fonts, remove_button(modal), "Retirer");
}
