import fs from 'node:fs';
import path from 'node:path';
import YAML from 'yaml';
import {
  PLATFORMS,
  platformName,
  validateEmuConfig,
  type EmuConfig,
  type EmulatorSummary,
  type GameMetadata,
  type GameSummary,
  type RemoteFile,
} from '@nostalgia/shared';
import { type DB, now } from './db.js';
import type { LoadedSettings } from './config.js';
import { HttpError } from './auth.js';
import { log } from './log.js';

export const IGDB_RE = /\[igdbid-(\d+)\]/i;
export const ASSET_DIR = 'igdb';

export interface GameRow {
  id: number;
  platform: string;
  folder: string;
  rel_path: string;
  igdb_id: number;
  name: string;
  size_bytes: number;
  file_count: number;
  missing: number;
  hidden: number;
  metadata_json: string | null;
  metadata_status: string;
  metadata_error: string | null;
  added_at: string;
  updated_at: string;
}

export interface EmulatorRow {
  id: number;
  platform: string;
  folder: string;
  rel_path: string;
  name: string;
  size_bytes: number;
  file_count: number;
  config_json: string | null;
  config_error: string | null;
  missing: number;
  added_at: string;
  updated_at: string;
}

export interface UnsortedFile {
  platform: string;
  path: string; // relative to games/{platform}
  size: number;
  isDir: boolean;
  /** Suggested game title (cleaned file name). */
  suggestion: string;
}

export interface ScanResult {
  startedAt: string;
  finishedAt: string;
  games: number;
  newGames: number;
  missingGames: number;
  emulators: number;
  newEmulators: number;
  unsorted: UnsortedFile[];
  errors: string[];
}

/** Recursively list files below `dir` (relative, forward slashes). Symlinks are followed only for files. */
export function walkFiles(dir: string): RemoteFile[] {
  const out: RemoteFile[] = [];
  const rec = (abs: string, rel: string) => {
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(abs, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      const a = path.join(abs, e.name);
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) rec(a, r);
      else {
        try {
          const st = fs.statSync(a);
          if (st.isFile()) out.push({ path: r, size: st.size, mtime: Math.floor(st.mtimeMs) });
        } catch {
          /* ignore unreadable */
        }
      }
    }
  };
  rec(dir, '');
  out.sort((x, y) => x.path.localeCompare(y.path));
  return out;
}

/** Resolve `rel` below `base`, refusing path traversal. */
export function resolveInside(base: string, rel: string): string {
  const target = path.resolve(base, rel);
  const b = path.resolve(base);
  if (target !== b && !target.startsWith(b + path.sep)) throw new HttpError(400, 'Invalid path');
  return target;
}

export function sanitizeFolderName(name: string): string {
  return name
    .replace(/[<>:"/\\|?*\x00-\x1f]/g, '')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/[. ]+$/, '')
    .slice(0, 150);
}

export function gameNameFromFolder(folder: string): string {
  return folder.replace(IGDB_RE, '').replace(/\s+/g, ' ').trim() || folder;
}

/** Strip region / disc / revision tags from a file name to suggest a title. */
export function suggestTitle(file: string): string {
  let s = path.basename(file).replace(/\.[a-z0-9]{1,5}$/i, '');
  s = s.replace(/\s*[([][^)\]]*[)\]]/g, '');
  return s.replace(/[_]+/g, ' ').replace(/\s+/g, ' ').trim();
}

export function parseMetadata(row: GameRow): GameMetadata | null {
  if (!row.metadata_json) return null;
  try {
    return JSON.parse(row.metadata_json) as GameMetadata;
  } catch {
    return null;
  }
}

export function toGameSummary(row: GameRow): GameSummary {
  const m = parseMetadata(row);
  return {
    id: row.id,
    kind: 'game',
    name: m?.name || row.name,
    platform: row.platform,
    platformName: platformName(row.platform),
    folder: row.folder,
    igdbId: row.igdb_id,
    sizeBytes: row.size_bytes,
    fileCount: row.file_count,
    hasCover: !!m?.cover,
    hasArtwork: !!(m?.artworks?.length || m?.screenshots?.length),
    year: m?.year,
    genres: m?.genres ?? [],
    themes: m?.themes ?? [],
    gameModes: m?.gameModes ?? [],
    developers: m?.developers ?? [],
    publishers: m?.publishers ?? [],
    rating: m?.rating ?? m?.aggregatedRating,
    addedAt: row.added_at,
  };
}

export function toEmulatorSummary(row: EmulatorRow): EmulatorSummary {
  let config: EmuConfig | null = null;
  try {
    config = row.config_json ? (JSON.parse(row.config_json) as EmuConfig) : null;
  } catch {
    config = null;
  }
  return {
    id: row.id,
    kind: 'emulator',
    name: config?.name || row.name,
    platform: row.platform,
    platformName: platformName(row.platform),
    folder: row.folder,
    sizeBytes: row.size_bytes,
    fileCount: row.file_count,
    config,
    configError: row.config_error ?? undefined,
    supports: config ? (Object.keys(config.systems) as ('windows' | 'linux')[]) : [],
    addedAt: row.added_at,
  };
}

export function readEmuConfig(dir: string): { config: EmuConfig | null; error: string | null; file: string | null } {
  const file = ['emu.yaml', 'emu.yml'].map((f) => path.join(dir, f)).find((f) => fs.existsSync(f)) ?? null;
  if (!file) return { config: null, error: 'emu.yaml is missing', file: null };
  try {
    const doc = YAML.parse(fs.readFileSync(file, 'utf8'));
    return { config: validateEmuConfig(doc), error: null, file };
  } catch (e) {
    return { config: null, error: (e as Error).message, file };
  }
}

export class Library {
  constructor(private db: DB, private settings: LoadedSettings) {}

  get gamesPath() {
    return this.settings.gamesPath;
  }
  get emulatorsPath() {
    return this.settings.emulatorsPath;
  }

  /** Create the platform folders (RomM naming) if they do not exist. Never deletes anything. */
  ensureFolders() {
    for (const base of [this.gamesPath, this.emulatorsPath]) {
      fs.mkdirSync(base, { recursive: true });
      for (const p of PLATFORMS) fs.mkdirSync(path.join(base, p.slug), { recursive: true });
    }
  }

  gameDir(row: Pick<GameRow, 'rel_path'>) {
    return path.join(this.gamesPath, ...row.rel_path.split('/'));
  }
  emulatorDir(row: Pick<EmulatorRow, 'rel_path'>) {
    return path.join(this.emulatorsPath, ...row.rel_path.split('/'));
  }

  getGame(id: number): GameRow | undefined {
    return this.db.prepare('SELECT * FROM games WHERE id = ?').get(id) as GameRow | undefined;
  }
  getEmulator(id: number): EmulatorRow | undefined {
    return this.db.prepare('SELECT * FROM emulators WHERE id = ?').get(id) as EmulatorRow | undefined;
  }
  listGames(includeHidden = false): GameRow[] {
    return this.db
      .prepare(`SELECT * FROM games WHERE missing = 0 ${includeHidden ? '' : 'AND hidden = 0'} ORDER BY name COLLATE NOCASE`)
      .all() as unknown as GameRow[];
  }
  listAllGames(): GameRow[] {
    return this.db.prepare('SELECT * FROM games ORDER BY name COLLATE NOCASE').all() as unknown as GameRow[];
  }
  listEmulators(includeMissing = false): EmulatorRow[] {
    return this.db
      .prepare(`SELECT * FROM emulators ${includeMissing ? '' : 'WHERE missing = 0'} ORDER BY platform, name COLLATE NOCASE`)
      .all() as unknown as EmulatorRow[];
  }

  /** Full scan of games and emulators. */
  scan(): ScanResult {
    const startedAt = now();
    const errors: string[] = [];
    this.ensureFolders();
    const unsorted: UnsortedFile[] = [];
    const seenGames = new Set<string>();
    let newGames = 0;

    const platforms = fs
      .readdirSync(this.gamesPath, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => d.name);

    for (const platform of platforms) {
      const pdir = path.join(this.gamesPath, platform);
      let entries: fs.Dirent[] = [];
      try {
        entries = fs.readdirSync(pdir, { withFileTypes: true });
      } catch (e) {
        errors.push(`${platform}: ${(e as Error).message}`);
        continue;
      }
      for (const e of entries) {
        if (e.name.startsWith('.')) continue;
        const abs = path.join(pdir, e.name);
        const m = IGDB_RE.exec(e.name);
        if (e.isDirectory() && m) {
          const rel = `${platform}/${e.name}`;
          seenGames.add(rel);
          const files = walkFiles(abs);
          const size = files.reduce((a, f) => a + f.size, 0);
          const igdbId = Number(m[1]);
          const existing = this.db.prepare('SELECT * FROM games WHERE rel_path = ?').get(rel) as GameRow | undefined;
          if (existing) {
            this.db
              .prepare('UPDATE games SET size_bytes = ?, file_count = ?, missing = 0, igdb_id = ?, updated_at = ? WHERE id = ?')
              .run(size, files.length, igdbId, now(), existing.id);
            if (existing.igdb_id !== igdbId) this.resetMetadata(existing.id);
          } else {
            // Folder renamed outside of Nostalgia? Re-link the missing record so user libraries survive.
            const moved = this.db
              .prepare('SELECT * FROM games WHERE platform = ? AND igdb_id = ? AND missing = 1 ORDER BY id LIMIT 1')
              .get(platform, igdbId) as GameRow | undefined;
            if (moved) {
              this.db
                .prepare('UPDATE games SET rel_path = ?, folder = ?, size_bytes = ?, file_count = ?, missing = 0, updated_at = ? WHERE id = ?')
                .run(rel, e.name, size, files.length, now(), moved.id);
            } else {
              this.db
                .prepare(
                  `INSERT INTO games (platform, folder, rel_path, igdb_id, name, size_bytes, file_count, added_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
                )
                .run(platform, e.name, rel, igdbId, gameNameFromFolder(e.name), size, files.length, now(), now());
              newGames++;
            }
          }
          this.loadLocalMetadata(rel);
        } else {
          // Loose file or folder without an igdb id: needs to be imported by an admin.
          let size = 0;
          try {
            size = e.isDirectory() ? walkFiles(abs).reduce((a, f) => a + f.size, 0) : fs.statSync(abs).size;
          } catch {
            /* ignore */
          }
          unsorted.push({ platform, path: e.name, size, isDir: e.isDirectory(), suggestion: suggestTitle(e.name) });
        }
      }
    }

    // Mark vanished games as missing (never delete rows - user libraries reference them).
    let missingGames = 0;
    for (const g of this.db.prepare('SELECT id, rel_path FROM games WHERE missing = 0').all() as { id: number; rel_path: string }[]) {
      if (!seenGames.has(g.rel_path)) {
        this.db.prepare('UPDATE games SET missing = 1, updated_at = ? WHERE id = ?').run(now(), g.id);
        missingGames++;
      }
    }

    // Emulators
    const seenEmus = new Set<string>();
    let newEmulators = 0;
    for (const platform of fs.readdirSync(this.emulatorsPath, { withFileTypes: true }).filter((d) => d.isDirectory())) {
      const pdir = path.join(this.emulatorsPath, platform.name);
      for (const e of fs.readdirSync(pdir, { withFileTypes: true })) {
        if (!e.isDirectory() || e.name.startsWith('.')) continue;
        const rel = `${platform.name}/${e.name}`;
        seenEmus.add(rel);
        const abs = path.join(pdir, e.name);
        const files = walkFiles(abs);
        const size = files.reduce((a, f) => a + f.size, 0);
        const { config, error } = readEmuConfig(abs);
        if (error) errors.push(`emulator ${rel}: ${error}`);
        const existing = this.db.prepare('SELECT id FROM emulators WHERE rel_path = ?').get(rel) as { id: number } | undefined;
        const name = config?.name || e.name;
        if (existing) {
          this.db
            .prepare('UPDATE emulators SET name = ?, size_bytes = ?, file_count = ?, config_json = ?, config_error = ?, missing = 0, updated_at = ? WHERE id = ?')
            .run(name, size, files.length, config ? JSON.stringify(config) : null, error, now(), existing.id);
        } else {
          this.db
            .prepare(
              `INSERT INTO emulators (platform, folder, rel_path, name, size_bytes, file_count, config_json, config_error, added_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
            )
            .run(platform.name, e.name, rel, name, size, files.length, config ? JSON.stringify(config) : null, error, now(), now());
          newEmulators++;
        }
      }
    }
    for (const em of this.db.prepare('SELECT id, rel_path FROM emulators WHERE missing = 0').all() as { id: number; rel_path: string }[]) {
      if (!seenEmus.has(em.rel_path)) this.db.prepare('UPDATE emulators SET missing = 1, updated_at = ? WHERE id = ?').run(now(), em.id);
    }

    const counts = {
      games: (this.db.prepare('SELECT COUNT(*) c FROM games WHERE missing = 0').get() as { c: number }).c,
      emulators: (this.db.prepare('SELECT COUNT(*) c FROM emulators WHERE missing = 0').get() as { c: number }).c,
    };
    return { startedAt, finishedAt: now(), ...counts, newGames, missingGames, newEmulators, unsorted, errors };
  }

  /** If igdb/metadata.json exists in the folder, load it into the DB (it is the source of truth). */
  loadLocalMetadata(rel: string) {
    const row = this.db.prepare('SELECT * FROM games WHERE rel_path = ?').get(rel) as GameRow | undefined;
    if (!row) return;
    const file = path.join(this.gameDir(row), ASSET_DIR, 'metadata.json');
    if (!fs.existsSync(file)) {
      if (row.metadata_status === 'ok') this.resetMetadata(row.id);
      return;
    }
    try {
      const txt = fs.readFileSync(file, 'utf8');
      const meta = JSON.parse(txt) as GameMetadata;
      if (meta.igdbId !== row.igdb_id) {
        this.resetMetadata(row.id);
        return;
      }
      if (txt !== row.metadata_json || row.metadata_status !== 'ok') {
        this.db
          .prepare("UPDATE games SET metadata_json = ?, metadata_status = 'ok', metadata_error = NULL, name = ? WHERE id = ?")
          .run(txt, meta.name || row.name, row.id);
      }
    } catch (e) {
      log.warn(`Invalid metadata.json in ${rel}: ${(e as Error).message}`);
    }
  }

  resetMetadata(id: number) {
    this.db.prepare("UPDATE games SET metadata_json = NULL, metadata_status = 'pending', metadata_error = NULL WHERE id = ?").run(id);
  }

  /**
   * Import loose files/folders from games/{platform}/ into a proper game folder
   * "{Name} [igdbid-{id}]". Files are moved (renamed), never deleted.
   */
  importEntries(platform: string, entries: string[], igdbId: number, name: string): string {
    if (!entries.length) throw new HttpError(400, 'Nothing to import');
    if (!Number.isInteger(igdbId) || igdbId <= 0) throw new HttpError(400, 'Invalid IGDB id');
    const pdir = resolveInside(this.gamesPath, platform);
    const folder = `${sanitizeFolderName(name)} [igdbid-${igdbId}]`;
    const target = resolveInside(pdir, folder);
    fs.mkdirSync(target, { recursive: true });
    for (const entry of entries) {
      const src = resolveInside(pdir, entry);
      if (path.dirname(src) !== pdir) throw new HttpError(400, `Not a top-level entry: ${entry}`);
      if (!fs.existsSync(src)) throw new HttpError(404, `Not found: ${entry}`);
      const st = fs.statSync(src);
      if (st.isDirectory()) {
        // Move the folder's content into the game folder.
        for (const child of fs.readdirSync(src)) {
          const dst = path.join(target, child);
          if (fs.existsSync(dst)) throw new HttpError(409, `Target already exists: ${folder}/${child}`);
          fs.renameSync(path.join(src, child), dst);
        }
        // Remove the source folder only if it is now completely empty (rmdir refuses otherwise).
        try {
          fs.rmdirSync(src);
        } catch {
          /* not empty - keep it */
        }
      } else {
        const dst = path.join(target, path.basename(src));
        if (fs.existsSync(dst)) throw new HttpError(409, `Target already exists: ${folder}/${path.basename(src)}`);
        fs.renameSync(src, dst);
      }
    }
    log.info(`Imported ${entries.length} entr${entries.length === 1 ? 'y' : 'ies'} into ${platform}/${folder}`);
    return `${platform}/${folder}`;
  }

  /** Change the IGDB id of a game by renaming its folder. */
  rematchGame(id: number, igdbId: number, name?: string): GameRow {
    const row = this.getGame(id);
    if (!row) throw new HttpError(404, 'Game not found');
    if (!Number.isInteger(igdbId) || igdbId <= 0) throw new HttpError(400, 'Invalid IGDB id');
    const title = sanitizeFolderName(name || gameNameFromFolder(row.folder));
    const folder = `${title} [igdbid-${igdbId}]`;
    const src = this.gameDir(row);
    const dst = resolveInside(path.join(this.gamesPath, row.platform), folder);
    if (src !== dst) {
      if (fs.existsSync(dst)) throw new HttpError(409, `Folder already exists: ${folder}`);
      fs.renameSync(src, dst);
    }
    // The old metadata in igdb/ belongs to the previous id; it will be refreshed (files overwritten, not deleted).
    this.db
      .prepare("UPDATE games SET folder = ?, rel_path = ?, igdb_id = ?, name = ?, metadata_status = 'pending', metadata_json = NULL, updated_at = ? WHERE id = ?")
      .run(folder, `${row.platform}/${folder}`, igdbId, title, now(), id);
    return this.getGame(id)!;
  }

  setHidden(id: number, hidden: boolean) {
    this.db.prepare('UPDATE games SET hidden = ? WHERE id = ?').run(hidden ? 1 : 0, id);
  }

  /** Rewrite an emulator's emu.yaml after validating it. The previous file is kept in the data directory. */
  writeEmuYaml(id: number, text: string) {
    const row = this.getEmulator(id);
    if (!row) throw new HttpError(404, 'Emulator not found');
    let config: EmuConfig;
    try {
      config = validateEmuConfig(YAML.parse(text));
    } catch (e) {
      throw new HttpError(400, (e as Error).message);
    }
    const dir = this.emulatorDir(row);
    const file = path.join(dir, 'emu.yaml');
    if (fs.existsSync(file)) {
      // Keep the previous version outside of the emulator folder so it is not sent to clients.
      const backupDir = path.join(this.settings.dataPath, 'emu-yaml-history', row.platform, row.folder);
      fs.mkdirSync(backupDir, { recursive: true });
      fs.copyFileSync(file, path.join(backupDir, `emu.yaml.${Date.now()}`));
    }
    fs.writeFileSync(file, text, 'utf8');
    this.db
      .prepare('UPDATE emulators SET config_json = ?, config_error = NULL, name = ?, updated_at = ? WHERE id = ?')
      .run(JSON.stringify(config), config.name || row.folder, now(), id);
  }
}
