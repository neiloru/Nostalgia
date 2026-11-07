/**
 * Platform folders follow the RomM folder naming convention (fs slugs).
 * https://docs.romm.app/latest/platforms/supported-platforms/
 *
 * `igdb` is the IGDB platform id (used to narrow IGDB searches).
 * `ext` are typical launchable file extensions in priority order (used as a
 * fallback when an emulator's emu.yaml does not list `extensions`).
 */
export interface PlatformInfo {
  slug: string;
  name: string;
  short: string;
  igdb?: number;
  ext: string[];
}

const DISC = ['m3u', 'chd', 'cue', 'ccd', 'iso', 'cso', 'pbp', 'gdi', 'cdi', 'img', 'bin'];

export const PLATFORMS: PlatformInfo[] = [
  { slug: '3do', name: '3DO Interactive Multiplayer', short: '3DO', igdb: 50, ext: DISC },
  { slug: '3ds', name: 'Nintendo 3DS', short: '3DS', igdb: 37, ext: ['3ds', 'cci', 'cia', 'cxi'] },
  { slug: 'amiga', name: 'Amiga', short: 'Amiga', igdb: 16, ext: ['adf', 'hdf', 'lha', 'ipf', 'zip'] },
  { slug: 'amiga-cd32', name: 'Amiga CD32', short: 'CD32', igdb: 114, ext: DISC },
  { slug: 'arcade', name: 'Arcade', short: 'Arcade', igdb: 52, ext: ['zip', '7z'] },
  { slug: 'atari2600', name: 'Atari 2600', short: '2600', igdb: 59, ext: ['a26', 'bin', 'zip'] },
  { slug: 'atari5200', name: 'Atari 5200', short: '5200', igdb: 66, ext: ['a52', 'bin', 'zip'] },
  { slug: 'atari7800', name: 'Atari 7800', short: '7800', igdb: 60, ext: ['a78', 'bin', 'zip'] },
  { slug: 'c64', name: 'Commodore C64/128/MAX', short: 'C64', igdb: 15, ext: ['d64', 't64', 'prg', 'crt', 'tap', 'zip'] },
  { slug: 'colecovision', name: 'ColecoVision', short: 'Coleco', igdb: 68, ext: ['col', 'rom', 'zip'] },
  { slug: 'dc', name: 'Dreamcast', short: 'DC', igdb: 23, ext: ['m3u', 'chd', 'gdi', 'cdi', 'cue', 'iso'] },
  { slug: 'dos', name: 'DOS', short: 'DOS', igdb: 13, ext: ['exe', 'bat', 'com', 'conf', 'zip'] },
  { slug: 'fds', name: 'Family Computer Disk System', short: 'FDS', igdb: 51, ext: ['fds', 'zip'] },
  { slug: 'gamegear', name: 'Sega Game Gear', short: 'GG', igdb: 35, ext: ['gg', 'zip'] },
  { slug: 'gb', name: 'Game Boy', short: 'GB', igdb: 33, ext: ['gb', 'zip'] },
  { slug: 'gba', name: 'Game Boy Advance', short: 'GBA', igdb: 24, ext: ['gba', 'zip'] },
  { slug: 'gbc', name: 'Game Boy Color', short: 'GBC', igdb: 22, ext: ['gbc', 'gb', 'zip'] },
  { slug: 'genesis', name: 'Sega Mega Drive/Genesis', short: 'Genesis', igdb: 29, ext: ['md', 'gen', 'smd', 'bin', 'zip'] },
  { slug: 'intellivision', name: 'Intellivision', short: 'Intv', igdb: 67, ext: ['int', 'rom', 'bin', 'zip'] },
  { slug: 'jaguar', name: 'Atari Jaguar', short: 'Jaguar', igdb: 62, ext: ['j64', 'jag', 'zip'] },
  { slug: 'lynx', name: 'Atari Lynx', short: 'Lynx', igdb: 61, ext: ['lnx', 'zip'] },
  { slug: 'msx', name: 'MSX', short: 'MSX', igdb: 27, ext: ['rom', 'mx1', 'mx2', 'dsk', 'zip'] },
  { slug: 'n64', name: 'Nintendo 64', short: 'N64', igdb: 4, ext: ['z64', 'n64', 'v64', 'zip'] },
  { slug: 'nds', name: 'Nintendo DS', short: 'NDS', igdb: 20, ext: ['nds', 'zip'] },
  { slug: 'neo-geo-cd', name: 'Neo Geo CD', short: 'NGCD', igdb: 136, ext: DISC },
  { slug: 'neo-geo-pocket', name: 'Neo Geo Pocket', short: 'NGP', igdb: 119, ext: ['ngp', 'zip'] },
  { slug: 'neo-geo-pocket-color', name: 'Neo Geo Pocket Color', short: 'NGPC', igdb: 120, ext: ['ngc', 'ngp', 'zip'] },
  { slug: 'neogeoaes', name: 'Neo Geo AES', short: 'AES', igdb: 80, ext: ['zip', '7z'] },
  { slug: 'neogeomvs', name: 'Neo Geo MVS', short: 'MVS', igdb: 79, ext: ['zip', '7z'] },
  { slug: 'nes', name: 'Nintendo Entertainment System', short: 'NES', igdb: 18, ext: ['nes', 'unf', 'zip'] },
  { slug: 'ngc', name: 'Nintendo GameCube', short: 'GC', igdb: 21, ext: ['rvz', 'gcz', 'iso', 'gcm', 'ciso', 'm3u'] },
  { slug: 'pc-fx', name: 'PC-FX', short: 'PC-FX', igdb: 274, ext: DISC },
  { slug: 'ps2', name: 'PlayStation 2', short: 'PS2', igdb: 8, ext: ['m3u', 'chd', 'iso', 'cso', 'zso', 'gz', 'cue', 'bin'] },
  { slug: 'ps3', name: 'PlayStation 3', short: 'PS3', igdb: 9, ext: ['iso', 'pkg', 'bin'] },
  { slug: 'psp', name: 'PlayStation Portable', short: 'PSP', igdb: 38, ext: ['iso', 'cso', 'chd', 'pbp'] },
  { slug: 'psvita', name: 'PlayStation Vita', short: 'Vita', igdb: 46, ext: ['vpk', 'zip'] },
  { slug: 'psx', name: 'PlayStation', short: 'PS1', igdb: 7, ext: DISC },
  { slug: 'saturn', name: 'Sega Saturn', short: 'Saturn', igdb: 32, ext: DISC },
  { slug: 'scummvm', name: 'ScummVM', short: 'ScummVM', ext: ['scummvm', 'zip'] },
  { slug: 'sega32', name: 'Sega 32X', short: '32X', igdb: 30, ext: ['32x', 'bin', 'zip'] },
  { slug: 'segacd', name: 'Sega CD', short: 'SegaCD', igdb: 78, ext: DISC },
  { slug: 'sfam', name: 'Super Famicom', short: 'SFC', igdb: 58, ext: ['sfc', 'smc', 'zip'] },
  { slug: 'sg1000', name: 'SG-1000', short: 'SG-1000', igdb: 84, ext: ['sg', 'zip'] },
  { slug: 'sms', name: 'Sega Master System/Mark III', short: 'SMS', igdb: 64, ext: ['sms', 'zip'] },
  { slug: 'snes', name: 'Super Nintendo Entertainment System', short: 'SNES', igdb: 19, ext: ['sfc', 'smc', 'zip'] },
  { slug: 'switch', name: 'Nintendo Switch', short: 'Switch', igdb: 130, ext: ['nsp', 'xci', 'nsz', 'xcz'] },
  { slug: 'tg16', name: 'TurboGrafx-16/PC Engine', short: 'TG16', igdb: 86, ext: ['pce', 'zip'] },
  { slug: 'turbografx-cd', name: 'Turbografx-16/PC Engine CD', short: 'TG-CD', igdb: 150, ext: DISC },
  { slug: 'vectrex', name: 'Vectrex', short: 'Vectrex', igdb: 70, ext: ['vec', 'bin', 'zip'] },
  { slug: 'virtualboy', name: 'Virtual Boy', short: 'VB', igdb: 87, ext: ['vb', 'vboy', 'zip'] },
  { slug: 'wii', name: 'Wii', short: 'Wii', igdb: 5, ext: ['rvz', 'wbfs', 'iso', 'wad', 'm3u'] },
  { slug: 'wiiu', name: 'Wii U', short: 'Wii U', igdb: 41, ext: ['wua', 'wud', 'wux', 'rpx'] },
  { slug: 'wonderswan', name: 'WonderSwan', short: 'WS', igdb: 57, ext: ['ws', 'zip'] },
  { slug: 'wonderswan-color', name: 'WonderSwan Color', short: 'WSC', igdb: 123, ext: ['wsc', 'zip'] },
  { slug: 'xbox', name: 'Xbox', short: 'Xbox', igdb: 11, ext: ['iso', 'xiso'] },
  { slug: 'xbox360', name: 'Xbox 360', short: 'X360', igdb: 12, ext: ['iso', 'xex', 'zar'] },
  { slug: 'zxs', name: 'ZX Spectrum', short: 'ZX', igdb: 26, ext: ['tzx', 'tap', 'z80', 'sna', 'zip'] },
];

const bySlug = new Map(PLATFORMS.map((p) => [p.slug, p]));

export function platformInfo(slug: string): PlatformInfo {
  return bySlug.get(slug) ?? { slug, name: slug, short: slug.toUpperCase(), ext: DISC };
}

export function platformName(slug: string): string {
  return platformInfo(slug).name;
}
