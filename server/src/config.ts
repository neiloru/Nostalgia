import fs from 'node:fs';
import path from 'node:path';
import YAML from 'yaml';
import { log } from './log.js';

export interface Settings {
  adminUsername: string;
  adminPassword: string;
  libraryDirectory: string;
  igdbClientId: string;
  igdbClientSecret: string;
  /** Additional settings (optional). */
  port: number;
  host: string;
  /** SQLite database and other server data. */
  databaseDirectory: string;
  /** Versioned cloud saves. Separate from the database. */
  savesDirectory: string;
  allowRegistration: boolean;
  scanIntervalMinutes: number;
  /** Check file hashes against hasheous.org during scheduled scans. */
  hasheousOnScheduledScan: boolean;
  /** Public URL used in logs only. */
  publicUrl?: string;
}

const DEFAULTS: Settings = {
  adminUsername: 'admin',
  adminPassword: 'change-me',
  libraryDirectory: './library',
  igdbClientId: '',
  igdbClientSecret: '',
  port: 8080,
  host: '0.0.0.0',
  databaseDirectory: './database',
  savesDirectory: './saves',
  allowRegistration: true,
  scanIntervalMinutes: 30,
  hasheousOnScheduledScan: false,
};

const TEMPLATE = `# Nostalgia server settings
# Paths are relative to this file.

# Credentials for the admin web interface (http://<server>:<port>/admin)
adminUsername: admin
adminPassword: change-me

# Root of the game library. Contains "games/{platform}/{Name} [igdbid-123]/" and
# "emulators/{platform}/{Emulator name}/" (with an emu.yaml).
libraryDirectory: ./library

# Twitch / IGDB API credentials (https://api-docs.igdb.com/#account-creation)
igdbClientId: ""
igdbClientSecret: ""

# --- optional ---
port: 8080
host: 0.0.0.0
# SQLite database (users, library, play time). Keep it on a local disk.
databaseDirectory: ./database
# Versioned cloud saves, one zip per upload. Never deleted.
savesDirectory: ./saves
allowRegistration: true
scanIntervalMinutes: 30
hasheousOnScheduledScan: false
`;

export interface LoadedSettings extends Settings {
  file: string;
  libraryPath: string;
  databasePath: string;
  savesPath: string;
  /** "<database>/saves" of servers from before the split; moved to savesPath on start. */
  legacySavesPath: string;
  gamesPath: string;
  emulatorsPath: string;
}

export function resolveSettingsFile(argv = process.argv): string {
  const i = argv.indexOf('--settings');
  if (i >= 0 && argv[i + 1]) return path.resolve(argv[i + 1]);
  if (process.env.NOSTALGIA_SETTINGS) return path.resolve(process.env.NOSTALGIA_SETTINGS);
  return path.resolve('settings.yaml');
}

export function loadSettings(file = resolveSettingsFile()): LoadedSettings {
  if (!fs.existsSync(file)) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, TEMPLATE, 'utf8');
    log.warn(`Created default settings file at ${file} - change the admin password!`);
  }
  const raw = (YAML.parse(fs.readFileSync(file, 'utf8')) ?? {}) as Partial<Settings> & { dataDirectory?: string };
  // Older settings files: "dataDirectory" held the database and the saves.
  if (raw.databaseDirectory === undefined && raw.dataDirectory !== undefined) raw.databaseDirectory = raw.dataDirectory;
  delete raw.dataDirectory;
  const s: Settings = { ...DEFAULTS, ...raw };
  // Environment overrides are handy for containers.
  const env = process.env;
  if (env.NOSTALGIA_PORT) s.port = Number(env.NOSTALGIA_PORT);
  if (env.NOSTALGIA_LIBRARY) s.libraryDirectory = env.NOSTALGIA_LIBRARY;
  if (env.NOSTALGIA_DATABASE ?? env.NOSTALGIA_DATA) s.databaseDirectory = (env.NOSTALGIA_DATABASE ?? env.NOSTALGIA_DATA)!;
  if (env.NOSTALGIA_SAVES) s.savesDirectory = env.NOSTALGIA_SAVES;
  if (env.IGDB_CLIENT_ID) s.igdbClientId = env.IGDB_CLIENT_ID;
  if (env.IGDB_CLIENT_SECRET) s.igdbClientSecret = env.IGDB_CLIENT_SECRET;
  s.port = Number(s.port) || DEFAULTS.port;
  s.scanIntervalMinutes = Number(s.scanIntervalMinutes) || 0;
  s.igdbClientId = String(s.igdbClientId ?? '').trim();
  s.igdbClientSecret = String(s.igdbClientSecret ?? '').trim();
  s.adminUsername = String(s.adminUsername);
  s.adminPassword = String(s.adminPassword);

  const base = path.dirname(file);
  const libraryPath = path.resolve(base, s.libraryDirectory);
  const databasePath = path.resolve(base, String(s.databaseDirectory || DEFAULTS.databaseDirectory));
  const savesPath = path.resolve(base, String(s.savesDirectory || DEFAULTS.savesDirectory));
  return {
    ...s,
    file,
    libraryPath,
    databasePath,
    savesPath,
    legacySavesPath: path.join(databasePath, 'saves'),
    gamesPath: path.join(libraryPath, 'games'),
    emulatorsPath: path.join(libraryPath, 'emulators'),
  };
}
