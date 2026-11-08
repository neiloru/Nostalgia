//! Design tokens of the Electron UI (styles.css): flat, neutral, controller first.
//! All sizes are in rems (16 px = 1 rem at 100 %), so the UI scale is one `set_rem_size` call.

use gpui_kit::{Hsla, Rems, rems, rgb, rgba};

/// CSS pixels at 100 % → rems.
pub fn r(px: f32) -> Rems {
    rems(px / 16.0)
}

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn ca(hex_rgba: u32) -> Hsla {
    rgba(hex_rgba).into()
}

pub fn bg() -> Hsla {
    c(0x0f0f11)
}
pub fn surface() -> Hsla {
    c(0x17171a)
}
pub fn surface2() -> Hsla {
    c(0x1f1f23)
}
pub fn surface3() -> Hsla {
    c(0x29292e)
}
pub fn surface4() -> Hsla {
    c(0x34343a)
}
pub fn line() -> Hsla {
    c(0x26262b)
}
pub fn text() -> Hsla {
    c(0xededef)
}
pub fn text2() -> Hsla {
    c(0xc4c4ca)
}
pub fn muted() -> Hsla {
    c(0x8a8a93)
}
pub fn faint() -> Hsla {
    c(0x5d5d66)
}
pub fn play() -> Hsla {
    c(0x33c27a)
}
pub fn play_ink() -> Hsla {
    c(0x06170e)
}
pub fn err() -> Hsla {
    c(0xf06a6a)
}
pub fn err_soft() -> Hsla {
    c(0xff9b9b)
}
pub fn info() -> Hsla {
    c(0x4c9df0)
}
pub fn warn() -> Hsla {
    c(0xe6b04a)
}
pub fn focus() -> Hsla {
    c(0xffffff)
}

pub const RADIUS: f32 = 8.0;
pub const RING: f32 = 3.0;

#[cfg(windows)]
pub const FONT: &str = "Segoe UI";
#[cfg(not(windows))]
pub const FONT: &str = "Noto Sans";

#[cfg(windows)]
pub const MONO: &str = "Cascadia Code";
#[cfg(not(windows))]
pub const MONO: &str = "DejaVu Sans Mono";
