import fs from 'node:fs';
import path from 'node:path';
import express, { type Request, type Response, type Router } from 'express';
import type { FileListing, GameDetail, LibraryState, PlaySession, PlaytimeEntry } from '@nostalgia/shared';
import { PLATFORMS, API_VERSION } from '@nostalgia/shared';
import { type Auth, HttpError, requireUser } from '../auth.js';
import type { DB } from '../db.js';
import { now } from '../db.js';
import type { EventHub } from '../events.js';
import { ASSET_DIR, type Library, parseMetadata, resolveInside, toEmulatorSummary, toGameSummary, walkFiles } from '../library.js';
import type { Saves } from '../saves.js';
import type { LoadedSettings } from '../config.js';

export interface ApiDeps {
  db: DB;
  auth: Auth;
  library: Library;
  saves: Saves;
  events: EventHub;
  settings: LoadedSettings;
}

const idParam = (req: Request, name = 'id') => {
  const n = Number(req.params[name]);
  if (!Number.isInteger(n) || n <= 0) throw new HttpError(400, `Invalid ${name}`);
  return n;
};

const device = (req: Request) => (typeof req.headers['x-device'] === 'string' ? req.headers['x-device'].slice(0, 100) : undefined);

function sendFileSafe(res: Response, base: string, rel: string, download = true) {
  const abs = resolveInside(base, rel);
  if (!fs.existsSync(abs) || !fs.statSync(abs).isFile()) throw new HttpError(404, 'File not found');
  if (download) res.setHeader('Content-Disposition', `attachment; filename*=UTF-8''${encodeURIComponent(path.basename(abs))}`);
  // express' sendFile supports HTTP Range requests (used to pause / resume downloads).
  res.sendFile(abs, { dotfiles: 'allow', acceptRanges: true, cacheControl: false, lastModified: true });
}

export function apiRouter(d: ApiDeps): Router {
  const r = express.Router();
  const { db, auth, library, saves, events } = d;
  const user = requireUser(auth);

  r.get('/health', (_req, res) => {
    res.json({ ok: true, name: 'nostalgia', apiVersion: API_VERSION, time: now(), registration: d.settings.allowRegistration });
  });

  // ---------- auth ----------
  r.post('/auth/register', express.json(), (req, res) => {
    if (!d.settings.allowRegistration) throw new HttpError(403, 'Registration is disabled on this server');
    const u = auth.register(req.body?.username, req.body?.password);
    res.status(201).json({ token: auth.createSession(u.id, device(req)), user: u });
  });

  r.post('/auth/login', express.json(), (req, res) => {
    const u = auth.login(req.body?.username, req.body?.password);
    res.json({ token: auth.createSession(u.id, device(req)), user: u });
  });

  r.post('/auth/logout', user, (req, res) => {
    auth.revokeSession(req.token!);
    res.json({ ok: true });
  });

  r.get('/me', user, (req, res) => res.json(req.user));

  r.post('/me/password', user, express.json(), (req, res) => {
    const { oldPassword, newPassword } = req.body ?? {};
    if (!auth.checkPassword(req.user!.id, String(oldPassword ?? ''))) throw new HttpError(403, 'Current password is wrong');
    auth.setPassword(req.user!.id, newPassword);
    auth.revokeAllForUser(req.user!.id, req.token); // sign out other devices
    res.json({ ok: true });
  });

  // ---------- catalog ----------
  r.get('/platforms', user, (_req, res) => res.json(PLATFORMS));

  r.get('/games', user, (_req, res) => {
    res.json(library.listGames().map(toGameSummary));
  });

  r.get('/games/:id', user, (req, res) => {
    const row = library.getGame(idParam(req));
    if (!row || row.missing || row.hidden) throw new HttpError(404, 'Game not found');
    const detail: GameDetail = { ...toGameSummary(row), metadata: parseMetadata(row) };
    res.json(detail);
  });

  r.get('/games/:id/files', user, (req, res) => {
    const row = library.getGame(idParam(req));
    if (!row || row.missing || row.hidden) throw new HttpError(404, 'Game not found');
    const files = walkFiles(library.gameDir(row));
    const listing: FileListing = { id: row.id, folder: row.folder, files, totalBytes: files.reduce((a, f) => a + f.size, 0) };
    res.json(listing);
  });

  r.get('/games/:id/download', user, (req, res) => {
    const row = library.getGame(idParam(req));
    if (!row || row.missing || row.hidden) throw new HttpError(404, 'Game not found');
    sendFileSafe(res, library.gameDir(row), String(req.query.path ?? ''));
  });

  r.get('/games/:id/media/:name', user, (req, res) => {
    const row = library.getGame(idParam(req));
    if (!row) throw new HttpError(404, 'Game not found');
    const name = String(req.params.name);
    if (!/^[\w.-]+\.(jpg|jpeg|png|webp)$/i.test(name)) throw new HttpError(400, 'Invalid media name');
    res.setHeader('Cache-Control', 'private, max-age=86400');
    sendFileSafe(res, path.join(library.gameDir(row), ASSET_DIR), name, false);
  });

  r.get('/emulators', user, (_req, res) => res.json(library.listEmulators().map(toEmulatorSummary)));

  r.get('/emulators/:id', user, (req, res) => {
    const row = library.getEmulator(idParam(req));
    if (!row || row.missing) throw new HttpError(404, 'Emulator not found');
    res.json(toEmulatorSummary(row));
  });

  r.get('/emulators/:id/files', user, (req, res) => {
    const row = library.getEmulator(idParam(req));
    if (!row || row.missing) throw new HttpError(404, 'Emulator not found');
    const files = walkFiles(library.emulatorDir(row));
    const listing: FileListing = { id: row.id, folder: row.folder, files, totalBytes: files.reduce((a, f) => a + f.size, 0) };
    res.json(listing);
  });

  r.get('/emulators/:id/download', user, (req, res) => {
    const row = library.getEmulator(idParam(req));
    if (!row || row.missing) throw new HttpError(404, 'Emulator not found');
    sendFileSafe(res, library.emulatorDir(row), String(req.query.path ?? ''));
  });

  // ---------- personal library ----------
  const libraryState = (userId: number): LibraryState => ({
    games: (db.prepare('SELECT game_id FROM user_games WHERE user_id = ? ORDER BY added_at').all(userId) as { game_id: number }[]).map((x) => x.game_id),
    emulators: (db.prepare('SELECT emulator_id FROM user_emulators WHERE user_id = ? ORDER BY added_at').all(userId) as { emulator_id: number }[]).map(
      (x) => x.emulator_id,
    ),
  });

  const changed = (req: Request, res: Response) => {
    const state = libraryState(req.user!.id);
    events.toUser(req.user!.id, { type: 'library', library: state }, device(req));
    res.json(state);
  };

  r.get('/library', user, (req, res) => res.json(libraryState(req.user!.id)));

  r.post('/library/games/:id', user, (req, res) => {
    const row = library.getGame(idParam(req));
    if (!row || row.missing) throw new HttpError(404, 'Game not found');
    db.prepare('INSERT OR IGNORE INTO user_games (user_id, game_id, added_at) VALUES (?, ?, ?)').run(req.user!.id, row.id, now());
    // Emulators for this platform are added automatically.
    for (const emu of library.listEmulators().filter((e) => e.platform === row.platform)) {
      db.prepare('INSERT OR IGNORE INTO user_emulators (user_id, emulator_id, added_at, auto) VALUES (?, ?, ?, 1)').run(req.user!.id, emu.id, now());
    }
    changed(req, res);
  });

  r.delete('/library/games/:id', user, (req, res) => {
    db.prepare('DELETE FROM user_games WHERE user_id = ? AND game_id = ?').run(req.user!.id, idParam(req));
    changed(req, res);
  });

  r.post('/library/emulators/:id', user, (req, res) => {
    const row = library.getEmulator(idParam(req));
    if (!row || row.missing) throw new HttpError(404, 'Emulator not found');
    db.prepare('INSERT OR IGNORE INTO user_emulators (user_id, emulator_id, added_at, auto) VALUES (?, ?, ?, 0)').run(req.user!.id, row.id, now());
    changed(req, res);
  });

  r.delete('/library/emulators/:id', user, (req, res) => {
    db.prepare('DELETE FROM user_emulators WHERE user_id = ? AND emulator_id = ?').run(req.user!.id, idParam(req));
    changed(req, res);
  });

  // ---------- cloud saves (per emulator, versioned, never deleted) ----------
  r.get('/saves', user, (req, res) => res.json(saves.latestAll(req.user!.id)));

  r.get('/saves/:emulatorId', user, (req, res) => res.json(saves.list(req.user!.id, idParam(req, 'emulatorId'))));

  r.post('/saves/:emulatorId', user, async (req, res) => {
    const emulatorId = idParam(req, 'emulatorId');
    if (!library.getEmulator(emulatorId)) throw new HttpError(404, 'Emulator not found');
    const header = (n: string) => (typeof req.headers[n] === 'string' ? decodeURIComponent(req.headers[n] as string) : undefined);
    const result = await saves.store(req.user!.id, emulatorId, req, {
      device: device(req),
      note: header('x-save-note'),
      expectedSha256: header('x-content-sha256'),
    });
    if (result.created) events.toUser(req.user!.id, { type: 'save', emulatorId, version: result.version, fromDevice: device(req) }, device(req));
    res.status(result.created ? 201 : 200).json(result);
  });

  r.get('/saves/version/:id/download', user, (req, res) => {
    const { version, file } = saves.get(req.user!.id, idParam(req));
    res.setHeader('X-Save-Version', String(version.version));
    res.setHeader('X-Content-Sha256', version.sha256);
    res.sendFile(file, { dotfiles: 'allow' });
  });

  // ---------- play time ----------
  r.post('/playtime', user, express.json(), (req, res) => {
    const sessions: PlaySession[] = Array.isArray(req.body) ? req.body : [req.body];
    let stored = 0;
    for (const s of sessions) {
      const seconds = Math.round(Number(s?.seconds));
      if (!s?.sessionId || !Number.isFinite(seconds) || seconds <= 0 || !library.getGame(Number(s.gameId))) continue;
      const r2 = db
        .prepare('INSERT OR IGNORE INTO play_sessions (session_id, user_id, game_id, started_at, seconds, device, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)')
        .run(String(s.sessionId).slice(0, 64), req.user!.id, Number(s.gameId), String(s.startedAt ?? now()), Math.min(seconds, 7 * 86400), device(req) ?? null, now());
      stored += Number(r2.changes);
    }
    res.json({ ok: true, stored });
  });

  r.get('/playtime', user, (req, res) => {
    const rows = db
      .prepare(
        `SELECT game_id gameId, SUM(seconds) seconds, COUNT(*) sessions, MAX(datetime(started_at, '+' || seconds || ' seconds')) lastPlayed
         FROM play_sessions WHERE user_id = ? GROUP BY game_id`,
      )
      .all(req.user!.id) as unknown as PlaytimeEntry[];
    res.json(rows.map((x) => ({ ...x, lastPlayed: x.lastPlayed ? new Date(x.lastPlayed + 'Z').toISOString() : undefined })));
  });

  // ---------- push events ----------
  r.get('/events', user, (req, res) => {
    events.add(res, req.user!.id, device(req) ?? (typeof req.query.device === 'string' ? req.query.device : undefined));
  });

  return r;
}
