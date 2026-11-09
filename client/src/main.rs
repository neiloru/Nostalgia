//! Nostalgia client (Rust + GPUI). Uses the same data folder and server protocol as the Electron
//! client, so either can be used with the same installs, settings and cloud saves.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod client;
mod downloads;
mod launcher;
mod model;
mod savesync;
mod store;
mod ui;
mod util;
mod util_url;

/// GPUI renders in physical pixels and expects per-monitor DPI awareness (normally from the
/// application manifest); without it windows are created at the wrong size on scaled displays.
#[cfg(windows)]
fn dpi_awareness() {
    unsafe extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }
    const PER_MONITOR_AWARE_V2: isize = -4;
    unsafe { SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) };
}

fn main() {
    #[cfg(windows)]
    dpi_awareness();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("nostalgia {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(4).thread_name("nostalgia-io").build().expect("tokio runtime");
    let paths = client::Paths::detect();
    // One instance per data folder (two would overwrite each other's files), like the Electron client.
    let _ = std::fs::create_dir_all(&paths.user_data);
    let _instance = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(paths.user_data.join("instance.lock")) {
        Ok(f) => match f.try_lock() {
            Ok(()) => Some(f),
            Err(_) => {
                eprintln!("Nostalgia is already running ({})", paths.user_data.display());
                return;
            }
        },
        Err(_) => None,
    };
    let (ui_tx, ui_rx) = futures_channel::mpsc::unbounded();
    let client = client::Client::new(runtime.handle().clone(), paths, ui_tx);
    let fullscreen = args.iter().any(|a| a == "--fullscreen") || (!args.iter().any(|a| a == "--windowed") && client.settings().start_fullscreen);
    client.start();
    ui::run(client.clone(), ui_rx, fullscreen);
    client.flush();
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
}
