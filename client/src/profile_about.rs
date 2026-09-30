use std::borrow::Cow;

use notan::draw::{Draw, DrawShapes};
use notan::prelude::*;

use crate::state::ApiUserProfile;
use crate::theme;
use crate::ui::{self, Face, Fonts, Rect, SharpText, Ui};

const LINE_H: f32 = 24.0;
const LABEL_H: f32 = 22.0;
const BLOCK_GAP: f32 = 8.0;
const OVERLAY_MUSIC_LINES: usize = 6;

pub struct Summary<'a> {
    bio: Vec<Cow<'a, str>>,
    music: Vec<Cow<'a, str>>,
    pub cut: bool,
}

fn block_height(lines: usize) -> f32 {
    if lines == 0 {
        0.0
    } else {
        LABEL_H + lines as f32 * LINE_H + BLOCK_GAP
    }
}

fn bio_room(height: f32, music: usize) -> usize {
    (((height - block_height(music) - LABEL_H) / LINE_H) as usize).max(1)
}

fn clamp<'a>(fonts: &Fonts, text: &'a str, room: f32, max: usize) -> (Vec<Cow<'a, str>>, bool) {
    if text.is_empty() {
        return (Vec::new(), false);
    }
    let mut lines: Vec<Cow<str>> = fonts
        .wrap(Face::Text, text, theme::size::BODY, room)
        .into_iter()
        .map(Cow::Borrowed)
        .collect();
    let cut = lines.len() > max;
    if cut {
        lines.truncate(max);
        if let Some(last) = lines.last_mut() {
            let ended = format!("{}…", last.trim_end());
            *last = Cow::Owned(fonts.fit(Face::Text, &ended, theme::size::BODY, room).into_owned());
        }
    }
    (lines, cut)
}

pub fn summary<'a>(fonts: &Fonts, info: &'a ApiUserProfile, area: Rect, music_lines: usize) -> Summary<'a> {
    let (music, music_cut) = clamp(
        fonts,
        info.favorite_music.as_deref().unwrap_or_default(),
        area.w,
        music_lines,
    );
    let (bio, bio_cut) = clamp(
        fonts,
        info.bio.as_deref().unwrap_or_default(),
        area.w,
        bio_room(area.h, music.len()),
    );
    Summary {
        bio,
        music,
        cut: bio_cut || music_cut,
    }
}

pub fn draw_summary(draw: &mut Draw, ui: &Ui, fonts: &Fonts, summary: &Summary, area: Rect) {
    let pal = ui.palette();
    let mut y = area.y;
    let blocks = [("Bio", &summary.bio, pal.text), ("Musique", &summary.music, pal.accent)];
    for (label, lines, color) in blocks {
        if lines.is_empty() {
            continue;
        }
        draw.sharp_text(&fonts.text, label)
            .position(area.x, y)
            .size(theme::size::SMALL)
            .v_align_middle()
            .color(pal.text_dim);
        for (i, line) in lines.iter().enumerate() {
            draw.sharp_text(&fonts.text, line)
                .position(area.x, y + LABEL_H + i as f32 * LINE_H)
                .size(theme::size::BODY)
                .v_align_middle()
                .color(color);
        }
        y += block_height(lines.len());
    }
}

pub fn more_link(area: Rect) -> Rect {
    Rect::at(area.x + area.w - 96.0, area.y - 13.0, 96.0, 26.0)
}

fn overlay_card(ui: &Ui) -> Rect {
    let view = ui.view();
    Rect::at(view.w / 2.0 - 320.0, 110.0, 640.0, view.h - 220.0)
}

fn overlay_close(card: Rect) -> Rect {
    Rect::at(card.x + card.w - 64.0, card.y + 14.0, 48.0, 40.0)
}

pub fn overlay_closed(app: &App, ui: &Ui) -> bool {
    let view = ui.view();
    let card = overlay_card(ui);
    let beside = ui.clicked(Rect::at(0.0, 0.0, view.w, view.h)) && !ui.clicked(card);
    beside || ui.clicked(overlay_close(card)) || app.keyboard.was_pressed(KeyCode::Escape)
}

pub fn draw_overlay(draw: &mut Draw, ui: &Ui, fonts: &Fonts, info: &ApiUserProfile) {
    let pal = ui.palette();
    let view = ui.view();
    draw.rect((0.0, 0.0), (view.w, view.h)).color(pal.scrim_strong);
    let card = overlay_card(ui);
    ui::card(draw, &pal, card);
    let title = fonts.fit(
        Face::Display,
        &info.username,
        theme::size::HEADING,
        card.w - 24.0 - 80.0,
    );
    draw.sharp_text(&fonts.display, &title)
        .position(card.x + 24.0, card.y + 34.0)
        .size(theme::size::HEADING)
        .v_align_middle()
        .color(pal.text);
    ui.button(draw, fonts, overlay_close(card), "X");

    let area = Rect::at(card.x + 24.0, card.y + 92.0, card.w - 48.0, card.h - 92.0 - 24.0);
    draw_summary(draw, ui, fonts, &summary(fonts, info, area, OVERLAY_MUSIC_LINES), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_blocks_stack_as_they_always_did() {
        assert!((block_height(1) - 54.0).abs() < f32::EPSILON);
        assert!(block_height(0).abs() < f32::EPSILON, "nothing to show takes no room");
    }

    #[test]
    fn the_bio_gets_the_lines_the_music_leaves() {
        let height = 200.0;
        assert_eq!(bio_room(height, 0), 7);
        assert_eq!(bio_room(height, 1), 5);
        assert!(block_height(bio_room(height, 1)) + block_height(1) <= height + BLOCK_GAP);
        assert_eq!(bio_room(10.0, 3), 1, "never less than a line");
    }
}
