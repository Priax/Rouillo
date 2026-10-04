use notan::prelude::{KeyCode, Keyboard};

const REPEAT_DELAY: f32 = 0.4;
const REPEAT_EVERY: f32 = 0.035;

#[derive(Default)]
pub struct KeyRepeat {
    held: Option<f32>,
    fired: bool,
}

impl KeyRepeat {
    pub fn update(&mut self, down: bool, dt: f32) {
        self.fired = match (down, self.held) {
            (false, _) => {
                self.held = None;
                false
            }
            (true, None) => {
                self.held = Some(0.0);
                true
            }
            (true, Some(t)) => {
                self.held = Some(t + dt);
                repeats(t) < repeats(t + dt)
            }
        };
    }

    pub fn fired(&self) -> bool {
        self.fired
    }
}

#[derive(Default)]
pub struct EditKeys {
    pub erase: KeyRepeat,
    pub delete: KeyRepeat,
    pub left: KeyRepeat,
    pub right: KeyRepeat,
    pub up: KeyRepeat,
    pub down: KeyRepeat,
    /// Backspaces this frame: a browser counts them itself, since a phone's
    /// keyboard gives no key code for them.
    pub erases: u32,
    pub home: bool,
    pub end: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub select_all: bool,
    pub copy: bool,
    pub cut: bool,
    pub paste: bool,
    pub undo: bool,
    pub redo: bool,
    #[cfg(not(target_arch = "wasm32"))]
    typed: Shortcuts,
}

#[derive(Default, Clone, Copy)]
pub struct Shortcuts {
    pub select_all: bool,
    pub copy: bool,
    pub cut: bool,
    pub paste: bool,
    pub undo: bool,
    pub redo: bool,
}

impl EditKeys {
    pub fn update(&mut self, keyboard: &Keyboard, dt: f32) {
        let keys = [
            (&mut self.erase, KeyCode::Backspace),
            (&mut self.delete, KeyCode::Delete),
            (&mut self.left, KeyCode::ArrowLeft),
            (&mut self.right, KeyCode::ArrowRight),
            (&mut self.up, KeyCode::ArrowUp),
            (&mut self.down, KeyCode::ArrowDown),
        ];
        for (key, code) in keys {
            key.update(keyboard.is_down(code), dt);
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.erases = crate::web::take_erases();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.erases = u32::from(self.erase.fired());
        }
        self.home = keyboard.was_pressed(KeyCode::Home);
        self.end = keyboard.was_pressed(KeyCode::End);
        let down = |codes: &[KeyCode]| codes.iter().any(|&c| keyboard.is_down(c));
        self.shift = down(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        self.ctrl = down(&[
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ]);
        #[cfg(target_arch = "wasm32")]
        let typed = crate::web::take_shortcuts();
        #[cfg(not(target_arch = "wasm32"))]
        let typed = std::mem::take(&mut self.typed);
        self.select_all = typed.select_all;
        self.copy = typed.copy;
        self.cut = typed.cut;
        self.paste = typed.paste;
        self.undo = typed.undo;
        self.redo = typed.redo;
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// A letter typed with Ctrl held: the letter follows the keyboard layout,
    /// where key codes are positions.
    pub fn shortcut(&mut self, letter: char) {
        match letter.to_ascii_lowercase() {
            'a' => self.typed.select_all = true,
            'c' => self.typed.copy = true,
            'x' => self.typed.cut = true,
            'v' => self.typed.paste = true,
            'z' if self.shift => self.typed.redo = true,
            'z' => self.typed.undo = true,
            'y' => self.typed.redo = true,
            _ => {}
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// The letter of a key typed with Ctrl held. Most systems send it as a
/// control character (Ctrl+A as 1), others as the letter itself; anything
/// else, such as AltGr characters on Windows, is typed text.
pub fn ctrl_letter(c: char) -> Option<char> {
    match c {
        '\u{1}'..='\u{1a}' => char::from_u32(c as u32 - 1 + 'a' as u32),
        c if c.is_ascii_alphabetic() => Some(c),
        _ => None,
    }
}

fn repeats(held: f32) -> u32 {
    if held < REPEAT_DELAY {
        0
    } else {
        ((held - REPEAT_DELAY) / REPEAT_EVERY) as u32 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(key: &mut KeyRepeat, secs: f32) -> usize {
        let dt = 1.0 / 60.0;
        (0..(secs / dt).round() as usize)
            .filter(|_| {
                key.update(true, dt);
                key.fired()
            })
            .count()
    }

    #[test]
    fn ctrl_letters_arrive_as_control_characters_or_letters() {
        assert_eq!(ctrl_letter('\u{1}'), Some('a'));
        assert_eq!(ctrl_letter('\u{1a}'), Some('z'));
        assert_eq!(ctrl_letter('V'), Some('V'));
        assert_eq!(ctrl_letter('@'), None, "AltGr on Windows holds Ctrl too");
        assert_eq!(ctrl_letter('\u{7f}'), None);
    }

    #[test]
    fn a_press_fires_once_then_waits() {
        let mut key = KeyRepeat::default();
        assert_eq!(hold(&mut key, REPEAT_DELAY - 0.05), 1);
    }

    #[test]
    fn holding_repeats_steadily() {
        let mut key = KeyRepeat::default();
        let fired = hold(&mut key, REPEAT_DELAY + 1.0);
        assert!((25..=31).contains(&fired), "{fired} erasures in 1.4s");
    }

    #[test]
    fn releasing_resets_the_delay() {
        let mut key = KeyRepeat::default();
        hold(&mut key, 2.0);
        key.update(false, 1.0 / 60.0);
        assert!(!key.fired());
        assert_eq!(hold(&mut key, REPEAT_DELAY - 0.05), 1);
    }
}
