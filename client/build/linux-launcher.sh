#!/usr/bin/env bash
# Nostalgia launcher for Linux / SteamOS (installed by scripts/after-pack.cjs in front of the
# Electron binary, which is renamed to nostalgia-bin).
#
# Started by Steam (non-Steam game, Game Mode / gamescope) Chromium crashes on startup:
#   - its sandbox cannot be set up inside Steam's launch environment ("FATAL ... sandbox"),
#   - Steam's overlay (LD_PRELOAD gameoverlayrenderer.so) is injected into every Chromium process,
#   - gamescope only offers X11 (Xwayland) to applications.
# So under Steam/gamescope: no sandbox, no injected overlay, X11. The original LD_PRELOAD is
# handed back to the emulators (NOSTALGIA_LD_PRELOAD). Output goes to a log file when there is
# no terminal: ~/.config/Nostalgia/logs/launch.log

here="$(dirname "$(readlink -f "$0")")"
bin="$here/nostalgia-bin"
extra=()

under_steam=0
if [ -n "${SteamGameId:-}${SteamAppId:-}${SteamOverlayGameId:-}${SteamEnv:-}" ]; then under_steam=1; fi
under_gamescope=0
if [ "${XDG_CURRENT_DESKTOP:-}" = gamescope ] || [ -n "${GAMESCOPE_WAYLAND_DISPLAY:-}" ]; then under_gamescope=1; fi

if [ "$under_steam" = 1 ] || [ "$under_gamescope" = 1 ] || [ -n "${NOSTALGIA_NO_SANDBOX:-}" ]; then
  extra+=(--no-sandbox)
fi
if [ "$under_steam" = 1 ] && [ -n "${LD_PRELOAD:-}" ]; then
  export NOSTALGIA_LD_PRELOAD="$LD_PRELOAD"
  unset LD_PRELOAD
fi
if [ "$under_gamescope" = 1 ]; then
  extra+=(--ozone-platform=x11)
fi

if [ -t 2 ]; then
  exec "$bin" "${extra[@]}" "$@"
fi

log_dir="${XDG_CONFIG_HOME:-$HOME/.config}/Nostalgia/logs"
log="$log_dir/launch.log"
if mkdir -p "$log_dir" 2>/dev/null; then
  # Keep the previous run, start a fresh log.
  [ -f "$log" ] && mv -f "$log" "$log_dir/launch.previous.log"
  {
    echo "=== $(date '+%Y-%m-%d %H:%M:%S') Nostalgia start"
    echo "steam=$under_steam gamescope=$under_gamescope desktop=${XDG_CURRENT_DESKTOP:-} display=${DISPLAY:-} wayland=${WAYLAND_DISPLAY:-}"
    echo "args: ${extra[*]} $*"
  } >"$log"
  exec "$bin" "${extra[@]}" "$@" >>"$log" 2>&1
fi
exec "$bin" "${extra[@]}" "$@"
