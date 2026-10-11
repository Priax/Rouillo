use notan::prelude::*;
use serde::{Deserialize, Serialize};

use crate::pads::{Pad, Pads};
use crate::storage;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Left,
    Right,
    SoftDrop,
    HardDrop,
    RotateCw,
    RotateCcw,
    Strategy,
    NextTarget,
}

pub const ACTIONS: usize = 8;

impl Action {
    pub const ALL: [Self; ACTIONS] = [
        Self::Left,
        Self::Right,
        Self::SoftDrop,
        Self::HardDrop,
        Self::RotateCw,
        Self::RotateCcw,
        Self::Strategy,
        Self::NextTarget,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "Gauche",
            Self::Right => "Droite",
            Self::SoftDrop => "Descendre",
            Self::HardDrop => "Poser",
            Self::RotateCw => "Tourner (horaire)",
            Self::RotateCcw => "Tourner (anti-horaire)",
            Self::Strategy => "Changer de stratégie",
            Self::NextTarget => "Cible suivante",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Keys that keep their meaning everywhere: Escape pauses or goes back, and
/// both erasing keys clear a binding while one is being chosen.
pub const RESERVED: [KeyCode; 3] = [KeyCode::Escape, KeyCode::Backspace, KeyCode::Delete];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub label: String,
}

impl KeyBinding {
    pub fn new(code: KeyCode) -> Self {
        Self {
            code,
            label: key_label(code),
        }
    }
}

pub fn key_label(code: KeyCode) -> String {
    let name = format!("{code:?}");
    let fixed = match code {
        KeyCode::ArrowLeft => "Flèche gauche",
        KeyCode::ArrowRight => "Flèche droite",
        KeyCode::ArrowUp => "Flèche haut",
        KeyCode::ArrowDown => "Flèche bas",
        KeyCode::Space => "Espace",
        KeyCode::Enter => "Entrée",
        KeyCode::NumpadEnter => "Entrée (pavé)",
        KeyCode::ShiftLeft => "Maj gauche",
        KeyCode::ShiftRight => "Maj droite",
        KeyCode::ControlLeft => "Ctrl gauche",
        KeyCode::ControlRight => "Ctrl droite",
        KeyCode::AltLeft => "Alt",
        KeyCode::AltRight => "Alt Gr",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_owned();
    }
    if let Some(rest) = name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")) {
        return rest.to_owned();
    }
    if let Some(rest) = name.strip_prefix("Numpad") {
        return format!("Pavé {rest}");
    }
    name
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bindings {
    pub keys: [[Option<KeyBinding>; 2]; ACTIONS],
    pub pads: [Option<Pad>; ACTIONS],
}

impl Default for Bindings {
    fn default() -> Self {
        let key = |code| Some(KeyBinding::new(code));
        Self {
            keys: [
                [key(KeyCode::ArrowLeft), None],
                [key(KeyCode::ArrowRight), None],
                [key(KeyCode::ArrowDown), None],
                [key(KeyCode::Space), key(KeyCode::Enter)],
                [key(KeyCode::ArrowUp), key(KeyCode::KeyZ)],
                [key(KeyCode::KeyX), key(KeyCode::KeyW)],
                [key(KeyCode::KeyC), None],
                [key(KeyCode::KeyV), None],
            ],
            pads: [
                Some(Pad::DPadLeft),
                Some(Pad::DPadRight),
                Some(Pad::DPadDown),
                Some(Pad::DPadUp),
                Some(Pad::South),
                Some(Pad::East),
                Some(Pad::North),
                Some(Pad::RightBumper),
            ],
        }
    }
}

const STORAGE_KEY: &str = "rouillo_bindings";

#[derive(Deserialize)]
struct Stored {
    keys: Vec<[Option<KeyBinding>; 2]>,
    pads: Vec<Option<Pad>>,
}

impl Bindings {
    pub fn load() -> Self {
        storage::get(STORAGE_KEY)
            .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
            .map_or_else(Self::default, Self::from_stored)
    }

    /// Saved bindings from before an action existed give it its default
    /// key and button, unless the player already uses them for something.
    fn from_stored(stored: Stored) -> Self {
        let defaults = Self::default();
        let known = stored.keys.len();
        let mut keys = stored.keys.into_iter();
        let mut pads = stored.pads.into_iter();
        let mut out = Self {
            keys: std::array::from_fn(|_| keys.next().unwrap_or_default()),
            pads: std::array::from_fn(|_| pads.next().flatten()),
        };
        for action in Action::ALL.into_iter().skip(known) {
            let i = action.index();
            let used = |code: KeyCode, b: &Self| b.keys.iter().flatten().flatten().any(|k| k.code == code);
            for (slot, key) in defaults.keys[i].iter().enumerate() {
                if let Some(key) = key.as_ref().filter(|k| !used(k.code, &out)) {
                    out.keys[i][slot] = Some(key.clone());
                }
            }
            out.pads[i] = defaults.pads[i].filter(|p| !out.pads.contains(&Some(*p)));
        }
        out
    }

    pub fn save(&self) {
        if let Ok(text) = serde_json::to_string(self) {
            storage::set(STORAGE_KEY, &text);
        }
    }

    pub fn key(&self, action: Action, slot: usize) -> Option<&KeyBinding> {
        self.keys[action.index()][slot].as_ref()
    }

    pub fn pad(&self, action: Action) -> Option<Pad> {
        self.pads[action.index()]
    }

    /// Binds `code` to one slot. A key does one thing only, so it leaves
    /// any other slot that held it.
    pub fn set_key(&mut self, action: Action, slot: usize, binding: Option<KeyBinding>) {
        if let Some(b) = &binding {
            for other in self.keys.iter_mut().flatten() {
                if other.as_ref().is_some_and(|o| o.code == b.code) {
                    *other = None;
                }
            }
        }
        self.keys[action.index()][slot] = binding;
    }

    pub fn set_pad(&mut self, action: Action, pad: Option<Pad>) {
        if pad.is_some() {
            for other in &mut self.pads {
                if *other == pad {
                    *other = None;
                }
            }
        }
        self.pads[action.index()] = pad;
    }

    /// Changes the label of the key in that slot, when it is `code`: the text
    /// the keyboard typed names the key better than its position does.
    pub fn relabel(&mut self, action: Action, slot: usize, code: KeyCode, label: &str) {
        if let Some(b) = self.keys[action.index()][slot].as_mut().filter(|b| b.code == code) {
            label.clone_into(&mut b.label);
        }
    }
}

/// What the player asked for this frame, from every input at once.
#[derive(Default, Clone, Copy, Debug)]
pub struct Frame {
    held: [bool; ACTIONS],
    pressed: [bool; ACTIONS],
    pub pause: bool,
    pub restart: bool,
}

impl Frame {
    pub fn held(&self, action: Action) -> bool {
        self.held[action.index()]
    }

    pub fn pressed(&self, action: Action) -> bool {
        self.pressed[action.index()]
    }

    #[cfg(test)]
    pub fn pressing(action: Action) -> Self {
        let mut frame = Self::default();
        frame.pressed[action.index()] = true;
        frame
    }
}

#[derive(Default)]
pub struct Controls {
    pub bindings: Bindings,
    prev_held: [bool; ACTIONS],
    pub frame: Frame,
}

impl Controls {
    pub fn load() -> Self {
        Self {
            bindings: Bindings::load(),
            ..Self::default()
        }
    }

    pub fn update(&mut self, keyboard: &Keyboard, pads: &Pads, touch: [bool; ACTIONS], touch_pause: bool) {
        let mut frame = Frame {
            pause: keyboard.was_pressed(KeyCode::Escape) || pads.pressed(Pad::Start) || touch_pause,
            restart: keyboard.was_pressed(KeyCode::KeyR) || pads.pressed(Pad::Select),
            ..Frame::default()
        };
        for action in Action::ALL {
            let i = action.index();
            let keys = self.bindings.keys[i].iter().flatten();
            let mut typed = false;
            let mut down = touch[i];
            for b in keys {
                typed |= keyboard.was_pressed(b.code);
                down |= keyboard.is_down(b.code);
            }
            if let Some(pad) = self.bindings.pads[i] {
                down |= pads.held(pad);
            }
            frame.held[i] = down;
            frame.pressed[i] = typed || (down && !self.prev_held[i]);
        }
        self.prev_held = frame.held;
        self.frame = frame;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_moved_to_another_action_leaves_the_first() {
        let mut b = Bindings::default();
        b.set_key(Action::HardDrop, 0, Some(KeyBinding::new(KeyCode::ArrowUp)));
        assert_eq!(b.key(Action::HardDrop, 0).map(|k| k.code), Some(KeyCode::ArrowUp));
        assert_eq!(b.key(Action::RotateCw, 0), None, "ArrowUp still rotated");
        assert_eq!(b.key(Action::RotateCw, 1).map(|k| k.code), Some(KeyCode::KeyZ));
    }

    #[test]
    fn bindings_saved_before_the_targeting_actions_keep_working() {
        let mut old = Bindings::default();
        old.set_key(Action::RotateCw, 1, Some(KeyBinding::new(KeyCode::KeyC)));
        let text = serde_json::to_string(&old).expect("json");
        let mut value: serde_json::Value = serde_json::from_str(&text).expect("value");
        for field in ["keys", "pads"] {
            value[field].as_array_mut().expect("an array").truncate(6);
        }
        let b = Bindings::from_stored(serde_json::from_value(value).expect("stored"));
        assert_eq!(b.key(Action::RotateCw, 1).map(|k| k.code), Some(KeyCode::KeyC));
        assert_eq!(b.key(Action::Strategy, 0), None, "C was taken already");
        assert_eq!(b.key(Action::NextTarget, 0).map(|k| k.code), Some(KeyCode::KeyV));
        assert_eq!(b.pad(Action::Strategy), Some(Pad::North));
    }

    #[test]
    fn a_pad_button_does_one_thing() {
        let mut b = Bindings::default();
        b.set_pad(Action::RotateCcw, Some(Pad::South));
        assert_eq!(b.pad(Action::RotateCcw), Some(Pad::South));
        assert_eq!(b.pad(Action::RotateCw), None);
    }

    #[test]
    fn bindings_survive_a_save() {
        let mut b = Bindings::default();
        b.set_key(Action::Left, 1, Some(KeyBinding::new(KeyCode::KeyA)));
        b.relabel(Action::Left, 1, KeyCode::KeyA, "Q");
        b.save();
        assert_eq!(Bindings::load(), b);
        assert_eq!(b.key(Action::Left, 1).map(|k| k.label.as_str()), Some("Q"));
    }

    #[test]
    fn a_relabel_for_another_key_is_ignored() {
        let mut b = Bindings::default();
        b.relabel(Action::Left, 0, KeyCode::KeyQ, "Q");
        assert_eq!(b.key(Action::Left, 0).map(|k| k.label.as_str()), Some("Flèche gauche"));
    }

    #[test]
    fn keys_are_named_for_people() {
        assert_eq!(key_label(KeyCode::KeyZ), "Z");
        assert_eq!(key_label(KeyCode::Digit4), "4");
        assert_eq!(key_label(KeyCode::Numpad4), "Pavé 4");
        assert_eq!(key_label(KeyCode::Space), "Espace");
        assert_eq!(key_label(KeyCode::F5), "F5");
    }
}
