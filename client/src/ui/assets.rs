//! Embedded icons (the same SVGs as the Electron UI), with gpui-kit's component icons as fallback.

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

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ICONS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = ICONS.iter().map(|(p, _)| *p).filter(|p| p.starts_with(path)).map(Into::into).collect();
        out.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(out)
    }
}

pub fn icon_path(name: &str) -> SharedString {
    format!("nostalgia/icons/{name}.svg").into()
}
