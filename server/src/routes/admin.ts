import fs from 'node:fs';
import path from 'node:path';
import express, { type Request, type Router } from 'express';
import { PLATFORMS, platformName } from '@nostalgia/shared';
import { ADMIN_COOKIE, type Auth, HttpError, cookie, requireAdmin, validateUsername } from '../auth.js';
import type { DB } from '../db.js';
import { now } from '../db.js';
import type { EventHub } from '../events.js';
import type { Igdb } from '../igdb.js';
import type { Jobs } from '../jobs.js';
import { type Library, parseMetadata, readEmuConfig, toEmulatorSummary, toGameSummary } from '../library.js';
import type { Saves } from '../saves.js';
import type { LoadedSettings } from '../config.js';

export interface AdminDeps {
  db: DB;
  auth: Auth;
  library: Library;
  saves: Saves;
  jobs: Jobs;
  igdb: Igdb;
  events: EventHub;
  settings: LoadedSettings;
}

const idParam = (req: Request) => {
  const n = Number(req.params.id);
  if (!Number.isInteger(n) || n <= 0) throw new HttpError(400, 'Invalid id');
  return n;
};

export function adminRouter(d: AdminDeps): Router {
  const r = express.Router();
  const { db, auth, library, jobs, igdb, saves, events, settings } = d;
  r.use(express.json({ limit: '2mb' }));

  r.post('/login', (req, res) => {
    const token = auth.adminLogin(req.body?.username, req.body?.password);
    res.setHeader('Set-Cookie', `${ADMIN_COOKIE}=${token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200`);
    res.json({ ok: true });
  });

  r.post('/logout', (req, res) => {
    const t = cookie(req, ADMIN_COOKIE);
    if (t) auth.adminLogout(t);
    res.setHeader('Set-Cookie', `${ADMIN_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0`);
    res.json({ ok: true });
  });

  r.use(requireAdmin(auth));

  r.get('/overview', (_req, res) => {
    const c = (sql: string) => (db.prepare(sql).get() as { c: number }).c;
    res.json({
      games: c('SELECT COUNT(*) c FROM games WHERE missing = 0'),
      missingGames: c('SELECT COUNT(*) c FROM games WHERE missing = 1'),
      emulators: c('SELECT COUNT(*) c FROM emulators WHERE missing = 0'),
      users: c('SELECT COUNT(*) c FROM users'),
      saves: c('SELECT COUNT(*) c FROM saves'),
      saveBytes: c('SELECT COALESCE(SUM(size),0) c FROM saves'),
      playSeconds: c('SELECT COALESCE(SUM(seconds),0) c FROM play_sessions'),
      librarySize: c('SELECT COALESCE(SUM(size_bytes),0) c FROM games WHERE missing = 0'),
      metadataMissing: c("SELECT COUNT(*) c FROM games WHERE missing = 0 AND metadata_status != 'ok'"),
      unsorted: jobs.unsorted.length,
      connectedClients: events.count,
      onlineUsers: events.onlineUserIds().length,
      igdbConfigured: igdb.configured,
      registration: settings.allowRegistration,
      libraryPath: settings.libraryPath,
      dataPath: settings.dataPath,
      savesPath: settings.savesPath,
      savesStorage: saves.check(),
      settingsFile: settings.file,
      jobs: jobs.status,
    });
  });

  r.get('/jobs', (_req, res) => res.json(jobs.status));

  r.post('/scan', (req, res) => {
    const hash = !!req.body?.hash;
    if (jobs.status.scan.running) throw new HttpError(409, 'A scan is already running');
    void jobs.scan({ hash });
    res.status(202).json({ ok: true });
  });

  r.get('/platforms', (_req, res) => res.json(PLATFORMS));

  // ---------- games ----------
  r.get('/games', (_req, res) => {
    const hashRows = db
      .prepare(
        `SELECT game_id, COUNT(*) files, SUM(hasheous_status = 'match') matched, SUM(hasheous_status = 'nomatch') unmatched,
                SUM(hasheous_status = 'error') errors FROM file_hashes GROUP BY game_id`,
      )
      .all() as { game_id: number; files: number; matched: number; unmatched: number; errors: number }[];
    const hashes = new Map(hashRows.map((h) => [h.game_id, h]));
    const libCount = new Map(
      (db.prepare('SELECT game_id, COUNT(*) c FROM user_games GROUP BY game_id').all() as { game_id: number; c: number }[]).map((x) => [x.game_id, x.c]),
    );
    res.json(
      library.listAllGames().map((g) => ({
        ...toGameSummary(g),
        relPath: g.rel_path,
        missing: !!g.missing,
        hidden: !!g.hidden,
        metadataStatus: g.metadata_status,
        metadataError: g.metadata_error,
        hash: hashes.get(g.id) ?? null,
        inLibraries: libCount.get(g.id) ?? 0,
      })),
    );
  });

  r.get('/games/:id', (req, res) => {
    const g = library.getGame(idParam(req));
    if (!g) throw new HttpError(404, 'Game not found');
    const files = db
      .prepare('SELECT rel_path path, size, md5, sha1, crc32, hasheous_status status, hasheous_name name, checked_at checkedAt FROM file_hashes WHERE game_id = ? ORDER BY rel_path')
      .all(g.id);
    res.json({ ...toGameSummary(g), relPath: g.rel_path, missing: !!g.missing, hidden: !!g.hidden, metadataStatus: g.metadata_status, metadataError: g.metadata_error, metadata: parseMetadata(g), hashes: files });
  });

  r.get('/games/:id/media/:name', (req, res) => {
    const g = library.getGame(idParam(req));
    const name = String(req.params.name);
    if (!g || !/^[\w.-]+\.(jpg|jpeg|png|webp)$/i.test(name)) throw new HttpError(404, 'Not found');
    const file = path.join(library.gameDir(g), 'igdb', name);
    if (!fs.existsSync(file)) throw new HttpError(404, 'Not found');
    res.setHeader('Cache-Control', 'private, max-age=3600');
    res.sendFile(file);
  });

  r.post('/games/:id/refresh', (req, res) => {
    const id = idParam(req);
    library.resetMetadata(id);
    jobs.queueMetadata(id);
    res.status(202).json({ ok: true });
  });

  r.post('/games/:id/hash', (req, res) => {
    jobs.queueHash(idParam(req));
    res.status(202).json({ ok: true });
  });

  r.post('/games/:id/match', (req, res) => {
    const row = library.rematchGame(idParam(req), Number(req.body?.igdbId), req.body?.name);
    jobs.queueMetadata(row.id);
    events.toAll({ type: 'catalog' });
    res.json(toGameSummary(row));
  });

  r.post('/games/:id/hidden', (req, res) => {
    library.setHidden(idParam(req), !!req.body?.hidden);
    events.toAll({ type: 'catalog' });
    res.json({ ok: true });
  });

  // ---------- import of unsorted files ----------
  r.get('/unsorted', (_req, res) => res.json(jobs.unsorted.map((u) => ({ ...u, platformName: platformName(u.platform) }))));

  r.post('/unsorted/identify', async (req, res) => {
    res.json(await jobs.identify(String(req.body?.platform ?? ''), String(req.body?.path ?? '')));
  });

  r.post('/import', async (req, res) => {
    const { platform, entries, igdbId, name } = req.body ?? {};
    if (!platform || !Array.isArray(entries)) throw new HttpError(400, 'platform and entries are required');
    const rel = library.importEntries(String(platform), entries.map(String), Number(igdbId), String(name ?? ''));
    await jobs.scan();
    res.json({ ok: true, relPath: rel });
  });

  r.get('/igdb/search', async (req, res) => {
    if (!igdb.configured) throw new HttpError(400, 'IGDB credentials are not configured in the settings file');
    res.json(await igdb.search(String(req.query.q ?? ''), typeof req.query.platform === 'string' ? req.query.platform : undefined));
  });

  // ---------- emulators ----------
  r.get('/emulators', (_req, res) => {
    const libCount = new Map(
      (db.prepare('SELECT emulator_id, COUNT(*) c FROM user_emulators GROUP BY emulator_id').all() as { emulator_id: number; c: number }[]).map((x) => [
        x.emulator_id,
        x.c,
      ]),
    );
    res.json(library.listEmulators(true).map((e) => ({ ...toEmulatorSummary(e), relPath: e.rel_path, missing: !!e.missing, inLibraries: libCount.get(e.id) ?? 0 })));
  });

  r.get('/emulators/:id/yaml', (req, res) => {
    const e = library.getEmulator(idParam(req));
    if (!e) throw new HttpError(404, 'Emulator not found');
    const { file } = readEmuConfig(library.emulatorDir(e));
    res.json({ text: file ? fs.readFileSync(file, 'utf8') : '', exists: !!file, path: file ?? path.join(library.emulatorDir(e), 'emu.yaml') });
  });

  r.put('/emulators/:id/yaml', (req, res) => {
    library.writeEmuYaml(idParam(req), String(req.body?.text ?? ''));
    events.toAll({ type: 'catalog' });
    res.json({ ok: true });
  });

  // ---------- users ----------
  r.get('/users', (_req, res) => {
    const rows = db.prepare('SELECT id, username, created_at createdAt, disabled FROM users ORDER BY username COLLATE NOCASE').all() as {
      id: number;
      username: string;
      createdAt: string;
      disabled: number;
    }[];
    const online = new Set(events.onlineUserIds());
    res.json(
      rows.map((u) => ({
        ...u,
        disabled: !!u.disabled,
        online: online.has(u.id),
        games: (db.prepare('SELECT COUNT(*) c FROM user_games WHERE user_id = ?').get(u.id) as { c: number }).c,
        playSeconds: (db.prepare('SELECT COALESCE(SUM(seconds),0) c FROM play_sessions WHERE user_id = ?').get(u.id) as { c: number }).c,
        saves: saves.countForUser(u.id),
        lastSeen: (db.prepare('SELECT MAX(last_seen) c FROM sessions WHERE user_id = ?').get(u.id) as { c: string | null }).c,
      })),
    );
  });

  r.post('/users', (req, res) => {
    const u = auth.register(req.body?.username, req.body?.password);
    res.status(201).json(u);
  });

  r.get('/users/:id', (req, res) => {
    const id = idParam(req);
    const u = auth.getUser(id);
    if (!u) throw new HttpError(404, 'User not found');
    const games = db
      .prepare(
        `SELECT g.id, g.name, g.platform, ug.added_at addedAt,
           (SELECT COALESCE(SUM(seconds),0) FROM play_sessions p WHERE p.user_id = ug.user_id AND p.game_id = g.id) seconds
         FROM user_games ug JOIN games g ON g.id = ug.game_id WHERE ug.user_id = ? ORDER BY g.name`,
      )
      .all(id);
    const saveRows = db
      .prepare(
        `SELECT s.id, s.version, s.size, s.device, s.note, s.created_at createdAt, e.name emulator, e.platform
         FROM saves s JOIN emulators e ON e.id = s.emulator_id WHERE s.user_id = ? ORDER BY s.created_at DESC LIMIT 200`,
      )
      .all(id);
    res.json({ user: u, games, saves: saveRows });
  });

  r.post('/users/:id/password', (req, res) => {
    const id = idParam(req);
    if (!auth.getUser(id)) throw new HttpError(404, 'User not found');
    auth.setPassword(id, req.body?.password);
    auth.revokeAllForUser(id);
    res.json({ ok: true });
  });

  r.post('/users/:id/disabled', (req, res) => {
    const id = idParam(req);
    db.prepare('UPDATE users SET disabled = ? WHERE id = ?').run(req.body?.disabled ? 1 : 0, id);
    if (req.body?.disabled) auth.revokeAllForUser(id);
    res.json({ ok: true });
  });

  r.post('/users/:id/rename', (req, res) => {
    const id = idParam(req);
    const name = validateUsername(req.body?.username);
    const clash = db.prepare('SELECT id FROM users WHERE username = ? AND id != ?').get(name, id);
    if (clash) throw new HttpError(409, 'Username already taken');
    db.prepare('UPDATE users SET username = ? WHERE id = ?').run(name, id);
    res.json({ ok: true, at: now() });
  });

  r.get('/saves/:id/download', (req, res) => {
    const row = db.prepare('SELECT user_id FROM saves WHERE id = ?').get(idParam(req)) as { user_id: number } | undefined;
    if (!row) throw new HttpError(404, 'Save not found');
    const { file, version } = saves.get(row.user_id, idParam(req));
    res.download(file, `save-u${row.user_id}-e${version.emulatorId}-v${version.version}.zip`);
  });

  return r;
}
