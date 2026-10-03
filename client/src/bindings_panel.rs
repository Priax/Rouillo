use notan::draw::Draw;
use notan::prelude::*;

use crate::controls::{Action, Bindings, KeyBinding, ACTIONS, RESERVED};
use crate::pads::Pads;
use crate::theme;
use crate::ui::{self, Fonts, Rect, SharpText, Ui, View};

const TOP: f32 = 360.0;
const TITLE_H: f32 = 56.0;
const ROW_H: f32 = 44.0;
const CELL_W: f32 = 140.0;
const CELL_H: f32 = 36.0;
const RELABEL_FRAMES: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Key(usize),
    Pad,
}

#[derive(Default)]
pub struct BindingsPanel {
    capture: Option<(Action, Slot)>,
    relabel: Option<(Action, usize, KeyCode, u8)>,
    typed: Option<char>,
}

fn card(view: View) -> Rect {
    Rect::at(
        view.w / 2.0 - 380.0,
        TOP,
        760.0,
        TITLE_H + ACTIONS as f32 * ROW_H + 12.0,
    )
}

fn cell(view: View, row: usize, slot: Slot) -> Rect {
    let c = card(view);
    let col = match slot {
        Slot::Key(i) => i as f32,
        Slot::Pad => 2.0,
    };
    let y = c.y + TITLE_H + 6.0 + row as f32 * ROW_H + (ROW_H - CELL_H) / 2.0;
    Rect::at(c.x + 300.0 + col * (CELL_W + 10.0), y, CELL_W, CELL_H)
}

pub fn reset_btn(view: View) -> Rect {
    Rect::at(260.0, view.h - 80.0, 260.0, 54.0)
}

const SLOTS: [Slot; 3] = [Slot::Key(0), Slot::Key(1), Slot::Pad];

impl BindingsPanel {
    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Called with each character typed on the settings screen.
    pub fn type_char(&mut self, c: char) {
        if !c.is_whitespace() && !c.is_control() {
            self.typed = Some(c);
        }
    }

    /// Returns whether the bindings changed and must be saved.
    pub fn update(&mut self, app: &App, ui: &Ui, pads: &Pads, bindings: &mut Bindings) -> bool {
        let typed = self.typed.take();
        let mut changed = self.apply_label(typed, bindings);
        let view = ui.view();
        let keyboard = &app.keyboard;
        let erase = keyboard.was_pressed(KeyCode::Backspace) || keyboard.was_pressed(KeyCode::Delete);
        match self.capture {
            Some(_) if keyboard.was_pressed(KeyCode::Escape) => self.capture = None,
            Some((action, Slot::Key(slot))) => {
                let key = keyboard.pressed.iter().copied().find(|k| !RESERVED.contains(k));
                if erase {
                    bindings.set_key(action, slot, None);
                    self.capture = None;
                    changed = true;
                } else if let Some(code) = key {
                    bindings.set_key(action, slot, Some(KeyBinding::new(code)));
                    self.relabel = Some((action, slot, code, RELABEL_FRAMES));
                    self.apply_label(typed, bindings);
                    self.capture = None;
                    changed = true;
                }
            }
            Some((action, Slot::Pad)) => {
                if erase {
                    bindings.set_pad(action, None);
                    self.capture = None;
                    changed = true;
                } else if let Some(pad) = pads.any_pressed() {
                    bindings.set_pad(action, Some(pad));
                    self.capture = None;
                    changed = true;
                }
            }
            None => {
                for (row, action) in Action::ALL.into_iter().enumerate() {
                    for slot in SLOTS {
                        if ui.clicked(cell(view, row, slot)) {
                            self.capture = Some((action, slot));
                        }
                    }
                }
                if ui.clicked(reset_btn(view)) {
                    *bindings = Bindings::default();
                    changed = true;
                }
            }
        }
        changed
    }

    fn apply_label(&mut self, typed: Option<char>, bindings: &mut Bindings) -> bool {
        let Some((action, slot, code, left)) = self.relabel else {
            return false;
        };
        if let Some(c) = typed {
            bindings.relabel(action, slot, code, &c.to_uppercase().to_string());
            self.relabel = None;
            return true;
        }
        self.relabel = left.checked_sub(1).map(|left| (action, slot, code, left));
        false
    }

    pub fn draw(&self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, bindings: &Bindings) {
        let pal = ui.palette();
        let view = ui.view();
        let c = card(view);
        ui::card(draw, &pal, c);
        draw.sharp_text(&fonts.display, "Touches")
            .position(c.x + 20.0, c.y + TITLE_H / 2.0)
            .size(theme::size::EMPHASIS)
            .v_align_middle()
            .color(pal.text_dim);
        for (slot, heading) in SLOTS.into_iter().zip(["Touche 1", "Touche 2", "Manette"]) {
            let r = cell(view, 0, slot);
            draw.sharp_text(&fonts.text, heading)
                .position(r.x + r.w / 2.0, c.y + TITLE_H / 2.0)
                .size(theme::size::SMALL)
                .h_align_center()
                .v_align_middle()
                .color(pal.text_muted);
        }
        ui::divider(draw, &pal, c.x + 20.0, c.y + TITLE_H, c.w - 40.0);

        for (row, action) in Action::ALL.into_iter().enumerate() {
            let r = cell(view, row, Slot::Pad);
            draw.sharp_text(&fonts.text, action.label())
                .position(c.x + 20.0, r.y + r.h / 2.0)
                .size(theme::size::BODY)
                .v_align_middle()
                .color(pal.text);
            for slot in SLOTS {
                let label = if self.capture == Some((action, slot)) {
                    "...".to_owned()
                } else {
                    match slot {
                        Slot::Key(i) => bindings.key(action, i).map(|k| k.label.clone()),
                        Slot::Pad => bindings.pad(action).map(|p| p.label().to_owned()),
                    }
                    .unwrap_or_else(|| "-".to_owned())
                };
                ui.button(draw, fonts, cell(view, row, slot), label.as_str());
            }
        }

        let hint = match self.capture {
            Some((_, Slot::Key(_))) => "Appuyez sur une touche (Échap: annuler, Retour arrière: effacer)",
            Some((_, Slot::Pad)) => "Appuyez sur un bouton de la manette (Échap: annuler, Retour arrière: effacer)",
            None => "Start met en pause, Select relance une partie terminée.",
        };
        draw.sharp_text(&fonts.text, hint)
            .position(view.w / 2.0, c.y + c.h + 14.0)
            .size(theme::size::SMALL)
            .h_align_center()
            .v_align_middle()
            .color(if self.capturing() { pal.accent } else { pal.text_muted });
        ui.button(draw, fonts, reset_btn(view), "Touches par défaut");
    }
}
