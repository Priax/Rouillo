use std::collections::HashSet;

use aero::input::{Input, PadAxis, PadButton};
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

    const fn button(self) -> PadButton {
        match self {
            Self::South => PadButton::South,
            Self::East => PadButton::East,
            Self::West => PadButton::West,
            Self::North => PadButton::North,
            Self::LeftBumper => PadButton::LeftBumper,
            Self::RightBumper => PadButton::RightBumper,
            Self::LeftTrigger => PadButton::LeftTrigger,
            Self::RightTrigger => PadButton::RightTrigger,
            Self::DPadUp => PadButton::DPadUp,
            Self::DPadDown => PadButton::DPadDown,
            Self::DPadLeft => PadButton::DPadLeft,
            Self::DPadRight => PadButton::DPadRight,
            Self::Start => PadButton::Start,
            Self::Select => PadButton::Select,
        }
    }
}

/// Every connected gamepad merged into one, as Aero reads them: what any of them holds is held, and the left stick
/// counts as the D-pad.
#[derive(Default)]
pub struct Pads {
    held: HashSet<Pad>,
    pressed: HashSet<Pad>,
}

impl Pads {
    pub fn update(&mut self, input: &Input) {
        let all = Pad::BINDABLE.into_iter().chain([Pad::Start, Pad::Select]);
        let mut held: HashSet<Pad> = all.clone().filter(|p| input.pad_held(p.button())).collect();
        let (x, y) = (input.pad_axis(PadAxis::LeftStickX), input.pad_axis(PadAxis::LeftStickY));
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
        let tapped = all.filter(|p| input.pad_pressed(p.button()));
        self.pressed = held.difference(&self.held).copied().chain(tapped).collect();
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
