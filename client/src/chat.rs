use std::collections::VecDeque;

use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;
use shared::MAX_CHAT_CHARS;

use crate::connection::now_secs;
use crate::theme;
use crate::ui::{field_clicked, text_field, EditKeys, Face, Field, Fonts, Rect, SharpText, TextInput, Ui};

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
    pub input: TextInput,
    /// In a game, whether the player is typing (the box takes the keys).
    pub open: bool,
    /// Whether the field was touched: only then does a phone show its keyboard.
    pub focused: bool,
}

impl Chat {
    pub fn push(&mut self, from: &str, text: &str, spectator: bool) {
        if self.lines.len() == KEPT {
            self.lines.pop_front();
        }
        self.lines.push_back(Line {
            text: format!("{from}: {text}"),
            spectator,
            at: now_secs(),
        });
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.input.clear();
        self.open = false;
        self.focused = false;
    }

    pub fn type_char(&mut self, c: char) {
        if self.input.chars().count() < MAX_CHAT_CHARS {
            self.input.insert(c);
        }
    }

    /// Applies editing keys; returns the message to send when Enter was pressed.
    pub fn edit(&mut self, keyboard: &Keyboard, keys: &EditKeys) -> Option<String> {
        self.input.edit(keys);
        if !keyboard.was_pressed(KeyCode::Enter) && !keyboard.was_pressed(KeyCode::NumpadEnter) {
            return None;
        }
        let text = self.input.trim().to_owned();
        self.input.clear();
        (!text.is_empty()).then_some(text)
    }

    pub fn click(&mut self, ui: &Ui, fonts: &Fonts, area: Rect) -> bool {
        let clicked = field_clicked(ui, fonts, input_rect(area), &mut self.input);
        if clicked {
            self.focused = true;
        }
        clicked || ui.clicked(area)
    }

    fn draw_lines(&self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, area: Rect, fade: bool) {
        let pal = ui.palette();
        let now = now_secs();
        let mut y = area.y + area.h - INPUT_H - PAD - LINE_H / 2.0;
        for line in self.lines.iter().rev() {
            let alpha = if fade {
                let left = FADE_AFTER + FADE_FOR - (now - line.at);
                (left / FADE_FOR).clamp(0.0, 1.0) as f32
            } else {
                1.0
            };
            if alpha <= 0.0 {
                break;
            }
            let color = if line.spectator { pal.text_dim } else { pal.text };
            let rows = fonts.wrap(Face::Text, &line.text, theme::size::SMALL, area.w - 2.0 * PAD);
            for row in rows.iter().rev() {
                if y < area.y + PAD {
                    return;
                }
                draw.sharp_text(&fonts.text, row)
                    .position(area.x + PAD, y)
                    .size(theme::size::SMALL)
                    .v_align_middle()
                    .color(color.with_alpha(alpha));
                y -= LINE_H;
            }
        }
    }

    /// The lobby's chat: a card with the conversation and its field.
    pub fn draw_card(&self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, area: Rect) {
        let pal = ui.palette();
        crate::ui::card(draw, &pal, area);
        self.draw_lines(draw, ui, fonts, area, false);
        let field = Field {
            placeholder: "Écrire un message...",
            input: &self.input,
            focused: true,
        };
        text_field(draw, ui, fonts, input_rect(area), &field);
    }

    /// The in-game chat: recent messages that fade, and the field once open.
    pub fn draw_overlay(&self, draw: &mut Draw, ui: &Ui, fonts: &Fonts, area: Rect, hint: Option<&str>) {
        let pal = ui.palette();
        if self.open {
            draw.rect((area.x, area.y), (area.w, area.h))
                .corner_radius(theme::RADIUS)
                .color(Color::BLACK.with_alpha(0.55));
        }
        self.draw_lines(draw, ui, fonts, area, !self.open);
        if self.open {
            let field = Field {
                placeholder: "Entrée pour envoyer, Échap pour fermer",
                input: &self.input,
                focused: true,
            };
            text_field(draw, ui, fonts, input_rect(area), &field);
        } else if let Some(hint) = hint {
            draw.sharp_text(&fonts.text, hint)
                .position(area.x + PAD, area.y + area.h - INPUT_H / 2.0 - PAD / 2.0)
                .size(theme::size::SMALL)
                .v_align_middle()
                .color(pal.text_muted);
        }
    }
}

fn input_rect(area: Rect) -> Rect {
    Rect::at(
        area.x + PAD / 2.0,
        area.y + area.h - INPUT_H - PAD / 2.0,
        area.w - PAD,
        INPUT_H,
    )
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

    #[test]
    fn typing_stops_at_the_limit() {
        let mut chat = Chat::default();
        for _ in 0..MAX_CHAT_CHARS + 10 {
            chat.type_char('x');
        }
        assert_eq!(chat.input.chars().count(), MAX_CHAT_CHARS);
    }
}
