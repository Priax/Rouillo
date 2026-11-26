use std::sync::Arc;

use crate::http::{self, HttpSlot};
use crate::picker::{self, Picked};
use crate::state::{ApiUserProfile, ProfileData, State};
use crate::theme;
use crate::tick::Tick;
use crate::ui::{widgets, Face, Gui, Rect, Status, FILL};

const BIO_LINES: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
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
        let bio = Rect::centered_on(cx, 215.0, 600.0, widgets::area_height(BIO_LINES));

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

pub struct EditForm {
    pub bio: String,
    pub music: String,
    /// The field last typed in, which keeps the keyboard.
    pub focused: EditField,
    pub pending: Option<HttpSlot>,
    pub status: Status,
    picking: Option<Picture>,
    uploading: Option<HttpSlot>,
}

impl Default for EditForm {
    fn default() -> Self {
        Self::open(&ApiUserProfile::default())
    }
}

impl EditForm {
    pub fn open(info: &ApiUserProfile) -> Self {
        Self {
            bio: info.bio.clone().unwrap_or_default(),
            music: info.favorite_music.clone().unwrap_or_default(),
            focused: EditField::default(),
            pending: None,
            status: Status::default(),
            picking: None,
            uploading: None,
        }
    }

    const fn value_mut(&mut self, field: EditField) -> &mut String {
        match field {
            EditField::Bio => &mut self.bio,
            EditField::Music => &mut self.music,
        }
    }

    fn left(&self, field: EditField) -> usize {
        let text = match field {
            EditField::Bio => &self.bio,
            EditField::Music => &self.music,
        };
        field.max().saturating_sub(text.chars().count())
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
    let [save, back] = Rect::centered_on(cx, y, 440.0, 54.0).columns(40.0, [FILL, FILL]);

    (save, back)
}

fn picture_buttons(cx: f32) -> [(Picture, bool, Rect); 4] {
    let y = buttons(cx).0.y + 54.0 + 50.0;
    let [a, b, c, d] = Rect::centered_on(cx, y, 630.0, 46.0).columns(10.0, [FILL; 4]);

    [(Picture::Avatar, true, a), (Picture::Avatar, false, b), (Picture::Banner, true, c), (Picture::Banner, false, d)]
}

fn picture_url(info: &ApiUserProfile, picture: Picture) -> Option<&str> {
    match picture {
        Picture::Avatar => info.avatar_url.as_deref(),
        Picture::Banner => info.banner_url.as_deref(),
    }
}

#[derive(serde::Deserialize)]
struct Pictures {
    avatar_url: Option<String>,
    banner_url: Option<String>,
}

#[derive(serde::Deserialize)]
struct PatchResp {
    bio: Option<String>,
    favorite_music: Option<String>,
}

fn poll_pictures(state: &mut State, p: &mut ProfileData) {
    let token = state.auth.as_ref().map(|a| a.token.clone());
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

    let Some(result) = http::take_json::<Pictures>(&mut form.uploading) else {
        return;
    };
    match result {
        Ok(urls) => {
            p.core.info.avatar_url.clone_from(&urls.avatar_url);
            p.core.info.banner_url = urls.banner_url;
            if let Some(auth) = state.auth.as_mut() {
                auth.avatar_url = urls.avatar_url;
            }
            form.status = Status::success("Image mise à jour.");
        }
        Err(msg) => form.status = Status::error(msg),
    }
}

pub fn poll(state: &mut State, p: &mut ProfileData) {
    poll_pictures(state, p);

    let Some(form) = p.edit.as_mut() else { return };
    let Some(result) = http::take_json::<PatchResp>(&mut form.pending) else {
        return;
    };
    match result {
        Ok(data) => {
            p.core.info.bio = data.bio;
            p.core.info.favorite_music = data.favorite_music;
            p.edit = None;
        }
        Err(msg) => form.status = Status::error(msg),
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

/// Escape closes the form; the fields and the buttons are in [`show`].
pub fn update(tick: &Tick, profile: &mut ProfileData) {
    if tick.back() {
        profile.edit = None;
    }
}

pub fn show(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, token: Option<String>, profile: &mut ProfileData) {
    let pal = gui.ui.palette();
    let cx = gui.ui.view().w / 2.0;
    let (save_btn, cancel_btn) = buttons(cx);
    let info = &profile.core.info;
    let Some(form) = profile.edit.as_mut() else { return };
    let saving = form.pending.is_some();
    let busy = form.uploading.is_some();

    if !ui.cx.wants_keyboard() {
        widgets::focus_field(ui, form.focused);
    }
    let mut submitted = false;
    for which in EditField::ALL {
        let rect = which.rect(cx);
        let label_y = rect.y - 15.0;
        let left = form.left(which);
        let left_color = match left {
            0 => theme::DANGER,
            1..=20 => theme::WARNING,
            _ => pal.text_muted,
        };
        let fonts = &mut gui.fonts;
        fonts
            .write(ui.draw, Face::Text, which.label())
            .left(rect.x, label_y)
            .size(theme::size::LABEL)
            .color(pal.text_dim);
        fonts
            .write(ui.draw, Face::Text, &left.to_string())
            .right(rect.x + rect.w, label_y)
            .size(theme::size::SMALL)
            .color(left_color);
        let field =
            widgets::field().placeholder(which.placeholder()).max_chars(which.max()).multiline(which == EditField::Bio);
        let typed = widgets::text_field(ui, gui, which, rect, field, form.value_mut(which));
        if typed.response.focused {
            form.focused = which;
        }
        submitted |= typed.submitted;
    }
    let fonts = &mut gui.fonts;

    let saved = widgets::button(ui, "save", save_btn, if saving { "Sauvegarde..." } else { "Enregistrer" }, !saving);
    let cancelled = widgets::button(ui, "cancel", cancel_btn, "Annuler", true);
    let mut asked = None;
    for (picture, change, rect) in picture_buttons(cx) {
        let label = match (picture, change) {
            (Picture::Avatar, true) => "Changer l'avatar",
            (Picture::Avatar, false) => "Retirer l'avatar",
            (Picture::Banner, true) => "Changer la bannière",
            (Picture::Banner, false) => "Retirer la bannière",
        };
        let enabled = !busy && (change || picture_url(info, picture).is_some());
        if widgets::button(ui, label, rect, label, enabled) {
            asked = Some((picture, change));
        }
    }
    if let Some((msg, color)) = form.status.shown(&pal) {
        fonts
            .write(ui.draw, Face::Text, msg)
            .center(cx, save_btn.y + save_btn.h + 21.0)
            .size(theme::size::LABEL)
            .color(color);
    }

    match asked {
        Some((picture, true)) => {
            form.picking = Some(picture);
            picker::open();
        }
        Some((picture, false)) => {
            let slot = http::new_slot();
            http::delete_req(http::api_url(picture.path()), token.clone(), Arc::clone(&slot));
            form.uploading = Some(slot);
        }
        None => {}
    }
    if saved || submitted {
        save(form, token);
    } else if cancelled {
        profile.edit = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_emptied_field_is_sent_empty_not_left_out() {
        let mut form = EditForm { bio: "  \n".to_owned(), music: " Tsu ".to_owned(), ..EditForm::default() };
        let body: serde_json::Value = serde_json::from_str(&form.body()).expect("json");
        assert_eq!(body["bio"], "");
        assert_eq!(body["favorite_music"], "Tsu");
        form.music.clear();
        let body: serde_json::Value = serde_json::from_str(&form.body()).expect("json");
        assert_eq!(body["favorite_music"], "");
    }

    #[test]
    fn opening_the_form_starts_from_the_saved_profile() {
        let info = ApiUserProfile { bio: Some("salut".into()), ..ApiUserProfile::default() };
        let form = EditForm::open(&info);
        assert_eq!((&*form.bio, &*form.music), ("salut", ""));
        assert_eq!(form.focused, EditField::Bio);
    }
}
