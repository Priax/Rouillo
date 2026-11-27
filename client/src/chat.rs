use std::collections::VecDeque;

use aero::geom::Color;
use aero::ui::Sense;
use shared::MAX_CHAT_CHARS;

use crate::connection::now_secs;
use crate::theme;
use crate::theme::Palette;
use crate::ui::{widgets, Face, Gui, Rect, Typeset};

const KEPT: usize = 50;
const LINE_H: f32 = 20.0;
const INPUT_H: f32 = 40.0;
const PAD: f32 = 12.0;
const FADE_AFTER: f64 = 8.0;
const FADE_FOR: f64 = 2.0;

struct Line {
    text: String,
    spectator: bool,
    at: f64,
}

#[derive(Default)]
pub struct Chat {
    lines: VecDeque<Line>,
    pub input: String,
    /// In a game, whether the field is open, with the keyboard.
    pub open: bool,
    /// The field takes the keyboard, or lets go of it, at its next frame.
    focus: Option<bool>,
}

const FIELD: &str = "chat";

impl Chat {
    pub fn push(&mut self, from: &str, text: &str, spectator: bool) {
        if self.lines.len() == KEPT {
            self.lines.pop_front();
        }
        self.lines.push_back(Line { text: format!("{from}: {text}"), spectator, at: now_secs() });
    }

    /// The field takes the keyboard at its next frame.
    pub const fn take_keyboard(&mut self) {
        self.focus = Some(true);
    }

    /// The field lets go of the keyboard at its next frame.
    pub const fn release_keyboard(&mut self) {
        self.focus = Some(false);
    }

    /// The field; returns the message Enter sends.
    fn show_field(
        &mut self,
        ui: &mut aero::ui::Ui<'_>,
        gui: &mut Gui,
        area: Rect,
        placeholder: &str,
    ) -> Option<String> {
        match self.focus.take() {
            Some(true) => widgets::focus_field(ui, FIELD),
            Some(false) if ui.is_focused(ui.id(FIELD)) => ui.blur(),
            _ => {}
        }
        let field = widgets::field().placeholder(placeholder).max_chars(MAX_CHAT_CHARS);
        if !widgets::text_field(ui, gui, FIELD, input_rect(area), field, &mut self.input).submitted {
            return None;
        }
        let text = self.input.trim().to_owned();
        self.input.clear();
        (!text.is_empty()).then_some(text)
    }

    fn draw_lines(&self, ui: &mut aero::ui::Ui<'_>, pal: &Palette, area: Rect, fade: bool) {
        let now = now_secs();

        let mut y = area.y() + area.h() - INPUT_H - PAD - LINE_H / 2.0;
        for line in self.lines.iter().rev() {
            let alpha = if fade {
                let left = FADE_AFTER + FADE_FOR - (now - line.at);
                if left > FADE_FOR {
                    crate::ui::redraw_after(ui.cx, left - FADE_FOR);
                } else if left > 0.0 {
                    ui.cx.request_redraw();
                }
                (left / FADE_FOR).clamp(0.0, 1.0) as f32
            } else {
                1.0
            };
            if alpha <= 0.0 {
                break;
            }
            let color = if line.spectator { pal.text_dim } else { pal.text };
            let rows = ui.cx.fonts.wrap(Face::Text, &line.text, theme::size::SMALL, area.w() - 2.0 * PAD);

            for row in rows.iter().rev() {
                if y < area.y() + PAD {
                    return;
                }
                ui.cx
                    .fonts
                    .write(ui.draw, Face::Text, row)
                    .left(area.x() + PAD, y)
                    .size(theme::size::SMALL)
                    .color(color.with_alpha(alpha));
                y -= LINE_H;
            }
        }
    }

    /// The lobby's chat: a card with the conversation and its field. Returns the message to send.
    pub fn show_card(&mut self, ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, area: Rect) -> Option<String> {
        crate::ui::card(ui.draw, &gui.ui.palette(), area);
        self.draw_lines(ui, &gui.ui.palette(), area, false);
        self.show_field(ui, gui, area, "Écrire un message...")
    }

    /// The in-game chat: recent messages that fade, and the field once open; a click on it opens it. Returns the
    /// message to send.
    pub fn show_overlay(
        &mut self,
        ui: &mut aero::ui::Ui<'_>,
        gui: &mut Gui,
        area: Rect,
        hint: Option<&str>,
    ) -> Option<String> {
        let pal = gui.ui.palette();
        if !self.open {
            if ui.is_focused(ui.id(FIELD)) {
                ui.blur();
            }
            self.draw_lines(ui, &pal, area, true);
            if let Some(hint) = hint {
                ui.cx
                    .fonts
                    .write(ui.draw, Face::Text, hint)
                    .left(area.x() + PAD, area.y() + area.h() - INPUT_H / 2.0 - PAD / 2.0)
                    .size(theme::size::SMALL)
                    .color(pal.text_muted);
                if ui.interact(ui.id("chat area"), area, Sense::CLICK).clicked {
                    self.open = true;
                    self.take_keyboard();
                }
            }
            return None;
        }
        ui.draw.rect(area).radius(theme::RADIUS).color(Color::BLACK.with_alpha(0.55));
        self.draw_lines(ui, &gui.ui.palette(), area, false);
        let said = self.show_field(ui, gui, area, "Entrée pour envoyer, Échap pour fermer");
        if said.is_some() {
            self.open = false;
            ui.blur();
        }
        said
    }
}

fn input_rect(area: Rect) -> Rect {
    area.inset([0.0, PAD / 2.0, PAD / 2.0, PAD / 2.0]).bottom_strip(INPUT_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_keeps_the_latest_messages() {
        let mut chat = Chat::default();
        for i in 0..KEPT + 5 {
            chat.push("a", &i.to_string(), false);
        }
        assert_eq!(chat.lines.len(), KEPT);
        assert_eq!(chat.lines.front().map(|l| l.text.as_str()), Some("a: 5"));
    }
}
