import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Readable } from 'node:stream';
import { loadSettings } from '../src/config.js';
import { openDb } from '../src/db.js';
import { Saves } from '../src/saves.js';

const write = (body: string) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'nostalgia-config-'));
  const file = path.join(dir, 'settings.yaml');
  fs.writeFileSync(file, body);
  return { dir, file };
};

test('database and cloud saves are separate folders', () => {
  const { dir, file } = write('');
  const s = loadSettings(file);
  assert.equal(s.databasePath, path.join(dir, 'database'));
  assert.equal(s.savesPath, path.join(dir, 'saves'));

  const custom = write('databaseDirectory: ./db\nsavesDirectory: ./elsewhere/saves\n');
  const c = loadSettings(custom.file);
  assert.equal(c.databasePath, path.join(custom.dir, 'db'));
  assert.equal(c.savesPath, path.join(custom.dir, 'elsewhere', 'saves'));
});

test('old settings files: dataDirectory is the database folder', () => {
  const { dir, file } = write('dataDirectory: ./data\n');
  const s = loadSettings(file);
  assert.equal(s.databasePath, path.join(dir, 'data'));
  assert.equal(s.savesPath, path.join(dir, 'saves'));
  assert.equal(s.legacySavesPath, path.join(dir, 'data', 'saves'));
});

test('saves from the old layout are moved, never deleted', () => {
  const { dir } = write('');
  const old = path.join(dir, 'data', 'saves');
  fs.mkdirSync(path.join(old, '1', '2'), { recursive: true });
  fs.writeFileSync(path.join(old, '1', '2', 'v00001-x.zip'), 'save one');
  fs.writeFileSync(path.join(old, '1', '2', 'v00002-x.zip'), 'save two');
  const db = openDb(path.join(dir, 'data'));
  const saves = new Saves(db, path.join(dir, 'saves'));
  assert.deepEqual(saves.migrateFrom(old), { moved: 2, conflicts: [] });
  assert.equal(fs.readFileSync(path.join(dir, 'saves', '1', '2', 'v00002-x.zip'), 'utf8'), 'save two');
  assert.equal(fs.existsSync(old), false);
  const kept = fs.readdirSync(path.join(dir, 'data')).find((f) => f.startsWith('saves-moved-'));
  assert.ok(kept, 'old folder is kept under a new name');
  assert.equal(saves.migrateFrom(old), null); // nothing left to do
  db.close();
});

test('the startup check reports missing save files', async () => {
  const { dir } = write('');
  const db = openDb(path.join(dir, 'database'));
  const saves = new Saves(db, path.join(dir, 'saves'));
  db.prepare("INSERT INTO users (username, password_hash, created_at) VALUES ('u', 'x', '2026-01-01')").run();
  db.prepare("INSERT INTO emulators (platform, folder, rel_path, name, added_at, updated_at) VALUES ('psx', 'Emu', 'psx/Emu', 'Emu', 'x', 'x')").run();
  const user = db.prepare('SELECT id FROM users').get() as { id: number };
  const emu = db.prepare('SELECT id FROM emulators').get() as { id: number };
  const v = await saves.store(user.id, emu.id, Readable.from([Buffer.from('save data')]), {});
  assert.deepEqual(saves.check(), { writable: true, error: undefined, checked: 1, missing: 0 });
  fs.rmSync(saves.get(user.id, v.version.id).file);
  assert.equal(saves.check().missing, 1);
  db.close();
});
