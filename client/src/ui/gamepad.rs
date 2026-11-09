//! Game controller input (gilrs) translated to navigation commands, like the Gamepad API loop of
//! the Electron UI: the stick uses the dominant axis with hysteresis, the last pressed direction
//! repeats after 380 ms every 110 ms while it is held.
//!
//! Commands come from gilrs' press / axis events, not from polling the button state: a short tap
//! whose press and release arrive between two polls (Windows.Gaming.Input delivers them in bursts,
//! e.g. right after the window got the focus) would otherwise be lost.

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
const POLL: Duration = Duration::from_millis(16);

/// Input of one controller, already mapped by gilrs.
#[derive(Debug, Clone, Copy)]
enum Input {
    Button(gilrs::Button, bool),
    Axis(gilrs::Axis, f32),
    Disconnected,
}

#[derive(Default)]
struct PadState {
    /// D-pad buttons held (Up, Down, Left, Right).
    dpad: [bool; 4],
    stick: (f32, f32),
    stick_dir: Option<Dir>,
}

/// Turns controller input into commands. Pure state machine (no gilrs handles), so it is tested
/// without a controller.
#[derive(Default)]
struct Translator {
    pads: HashMap<usize, PadState>,
    /// The direction that repeats while held: when it started or last fired, and whether it
    /// already repeated.
    repeat: Option<(Dir, Instant, bool)>,
}

fn dir_index(d: Dir) -> usize {
    match d {
        Dir::Up => 0,
        Dir::Down => 1,
        Dir::Left => 2,
        Dir::Right => 3,
    }
}

impl Translator {
    fn input(&mut self, pad: usize, input: Input, now: Instant) -> Option<PadCommand> {
        use gilrs::{Axis, Button};
        if let Input::Disconnected = input {
            self.pads.remove(&pad);
            return None;
        }
        let state = self.pads.entry(pad).or_default();
        match input {
            Input::Button(button, down) => {
                let dir = match button {
                    Button::DPadUp => Some(Dir::Up),
                    Button::DPadDown => Some(Dir::Down),
                    Button::DPadLeft => Some(Dir::Left),
                    Button::DPadRight => Some(Dir::Right),
                    _ => None,
                };
                if let Some(d) = dir {
                    state.dpad[dir_index(d)] = down;
                    return down.then(|| self.start_move(d, now));
                }
                if !down {
                    return None;
                }
                Some(match button {
                    Button::South => PadCommand::A,
                    Button::East => PadCommand::B,
                    Button::West => PadCommand::X,
                    Button::North => PadCommand::Y,
                    // gilrs: LeftTrigger is the shoulder button (LB / L1), LeftTrigger2 the trigger.
                    Button::LeftTrigger => PadCommand::Lb,
                    Button::RightTrigger => PadCommand::Rb,
                    Button::Start => PadCommand::Start,
                    _ => return None,
                })
            }
            Input::Axis(axis @ (Axis::LeftStickX | Axis::LeftStickY), value) => {
                if axis == Axis::LeftStickX {
                    state.stick.0 = value;
                } else {
                    // gilrs: up is positive on the Y axis.
                    state.stick.1 = -value;
                }
                let (ax, ay) = state.stick;
                let mag = (ax * ax + ay * ay).sqrt();
                let dominant = if ax.abs() > ay.abs() {
                    if ax < 0.0 { Dir::Left } else { Dir::Right }
                } else if ay < 0.0 {
                    Dir::Up
                } else {
                    Dir::Down
                };
                let prev = state.stick_dir;
                let dir = match prev {
                    Some(p) if mag > STICK_RELEASE && (p == dominant || mag < STICK_PRESS) => Some(p),
                    _ if mag > STICK_PRESS => Some(dominant),
                    _ => None,
                };
                state.stick_dir = dir;
                match dir {
                    Some(d) if prev != Some(d) => Some(self.start_move(d, now)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn start_move(&mut self, d: Dir, now: Instant) -> PadCommand {
        self.repeat = Some((d, now, false));
        PadCommand::Move(d)
    }

    fn held(&self, d: Dir) -> bool {
        self.pads.values().any(|p| p.dpad[dir_index(d)] || p.stick_dir == Some(d))
    }

    /// Repeat of the held direction, if it is due.
    fn tick(&mut self, now: Instant) -> Option<PadCommand> {
        let (d, since, repeated) = self.repeat?;
        if !self.held(d) {
            self.repeat = None;
            return None;
        }
        let wait = if repeated { REPEAT_RATE } else { REPEAT_DELAY };
        if now.duration_since(since) < wait {
            return None;
        }
        self.repeat = Some((d, now, true));
        Some(PadCommand::Move(d))
    }
}

/// Commands with the family of the controller that sent them.
pub fn start() -> futures_channel::mpsc::UnboundedReceiver<(PadCommand, PadKind)> {
    let (tx, rx) = futures_channel::mpsc::unbounded();
    std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || {
            let debug = std::env::var("NOSTALGIA_DEBUG").is_ok();
            let mut gilrs = match gilrs::Gilrs::new() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("[gilrs] controllers unavailable: {e}");
                    return;
                }
            };
            if debug {
                for (id, pad) in gilrs.gamepads() {
                    eprintln!("[gilrs] {id:?} present: {} ({:04x?}:{:04x?})", pad.name(), pad.vendor_id(), pad.product_id());
                }
            }
            let mut translator = Translator::default();
            let mut kind = PadKind::default_for_system();
            loop {
                let now = Instant::now();
                let mut commands: Vec<PadCommand> = Vec::new();
                while let Some(ev) = gilrs.next_event() {
                    use gilrs::EventType as E;
                    if debug && !matches!(ev.event, E::ButtonChanged(..)) {
                        let pad = gilrs.gamepad(ev.id);
                        eprintln!("[gilrs] {:?} {:?} ({}, {:04x?}:{:04x?})", ev.id, ev.event, pad.name(), pad.vendor_id(), pad.product_id());
                    }
                    let input = match ev.event {
                        E::ButtonPressed(b, _) => Input::Button(b, true),
                        E::ButtonReleased(b, _) => Input::Button(b, false),
                        E::AxisChanged(a, v, _) => Input::Axis(a, v),
                        E::Disconnected => Input::Disconnected,
                        _ => continue,
                    };
                    if let Some(cmd) = translator.input(ev.id.into(), input, now) {
                        // The prompts follow the controller that is in use.
                        let pad = gilrs.gamepad(ev.id);
                        kind = PadKind::detect(pad.vendor_id(), pad.product_id(), pad.name());
                        commands.push(cmd);
                    }
                }
                commands.extend(translator.tick(now));
                for cmd in commands {
                    if tx.unbounded_send((cmd, kind)).is_err() {
                        return;
                    }
                }
                std::thread::sleep(POLL);
            }
        })
        .ok();
    rx
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

    use super::{Dir, Input, PadCommand as P, Translator, REPEAT_DELAY, REPEAT_RATE};
    use gilrs::{Axis, Button};
    use std::time::{Duration, Instant};

    #[test]
    fn a_tap_counts_even_when_press_and_release_arrive_together() {
        let mut t = Translator::default();
        let now = Instant::now();
        assert_eq!(t.input(0, Input::Button(Button::DPadDown, true), now), Some(P::Move(Dir::Down)));
        assert_eq!(t.input(0, Input::Button(Button::DPadDown, false), now), None);
        assert_eq!(t.input(0, Input::Button(Button::South, true), now), Some(P::A));
        assert_eq!(t.input(0, Input::Button(Button::South, false), now), None);
        assert_eq!(t.tick(now + REPEAT_DELAY * 2), None);
    }

    #[test]
    fn a_held_direction_repeats_until_released() {
        let mut t = Translator::default();
        let t0 = Instant::now();
        assert_eq!(t.input(0, Input::Button(Button::DPadRight, true), t0), Some(P::Move(Dir::Right)));
        assert_eq!(t.tick(t0 + REPEAT_DELAY / 2), None);
        let t1 = t0 + REPEAT_DELAY;
        assert_eq!(t.tick(t1), Some(P::Move(Dir::Right)));
        assert_eq!(t.tick(t1 + REPEAT_RATE / 2), None);
        assert_eq!(t.tick(t1 + REPEAT_RATE), Some(P::Move(Dir::Right)));
        t.input(0, Input::Button(Button::DPadRight, false), t1 + REPEAT_RATE);
        assert_eq!(t.tick(t1 + REPEAT_RATE * 3), None);
    }

    #[test]
    fn the_stick_moves_once_per_push_with_hysteresis() {
        let mut t = Translator::default();
        let now = Instant::now();
        let ms = |n| now + Duration::from_millis(n);
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickY, 0.0), now), None);
        // Up is positive in gilrs.
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickY, 0.9), ms(10)), Some(P::Move(Dir::Up)));
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickY, 0.5), ms(20)), None);
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickY, 0.9), ms(30)), None);
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickY, 0.1), ms(40)), None);
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickX, -0.8), ms(50)), Some(P::Move(Dir::Left)));
    }

    #[test]
    fn the_first_stick_push_counts() {
        // A centred stick sends no event before it is pushed.
        let mut t = Translator::default();
        let now = Instant::now();
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickY, -1.0), now), Some(P::Move(Dir::Down)));
        assert_eq!(t.input(0, Input::Axis(Axis::LeftStickY, -0.98), now), None);
        assert_eq!(t.tick(now + REPEAT_DELAY), Some(P::Move(Dir::Down)));
        t.input(0, Input::Axis(Axis::LeftStickY, 0.0), now + REPEAT_DELAY);
        assert_eq!(t.tick(now + REPEAT_DELAY * 4), None);
    }

    #[test]
    fn a_disconnected_controller_stops_repeating() {
        let mut t = Translator::default();
        let now = Instant::now();
        t.input(3, Input::Button(Button::DPadUp, true), now);
        t.input(3, Input::Disconnected, now);
        assert_eq!(t.tick(now + REPEAT_DELAY), None);
    }
}
