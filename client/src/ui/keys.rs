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
    pub left: KeyRepeat,
    pub right: KeyRepeat,
    pub up: KeyRepeat,
    pub down: KeyRepeat,
}

impl EditKeys {
    pub fn update(&mut self, keyboard: &Keyboard, dt: f32) {
        let keys = [
            (&mut self.erase, KeyCode::Backspace),
            (&mut self.left, KeyCode::ArrowLeft),
            (&mut self.right, KeyCode::ArrowRight),
            (&mut self.up, KeyCode::ArrowUp),
            (&mut self.down, KeyCode::ArrowDown),
        ];
        for (key, code) in keys {
            key.update(keyboard.is_down(code), dt);
        }
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
