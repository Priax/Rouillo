use crate::state::{Screen, State};

/// What a screen wants once its frame is done. Screens have no access to
/// the navigation: this is how they move.
#[must_use]
pub enum Transition {
    Stay,
    /// Leaves everything behind for this screen.
    To(Screen),
    /// Opens this screen over the current one, kept as it is underneath.
    Push(Screen),
    /// Goes back to the screen underneath.
    Pop,
}

/// The screen on display, over the ones it was opened from.
pub struct Nav {
    current: Screen,
    below: Vec<Screen>,
}

impl Nav {
    pub const fn new(current: Screen) -> Self {
        Self {
            current,
            below: Vec::new(),
        }
    }

    pub const fn current(&self) -> &Screen {
        &self.current
    }

    pub const fn current_mut(&mut self) -> &mut Screen {
        &mut self.current
    }

    pub fn apply(&mut self, transition: Transition) {
        match transition {
            Transition::Stay => {}
            Transition::To(screen) => {
                self.below.clear();
                self.current = screen;
            }
            Transition::Push(screen) => self.below.push(std::mem::replace(&mut self.current, screen)),
            Transition::Pop => self.current = self.below.pop().unwrap_or(Screen::Menu),
        }
    }

    /// Drops what belonged to an account that is gone.
    pub fn forget_account(&mut self) {
        self.below.retain(|screen| !screen.needs_account());
        if self.current.needs_account() {
            self.apply(Transition::To(Screen::Menu));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn is_menu(nav: &Nav) -> bool {
        matches!(nav.current, Screen::Menu)
    }

    #[test]
    fn a_pushed_screen_goes_back_to_the_one_under_it() {
        let mut nav = Nav::new(Screen::Menu);
        nav.apply(Transition::Push(Screen::Settings));
        assert!(matches!(nav.current, Screen::Settings));
        nav.apply(Transition::Pop);
        assert!(is_menu(&nav));
    }

    #[test]
    fn a_lost_account_leaves_no_screen_of_it_behind() {
        let mut nav = Nav::new(Screen::Menu);
        nav.apply(Transition::Push(Screen::Ranked));
        nav.apply(Transition::Push(Screen::Settings));
        nav.forget_account();
        assert!(matches!(nav.current, Screen::Settings));
        nav.apply(Transition::Pop);
        assert!(is_menu(&nav));
        nav.apply(Transition::Push(Screen::Ranked));
        nav.forget_account();
        assert!(is_menu(&nav));
    }

    #[test]
    fn going_to_a_screen_forgets_those_under_it() {
        let mut nav = Nav::new(Screen::Settings);
        nav.apply(Transition::Push(Screen::PlayMenu));
        nav.apply(Transition::To(Screen::Title));
        nav.apply(Transition::Pop);
        assert!(is_menu(&nav), "nothing left to go back to");
    }
}
