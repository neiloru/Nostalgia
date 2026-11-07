import fs from 'node:fs';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

export type DB = DatabaseSync;

const SCHEMA = `
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  disabled INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  device TEXT,
  created_at TEXT NOT NULL,
  last_seen TEXT NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS games (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  platform TEXT NOT NULL,
  folder TEXT NOT NULL,
  rel_path TEXT NOT NULL UNIQUE,
  igdb_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  file_count INTEGER NOT NULL DEFAULT 0,
  missing INTEGER NOT NULL DEFAULT 0,
  hidden INTEGER NOT NULL DEFAULT 0,
  metadata_json TEXT,
  metadata_status TEXT NOT NULL DEFAULT 'pending',
  metadata_error TEXT,
  added_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS emulators (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  platform TEXT NOT NULL,
  folder TEXT NOT NULL,
  rel_path TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  file_count INTEGER NOT NULL DEFAULT 0,
  config_json TEXT,
  config_error TEXT,
  missing INTEGER NOT NULL DEFAULT 0,
  added_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_games (
  user_id INTEGER NOT NULL REFERENCES users(id),
  game_id INTEGER NOT NULL REFERENCES games(id),
  added_at TEXT NOT NULL,
  PRIMARY KEY (user_id, game_id)
);

CREATE TABLE IF NOT EXISTS user_emulators (
  user_id INTEGER NOT NULL REFERENCES users(id),
  emulator_id INTEGER NOT NULL REFERENCES emulators(id),
  added_at TEXT NOT NULL,
  auto INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, emulator_id)
);

-- Cloud saves are versioned per user and emulator. Rows and files are never deleted.
CREATE TABLE IF NOT EXISTS saves (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  emulator_id INTEGER NOT NULL REFERENCES emulators(id),
  version INTEGER NOT NULL,
  file TEXT NOT NULL,
  size INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  device TEXT,
  note TEXT,
  created_at TEXT NOT NULL,
  UNIQUE (user_id, emulator_id, version)
);

CREATE TABLE IF NOT EXISTS play_sessions (
  session_id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  game_id INTEGER NOT NULL REFERENCES games(id),
  started_at TEXT NOT NULL,
  seconds INTEGER NOT NULL,
  device TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file_hashes (
  game_id INTEGER NOT NULL REFERENCES games(id),
  rel_path TEXT NOT NULL,
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL,
  md5 TEXT,
  sha1 TEXT,
  crc32 TEXT,
  hasheous_status TEXT,
  hasheous_name TEXT,
  hasheous_json TEXT,
  checked_at TEXT,
  PRIMARY KEY (game_id, rel_path)
);

CREATE INDEX IF NOT EXISTS idx_saves_user_emu ON saves(user_id, emulator_id, version);
CREATE INDEX IF NOT EXISTS idx_play_user ON play_sessions(user_id, game_id);
`;

export function openDb(dataPath: string): DB {
  fs.mkdirSync(dataPath, { recursive: true });
  const db = new DatabaseSync(path.join(dataPath, 'nostalgia.db'));
  db.exec(SCHEMA);
  return db;
}

export const now = () => new Date().toISOString();
