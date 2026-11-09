//! Windows: embeds the app icon and version info into nostalgia.exe (assets/app/nostalgia.rc).
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app/nostalgia.rc");
    println!("cargo:rerun-if-changed=assets/app/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let num = |key: &str| std::env::var(key).unwrap();
    let macros = [
        format!("VERSION_MAJOR={}", num("CARGO_PKG_VERSION_MAJOR")),
        format!("VERSION_MINOR={}", num("CARGO_PKG_VERSION_MINOR")),
        format!("VERSION_PATCH={}", num("CARGO_PKG_VERSION_PATCH")),
        format!("VERSION_STRING=\"{version}\""),
    ];
    embed_resource::compile("assets/app/nostalgia.rc", &macros).manifest_optional().unwrap();
}
