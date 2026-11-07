import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { AddressInfo } from 'node:net';
import type { Server } from 'node:http';
import { createServer } from '../src/index.js';

let dir: string;
let server: Server;
let base: string;
let token: string;
let adminCookie: string;

const json = async (p: string, init: Omit<RequestInit, 'body'> & { body?: unknown } = {}, auth = true) => {
  const res = await fetch(base + p, {
    ...init,
    headers: {
      ...(init.body !== undefined && !(init.body instanceof Uint8Array) ? { 'Content-Type': 'application/json' } : {}),
      ...(auth && token ? { Authorization: `Bearer ${token}` } : {}),
      ...(adminCookie && p.startsWith('/admin') ? { Cookie: adminCookie } : {}),
      ...((init.headers as Record<string, string>) ?? {}),
    },
    body: init.body === undefined || init.body instanceof Uint8Array ? (init.body as Uint8Array | undefined) : JSON.stringify(init.body),
  });
  const text = await res.text();
  return { status: res.status, headers: res.headers, data: text ? (() => { try { return JSON.parse(text); } catch { return text; } })() : null };
};

before(async () => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), 'nostalgia-test-'));
  const lib = path.join(dir, 'library');
  // A tagged game, a loose multi-disc set, and an emulator.
  fs.mkdirSync(path.join(lib, 'games', 'psx', 'Test Game [igdbid-425]'), { recursive: true });
  fs.writeFileSync(path.join(lib, 'games', 'psx', 'Test Game [igdbid-425]', 'Test (Disc 1).cue'), 'FILE "Test (Disc 1).bin" BINARY');
  fs.writeFileSync(path.join(lib, 'games', 'psx', 'Test Game [igdbid-425]', 'Test (Disc 1).bin'), Buffer.alloc(4096, 7));
  fs.writeFileSync(path.join(lib, 'games', 'psx', 'Loose (Disc 1).bin'), 'a');
  fs.writeFileSync(path.join(lib, 'games', 'psx', 'Loose (Disc 2).bin'), 'b');
  fs.mkdirSync(path.join(lib, 'emulators', 'psx', 'Emu'), { recursive: true });
  fs.writeFileSync(
    path.join(lib, 'emulators', 'psx', 'Emu', 'emu.yaml'),
    'name: TestEmu\nsystems:\n  windows:\n    executable: emu.exe\n    saves: ["{emulatorDir}/saves"]\n  linux:\n    executable: emu\n',
  );
  fs.writeFileSync(path.join(lib, 'emulators', 'psx', 'Emu', 'emu.exe'), 'binary');
  const settings = path.join(dir, 'settings.yaml');
  fs.writeFileSync(settings, `adminUsername: admin\nadminPassword: secret\nlibraryDirectory: ./library\ndataDirectory: ./data\nigdbClientId: ""\nigdbClientSecret: ""\n`);
  const s = await createServer(settings);
  await s.jobs.scan();
  server = s.app.listen(0);
  await new Promise((r) => server.once('listening', r));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

after(() => {
  server?.close();
  try {
    fs.rmSync(dir, { recursive: true, force: true });
  } catch {
    /* sqlite may still hold the file on windows */
  }
});

test('platform folders are created (RomM names)', () => {
  for (const p of ['ps2', 'snes', 'gba', 'n64']) assert.ok(fs.existsSync(path.join(dir, 'library', 'games', p)));
  assert.ok(fs.existsSync(path.join(dir, 'library', 'emulators', 'ps2')));
});

test('register, login, change password', async () => {
  const r = await json('/api/auth/register', { method: 'POST', body: { username: 'alice', password: 'password1' } }, false);
  assert.equal(r.status, 201);
  token = r.data.token;
  assert.equal((await json('/api/auth/register', { method: 'POST', body: { username: 'ALICE', password: 'password1' } }, false)).status, 409);
  assert.equal((await json('/api/me/password', { method: 'POST', body: { oldPassword: 'wrong', newPassword: 'password2' } })).status, 403);
  assert.equal((await json('/api/me/password', { method: 'POST', body: { oldPassword: 'password1', newPassword: 'password2' } })).status, 200);
  const l = await json('/api/auth/login', { method: 'POST', body: { username: 'alice', password: 'password2' } }, false);
  assert.equal(l.status, 200);
  assert.equal((await json('/api/games', {}, false)).status, 401);
});

test('catalog, library with automatic emulators, files and range downloads', async () => {
  const games = (await json('/api/games')).data;
  assert.equal(games.length, 1);
  const g = games[0];
  assert.equal(g.igdbId, 425);
  const emus = (await json('/api/emulators')).data;
  assert.equal(emus[0].name, 'TestEmu');
  const lib = (await json(`/api/library/games/${g.id}`, { method: 'POST' })).data;
  assert.deepEqual(lib, { games: [g.id], emulators: [emus[0].id] });
  const files = (await json(`/api/games/${g.id}/files`)).data;
  assert.equal(files.files.length, 2);
  const res = await fetch(`${base}/api/games/${g.id}/download?path=${encodeURIComponent('Test (Disc 1).bin')}`, {
    headers: { Authorization: `Bearer ${token}`, Range: 'bytes=100-199' },
  });
  assert.equal(res.status, 206);
  assert.equal((await res.arrayBuffer()).byteLength, 100);
  assert.equal((await json(`/api/games/${g.id}/download?path=${encodeURIComponent('../../../settings.yaml')}`)).status, 400);
});

test('cloud saves are versioned, deduplicated and never overwritten', async () => {
  const emu = (await json('/api/emulators')).data[0];
  const up = (body: string) => json(`/api/saves/${emu.id}`, { method: 'POST', body: new TextEncoder().encode(body), headers: { 'Content-Type': 'application/zip', 'X-Device': 'test' } });
  assert.equal((await up('one')).data.version.version, 1);
  const again = await up('one');
  assert.equal(again.data.created, false);
  assert.equal((await up('two')).data.version.version, 2);
  const list = (await json(`/api/saves/${emu.id}`)).data;
  assert.deepEqual(list.map((v: { version: number }) => v.version), [2, 1]);
  const old = await fetch(`${base}/api/saves/version/${list[1].id}/download`, { headers: { Authorization: `Bearer ${token}` } });
  assert.equal(await old.text(), 'one');
  const files = fs.readdirSync(path.join(dir, 'data', 'saves'), { recursive: true }).filter((f) => String(f).endsWith('.zip'));
  assert.equal(files.length, 2);
});

test('play time sessions are idempotent', async () => {
  const g = (await json('/api/games')).data[0];
  const s = { sessionId: 'abc', gameId: g.id, startedAt: new Date().toISOString(), seconds: 125 };
  await json('/api/playtime', { method: 'POST', body: [s, s] });
  await json('/api/playtime', { method: 'POST', body: s });
  const pt = (await json('/api/playtime')).data;
  assert.equal(pt[0].seconds, 125);
});

test('admin: login, unsorted files and import moves (never deletes) files', async () => {
  const bad = await fetch(`${base}/admin/api/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'admin', password: 'x' }) });
  assert.equal(bad.status, 401);
  const ok = await fetch(`${base}/admin/api/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'admin', password: 'secret' }) });
  adminCookie = ok.headers.get('set-cookie')!.split(';')[0];
  const unsorted = (await json('/admin/api/unsorted')).data;
  assert.deepEqual(unsorted.map((u: { path: string }) => u.path).sort(), ['Loose (Disc 1).bin', 'Loose (Disc 2).bin']);
  const imp = await json('/admin/api/import', { method: 'POST', body: { platform: 'psx', entries: ['Loose (Disc 1).bin', 'Loose (Disc 2).bin'], igdbId: 427, name: 'Loose: Game?' } });
  assert.equal(imp.status, 200);
  const folder = path.join(dir, 'library', 'games', 'psx', 'Loose Game [igdbid-427]');
  assert.deepEqual(fs.readdirSync(folder).sort(), ['Loose (Disc 1).bin', 'Loose (Disc 2).bin']);
  assert.equal((await json('/api/games')).data.length, 2);
  assert.equal((await json('/admin/api/users')).data[0].username, 'alice');
});
