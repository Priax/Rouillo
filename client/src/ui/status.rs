use notan::prelude::Color;

use crate::theme::{self, Palette};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Empty,
    Info(String),
    Success(String),
    Error(String),
}

impl Status {
    pub fn info(msg: impl Into<String>) -> Self {
        Self::Info(msg.into())
    }

    pub fn success(msg: impl Into<String>) -> Self {
        Self::Success(msg.into())
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self::Error(msg.into())
    }

    pub fn clear(&mut self) {
        *self = Self::Empty;
    }

    pub fn shown(&self, pal: &Palette) -> Option<(&str, Color)> {
        match self {
            Self::Empty => None,
            Self::Info(m) => Some((m, pal.text_muted)),
            Self::Success(m) => Some((m, theme::SUCCESS)),
            Self::Error(m) => Some((m, theme::DANGER)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_of_message_decides_its_colour() {
        let pal = Palette::new(theme::hue::PURPLE);
        assert_eq!(Status::Empty.shown(&pal), None);
        let colour = |s: Status| s.shown(&pal).map(|(_, c)| c);
        assert_eq!(colour(Status::error("Erreur réseau")), Some(theme::DANGER));
        assert_eq!(colour(Status::success("Demande envoyée !")), Some(theme::SUCCESS));
        assert_eq!(colour(Status::info("Erreur n'est qu'un mot ici")), Some(pal.text_muted));
    }
}
