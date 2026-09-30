use std::borrow::Cow;

use notan::draw::Draw;
use notan::prelude::*;

use crate::state::ApiUserProfile;
use crate::theme;
use crate::ui::{Face, Fonts, Modal, Rect, SharpText, Ui};

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

pub const MORE: &str = "Voir plus";
const OVERLAY_W: f32 = 640.0;

pub fn draw_more_link(draw: &mut Draw, ui: &Ui, fonts: &Fonts, area: Rect) {
    let w = fonts.width(Face::Text, MORE, theme::size::BODY) + 8.0;
    let link = Rect::at(area.x + area.w - w, area.y - 13.0, w, 26.0);
    ui.link(draw, fonts, link, MORE, true);
}

pub fn overlay_closed(app: &App, ui: &Ui) -> bool {
    Modal::new(ui.view(), OVERLAY_W).dismissed(ui) || app.keyboard.was_pressed(KeyCode::Escape)
}

pub fn draw_overlay(draw: &mut Draw, ui: &Ui, fonts: &Fonts, info: &ApiUserProfile) {
    let modal = Modal::new(ui.view(), OVERLAY_W);
    let title = fonts.fit(
        Face::Display,
        &info.username,
        theme::size::HEADING,
        modal.card.w - 120.0,
    );
    modal.draw(draw, ui, fonts, &title);
    let body = modal.body();
    let area = Rect::at(body.x, body.y + LABEL_H, body.w, body.h - LABEL_H);
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
