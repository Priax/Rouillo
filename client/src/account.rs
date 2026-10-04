use std::sync::Arc;

use notan::draw::Draw;
use notan::prelude::*;

use crate::http::{self, HttpSlot};
use crate::state::MAX_PASSWORD_CHARS;
use crate::theme;
use crate::ui::{field_clicked, text_field, EditKeys, Field, Fonts, Rect, SharpText, Status, TextInput, Ui};

const MIN_PASSWORD: usize = 8;
const FIELD_W: f32 = 460.0;
const FIELD_H: f32 = 46.0;
const FIELD_STEP: f32 = 88.0;
const TOP: f32 = 230.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Choose,
    Username,
    Password,
    LogoutAll,
    Delete,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    Username,
    Password,
    LogoutAll,
    Delete,
    Back,
}

impl Choice {
    const ALL: [Self; 5] = [
        Self::Username,
        Self::Password,
        Self::LogoutAll,
        Self::Delete,
        Self::Back,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Username => "Changer de pseudo",
            Self::Password => "Changer le mot de passe",
            Self::LogoutAll => "Se déconnecter de partout",
            Self::Delete => "Supprimer le compte",
            Self::Back => "Retour",
        }
    }

    fn rect(self, cx: f32) -> Rect {
        let i = Self::ALL.iter().position(|&c| c == self).unwrap_or(0);
        Rect::at(cx - 180.0, TOP + i as f32 * 74.0, 360.0, 54.0)
    }
}

#[derive(Default)]
pub struct AccountForm {
    page: Page,
    current: TextInput,
    new: TextInput,
    confirm: TextInput,
    focused: usize,
    pending: Option<HttpSlot>,
    status: Status,
}

pub enum Outcome {
    Stay,
    Close,
    LoggedOut(&'static str),
    Renamed(String),
}

impl AccountForm {
    pub fn open() -> Self {
        let mut form = Self::default();
        form.show(Page::Choose);
        form
    }

    fn show(&mut self, page: Page) {
        *self = Self {
            page,
            current: TextInput::masked().max_chars(MAX_PASSWORD_CHARS),
            new: if page == Page::Username {
                TextInput::default().max_chars(shared::MAX_USERNAME_CHARS)
            } else {
                TextInput::masked().max_chars(MAX_PASSWORD_CHARS)
            },
            confirm: TextInput::masked().max_chars(MAX_PASSWORD_CHARS),
            ..Self::default()
        };
    }

    fn labels(&self) -> &'static [&'static str] {
        match self.page {
            Page::Username => &["Mot de passe actuel", "Nouveau pseudo"],
            Page::Password => &["Mot de passe actuel", "Nouveau mot de passe", "Confirmez le nouveau"],
            Page::Delete => &["Mot de passe"],
            Page::Choose | Page::LogoutAll => &[],
        }
    }

    fn input(&self, i: usize) -> &TextInput {
        match i {
            0 => &self.current,
            1 => &self.new,
            _ => &self.confirm,
        }
    }

    fn input_mut(&mut self, i: usize) -> &mut TextInput {
        match i {
            0 => &mut self.current,
            1 => &mut self.new,
            _ => &mut self.confirm,
        }
    }

    pub fn typing(&mut self) -> Option<&mut TextInput> {
        (self.focused < self.labels().len()).then(|| self.input_mut(self.focused))
    }

    fn check(&self) -> Result<(), &'static str> {
        if !self.labels().is_empty() && self.current.is_empty() {
            return Err("Entrez votre mot de passe actuel.");
        }
        if self.page == Page::Username && !shared::valid_username(self.new.trim()) {
            return Err("Le pseudo doit faire 3 à 24 caractères (lettres, chiffres ou _).");
        }
        if self.page == Page::Password {
            if self.new.chars().count() < MIN_PASSWORD {
                return Err("Le nouveau mot de passe doit faire au moins 8 caractères.");
            }
            if *self.new != *self.confirm {
                return Err("Les deux nouveaux mots de passe ne correspondent pas.");
            }
        }
        Ok(())
    }

    fn submit(&mut self, token: Option<String>) {
        if self.pending.is_some() {
            return;
        }
        if let Err(msg) = self.check() {
            self.status = Status::error(msg);
            return;
        }
        let slot = http::new_slot();
        let (path, body) = match self.page {
            Page::Password => (
                "me/password",
                serde_json::json!({ "current": &*self.current, "new": &*self.new }).to_string(),
            ),
            Page::Username => (
                "me/username",
                serde_json::json!({ "username": self.new.trim(), "password": &*self.current }).to_string(),
            ),
            Page::LogoutAll => ("logout-all", String::new()),
            Page::Delete => (
                "me/delete",
                serde_json::json!({ "password": &*self.current }).to_string(),
            ),
            Page::Choose => return,
        };
        if body.is_empty() {
            http::post_empty(http::api_url(path), token, Arc::clone(&slot));
        } else {
            http::post_json(http::api_url(path), body, token, Arc::clone(&slot));
        }
        self.pending = Some(slot);
        self.status.clear();
    }

    fn poll(&mut self) -> Outcome {
        let Some(result) = http::take_json::<serde_json::Value>(&mut self.pending) else {
            return Outcome::Stay;
        };
        match result {
            Ok(reply) => match self.page {
                Page::Username => {
                    let name = reply["username"]
                        .as_str()
                        .map_or_else(|| self.new.trim().to_owned(), str::to_owned);
                    self.show(Page::Choose);
                    self.status = Status::success("Pseudo changé.");
                    Outcome::Renamed(name)
                }
                Page::Password => {
                    self.show(Page::Choose);
                    self.status = Status::success("Mot de passe changé, vos autres sessions sont déconnectées.");
                    Outcome::Stay
                }
                Page::LogoutAll => Outcome::LoggedOut("Toutes vos sessions ont été déconnectées."),
                Page::Delete => Outcome::LoggedOut("Votre compte a été supprimé."),
                Page::Choose => Outcome::Stay,
            },
            Err(msg) => {
                self.status = Status::error(msg);
                Outcome::Stay
            }
        }
    }
}

fn field_rect(cx: f32, page: Page, i: usize) -> Rect {
    let top = if page == Page::Delete { TOP + 70.0 } else { TOP + 30.0 };
    Rect::at(cx - FIELD_W / 2.0, top + i as f32 * FIELD_STEP, FIELD_W, FIELD_H)
}

fn buttons(cx: f32, form: &AccountForm) -> (Rect, Rect) {
    let y = match form.page {
        Page::LogoutAll => TOP + 90.0,
        _ => field_rect(cx, form.page, form.labels().len()).y,
    };
    (
        Rect::at(cx - 240.0, y, 230.0, 54.0),
        Rect::at(cx + 10.0, y, 230.0, 54.0),
    )
}

const fn confirm_label(page: Page) -> &'static str {
    match page {
        Page::Username | Page::Password => "Enregistrer",
        Page::LogoutAll => "Confirmer",
        Page::Delete => "Supprimer",
        Page::Choose => "",
    }
}

const fn warning(page: Page) -> Option<&'static str> {
    match page {
        Page::LogoutAll => Some("Toutes vos sessions seront fermées, celle-ci comprise."),
        Page::Delete => Some("Définitif: votre profil, votre ELO et vos amis seront supprimés !"),
        Page::Choose | Page::Username | Page::Password => None,
    }
}

pub fn update(
    app: &App,
    ui: &Ui,
    fonts: &Fonts,
    keys: &EditKeys,
    form: &mut AccountForm,
    token: Option<String>,
) -> Outcome {
    let outcome = form.poll();
    if !matches!(outcome, Outcome::Stay) {
        return outcome;
    }
    let cx = ui.view().w / 2.0;

    if form.page == Page::Choose {
        for choice in Choice::ALL {
            if ui.clicked(choice.rect(cx)) {
                match choice {
                    Choice::Username => form.show(Page::Username),
                    Choice::Password => form.show(Page::Password),
                    Choice::LogoutAll => form.show(Page::LogoutAll),
                    Choice::Delete => form.show(Page::Delete),
                    Choice::Back => return Outcome::Close,
                }
            }
        }
        if app.keyboard.was_pressed(KeyCode::Escape) {
            return Outcome::Close;
        }
        return Outcome::Stay;
    }

    let fields = form.labels().len();
    if fields > 0 {
        let i = form.focused;
        form.input_mut(i).edit(keys);
        if app.keyboard.was_pressed(KeyCode::Tab) {
            form.focused = (form.focused + 1) % fields;
        }
        for i in 0..fields {
            if field_clicked(ui, fonts, field_rect(cx, form.page, i), form.input_mut(i)) {
                form.focused = i;
            }
        }
    }
    let (confirm_btn, cancel_btn) = buttons(cx, form);
    if ui.clicked(confirm_btn) || app.keyboard.was_pressed(KeyCode::Enter) {
        form.submit(token);
    }
    if form.pending.is_none() && (ui.clicked(cancel_btn) || app.keyboard.was_pressed(KeyCode::Escape)) {
        form.show(Page::Choose);
    }
    Outcome::Stay
}

pub fn draw(ui: &Ui, draw: &mut Draw, fonts: &Fonts, form: &AccountForm, cx: f32) {
    let pal = ui.palette();
    let mut status_y = TOP - 40.0;

    if form.page == Page::Choose {
        for choice in Choice::ALL {
            ui.button(draw, fonts, choice.rect(cx), choice.label());
        }
    } else {
        if let Some(text) = warning(form.page) {
            let color = if form.page == Page::Delete {
                theme::DANGER
            } else {
                pal.text_dim
            };
            draw.sharp_text(&fonts.text, text)
                .position(cx, TOP + 20.0)
                .size(theme::size::LABEL)
                .h_align_center()
                .v_align_middle()
                .color(color);
        }
        for (i, label) in form.labels().iter().enumerate() {
            let rect = field_rect(cx, form.page, i);
            draw.sharp_text(&fonts.text, label)
                .position(rect.x, rect.y - 15.0)
                .size(theme::size::LABEL)
                .v_align_middle()
                .color(pal.text_dim);
            let field = Field {
                placeholder: "",
                input: form.input(i),
                focused: form.focused == i,
            };
            text_field(draw, ui, fonts, rect, &field);
        }
        let (confirm_btn, cancel_btn) = buttons(cx, form);
        let busy = form.pending.is_some();
        ui.button_enabled(
            draw,
            fonts,
            confirm_btn,
            if busy { "..." } else { confirm_label(form.page) },
            !busy,
        );
        ui.button(draw, fonts, cancel_btn, "Annuler");
        status_y = confirm_btn.y + confirm_btn.h + 26.0;
    }

    if let Some((msg, color)) = form.status.shown(&pal) {
        draw.sharp_text(&fonts.text, msg)
            .position(cx, status_y)
            .size(theme::size::LABEL)
            .h_align_center()
            .v_align_middle()
            .color(color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(form: &mut AccountForm, field: usize, text: &str) {
        form.focused = field;
        for c in text.chars() {
            if let Some(input) = form.typing() {
                input.type_char(c);
            }
        }
    }

    #[test]
    fn a_new_password_must_be_long_enough_and_confirmed() {
        let mut form = AccountForm::open();
        form.show(Page::Password);
        typed(&mut form, 0, "ancien123");
        typed(&mut form, 1, "court");
        typed(&mut form, 2, "court");
        assert!(form.check().is_err());
        form.show(Page::Password);
        typed(&mut form, 0, "ancien123");
        typed(&mut form, 1, "nouveau123");
        typed(&mut form, 2, "nouveau124");
        assert!(form.check().is_err());
        form.confirm.erase();
        typed(&mut form, 2, "3");
        assert!(form.check().is_ok());
    }

    #[test]
    fn typing_only_fills_the_fields_the_page_has() {
        let mut form = AccountForm::open();
        form.show(Page::Delete);
        typed(&mut form, 0, "secret");
        typed(&mut form, 1, "ailleurs");
        assert_eq!((&*form.current, &*form.new), ("secret", ""));
        form.show(Page::LogoutAll);
        typed(&mut form, 0, "x");
        assert_eq!(&*form.current, "");
    }

    #[test]
    fn the_current_password_is_required() {
        let mut form = AccountForm::open();
        form.show(Page::Delete);
        assert!(form.check().is_err());
        typed(&mut form, 0, "x");
        assert!(form.check().is_ok());
        form.show(Page::LogoutAll);
        assert!(form.check().is_ok());
    }

    #[test]
    fn a_new_username_is_checked_and_capped() {
        let mut form = AccountForm::open();
        form.show(Page::Username);
        typed(&mut form, 0, "secret123");
        typed(&mut form, 1, "a b");
        assert!(form.check().is_err());
        form.new.erase();
        form.new.erase();
        typed(&mut form, 1, "_Été");
        assert!(form.check().is_ok(), "{:?}", &*form.new);
        typed(&mut form, 1, &"x".repeat(40));
        assert_eq!(form.new.chars().count(), shared::MAX_USERNAME_CHARS);
    }

    #[test]
    fn passwords_stop_at_the_limit() {
        let mut form = AccountForm::open();
        form.show(Page::Delete);
        typed(&mut form, 0, &"a".repeat(100));
        assert_eq!(form.current.len(), MAX_PASSWORD_CHARS);
    }
}
