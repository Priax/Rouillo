use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http::{self, HttpSlot};
use crate::picker::{self, Picked};
use crate::state::{ApiUserProfile, State};
use crate::theme;
use crate::ui::{
    area_clicked, area_height, area_keys, field_clicked, text_area, text_field, Field, Fonts, Rect, SharpText, Status,
    TextInput, Ui,
};

const BIO_LINES: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditField {
    #[default]
    Bio,
    Music,
}

impl EditField {
    const ALL: [Self; 2] = [Self::Bio, Self::Music];

    const fn label(self) -> &'static str {
        match self {
            Self::Bio => "Bio",
            Self::Music => "Musique préférée",
        }
    }

    const fn placeholder(self) -> &'static str {
        match self {
            Self::Bio => "Votre bio",
            Self::Music => "Votre musique préférée",
        }
    }

    const fn max(self) -> usize {
        match self {
            Self::Bio => 500,
            Self::Music => 200,
        }
    }

    fn rect(self, cx: f32) -> Rect {
        let bio = Rect::at(cx - 300.0, 215.0, 600.0, area_height(BIO_LINES));
        match self {
            Self::Bio => bio,
            Self::Music => Rect::at(bio.x, bio.y + bio.h + 44.0, bio.w, 46.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Picture {
    Avatar,
    Banner,
}

impl Picture {
    const fn path(self) -> &'static str {
        match self {
            Self::Avatar => "me/avatar",
            Self::Banner => "me/banner",
        }
    }
}

#[derive(Default)]
pub struct EditForm {
    pub bio: TextInput,
    pub music: TextInput,
    pub focused: EditField,
    pub pending: Option<HttpSlot>,
    pub status: Status,
    picking: Option<Picture>,
    uploading: Option<HttpSlot>,
}

impl EditForm {
    pub fn open(info: &ApiUserProfile) -> Self {
        Self {
            bio: info.bio.clone().unwrap_or_default().into(),
            music: info.favorite_music.clone().unwrap_or_default().into(),
            ..Self::default()
        }
    }

    const fn value(&self, field: EditField) -> &TextInput {
        match field {
            EditField::Bio => &self.bio,
            EditField::Music => &self.music,
        }
    }

    const fn value_mut(&mut self, field: EditField) -> &mut TextInput {
        match field {
            EditField::Bio => &mut self.bio,
            EditField::Music => &mut self.music,
        }
    }

    fn left(&self, field: EditField) -> usize {
        field.max().saturating_sub(self.value(field).chars().count())
    }

    pub fn type_char(&mut self, c: char) {
        let field = self.focused;
        if self.left(field) == 0 || (self.value(field).is_empty() && c.is_whitespace()) {
            return;
        }
        self.value_mut(field).insert(c);
    }

    fn focus_next(&mut self) {
        self.focused = match self.focused {
            EditField::Bio => EditField::Music,
            EditField::Music => EditField::Bio,
        };
    }

    fn enter(&mut self) -> bool {
        if self.focused != EditField::Bio {
            return true;
        }
        let (before, after) = self.bio.around_caret();
        let around =
            before.len() - before.trim_end_matches('\n').len() + after.len() - after.trim_start_matches('\n').len();
        if around < 2 {
            self.type_char('\n');
        }
        false
    }

    fn body(&self) -> String {
        serde_json::json!({
            "bio": self.bio.trim(),
            "favorite_music": self.music.trim(),
        })
        .to_string()
    }
}

fn buttons(cx: f32) -> (Rect, Rect) {
    let music = EditField::Music.rect(cx);
    let y = music.y + music.h + 39.0;
    (
        Rect::at(cx - 220.0, y, 200.0, 54.0),
        Rect::at(cx + 20.0, y, 200.0, 54.0),
    )
}

fn picture_buttons(cx: f32) -> [(Picture, bool, Rect); 4] {
    let y = buttons(cx).0.y + 54.0 + 50.0;
    let at = |i: f32| Rect::at(cx - 315.0 + i * 160.0, y, 150.0, 46.0);
    [
        (Picture::Avatar, true, at(0.0)),
        (Picture::Avatar, false, at(1.0)),
        (Picture::Banner, true, at(2.0)),
        (Picture::Banner, false, at(3.0)),
    ]
}

fn picture_url(info: &ApiUserProfile, picture: Picture) -> Option<&str> {
    match picture {
        Picture::Avatar => info.avatar_url.as_deref(),
        Picture::Banner => info.banner_url.as_deref(),
    }
}

fn poll_pictures(state: &mut State) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let Some(p) = state.profile.as_mut() else { return };
    let Some(form) = p.edit.as_mut() else { return };
    if let Some(picture) = form.picking {
        match picker::take() {
            Some(Picked::File(bytes)) => {
                let slot = http::new_slot();
                http::put_bytes(http::api_url(picture.path()), bytes, token, Arc::clone(&slot));
                form.uploading = Some(slot);
                form.status = Status::info("Envoi de l'image...");
                form.picking = None;
            }
            Some(Picked::Failed(msg)) => {
                form.status = Status::error(msg);
                form.picking = None;
            }
            Some(Picked::Cancelled) => form.picking = None,
            None => {}
        }
    }
    let Some(result) = http::take(&mut form.uploading) else {
        return;
    };
    match result {
        Ok(resp) if resp.status == 200 => {
            #[derive(serde::Deserialize)]
            struct Pictures {
                avatar_url: Option<String>,
                banner_url: Option<String>,
            }
            if let Some(urls) = http::json::<Pictures>(&resp) {
                p.core.info.avatar_url.clone_from(&urls.avatar_url);
                p.core.info.banner_url = urls.banner_url;
                if let Some(auth) = state.auth.as_mut() {
                    auth.avatar_url = urls.avatar_url;
                }
                form.status = Status::success("Image mise à jour.");
            }
        }
        Ok(resp) => form.status = Status::error(http::error_message(&resp)),
        Err(e) => form.status = Status::error(http::network_error(&e)),
    }
}

pub fn poll(state: &mut State) {
    poll_pictures(state);
    let Some(p) = state.profile.as_mut() else { return };
    let Some(form) = p.edit.as_mut() else { return };
    let Some(result) = http::take(&mut form.pending) else {
        return;
    };
    match result {
        Ok(resp) if resp.status == 200 => {
            #[derive(serde::Deserialize)]
            struct PatchResp {
                bio: Option<String>,
                favorite_music: Option<String>,
            }
            if let Some(data) = http::json::<PatchResp>(&resp) {
                p.core.info.bio = data.bio;
                p.core.info.favorite_music = data.favorite_music;
                p.edit = None;
            }
        }
        Ok(resp) => form.status = Status::error(http::error_message(&resp)),
        Err(e) => form.status = Status::error(http::network_error(&e)),
    }
}

fn save(form: &mut EditForm, token: Option<String>) {
    if form.pending.is_some() {
        return;
    }
    let slot = http::new_slot();
    http::patch_json(http::api_url("me"), form.body(), token, Arc::clone(&slot));
    form.pending = Some(slot);
    form.status.clear();
}

pub fn update(app: &mut App, state: &mut State) {
    let cx = state.ui.view().w / 2.0;
    let token = state.auth.as_ref().map(|a| a.token.clone());
    let State {
        profile,
        ui,
        fonts,
        keys,
        ..
    } = state;
    let Some(profile) = profile.as_mut() else { return };
    let Some(form) = profile.edit.as_mut() else { return };

    match form.focused {
        EditField::Bio => area_keys(fonts, EditField::Bio.rect(cx), &mut form.bio, keys),
        EditField::Music => form.music.edit(keys),
    }
    if app.keyboard.was_pressed(KeyCode::Tab) {
        form.focus_next();
    }
    if area_clicked(ui, fonts, EditField::Bio.rect(cx), &mut form.bio) {
        form.focused = EditField::Bio;
    }
    if field_clicked(ui, fonts, EditField::Music.rect(cx), &mut form.music) {
        form.focused = EditField::Music;
    }
    if form.uploading.is_none() {
        for (picture, change, rect) in picture_buttons(cx) {
            if !ui.clicked(rect) {
                continue;
            }
            if change {
                form.picking = Some(picture);
                picker::open();
            } else if picture_url(&profile.core.info, picture).is_some() {
                let slot = http::new_slot();
                http::delete_req(http::api_url(picture.path()), token.clone(), Arc::clone(&slot));
                form.uploading = Some(slot);
            }
        }
    }
    let wants_save = app.keyboard.was_pressed(KeyCode::Enter) && form.enter();
    let (save_btn, cancel_btn) = buttons(cx);
    if ui.clicked(save_btn) || wants_save {
        save(form, token);
    }
    if ui.clicked(cancel_btn) || app.keyboard.was_pressed(KeyCode::Escape) {
        profile.edit = None;
    }
}

pub fn draw(ui: &Ui, draw: &mut Draw, fonts: &Fonts, form: &EditForm, info: &ApiUserProfile, cx: f32) {
    let pal = ui.palette();
    for which in EditField::ALL {
        let rect = which.rect(cx);
        let label_y = rect.y - 15.0;
        draw.sharp_text(&fonts.text, which.label())
            .position(rect.x, label_y)
            .size(theme::size::LABEL)
            .v_align_middle()
            .color(pal.text_dim);
        let focused = form.focused == which;
        let left = form.left(which);
        let left_color = match left {
            0 => theme::DANGER,
            1..=20 => theme::WARNING,
            _ => pal.text_muted,
        };
        draw.sharp_text(&fonts.text, &left.to_string())
            .position(rect.x + rect.w, label_y)
            .size(theme::size::SMALL)
            .h_align_right()
            .v_align_middle()
            .color(left_color);
        let field = Field {
            placeholder: which.placeholder(),
            input: form.value(which),
            focused,
        };
        if which == EditField::Bio {
            text_area(draw, ui, fonts, rect, &field);
        } else {
            text_field(draw, ui, fonts, rect, &field);
        }
    }

    let (save_btn, cancel_btn) = buttons(cx);
    let saving = form.pending.is_some();
    ui.button_enabled(
        draw,
        fonts,
        save_btn,
        if saving { "Sauvegarde..." } else { "Enregistrer" },
        !saving,
    );
    ui.button(draw, fonts, cancel_btn, "Annuler");

    let busy = form.uploading.is_some();
    for (picture, change, rect) in picture_buttons(cx) {
        let label = match (picture, change) {
            (Picture::Avatar, true) => "Changer l'avatar",
            (Picture::Avatar, false) => "Retirer l'avatar",
            (Picture::Banner, true) => "Changer la bannière",
            (Picture::Banner, false) => "Retirer la bannière",
        };
        let enabled = !busy && (change || picture_url(info, picture).is_some());
        ui.button_enabled(draw, fonts, rect, label, enabled);
    }

    if let Some((msg, color)) = form.status.shown(&pal) {
        draw.sharp_text(&fonts.text, msg)
            .position(cx, save_btn.y + save_btn.h + 21.0)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(form: &mut EditForm, text: &str) {
        for c in text.chars() {
            form.type_char(c);
        }
    }

    #[test]
    fn enter_breaks_the_line_in_the_bio_and_saves_elsewhere() {
        let mut form = EditForm::default();
        typed(&mut form, "a");
        assert!(!form.enter());
        assert_eq!(&*form.bio, "a\n");
        form.focus_next();
        assert!(form.enter());
        assert_eq!((&*form.bio, &*form.music), ("a\n", ""));
    }

    #[test]
    fn a_bio_holds_one_blank_line_in_a_row_and_none_in_front() {
        let mut form = EditForm::default();
        form.enter();
        typed(&mut form, "  ");
        assert_eq!(&*form.bio, "", "nothing the save would trim off the front");
        typed(&mut form, "a");
        for _ in 0..5 {
            form.enter();
        }
        assert_eq!(&*form.bio, "a\n\n");
        typed(&mut form, "b");
        form.enter();
        assert_eq!(&*form.bio, "a\n\nb\n");
        form.bio.set_caret(2);
        form.enter();
        assert_eq!(&*form.bio, "a\n\nb\n", "nor a second one from the middle of a break");
        form.bio.set_caret(4);
        typed(&mut form, "c");
        form.enter();
        assert_eq!(&*form.bio, "a\n\nbc\n\n");
    }

    #[test]
    fn a_full_field_takes_nothing_more() {
        let mut form = EditForm::default();
        typed(&mut form, &"é".repeat(600));
        assert_eq!(form.bio.chars().count(), EditField::Bio.max());
        assert_eq!(form.left(EditField::Bio), 0);
        assert!(!form.enter());
        assert_eq!(form.bio.chars().count(), EditField::Bio.max());
        form.bio.erase();
        assert_eq!(form.left(EditField::Bio), 1);

        form.focus_next();
        typed(&mut form, &"m".repeat(600));
        assert_eq!(form.music.len(), EditField::Music.max());
    }

    #[test]
    fn typing_goes_to_the_focused_field_only() {
        let mut form = EditForm::default();
        typed(&mut form, "bio");
        form.focus_next();
        typed(&mut form, "musique");
        form.music.erase();
        form.focus_next();
        assert_eq!((&*form.bio, &*form.music), ("bio", "musiqu"));
        assert_eq!(form.focused, EditField::Bio);
    }

    #[test]
    fn an_emptied_field_is_sent_empty_not_left_out() {
        let mut form = EditForm {
            bio: "  \n".to_owned().into(),
            music: " Tsu ".to_owned().into(),
            ..EditForm::default()
        };
        let body: serde_json::Value = serde_json::from_str(&form.body()).expect("json");
        assert_eq!(body["bio"], "");
        assert_eq!(body["favorite_music"], "Tsu");
        form.music.clear();
        let body: serde_json::Value = serde_json::from_str(&form.body()).expect("json");
        assert_eq!(body["favorite_music"], "");
    }

    #[test]
    fn opening_the_form_starts_from_the_saved_profile() {
        let info = ApiUserProfile {
            bio: Some("salut".into()),
            ..ApiUserProfile::default()
        };
        let form = EditForm::open(&info);
        assert_eq!((&*form.bio, &*form.music), ("salut", ""));
        assert_eq!(form.focused, EditField::Bio);
    }
}
