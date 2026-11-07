import fs from 'node:fs';
import path from 'node:path';
import type { DB } from './db.js';
import { now } from './db.js';
import type { Library, ScanResult, UnsortedFile } from './library.js';
import { resolveInside, walkFiles } from './library.js';
import type { Igdb } from './igdb.js';
import { hashFile, lookupHasheous, shouldHash, type FileHashes, type HasheousResult } from './hasheous.js';
import type { EventHub } from './events.js';
import { log } from './log.js';

export interface JobStatus {
  scan: { running: boolean; startedAt?: string; last?: ScanResult; lastError?: string };
  metadata: { running: boolean; queue: number; current?: string; done: number; failed: number };
  hashing: { running: boolean; queue: number; current?: string; filesDone: number; bytesDone: number; bytesTotal: number };
}

export class Jobs {
  status: JobStatus = {
    scan: { running: false },
    metadata: { running: false, queue: 0, done: 0, failed: 0 },
    hashing: { running: false, queue: 0, filesDone: 0, bytesDone: 0, bytesTotal: 0 },
  };
  private metaQueue: number[] = [];
  private metaCurrent: number | null = null;
  private hashQueue: number[] = [];
  private identifyCache = new Map<string, HasheousResult & { file?: string; md5?: string }>();

  constructor(
    private db: DB,
    private library: Library,
    private igdb: Igdb,
    private events: EventHub,
  ) {}

  get unsorted(): UnsortedFile[] {
    return this.status.scan.last?.unsorted ?? [];
  }

  /** Scan the library. Metadata for new games is fetched in the background; hashing is optional. */
  async scan(opts: { hash?: boolean } = {}): Promise<ScanResult | undefined> {
    if (this.status.scan.running) return undefined;
    this.status.scan = { ...this.status.scan, running: true, startedAt: now(), lastError: undefined };
    try {
      // Let the event loop breathe before the (synchronous) walk.
      await new Promise((r) => setImmediate(r));
      const result = this.library.scan();
      this.status.scan.last = result;
      log.info(
        `Scan done: ${result.games} games (+${result.newGames}), ${result.emulators} emulators (+${result.newEmulators}), ${result.unsorted.length} unsorted`,
      );
      const pending = this.db.prepare("SELECT id FROM games WHERE missing = 0 AND metadata_status = 'pending'").all() as { id: number }[];
      for (const p of pending) this.queueMetadata(p.id);
      if (opts.hash) for (const g of this.library.listGames(true)) this.queueHash(g.id);
      if (result.newGames || result.newEmulators || result.missingGames) this.events.toAll({ type: 'catalog' });
      return result;
    } catch (e) {
      this.status.scan.lastError = (e as Error).message;
      log.error('Scan failed', e);
      return undefined;
    } finally {
      this.status.scan.running = false;
    }
  }

  queueMetadata(gameId: number) {
    if (!this.metaQueue.includes(gameId) && this.metaCurrent !== gameId) this.metaQueue.push(gameId);
    this.status.metadata.queue = this.metaQueue.length;
    void this.runMetadata();
  }

  private async runMetadata() {
    if (this.status.metadata.running) return;
    this.status.metadata.running = true;
    let changed = false;
    try {
      while (this.metaQueue.length) {
        const id = this.metaQueue.shift()!;
        this.metaCurrent = id;
        this.status.metadata.queue = this.metaQueue.length;
        const row = this.library.getGame(id);
        if (!row || row.missing) continue;
        this.status.metadata.current = row.folder;
        if (!this.igdb.configured) {
          this.db
            .prepare("UPDATE games SET metadata_status = 'error', metadata_error = 'IGDB credentials not configured' WHERE id = ?")
            .run(id);
          this.status.metadata.failed++;
          continue;
        }
        try {
          const meta = await this.igdb.downloadInto(this.library.gameDir(row), row.igdb_id);
          this.db
            .prepare("UPDATE games SET metadata_json = ?, metadata_status = 'ok', metadata_error = NULL, name = ?, updated_at = ? WHERE id = ?")
            .run(JSON.stringify(meta, null, 2), meta.name, now(), id);
          // Asset files changed the folder size.
          const files = walkFiles(this.library.gameDir(row));
          this.db
            .prepare('UPDATE games SET size_bytes = ?, file_count = ? WHERE id = ?')
            .run(files.reduce((a, f) => a + f.size, 0), files.length, id);
          this.status.metadata.done++;
          changed = true;
          log.info(`Metadata downloaded for ${row.folder}`);
        } catch (e) {
          this.status.metadata.failed++;
          this.db.prepare("UPDATE games SET metadata_status = 'error', metadata_error = ? WHERE id = ?").run((e as Error).message, id);
          log.warn(`Metadata failed for ${row.folder}: ${(e as Error).message}`);
        }
      }
    } finally {
      this.status.metadata.running = false;
      this.status.metadata.current = undefined;
      this.metaCurrent = null;
      if (changed) this.events.toAll({ type: 'catalog' });
    }
  }

  queueHash(gameId: number) {
    if (!this.hashQueue.includes(gameId)) this.hashQueue.push(gameId);
    this.status.hashing.queue = this.hashQueue.length;
    void this.runHash();
  }

  private async runHash() {
    if (this.status.hashing.running) return;
    const h = this.status.hashing;
    h.running = true;
    h.filesDone = 0;
    h.bytesDone = 0;
    h.bytesTotal = 0;
    try {
      while (this.hashQueue.length) {
        const id = this.hashQueue.shift()!;
        h.queue = this.hashQueue.length;
        const row = this.library.getGame(id);
        if (!row || row.missing) continue;
        const dir = this.library.gameDir(row);
        for (const f of walkFiles(dir).filter((x) => shouldHash(x.path))) {
          const known = this.db
            .prepare('SELECT size, mtime, md5, sha1, crc32, hasheous_status FROM file_hashes WHERE game_id = ? AND rel_path = ?')
            .get(id, f.path) as { size: number; mtime: number; md5: string; sha1: string; crc32: string; hasheous_status: string } | undefined;
          const unchanged = known && known.size === f.size && known.mtime === f.mtime && known.md5;
          if (unchanged && known.hasheous_status !== 'error') continue;
          h.current = `${row.folder}/${f.path}`;
          // Only the lookup failed last time: reuse the stored hashes instead of reading the file again.
          let hashes: FileHashes;
          if (unchanged) hashes = { md5: known.md5, sha1: known.sha1, crc32: known.crc32 };
          else {
            h.bytesTotal += f.size;
            hashes = await hashFile(path.join(dir, f.path), (n) => (h.bytesDone += n));
          }
          const res = await lookupHasheous(hashes);
          this.db
            .prepare(
              `INSERT INTO file_hashes (game_id, rel_path, size, mtime, md5, sha1, crc32, hasheous_status, hasheous_name, hasheous_json, checked_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(game_id, rel_path) DO UPDATE SET size = excluded.size, mtime = excluded.mtime, md5 = excluded.md5,
                 sha1 = excluded.sha1, crc32 = excluded.crc32, hasheous_status = excluded.hasheous_status,
                 hasheous_name = excluded.hasheous_name, hasheous_json = excluded.hasheous_json, checked_at = excluded.checked_at`,
            )
            .run(
              id,
              f.path,
              f.size,
              f.mtime,
              hashes.md5,
              hashes.sha1,
              hashes.crc32,
              res.status,
              res.romName ?? res.name ?? res.error ?? null,
              res.raw ? JSON.stringify(res.raw) : null,
              now(),
            );
          h.filesDone++;
        }
      }
    } catch (e) {
      log.error('Hashing failed', e);
    } finally {
      h.running = false;
      h.current = undefined;
    }
  }

  /** Hash an unsorted entry (file or the first hashable file of a folder) and ask Hasheous what it is. */
  async identify(platform: string, entry: string): Promise<HasheousResult & { file?: string; md5?: string }> {
    const pdir = resolveInside(this.library.gamesPath, platform);
    const abs = resolveInside(pdir, entry);
    const st = fs.statSync(abs);
    let file = abs;
    if (st.isDirectory()) {
      const f = walkFiles(abs).find((x) => shouldHash(x.path));
      if (!f) return { status: 'nomatch' };
      file = path.join(abs, f.path);
    } else if (/\.cue$/i.test(abs)) {
      // Prefer the bin referenced by a cue sheet.
      const txt = fs.readFileSync(abs, 'utf8');
      const m = /FILE\s+"([^"]+)"/i.exec(txt);
      if (m && fs.existsSync(path.join(pdir, m[1]))) file = path.join(pdir, m[1]);
    }
    const fst = fs.statSync(file);
    const key = `${file}|${fst.size}|${fst.mtimeMs}`;
    const cached = this.identifyCache.get(key);
    if (cached) return cached;
    const hashes = await hashFile(file);
    const res = { ...(await lookupHasheous(hashes)), file: path.relative(pdir, file), md5: hashes.md5 };
    delete (res as { raw?: unknown }).raw;
    if (res.status !== 'error') this.identifyCache.set(key, res);
    return res;
  }
}
