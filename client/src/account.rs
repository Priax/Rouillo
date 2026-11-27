use std::sync::Arc;

use aero::edit::TextField;

use crate::http::{self, HttpSlot};
use crate::state::MAX_PASSWORD_CHARS;
use crate::theme;
use crate::tick::Tick;
use crate::ui::{widgets, Face, Gui, Label, Rect, Status, Typeset, FILL};

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
    const ALL: [Self; 5] = [Self::Username, Self::Password, Self::LogoutAll, Self::Delete, Self::Back];

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
        Rect::centered_on(cx, TOP, 360.0, 0.0).nth_row(i, 54.0, 20.0)
    }
}

#[derive(Default)]
pub struct AccountForm {
    page: Page,
    current: String,
    new: String,
    confirm: String,
    /// The field last typed in, which keeps the keyboard.
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
        *self = Self { page, ..Self::default() };
    }

    /// Field `i`'s options: the new username is the only one shown as typed.
    fn field(&self, i: usize) -> TextField<'static> {
        let label = self.labels()[i];
        if self.page == Page::Username && i == 1 {
            return widgets::field().label(label).max_chars(shared::MAX_USERNAME_CHARS);
        }
        widgets::field().label(label).max_chars(MAX_PASSWORD_CHARS).masked(true)
    }

    fn labels(&self) -> &'static [&'static str] {
        match self.page {
            Page::Username => &["Mot de passe actuel", "Nouveau pseudo"],
            Page::Password => &["Mot de passe actuel", "Nouveau mot de passe", "Confirmez le nouveau"],
            Page::Delete => &["Mot de passe"],
            Page::Choose | Page::LogoutAll => &[],
        }
    }

    fn input_mut(&mut self, i: usize) -> &mut String {
        match i {
            0 => &mut self.current,
            1 => &mut self.new,
            _ => &mut self.confirm,
        }
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
            Page::Password => {
                ("me/password", serde_json::json!({ "current": &*self.current, "new": &*self.new }).to_string())
            }
            Page::Username => (
                "me/username",
                serde_json::json!({ "username": self.new.trim(), "password": &*self.current }).to_string(),
            ),
            Page::LogoutAll => ("logout-all", String::new()),
            Page::Delete => ("me/delete", serde_json::json!({ "password": &*self.current }).to_string()),
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
                    let name = reply["username"].as_str().map_or_else(|| self.new.trim().to_owned(), str::to_owned);
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
    Rect::centered_on(cx, top, FIELD_W, 0.0).nth_row(i, FIELD_H, FIELD_STEP - FIELD_H)
}

fn buttons(cx: f32, form: &AccountForm) -> (Rect, Rect) {
    let y = match form.page {
        Page::LogoutAll => TOP + 90.0,
        _ => field_rect(cx, form.page, form.labels().len()).y(),
    };
    let [confirm, back] = Rect::centered_on(cx, y, 480.0, 54.0).columns(20.0, [FILL, FILL]);
    (confirm, back)
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

/// The answer to come and Escape; the fields and the buttons are in [`show`].
pub fn update(tick: &Tick, form: &mut AccountForm) -> Outcome {
    let outcome = form.poll();
    if !matches!(outcome, Outcome::Stay) {
        return outcome;
    }
    if form.page == Page::Choose {
        return if tick.back() { Outcome::Close } else { Outcome::Stay };
    }
    if form.pending.is_none() && tick.back() {
        form.show(Page::Choose);
    }
    Outcome::Stay
}

pub fn show(ui: &mut aero::ui::Ui<'_>, gui: &mut Gui, form: &mut AccountForm, token: Option<String>) -> Outcome {
    let pal = gui.stage.palette();
    let cx = gui.stage.view().w / 2.0;
    let choosing = form.page == Page::Choose;
    let (confirm_btn, cancel_btn) = buttons(cx, form);
    let busy = form.pending.is_some();
    let confirm = Label::busy_or(busy, confirm_label(form.page));
    let warning_color = if form.page == Page::Delete { theme::DANGER } else { pal.text_dim };
    let status_y = if choosing { TOP - 40.0 } else { confirm_btn.y() + confirm_btn.h() + 26.0 };

    let mut chosen = None;
    let (mut confirmed, mut cancelled) = (false, false);
    if choosing {
        for choice in Choice::ALL {
            if widgets::button(ui, choice.label(), choice.rect(cx), choice.label(), true) {
                chosen = Some(choice);
            }
        }
    } else {
        if let Some(text) = warning(form.page) {
            let fonts = &mut ui.cx.fonts;
            fonts.write(ui.draw, Face::Text, text).center(cx, TOP + 20.0).size(theme::size::LABEL).color(warning_color);
        }
        if !ui.cx.wants_keyboard() {
            widgets::focus_field(ui, ("account field", form.focused));
        }
        for (i, label) in form.labels().iter().enumerate() {
            let rect = field_rect(cx, form.page, i);
            ui.cx
                .fonts
                .write(ui.draw, Face::Text, label)
                .left(rect.x(), rect.y() - 15.0)
                .size(theme::size::LABEL)
                .color(pal.text_dim);
            let field = form.field(i);
            let typed = widgets::text_field(ui, gui, ("account field", i), rect, field, form.input_mut(i));
            if typed.response.focused {
                form.focused = i;
            }
            confirmed |= typed.submitted;
        }
        confirmed |= widgets::button(ui, "confirm", confirm_btn, confirm, !busy);
        cancelled = widgets::button(ui, "cancel", cancel_btn, "Annuler", true);
    }
    if let Some((msg, color)) = form.status.shown(&pal) {
        ui.cx.fonts.write(ui.draw, Face::Text, msg).center(cx, status_y).size(theme::size::LABEL).color(color);
    }

    match chosen {
        Some(Choice::Username) => form.show(Page::Username),
        Some(Choice::Password) => form.show(Page::Password),
        Some(Choice::LogoutAll) => form.show(Page::LogoutAll),
        Some(Choice::Delete) => form.show(Page::Delete),
        Some(Choice::Back) => return Outcome::Close,
        None => {}
    }
    if confirmed {
        form.submit(token);
    } else if cancelled && form.pending.is_none() {
        form.show(Page::Choose);
    }
    Outcome::Stay
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(form: &mut AccountForm, field: usize, text: &str) {
        form.input_mut(field).push_str(text);
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
        form.confirm.pop();
        typed(&mut form, 2, "3");
        assert!(form.check().is_ok());
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
    fn a_new_username_is_checked() {
        let mut form = AccountForm::open();
        form.show(Page::Username);
        typed(&mut form, 0, "secret123");
        typed(&mut form, 1, "a b");
        assert!(form.check().is_err());
        form.new.clear();
        typed(&mut form, 1, "_Été");
        assert!(form.check().is_ok(), "{:?}", form.new);
    }
}
