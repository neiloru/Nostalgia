//! Platform folders follow the RomM folder naming convention (fs slugs).
//! <https://docs.romm.app/latest/platforms/supported-platforms/>
//!
//! `igdb` is the IGDB platform id (used to narrow IGDB searches).
//! `ext` are typical launchable file extensions in priority order (used as a
//! fallback when an emulator's emu.yaml does not list `extensions`).

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PlatformInfo {
    pub slug: &'static str,
    pub name: &'static str,
    pub short: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb: Option<u32>,
    pub ext: &'static [&'static str],
}

pub const DISC: &[&str] = &["m3u", "chd", "cue", "ccd", "iso", "cso", "pbp", "gdi", "cdi", "img", "bin"];

pub const PLATFORMS: &[PlatformInfo] = &[
    PlatformInfo { slug: "3do", name: "3DO Interactive Multiplayer", short: "3DO", igdb: Some(50), ext: DISC },
    PlatformInfo { slug: "3ds", name: "Nintendo 3DS", short: "3DS", igdb: Some(37), ext: &["3ds", "cci", "cia", "cxi"] },
    PlatformInfo { slug: "amiga", name: "Amiga", short: "Amiga", igdb: Some(16), ext: &["adf", "hdf", "lha", "ipf", "zip"] },
    PlatformInfo { slug: "amiga-cd32", name: "Amiga CD32", short: "CD32", igdb: Some(114), ext: DISC },
    PlatformInfo { slug: "arcade", name: "Arcade", short: "Arcade", igdb: Some(52), ext: &["zip", "7z"] },
    PlatformInfo { slug: "atari2600", name: "Atari 2600", short: "2600", igdb: Some(59), ext: &["a26", "bin", "zip"] },
    PlatformInfo { slug: "atari5200", name: "Atari 5200", short: "5200", igdb: Some(66), ext: &["a52", "bin", "zip"] },
    PlatformInfo { slug: "atari7800", name: "Atari 7800", short: "7800", igdb: Some(60), ext: &["a78", "bin", "zip"] },
    PlatformInfo { slug: "c64", name: "Commodore C64/128/MAX", short: "C64", igdb: Some(15), ext: &["d64", "t64", "prg", "crt", "tap", "zip"] },
    PlatformInfo { slug: "colecovision", name: "ColecoVision", short: "Coleco", igdb: Some(68), ext: &["col", "rom", "zip"] },
    PlatformInfo { slug: "dc", name: "Dreamcast", short: "DC", igdb: Some(23), ext: &["m3u", "chd", "gdi", "cdi", "cue", "iso"] },
    PlatformInfo { slug: "dos", name: "DOS", short: "DOS", igdb: Some(13), ext: &["exe", "bat", "com", "conf", "zip"] },
    PlatformInfo { slug: "fds", name: "Family Computer Disk System", short: "FDS", igdb: Some(51), ext: &["fds", "zip"] },
    PlatformInfo { slug: "gamegear", name: "Sega Game Gear", short: "GG", igdb: Some(35), ext: &["gg", "zip"] },
    PlatformInfo { slug: "gb", name: "Game Boy", short: "GB", igdb: Some(33), ext: &["gb", "zip"] },
    PlatformInfo { slug: "gba", name: "Game Boy Advance", short: "GBA", igdb: Some(24), ext: &["gba", "zip"] },
    PlatformInfo { slug: "gbc", name: "Game Boy Color", short: "GBC", igdb: Some(22), ext: &["gbc", "gb", "zip"] },
    PlatformInfo { slug: "genesis", name: "Sega Mega Drive/Genesis", short: "Genesis", igdb: Some(29), ext: &["md", "gen", "smd", "bin", "zip"] },
    PlatformInfo { slug: "intellivision", name: "Intellivision", short: "Intv", igdb: Some(67), ext: &["int", "rom", "bin", "zip"] },
    PlatformInfo { slug: "jaguar", name: "Atari Jaguar", short: "Jaguar", igdb: Some(62), ext: &["j64", "jag", "zip"] },
    PlatformInfo { slug: "lynx", name: "Atari Lynx", short: "Lynx", igdb: Some(61), ext: &["lnx", "zip"] },
    PlatformInfo { slug: "msx", name: "MSX", short: "MSX", igdb: Some(27), ext: &["rom", "mx1", "mx2", "dsk", "zip"] },
    PlatformInfo { slug: "n64", name: "Nintendo 64", short: "N64", igdb: Some(4), ext: &["z64", "n64", "v64", "zip"] },
    PlatformInfo { slug: "nds", name: "Nintendo DS", short: "NDS", igdb: Some(20), ext: &["nds", "zip"] },
    PlatformInfo { slug: "neo-geo-cd", name: "Neo Geo CD", short: "NGCD", igdb: Some(136), ext: DISC },
    PlatformInfo { slug: "neo-geo-pocket", name: "Neo Geo Pocket", short: "NGP", igdb: Some(119), ext: &["ngp", "zip"] },
    PlatformInfo { slug: "neo-geo-pocket-color", name: "Neo Geo Pocket Color", short: "NGPC", igdb: Some(120), ext: &["ngc", "ngp", "zip"] },
    PlatformInfo { slug: "neogeoaes", name: "Neo Geo AES", short: "AES", igdb: Some(80), ext: &["zip", "7z"] },
    PlatformInfo { slug: "neogeomvs", name: "Neo Geo MVS", short: "MVS", igdb: Some(79), ext: &["zip", "7z"] },
    PlatformInfo { slug: "nes", name: "Nintendo Entertainment System", short: "NES", igdb: Some(18), ext: &["nes", "unf", "zip"] },
    PlatformInfo { slug: "ngc", name: "Nintendo GameCube", short: "GC", igdb: Some(21), ext: &["rvz", "gcz", "iso", "gcm", "ciso", "m3u"] },
    PlatformInfo { slug: "pc-fx", name: "PC-FX", short: "PC-FX", igdb: Some(274), ext: DISC },
    PlatformInfo { slug: "ps2", name: "PlayStation 2", short: "PS2", igdb: Some(8), ext: &["m3u", "chd", "iso", "cso", "zso", "gz", "cue", "bin"] },
    PlatformInfo { slug: "ps3", name: "PlayStation 3", short: "PS3", igdb: Some(9), ext: &["iso", "pkg", "bin"] },
    PlatformInfo { slug: "psp", name: "PlayStation Portable", short: "PSP", igdb: Some(38), ext: &["iso", "cso", "chd", "pbp"] },
    PlatformInfo { slug: "psvita", name: "PlayStation Vita", short: "Vita", igdb: Some(46), ext: &["vpk", "zip"] },
    PlatformInfo { slug: "psx", name: "PlayStation", short: "PS1", igdb: Some(7), ext: DISC },
    PlatformInfo { slug: "saturn", name: "Sega Saturn", short: "Saturn", igdb: Some(32), ext: DISC },
    PlatformInfo { slug: "scummvm", name: "ScummVM", short: "ScummVM", igdb: None, ext: &["scummvm", "zip"] },
    PlatformInfo { slug: "sega32", name: "Sega 32X", short: "32X", igdb: Some(30), ext: &["32x", "bin", "zip"] },
    PlatformInfo { slug: "segacd", name: "Sega CD", short: "SegaCD", igdb: Some(78), ext: DISC },
    PlatformInfo { slug: "sfam", name: "Super Famicom", short: "SFC", igdb: Some(58), ext: &["sfc", "smc", "zip"] },
    PlatformInfo { slug: "sg1000", name: "SG-1000", short: "SG-1000", igdb: Some(84), ext: &["sg", "zip"] },
    PlatformInfo { slug: "sms", name: "Sega Master System/Mark III", short: "SMS", igdb: Some(64), ext: &["sms", "zip"] },
    PlatformInfo { slug: "snes", name: "Super Nintendo Entertainment System", short: "SNES", igdb: Some(19), ext: &["sfc", "smc", "zip"] },
    PlatformInfo { slug: "switch", name: "Nintendo Switch", short: "Switch", igdb: Some(130), ext: &["nsp", "xci", "nsz", "xcz"] },
    PlatformInfo { slug: "tg16", name: "TurboGrafx-16/PC Engine", short: "TG16", igdb: Some(86), ext: &["pce", "zip"] },
    PlatformInfo { slug: "turbografx-cd", name: "Turbografx-16/PC Engine CD", short: "TG-CD", igdb: Some(150), ext: DISC },
    PlatformInfo { slug: "vectrex", name: "Vectrex", short: "Vectrex", igdb: Some(70), ext: &["vec", "bin", "zip"] },
    PlatformInfo { slug: "virtualboy", name: "Virtual Boy", short: "VB", igdb: Some(87), ext: &["vb", "vboy", "zip"] },
    PlatformInfo { slug: "wii", name: "Wii", short: "Wii", igdb: Some(5), ext: &["rvz", "wbfs", "iso", "wad", "m3u"] },
    PlatformInfo { slug: "wiiu", name: "Wii U", short: "Wii U", igdb: Some(41), ext: &["wua", "wud", "wux", "rpx"] },
    PlatformInfo { slug: "wonderswan", name: "WonderSwan", short: "WS", igdb: Some(57), ext: &["ws", "zip"] },
    PlatformInfo { slug: "wonderswan-color", name: "WonderSwan Color", short: "WSC", igdb: Some(123), ext: &["wsc", "zip"] },
    PlatformInfo { slug: "xbox", name: "Xbox", short: "Xbox", igdb: Some(11), ext: &["iso", "xiso"] },
    PlatformInfo { slug: "xbox360", name: "Xbox 360", short: "X360", igdb: Some(12), ext: &["iso", "xex", "zar"] },
    PlatformInfo { slug: "zxs", name: "ZX Spectrum", short: "ZX", igdb: Some(26), ext: &["tzx", "tap", "z80", "sna", "zip"] },
];

/// Owned variant for platforms that are not in the list (unknown folder names).
#[derive(Debug, Clone, Serialize)]
pub struct Platform {
    pub slug: String,
    pub name: String,
    pub short: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub igdb: Option<u32>,
    pub ext: Vec<String>,
}

pub fn find_platform(slug: &str) -> Option<&'static PlatformInfo> {
    PLATFORMS.iter().find(|p| p.slug == slug)
}

pub fn platform_info(slug: &str) -> Platform {
    match find_platform(slug) {
        Some(p) => Platform {
            slug: p.slug.into(),
            name: p.name.into(),
            short: p.short.into(),
            igdb: p.igdb,
            ext: p.ext.iter().map(|s| s.to_string()).collect(),
        },
        None => Platform {
            slug: slug.into(),
            name: slug.into(),
            short: slug.to_uppercase(),
            igdb: None,
            ext: DISC.iter().map(|s| s.to_string()).collect(),
        },
    }
}

pub fn platform_name(slug: &str) -> String {
    find_platform(slug).map(|p| p.name.to_string()).unwrap_or_else(|| slug.to_string())
}
