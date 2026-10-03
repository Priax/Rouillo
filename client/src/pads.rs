use std::collections::HashSet;

use gilrs::{Axis, Button, Gilrs};
use serde::{Deserialize, Serialize};

const STICK_DEAD_ZONE: f32 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Pad {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Start,
    Select,
}

impl Pad {
    /// The buttons a player may bind; Start and Select pause and restart.
    pub const BINDABLE: [Self; 12] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftBumper,
        Self::RightBumper,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::DPadUp,
        Self::DPadDown,
        Self::DPadLeft,
        Self::DPadRight,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::South => "A",
            Self::East => "B",
            Self::West => "X",
            Self::North => "Y",
            Self::LeftBumper => "LB",
            Self::RightBumper => "RB",
            Self::LeftTrigger => "LT",
            Self::RightTrigger => "RT",
            Self::DPadUp => "Croix haut",
            Self::DPadDown => "Croix bas",
            Self::DPadLeft => "Croix gauche",
            Self::DPadRight => "Croix droite",
            Self::Start => "Start",
            Self::Select => "Select",
        }
    }

    const fn button(self) -> Button {
        match self {
            Self::South => Button::South,
            Self::East => Button::East,
            Self::West => Button::West,
            Self::North => Button::North,
            Self::LeftBumper => Button::LeftTrigger,
            Self::RightBumper => Button::RightTrigger,
            Self::LeftTrigger => Button::LeftTrigger2,
            Self::RightTrigger => Button::RightTrigger2,
            Self::DPadUp => Button::DPadUp,
            Self::DPadDown => Button::DPadDown,
            Self::DPadLeft => Button::DPadLeft,
            Self::DPadRight => Button::DPadRight,
            Self::Start => Button::Start,
            Self::Select => Button::Select,
        }
    }
}

/// Every connected gamepad merged into one: what any of them holds is held.
#[derive(Default)]
pub struct Pads {
    gilrs: Option<Gilrs>,
    held: HashSet<Pad>,
    pressed: HashSet<Pad>,
}

impl Pads {
    pub fn new() -> Self {
        let gilrs = Gilrs::new()
            .map_err(|e| eprintln!("[pads] manettes indisponibles: {e}"))
            .ok();
        Self {
            gilrs,
            ..Self::default()
        }
    }

    pub fn update(&mut self) {
        let Some(gilrs) = self.gilrs.as_mut() else {
            return;
        };
        while gilrs.next_event().is_some() {}
        let mut held = HashSet::new();
        for (_, pad) in gilrs.gamepads() {
            for p in Pad::BINDABLE.into_iter().chain([Pad::Start, Pad::Select]) {
                if pad.is_pressed(p.button()) {
                    held.insert(p);
                }
            }
            let (x, y) = (pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY));
            if x <= -STICK_DEAD_ZONE {
                held.insert(Pad::DPadLeft);
            } else if x >= STICK_DEAD_ZONE {
                held.insert(Pad::DPadRight);
            }
            if y <= -STICK_DEAD_ZONE {
                held.insert(Pad::DPadDown);
            } else if y >= STICK_DEAD_ZONE {
                held.insert(Pad::DPadUp);
            }
        }
        self.pressed = held.difference(&self.held).copied().collect();
        self.held = held;
    }

    pub fn held(&self, pad: Pad) -> bool {
        self.held.contains(&pad)
    }

    pub fn pressed(&self, pad: Pad) -> bool {
        self.pressed.contains(&pad)
    }

    /// The bindable button pressed this frame, for a binding being chosen.
    pub fn any_pressed(&self) -> Option<Pad> {
        Pad::BINDABLE.into_iter().find(|p| self.pressed(*p))
    }
}
