//! Nostalgia server (Rust). Drop-in replacement for the Node.js server: same settings.yaml,
//! same SQLite database and save folders, same HTTP API and admin web interface.

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod events;
pub mod hasheous;
pub mod http;
pub mod igdb;
pub mod jobs;
pub mod library;
pub mod log;
pub mod routes_admin;
pub mod routes_api;
pub mod saves;
pub mod util;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use std::path::Path;
use std::sync::Arc;

pub use http::AppState;

/// Open everything below the settings file and build the router (no listening socket yet).
pub fn create_server(settings_file: &Path) -> anyhow::Result<(Router, AppState)> {
    let settings = Arc::new(config::load_settings(settings_file)?);
    let db = db::Db::open(&settings.database_path)?;
    let auth = Arc::new(auth::Auth::new(db.clone(), settings.admin_username.clone(), settings.admin_password.clone()));
    let library = library::Library::new(db.clone(), settings.clone());
    let http = reqwest::Client::builder().user_agent(concat!("nostalgia-server/", env!("CARGO_PKG_VERSION"))).build()?;
    let igdb = Arc::new(igdb::Igdb::new(settings.clone(), http.clone()));
    let events = Arc::new(events::EventHub::default());
    let jobs = jobs::Jobs::new(db.clone(), library.clone(), igdb.clone(), events.clone(), http);
    let saves = saves::Saves::new(db.clone(), &settings.saves_path)?;

    match saves.migrate_from(&settings.legacy_saves_path) {
        Ok(Some(m)) => {
            if m.moved > 0 {
                log::info(format!("Moved {} cloud save files from {} to {}", m.moved, settings.legacy_saves_path.display(), saves.root.display()));
            }
            if !m.conflicts.is_empty() {
                log::error(format!(
                    "{} save files exist in both {} and {} with different sizes; old folder kept. First: {}",
                    m.conflicts.len(),
                    settings.legacy_saves_path.display(),
                    saves.root.display(),
                    m.conflicts[0]
                ));
            }
        }
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    let check = saves.check();
    if !check.writable {
        log::error(format!("Cloud save folder {} is not writable: {}", saves.root.display(), check.error.unwrap_or_default()));
    }
    if check.missing > 0 {
        log::error(format!(
            "{} of {} newest cloud saves are missing in {}. Is the drive mounted? If you changed savesDirectory, move the old saves folder there.",
            check.missing,
            check.checked,
            saves.root.display()
        ));
    }

    library.ensure_folders()?;

    let state = AppState { settings, db, auth, library, saves, events, jobs, igdb };
    let router = Router::new()
        .nest("/api", routes_api::router())
        .nest("/admin/api", routes_admin::router())
        .merge(routes_admin::static_router())
        .fallback(|| async { error::HttpError::not_found("Not found") })
        .layer(DefaultBodyLimit::disable())
        .with_state(state.clone());
    Ok((router, state))
}

/// Serve until the process is stopped: initial scan, periodic scans.
pub async fn run(settings_file: &Path) -> anyhow::Result<()> {
    let (router, state) = create_server(settings_file)?;
    let s = state.settings.clone();
    let addr = format!("{}:{}", s.host, s.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let shown = if s.host == "0.0.0.0" { "localhost".to_string() } else { s.host.clone() };
    log::info(format!("Nostalgia server {} listening on http://{shown}:{}", env!("CARGO_PKG_VERSION"), s.port));
    log::info(format!("Admin UI: http://localhost:{}/admin  (library: {})", s.port, s.library_path.display()));
    if s.admin_password == "change-me" {
        log::warn(format!("The admin password is still \"change-me\" - edit {}", s.file.display()));
    }
    if s.igdb_client_id.is_empty() {
        log::warn("IGDB credentials missing - metadata, covers and artwork cannot be downloaded");
    }

    let jobs = state.jobs.clone();
    tokio::spawn(async move {
        jobs.scan(false).await;
        if s.scan_interval_minutes > 0.0 {
            let every = std::time::Duration::from_secs_f64(s.scan_interval_minutes * 60.0);
            let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
            loop {
                tick.tick().await;
                jobs.scan(s.hasheous_on_scheduled_scan).await;
            }
        }
    });

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            log::info("Shutting down");
        })
        .await?;
    Ok(())
}
