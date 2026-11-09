//! Game controller input (gilrs) translated to navigation commands, like the Gamepad API loop of
//! the Electron UI: one direction at a time, the stick uses the dominant axis with hysteresis,
//! held directions repeat after 380 ms every 110 ms.

use super::nav::Dir;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadCommand {
    Move(Dir),
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Start,
}

/// Controller family, for the button prompts (buttons are read by position: South is "A" on Xbox,
/// cross on PlayStation and "B" on Nintendo controllers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKind {
    Xbox,
    PlayStation,
    Switch,
    SteamDeck,
}

impl PadKind {
    /// Before any controller input: the Steam Deck inside Steam, else Xbox (the common layout).
    pub fn default_for_system() -> PadKind {
        if std::env::var("SteamDeck").as_deref() == Ok("1") { PadKind::SteamDeck } else { PadKind::Xbox }
    }

    fn detect(vendor: Option<u16>, product: Option<u16>, name: &str) -> PadKind {
        let name = name.to_lowercase();
        let has = |w: &[&str]| w.iter().any(|x| name.contains(x));
        match vendor {
            Some(0x054c) => return PadKind::PlayStation,
            Some(0x057e) => return PadKind::Switch,
            // Valve: the Steam Deck's own controls, or Steam's virtual pad on a Steam Deck.
            Some(0x28de) if product == Some(0x1205) || Self::default_for_system() == PadKind::SteamDeck => return PadKind::SteamDeck,
            _ => {}
        }
        if has(&["dualsense", "dualshock", "playstation", "ps3", "ps4", "ps5"]) {
            PadKind::PlayStation
        } else if has(&["nintendo", "switch", "joy-con", "joycon", "pro controller"]) {
            PadKind::Switch
        } else if has(&["steam deck"]) {
            PadKind::SteamDeck
        } else {
            Self::default_for_system()
        }
    }
}

const REPEAT_DELAY: Duration = Duration::from_millis(380);
const REPEAT_RATE: Duration = Duration::from_millis(110);
const STICK_PRESS: f32 = 0.6;
const STICK_RELEASE: f32 = 0.35;

/// Commands with the family of the controller that sent them.
pub fn start() -> futures_channel::mpsc::UnboundedReceiver<(PadCommand, PadKind)> {
    let (tx, rx) = futures_channel::mpsc::unbounded();
    std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || {
            let Ok(mut gilrs) = gilrs::Gilrs::new() else { return };
            let mut pressed: HashMap<&'static str, (Instant, bool)> = HashMap::new();
            let mut stick: HashMap<gilrs::GamepadId, Dir> = HashMap::new();
            let mut kind = PadKind::default_for_system();
            loop {
                while gilrs.next_event().is_some() {}
                let now = Instant::now();
                let mut active: Vec<&'static str> = Vec::new();
                let mut dir: Option<Dir> = None;
                for (id, pad) in gilrs.gamepads() {
                    use gilrs::{Axis, Button};
                    let b = |x: Button| pad.is_pressed(x);
                    let mut dpad = Vec::new();
                    if b(Button::DPadUp) {
                        dpad.push(Dir::Up);
                    }
                    if b(Button::DPadDown) {
                        dpad.push(Dir::Down);
                    }
                    if b(Button::DPadLeft) {
                        dpad.push(Dir::Left);
                    }
                    if b(Button::DPadRight) {
                        dpad.push(Dir::Right);
                    }
                    let d = if !dpad.is_empty() {
                        let held = dpad.iter().copied().find(|d| pressed.contains_key(dir_name(*d)));
                        held.or(if dpad.len() == 1 { Some(dpad[0]) } else { None })
                    } else {
                        // gilrs: up is positive on the Y axis.
                        let ax = pad.value(Axis::LeftStickX);
                        let ay = -pad.value(Axis::LeftStickY);
                        let mag = (ax * ax + ay * ay).sqrt();
                        let dominant = if ax.abs() > ay.abs() {
                            if ax < 0.0 { Dir::Left } else { Dir::Right }
                        } else if ay < 0.0 {
                            Dir::Up
                        } else {
                            Dir::Down
                        };
                        let prev = stick.get(&id).copied();
                        let d = match prev {
                            Some(p) if mag > STICK_RELEASE && (p == dominant || mag < STICK_PRESS) => Some(p),
                            _ if mag > STICK_PRESS => Some(dominant),
                            _ => None,
                        };
                        match d {
                            Some(x) => stick.insert(id, x),
                            None => stick.remove(&id),
                        };
                        d
                    };
                    let mut used = d.is_some();
                    if let Some(d) = d
                        && (dir.is_none() || pressed.contains_key(dir_name(d)))
                    {
                        dir = Some(d);
                    }
                    for (button, name) in [
                        (Button::South, "a"),
                        (Button::East, "b"),
                        (Button::West, "x"),
                        (Button::North, "y"),
                        (Button::LeftTrigger, "lb"),
                        (Button::RightTrigger, "rb"),
                        (Button::Start, "start"),
                    ] {
                        if b(button) {
                            active.push(name);
                            used = true;
                        }
                    }
                    // The prompts follow the controller that is in use.
                    if used {
                        kind = PadKind::detect(pad.vendor_id(), pad.product_id(), pad.name());
                    }
                }
                if let Some(d) = dir {
                    active.push(dir_name(d));
                }
                for name in &active {
                    let repeatable = Some(*name) == dir.map(dir_name);
                    let fire = match pressed.get_mut(name) {
                        None => {
                            pressed.insert(name, (now, false));
                            true
                        }
                        Some((t, repeated)) if repeatable => {
                            let wait = if *repeated { REPEAT_RATE } else { REPEAT_DELAY };
                            if now.duration_since(*t) >= wait {
                                *t = now;
                                *repeated = true;
                                true
                            } else {
                                false
                            }
                        }
                        Some(_) => false,
                    };
                    if fire && let Some(cmd) = command(name) && tx.unbounded_send((cmd, kind)).is_err() {
                        return;
                    }
                }
                pressed.retain(|k, _| active.contains(k));
                std::thread::sleep(Duration::from_millis(16));
            }
        })
        .ok();
    rx
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::Up => "up",
        Dir::Down => "down",
        Dir::Left => "left",
        Dir::Right => "right",
    }
}

fn command(name: &str) -> Option<PadCommand> {
    Some(match name {
        "up" => PadCommand::Move(Dir::Up),
        "down" => PadCommand::Move(Dir::Down),
        "left" => PadCommand::Move(Dir::Left),
        "right" => PadCommand::Move(Dir::Right),
        "a" => PadCommand::A,
        "b" => PadCommand::B,
        "x" => PadCommand::X,
        "y" => PadCommand::Y,
        "lb" => PadCommand::Lb,
        "rb" => PadCommand::Rb,
        "start" => PadCommand::Start,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::PadKind;

    #[test]
    fn detects_the_controller_family() {
        assert_eq!(PadKind::detect(Some(0x054c), Some(0x0ce6), "DualSense Wireless Controller"), PadKind::PlayStation);
        assert_eq!(PadKind::detect(None, None, "PS4 Controller"), PadKind::PlayStation);
        assert_eq!(PadKind::detect(Some(0x057e), Some(0x2009), "Pro Controller"), PadKind::Switch);
        assert_eq!(PadKind::detect(Some(0x28de), Some(0x1205), "Steam Deck"), PadKind::SteamDeck);
        if std::env::var("SteamDeck").is_err() {
            assert_eq!(PadKind::detect(Some(0x045e), Some(0x0b12), "Xbox Series X Controller"), PadKind::Xbox);
            assert_eq!(PadKind::detect(Some(0x28de), Some(0x11ff), "Steam Virtual Gamepad"), PadKind::Xbox);
        }
    }
}
