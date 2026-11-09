#!/usr/bin/env bash
# Builds a release package of the server or the client for the current OS into release/:
#
#   scripts/package.sh server   nostalgia-server-<version>-<os>-<arch>.zip (Windows) / .tar.gz
#                               binary, start script, settings.example.yaml, LICENSE, README.txt
#   scripts/package.sh client   Windows: Nostalgia-<version>-windows-x64.exe (single file)
#                               Linux:   Nostalgia-<version>-linux-x64.tar.gz (binary, icon, .desktop)
#                               macOS:   Nostalgia-<version>-macos-arm64.zip (Nostalgia.app)
#
# Pass --no-build to package an existing target/release build. Runs in Git Bash on Windows.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)

what=${1:-}
build=1
[ "${2:-}" = "--no-build" ] && build=0
case "$what" in server | client) ;; *) echo "Usage: $0 server|client [--no-build]" >&2; exit 1 ;; esac

version=$(scripts/bump-version.sh)
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN* | Windows_NT) os=windows; exe=.exe ;;
  Darwin) os=macos; exe= ;;
  Linux) os=linux; exe= ;;
  *) echo "unsupported OS $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) arch=$(uname -m) ;;
esac

mkdir -p release
out="$root/release"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# archive <folder name in $tmp> [app]: $out/<name>.zip on Windows and for the macOS app bundle
# (ditto keeps the bundle and its signature intact), else $out/<name>.tar.gz.
archive() {
  local name=$1 kind=${2:-}
  if [ "$os" = windows ]; then
    rm -f "$out/$name.zip"
    # bsdtar from Windows writes zip files (Git Bash's GNU tar cannot).
    local bsdtar
    bsdtar="$(cygpath -u "${SYSTEMROOT:-C:\\Windows}")/System32/tar.exe"
    (cd "$tmp" && "$bsdtar" -a -c -f "$(cygpath -w "$out/$name.zip")" "$name")
    echo "release/$name.zip"
  elif [ "$os" = macos ] && [ "$kind" = app ]; then
    rm -f "$out/$name.zip"
    (cd "$tmp" && ditto -c -k --keepParent "$name" "$out/$name.zip")
    echo "release/$name.zip"
  else
    tar -czf "$out/$name.tar.gz" -C "$tmp" "$name"
    echo "release/$name.tar.gz"
  fi
}

if [ "$what" = server ]; then
  [ $build = 1 ] && cargo build --release --locked -p nostalgia-server
  name="nostalgia-server-$version-$os-$arch"
  dir="$tmp/$name"
  mkdir -p "$dir"
  cp "target/release/nostalgia-server$exe" LICENSE "$dir/"
  # The committed example, never local edits of it (they may hold real credentials).
  git show HEAD:server/settings.example.yaml > "$dir/settings.example.yaml" 2> /dev/null \
    || cp server/settings.example.yaml "$dir/"
  if [ "$os" = windows ]; then
    printf '@echo off\r\ncd /d "%%~dp0"\r\n"%%~dp0nostalgia-server.exe" --settings settings.yaml %%*\r\nif errorlevel 1 pause\r\n' > "$dir/start.cmd"
    start="start.cmd"
  else
    printf '#!/usr/bin/env sh\ncd "$(dirname "$0")" || exit 1\nexec ./nostalgia-server --settings settings.yaml "$@"\n' > "$dir/start.sh"
    chmod +x "$dir/start.sh" "$dir/nostalgia-server"
    start="./start.sh"
  fi
  nl=$'\n'; [ "$os" = windows ] && nl=$'\r\n'
  {
    printf 'Nostalgia server %s (%s, %s)%s%s' "$version" "$os" "$arch" "$nl" "$nl"
    printf 'Start:   %s%s' "$start" "$nl"
    printf '         On first start settings.yaml is created next to this file. Set the admin%s' "$nl"
    printf '         password, libraryDirectory and the IGDB credentials there, then start again.%s' "$nl"
    printf 'Admin:   http://<this machine>:8080/admin%s' "$nl"
    printf 'Data:    ./database (SQLite database) and ./saves (cloud saves), see databaseDirectory and%s' "$nl"
    printf '         savesDirectory in settings.yaml. Back both up.%s%s' "$nl" "$nl"
    printf 'Nothing else needs to be installed.%s' "$nl"
    printf 'Documentation: https://github.com/neiloru/Nostalgia%s' "$nl"
  } > "$dir/README.txt"
  archive "$name"
  exit 0
fi

# ---- client ----
[ $build = 1 ] && cargo build --release --locked -p nostalgia
case "$os" in
  windows)
    cp target/release/nostalgia.exe "$out/Nostalgia-$version-windows-$arch.exe"
    echo "release/Nostalgia-$version-windows-$arch.exe"
    ;;
  linux)
    name="Nostalgia-$version-linux-$arch"
    dir="$tmp/$name"
    mkdir -p "$dir"
    cp target/release/nostalgia LICENSE "$dir/"
    cp client/assets/app/icons/256x256.png "$dir/nostalgia.png"
    chmod +x "$dir/nostalgia"
    cat > "$dir/nostalgia.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Nostalgia
Comment=Retro game library and launcher
Exec=nostalgia
Icon=nostalgia
Categories=Game;
EOF
    archive "$name"
    ;;
  macos)
    name="Nostalgia-$version-macos-$arch"
    dir="$tmp/$name"
    app="$dir/Nostalgia.app/Contents"
    mkdir -p "$app/MacOS" "$app/Resources"
    cp target/release/nostalgia "$app/MacOS/nostalgia"
    cp LICENSE "$dir/"
    # The app icon (Finder, Dock, Cmd+Tab): an .app without it shows the generic icon.
    command -v iconutil > /dev/null || { echo "iconutil not found (it ships with macOS)" >&2; exit 1; }
    {
      set_dir="$tmp/icon.iconset"
      mkdir -p "$set_dir"
      icons=client/assets/app/icons
      for s in 16 32 128 256 512; do
        cp "$icons/${s}x${s}.png" "$set_dir/icon_${s}x${s}.png"
      done
      cp "$icons/32x32.png" "$set_dir/icon_16x16@2x.png"
      cp "$icons/64x64.png" "$set_dir/icon_32x32@2x.png"
      cp "$icons/256x256.png" "$set_dir/icon_128x128@2x.png"
      cp "$icons/512x512.png" "$set_dir/icon_256x256@2x.png"
      iconutil -c icns "$set_dir" -o "$app/Resources/Nostalgia.icns"
    }
    cat > "$app/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Nostalgia</string>
  <key>CFBundleDisplayName</key><string>Nostalgia</string>
  <key>CFBundleIdentifier</key><string>io.github.neiloru.nostalgia</string>
  <key>CFBundleExecutable</key><string>nostalgia</string>
  <key>CFBundleIconFile</key><string>Nostalgia</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
    # Not notarized: an ad-hoc signature only (Apple Silicon refuses to run unsigned code).
    codesign --force --deep --sign - "$dir/Nostalgia.app"
    archive "$name" app
    ;;
esac
