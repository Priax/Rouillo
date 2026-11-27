use aero::input::KeyCode;

use crate::controls::{Action, Bindings, KeyBinding, ACTIONS, RESERVED};
use crate::pads::Pads;
use crate::theme;
use crate::tick::Tick;
use crate::ui::{self, px, widgets, Face, Frame, Gui, Icon, Label, Rect, Typeset, View};

const TOP: f32 = 360.0;
const TITLE_H: f32 = 48.0;
const ROW_H: f32 = 36.0;
const CELL_W: f32 = 140.0;
const CELL_H: f32 = 31.0;
const RELABEL_FRAMES: u8 = 3;
const SLOTS: [Slot; 3] = [Slot::Key(0), Slot::Key(1), Slot::Pad];

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
    Rect::centered_on(view.w / 2.0, TOP, 760.0, TITLE_H + ACTIONS as f32 * ROW_H + 12.0)
}

fn cell(view: View, row: usize, slot: Slot) -> Rect {
    let col = match slot {
        Slot::Key(i) => i,
        Slot::Pad => 2,
    };
    let line = card(view).below(TITLE_H + 6.0).nth_row(row, ROW_H, 0.0);
    let cells = line.inset([0.0, 0.0, 0.0, 300.0]).left_strip(3.0 * CELL_W + 20.0);

    cells.columns(10.0, [px(CELL_W); 3])[col].middle(CELL_H)
}

pub fn reset_btn(view: View) -> Rect {
    let back = Frame::of(view).back;
    Rect::at(back.x() + back.w() + 20.0, back.y(), 260.0, back.h())
}

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

    /// What the keys and the gamepad do while a binding waits for one; its clicks are in [`BindingsPanel::show`].
    /// Returns whether the bindings changed and must be saved.
    pub fn update(&mut self, tick: &Tick, pads: &Pads, bindings: &mut Bindings) -> bool {
        let typed = self.typed.take();
        let mut changed = self.apply_label(typed, bindings);
        let erase = tick.pressed(KeyCode::Backspace) || tick.pressed(KeyCode::Delete);

        match self.capture {
            Some(_) if tick.back() => self.capture = None,
            Some((action, slot)) => {
                match slot {
                    Slot::Key(i) if erase => bindings.set_key(action, i, None),
                    Slot::Pad if erase => bindings.set_pad(action, None),
                    Slot::Key(i) => match tick.input.pressed_keys().find(|k| !RESERVED.contains(k)) {
                        Some(code) => {
                            bindings.set_key(action, i, Some(KeyBinding::new(code)));
                            self.relabel = Some((action, i, code, RELABEL_FRAMES));
                            self.apply_label(typed, bindings);
                        }
                        None => return changed,
                    },
                    Slot::Pad => match pads.any_pressed() {
                        Some(pad) => bindings.set_pad(action, Some(pad)),
                        None => return changed,
                    },
                }
                self.capture = None;
                changed = true;
            }
            None => {}
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

    /// Draws the panel and answers its clicks: a cell clicked waits for a key or a button. Returns whether the
    /// bindings changed and must be saved.
    pub fn show(&mut self, ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, bindings: &mut Bindings) -> bool {
        let pal = gui.stage.palette();
        let view = gui.stage.view();
        let c = card(view);
        let reset = reset_btn(view);
        let hint = match self.capture {
            Some((_, Slot::Key(_))) => "Appuyez sur une touche (Échap: annuler, Retour arrière: effacer)",
            Some((_, Slot::Pad)) => "Appuyez sur un bouton de la manette (Échap: annuler, Retour arrière: effacer)",
            None => "Start met en pause, Select relance une partie terminée.",
        };
        let hint_color = if self.capturing() { pal.accent } else { pal.text_muted };
        let fonts = &mut ui.cx.fonts;

        ui::card(ui.draw, &pal, c);
        fonts
            .write(ui.draw, Face::Display, "Touches")
            .left(c.x() + 20.0, c.y() + TITLE_H / 2.0)
            .size(theme::size::EMPHASIS)
            .color(pal.text_dim);
        for (slot, heading) in SLOTS.into_iter().zip(["Touche 1", "Touche 2", "Manette"]) {
            let r = cell(view, 0, slot);
            fonts
                .write(ui.draw, Face::Text, heading)
                .center(r.x() + r.w() / 2.0, c.y() + TITLE_H / 2.0)
                .size(theme::size::SMALL)
                .color(pal.text_muted);
        }
        ui::divider(ui.draw, &pal, c.x() + 20.0, c.y() + TITLE_H, c.w() - 40.0);

        let mut clicked = None;
        for (row, action) in Action::ALL.into_iter().enumerate() {
            let r = cell(view, row, Slot::Pad);
            ui.cx
                .fonts
                .write(ui.draw, Face::Text, action.label())
                .left(c.x() + 20.0, r.y() + r.h() / 2.0)
                .size(theme::size::BODY)
                .color(pal.text);
            for (col, slot) in SLOTS.into_iter().enumerate() {
                let rect = cell(view, row, slot);
                let name = match slot {
                    Slot::Key(i) => bindings.key(action, i).map(|k| k.label.clone()),
                    Slot::Pad => bindings.pad(action).map(|p| p.label().to_owned()),
                }
                .unwrap_or_else(|| "-".to_owned());
                let waiting = self.capture == Some((action, slot));
                let label = if waiting { Label::Icon(Icon::Busy) } else { Label::Text(&name) };
                if widgets::button(ui, ("binding", row, col), rect, label, true) {
                    clicked = Some((action, slot));
                }
            }
        }

        ui.cx
            .fonts
            .write(ui.draw, Face::Text, hint)
            .left(reset.x() + reset.w() + 20.0, reset.y() + reset.h() / 2.0)
            .size(theme::size::SMALL)
            .color(hint_color);
        let reset_all = widgets::button(ui, "reset bindings", reset, "Touches par défaut", true);

        if let Some(cell) = clicked.filter(|_| !self.capturing()) {
            self.capture = Some(cell);
            // The key that answers must reach the capture, not the button it was asked from.
            ui.blur();
        }
        if reset_all {
            *bindings = Bindings::default();
            return true;
        }
        false
    }
}
