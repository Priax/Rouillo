use crate::state::{Screen, State};

/// What a screen wants once its frame is done. Screens have no access to
/// the navigation: this is how they move.
#[derive(Clone, Copy)]
#[must_use]
pub enum Transition {
    Stay,
    To(Screen),
}

/// The screen on display.
pub struct Nav {
    pub current: Screen,
}

impl Nav {
    pub fn apply(&mut self, transition: Transition) {
        match transition {
            Transition::Stay => {}
            Transition::To(screen) => self.current = screen,
        }
    }
}

/// The application: where the player is, and everything screens share.
/// They are apart so that a screen can hold its data and `State` together.
#[derive(notan::prelude::AppState)]
pub struct Root {
    pub nav: Nav,
    pub state: State,
}
