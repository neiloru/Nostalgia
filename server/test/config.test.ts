import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { loadSettings } from '../src/config.js';
import { openDb } from '../src/db.js';
import { Saves } from '../src/saves.js';

const write = (body: string) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'nostalgia-config-'));
  const file = path.join(dir, 'settings.yaml');
  fs.writeFileSync(file, body);
  return { dir, file };
};

test('cloud saves default to <dataDirectory>/saves', () => {
  const { dir, file } = write('dataDirectory: ./data\n');
  const s = loadSettings(file);
  assert.equal(s.savesPath, path.join(dir, 'data', 'saves'));
});

test('savesDirectory moves cloud saves away from the database', () => {
  const { dir, file } = write('dataDirectory: ./data\nsavesDirectory: ./share/saves\n');
  const s = loadSettings(file);
  assert.equal(s.dataPath, path.join(dir, 'data'));
  assert.equal(s.savesPath, path.join(dir, 'share', 'saves'));
  const prev = process.env.NOSTALGIA_SAVES;
  process.env.NOSTALGIA_SAVES = path.join(dir, 'env-saves');
  try {
    assert.equal(loadSettings(file).savesPath, path.join(dir, 'env-saves'));
  } finally {
    if (prev === undefined) delete process.env.NOSTALGIA_SAVES;
    else process.env.NOSTALGIA_SAVES = prev;
  }
});

test('the startup check reports missing save files', async () => {
  const { dir } = write('');
  const db = openDb(path.join(dir, 'data'));
  const saves = new Saves(db, path.join(dir, 'saves'));
  db.prepare("INSERT INTO users (username, password_hash, created_at) VALUES ('u', 'x', '2026-01-01')").run();
  db.prepare("INSERT INTO emulators (platform, folder, rel_path, name, added_at, updated_at) VALUES ('psx', 'Emu', 'psx/Emu', 'Emu', 'x', 'x')").run();
  const user = db.prepare('SELECT id FROM users').get() as { id: number };
  const emu = db.prepare('SELECT id FROM emulators').get() as { id: number };
  const { Readable } = await import('node:stream');
  const v = await saves.store(user.id, emu.id, Readable.from([Buffer.from('save data')]), {});
  assert.deepEqual(saves.check(), { writable: true, error: undefined, checked: 1, missing: 0 });
  fs.rmSync(saves.get(user.id, v.version.id).file);
  assert.equal(saves.check().missing, 1);
  db.close();
});
