//! The app icon where the operating system does not take it from the package:
//!
//! - Windows: embedded in nostalgia.exe (build.rs, resource 1), GPUI uses it for the window.
//! - macOS: Nostalgia.app has Nostalgia.icns; started outside a bundle (cargo run) the Dock icon is
//!   set at runtime.
//! - Linux: X11 takes the window icon from the window (_NET_WM_ICON). Wayland has no window icons:
//!   the compositor looks up the desktop entry named after the app id ("nostalgia"), so release
//!   builds install nostalgia.desktop and the icon into the user's data folder
//!   (~/.local/share/applications, ~/.local/share/icons), pointing at the running executable.

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux {
    use std::path::Path;
    use std::sync::Arc;

    pub const APP_ID: &str = "nostalgia";
    const ICON_PNG: &[u8] = include_bytes!("../assets/app/icons/256x256.png");

    /// Window icon for X11.
    pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
        let small = include_bytes!("../assets/app/icons/128x128.png");
        image::load_from_memory_with_format(small, image::ImageFormat::Png).ok().map(|i| Arc::new(i.into_rgba8()))
    }

    /// Desktop entry and icon for the app id (Wayland docks and task switchers, application menus).
    /// Rewritten only when the content changed (e.g. the executable was moved).
    pub fn install_desktop_entry() {
        // Development builds would register target/debug; sandboxed packages bring their own entry.
        if cfg!(debug_assertions) || std::env::var_os("FLATPAK_ID").is_some() || std::env::var_os("SNAP").is_some() {
            return;
        }
        let (Some(data), Ok(exe)) = (dirs::data_dir(), std::env::current_exe()) else { return };
        // An AppImage runs from a temporary mount; the image file itself is what has to be started.
        let exe = std::env::var_os("APPIMAGE").map(Into::into).unwrap_or(exe);
        let icon = data.join("icons/hicolor/256x256/apps").join(format!("{APP_ID}.png"));
        write_if_changed(&icon, ICON_PNG);
        let entry = desktop_entry(&exe.to_string_lossy());
        write_if_changed(&data.join("applications").join(format!("{APP_ID}.desktop")), entry.as_bytes());
    }

    fn desktop_entry(exe: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName=Nostalgia\nComment=Retro game library and launcher\nExec={}\nIcon={APP_ID}\nStartupWMClass={APP_ID}\nCategories=Game;\nTerminal=false\n",
            exec_quote(exe)
        )
    }

    /// Exec key quoting of the Desktop Entry spec: the argument in double quotes with `"`, `` ` ``,
    /// `$` and `\` backslash-escaped, then backslashes escaped again for the string value.
    fn exec_quote(arg: &str) -> String {
        let mut quoted = String::from("\"");
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted.replace('\\', "\\\\").replace('%', "%%")
    }

    fn write_if_changed(path: &Path, content: &[u8]) {
        if std::fs::read(path).ok().as_deref() == Some(content) {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, content);
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn quotes_the_exec_path() {
            assert_eq!(super::exec_quote("/opt/My Games/nostalgia"), "\"/opt/My Games/nostalgia\"");
            assert_eq!(super::exec_quote("/a/$b/100%"), "\"/a/\\\\$b/100%%\"");
        }

        #[test]
        fn decodes_the_window_icon() {
            let icon = super::window_icon().expect("icon");
            assert_eq!((icon.width(), icon.height()), (128, 128));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub use linux::{install_desktop_entry, window_icon};

/// Dock icon when the client runs outside Nostalgia.app (the bundle's icon is used inside it).
/// Call after the application has started (on the main thread).
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let bundled = std::env::current_exe().map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/")).unwrap_or(false);
    if bundled {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(include_bytes!("../assets/app/icons/512x512.png"));
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else { return };
    let app = NSApplication::sharedApplication(mtm);
    unsafe { app.setApplicationIconImage(Some(&image)) };
}
