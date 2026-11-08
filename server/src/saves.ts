import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import type { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { Transform } from 'node:stream';
import type { SaveVersion } from '@nostalgia/shared';
import { type DB, now } from './db.js';
import { HttpError } from './auth.js';

const MAX_SAVE_BYTES = 2 * 1024 * 1024 * 1024; // 2 GiB per snapshot

interface SaveRow {
  id: number;
  user_id: number;
  emulator_id: number;
  version: number;
  file: string;
  size: number;
  sha256: string;
  device: string | null;
  note: string | null;
  created_at: string;
}

const toVersion = (r: SaveRow): SaveVersion => ({
  id: r.id,
  emulatorId: r.emulator_id,
  version: r.version,
  size: r.size,
  sha256: r.sha256,
  device: r.device ?? undefined,
  note: r.note ?? undefined,
  createdAt: r.created_at,
});

/**
 * Versioned cloud saves, stored per user and per emulator:
 *   <savesDirectory>/<userId>/<emulatorId>/v<version>-<timestamp>.zip
 * The database keeps paths relative to savesDirectory, so the folder can be moved.
 * Nothing in here ever deletes a save.
 */
export class Saves {
  readonly root: string;

  constructor(private db: DB, savesPath: string) {
    this.root = savesPath;
    fs.mkdirSync(this.root, { recursive: true });
  }

  /**
   * Startup check, mainly for network shares: is the folder writable, and are the newest
   * save files there? Missing files usually mean an unmounted share or a moved folder.
   */
  check(): { writable: boolean; error?: string; checked: number; missing: number } {
    let writable = true;
    let error: string | undefined;
    const probe = path.join(this.root, `.write-test-${crypto.randomUUID()}`);
    try {
      fs.writeFileSync(probe, 'nostalgia');
      fs.rmSync(probe, { force: true });
    } catch (e) {
      writable = false;
      error = (e as Error).message;
    }
    const rows = this.db
      .prepare('SELECT s.file FROM saves s JOIN (SELECT user_id, emulator_id, MAX(version) v FROM saves GROUP BY user_id, emulator_id) m ON m.user_id = s.user_id AND m.emulator_id = s.emulator_id AND m.v = s.version')
      .all() as { file: string }[];
    const missing = rows.filter((r) => !fs.existsSync(path.join(this.root, r.file))).length;
    return { writable, error, checked: rows.length, missing };
  }

  latestAll(userId: number): SaveVersion[] {
    const rows = this.db
      .prepare(
        `SELECT s.* FROM saves s JOIN (SELECT emulator_id, MAX(version) v FROM saves WHERE user_id = ? GROUP BY emulator_id) m
         ON m.emulator_id = s.emulator_id AND m.v = s.version WHERE s.user_id = ?`,
      )
      .all(userId, userId) as unknown as SaveRow[];
    return rows.map(toVersion);
  }

  latest(userId: number, emulatorId: number): SaveVersion | null {
    const r = this.db
      .prepare('SELECT * FROM saves WHERE user_id = ? AND emulator_id = ? ORDER BY version DESC LIMIT 1')
      .get(userId, emulatorId) as SaveRow | undefined;
    return r ? toVersion(r) : null;
  }

  list(userId: number, emulatorId: number): SaveVersion[] {
    return (
      this.db.prepare('SELECT * FROM saves WHERE user_id = ? AND emulator_id = ? ORDER BY version DESC').all(userId, emulatorId) as unknown as SaveRow[]
    ).map(toVersion);
  }

  get(userId: number, id: number): { version: SaveVersion; file: string } {
    const r = this.db.prepare('SELECT * FROM saves WHERE id = ? AND user_id = ?').get(id, userId) as SaveRow | undefined;
    if (!r) throw new HttpError(404, 'Save not found');
    return { version: toVersion(r), file: path.join(this.root, r.file) };
  }

  countForUser(userId: number): { count: number; bytes: number } {
    const r = this.db.prepare('SELECT COUNT(*) c, COALESCE(SUM(size),0) b FROM saves WHERE user_id = ?').get(userId) as { c: number; b: number };
    return { count: r.c, bytes: r.b };
  }

  /**
   * Store an uploaded snapshot (zip). If it is byte-identical to the newest version,
   * no new version is created and the existing one is returned (`created: false`).
   */
  async store(
    userId: number,
    emulatorId: number,
    body: Readable,
    meta: { device?: string; note?: string; expectedSha256?: string },
  ): Promise<{ version: SaveVersion; created: boolean }> {
    const dir = path.join(this.root, String(userId), String(emulatorId));
    fs.mkdirSync(dir, { recursive: true });
    const tmp = path.join(dir, `.upload-${crypto.randomUUID()}.tmp`);
    const hash = crypto.createHash('sha256');
    let size = 0;
    const meter = new Transform({
      transform(chunk: Buffer, _enc, cb) {
        size += chunk.length;
        if (size > MAX_SAVE_BYTES) return cb(new HttpError(413, 'Save too large'));
        hash.update(chunk);
        cb(null, chunk);
      },
    });
    try {
      await pipeline(body, meter, fs.createWriteStream(tmp));
    } catch (e) {
      fs.rmSync(tmp, { force: true }); // incomplete temp upload, not a save
      throw e;
    }
    const sha256 = hash.digest('hex');
    if (meta.expectedSha256 && meta.expectedSha256.toLowerCase() !== sha256) {
      fs.rmSync(tmp, { force: true });
      throw new HttpError(400, 'Checksum mismatch, upload corrupted');
    }
    if (size === 0) {
      fs.rmSync(tmp, { force: true });
      throw new HttpError(400, 'Empty save upload');
    }
    const latest = this.latest(userId, emulatorId);
    if (latest && latest.sha256 === sha256) {
      fs.rmSync(tmp, { force: true });
      return { version: latest, created: false };
    }
    const version = (latest?.version ?? 0) + 1;
    const stamp = new Date().toISOString().replace(/[-:]/g, '').replace(/\..+/, '');
    const rel = `${userId}/${emulatorId}/v${String(version).padStart(5, '0')}-${stamp}.zip`;
    fs.renameSync(tmp, path.join(this.root, rel));
    const res = this.db
      .prepare('INSERT INTO saves (user_id, emulator_id, version, file, size, sha256, device, note, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)')
      .run(userId, emulatorId, version, rel, size, sha256, meta.device?.slice(0, 100) ?? null, meta.note?.slice(0, 300) ?? null, now());
    const row = this.db.prepare('SELECT * FROM saves WHERE id = ?').get(Number(res.lastInsertRowid)) as unknown as SaveRow;
    return { version: toVersion(row), created: true };
  }
}
