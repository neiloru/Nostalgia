//! Embedded icons (the same SVGs as the Electron UI) and controller / keyboard prompts (Kenney
//! Input Prompts, CC0, assets/prompts), with gpui-kit's component icons as fallback.

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[$(
            (concat!("nostalgia/icons/", $name, ".svg"), include_bytes!(concat!("../../assets/icons/", $name, ".svg"))),
        )*];
    };
}

icons!(
    "back", "caret", "check", "chip", "cloud", "download", "expand", "folder", "forward", "gear", "history", "logout", "pause", "play",
    "plus", "refresh", "search", "star", "trash", "wifi-off", "x",
);

macro_rules! prompts {
    ($($name:literal),* $(,)?) => {
        const PROMPTS: &[(&str, &[u8])] = &[$(
            (concat!("nostalgia/prompts/", $name, ".svg"), include_bytes!(concat!("../../assets/prompts/", $name, ".svg"))),
        )*];
    };
}

prompts!(
    "xbox_button_a", "xbox_button_b", "xbox_button_x", "xbox_button_y", "xbox_lb", "xbox_rb", "xbox_button_menu", "xbox_dpad",
    "playstation_button_cross", "playstation_button_circle", "playstation_button_square", "playstation_button_triangle",
    "playstation_trigger_l1", "playstation_trigger_r1", "playstation5_button_options", "playstation_dpad",
    "switch_button_a", "switch_button_b", "switch_button_x", "switch_button_y", "switch_button_l", "switch_button_r", "switch_button_plus", "switch_dpad",
    "steamdeck_button_a", "steamdeck_button_b", "steamdeck_button_x", "steamdeck_button_y", "steamdeck_button_l1", "steamdeck_button_r1",
    "steamdeck_button_options", "steamdeck_dpad",
    "keyboard_enter", "keyboard_escape", "keyboard_f", "keyboard_q", "keyboard_e", "keyboard_arrows",
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ICONS.iter().chain(PROMPTS).find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = ICONS.iter().chain(PROMPTS).map(|(p, _)| *p).filter(|p| p.starts_with(path)).map(Into::into).collect();
        out.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(out)
    }
}

pub fn icon_path(name: &str) -> SharedString {
    format!("nostalgia/icons/{name}.svg").into()
}

pub fn prompt_path(name: &str) -> SharedString {
    format!("nostalgia/prompts/{name}.svg").into()
}
