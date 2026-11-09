# Nostalgia in Rust

A rewrite of the server and the client in Rust. Both are drop-in replacements: the Rust and the
Node.js / Electron versions can be mixed in any combination and share their data.

| Crate | What | Replaces |
| --- | --- | --- |
| `shared` | DTOs, platforms, emu.yaml validation, launch helpers | `shared/` |
| `server` | axum + SQLite, admin web UI embedded | `server/` |
| `client` | GPUI desktop client (gpui-kit), controller navigation (gilrs) | `client/` |

## Compatibility

- **Server**: reads the same `settings.yaml` (incl. the old `dataDirectory` key and the
  `NOSTALGIA_*` / `IGDB_*` environment variables), uses the same SQLite schema, save folder layout,
  password hashes (scrypt) and session tokens, and answers every `/api` and `/admin/api` request
  with the same JSON as the Node.js server. The admin UI is `server/admin`, embedded at build time.
- **Client**: same data folder as the Electron client (`%APPDATA%\@nostalgia\client`,
  `~/.config/@nostalgia/client`, a `nostalgia-data` folder next to the executable when a `portable`
  file exists, or `NOSTALGIA_DATA_DIR`) and the same JSON files; unknown fields are kept. Cloud save
  snapshots use the same zip layout and change fingerprint, so switching clients does not create
  extra save versions. UI-only preferences (vault filters, last server) live in `ui.json`.

## Build and run

```sh
cd rust
cargo test -p nostalgia-shared -p nostalgia-server
cargo run -p nostalgia-server -- --settings ../server/settings.yaml
cargo run -p nostalgia            # client; --fullscreen / --windowed
cargo build --release             # target/release/nostalgia-server, target/release/nostalgia
```

Docker (server): `docker build -f rust/Dockerfile -t nostalgia-server-rs .` from the repository root.

Linux needs the GPUI build dependencies for the client (Wayland / X11, Vulkan, fontconfig) and
libudev for controllers; see `.github/workflows/rust.yml`.

## Controls (client)

Same as the Electron client: D-pad / left stick / arrow keys move, A / Enter select, B / Esc back,
LB / RB or Q / E switch tabs, Y / F search, Start opens the settings. Ctrl + / − / 0 change the UI
scale, F11 toggles fullscreen.
