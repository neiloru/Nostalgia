import fs from 'node:fs';
import path from 'node:path';
import { Readable } from 'node:stream';
import { EventEmitter } from 'node:events';
import type { FileListing } from '@nostalgia/shared';
import type { Core } from './core.js';
import { JsonStore } from './store.js';
import type { DownloadJob, InstalledItem, ItemKind } from './types.js';

const PART = '.nostalgia-part';
export const MANIFEST = '.nostalgia.json';

function safeJoin(base: string, rel: string): string {
  const target = path.resolve(base, ...rel.split('/'));
  if (!target.startsWith(path.resolve(base) + path.sep)) throw new Error(`Refusing unsafe path ${rel}`);
  return target;
}

/**
 * Sequential download queue with pause / resume (HTTP Range) that survives restarts.
 * Emits `update` with the job list.
 */
export class DownloadManager extends EventEmitter {
  private store: JsonStore<{ jobs: DownloadJob[] }>;
  private active: { key: string; ctrl: AbortController } | null = null;
  private lastEmit = 0;

  constructor(private core: Core) {
    super();
    this.store = new JsonStore(path.join(core.userData, 'downloads.json'), { jobs: [] });
    // A job that was running when the app quit is resumed.
    for (const j of this.store.data.jobs) if (j.status === 'downloading') j.status = 'queued';
    core.on('online', () => {
      for (const j of this.jobs) if (j.status === 'error' && j.error?.includes('unreachable')) j.status = 'queued';
      this.pump();
    });
    core.on('login', () => this.pump());
  }

  get jobs(): DownloadJob[] {
    return this.store.data.jobs;
  }

  list() {
    return this.jobs;
  }

  private emitUpdate(force = false) {
    const t = Date.now();
    if (!force && t - this.lastEmit < 250) return;
    this.lastEmit = t;
    this.emit('update', this.jobs);
  }

  private persist() {
    this.store.saveSoon();
    this.emitUpdate(true);
  }

  find(kind: ItemKind, id: number) {
    return this.jobs.find((j) => j.kind === kind && j.id === id && j.status !== 'done');
  }

  async enqueue(kind: ItemKind, id: number, location?: string): Promise<DownloadJob> {
    const existing = this.find(kind, id);
    if (existing) {
      if (existing.status === 'paused' || existing.status === 'error') this.resume(existing.key);
      return existing;
    }
    if (!this.core.online) throw new Error('You are offline');
    if (this.core.installed(kind, id)) throw new Error('Already installed');
    const item = kind === 'game' ? this.core.game(id) : this.core.emulator(id);
    if (!item) throw new Error('Unknown item');
    const loc = location ?? this.core.settings.defaultLocation ?? this.core.settings.locations[0];
    if (!loc) throw new Error('Add a download location in the settings first');
    const listing = await this.core.api.json<FileListing>(`/${kind === 'game' ? 'games' : 'emulators'}/${id}/files`);
    const targetDir = path.join(loc, kind === 'game' ? 'games' : 'emulators', item.platform, listing.folder);
    const job: DownloadJob = {
      key: `${kind}:${id}`,
      kind,
      id,
      name: item.name,
      platform: item.platform,
      folder: listing.folder,
      location: loc,
      targetDir,
      files: listing.files,
      totalBytes: listing.totalBytes,
      receivedBytes: 0,
      status: 'queued',
      speed: 0,
      addedAt: new Date().toISOString(),
    };
    this.store.set((d) => {
      d.jobs = d.jobs.filter((j) => j.key !== job.key);
      d.jobs.push(job);
    });
    this.persist();
    this.pump();
    return job;
  }

  pause(key: string) {
    const j = this.jobs.find((x) => x.key === key);
    if (!j || (j.status !== 'downloading' && j.status !== 'queued')) return;
    j.status = 'paused';
    j.speed = 0;
    if (this.active?.key === key) this.active.ctrl.abort();
    this.persist();
  }

  resume(key: string) {
    const j = this.jobs.find((x) => x.key === key);
    if (!j || (j.status !== 'paused' && j.status !== 'error')) return;
    j.status = 'queued';
    j.error = undefined;
    this.persist();
    this.pump();
  }

  pauseAll() {
    for (const j of this.jobs) if (j.status === 'queued' || j.status === 'downloading') this.pause(j.key);
  }

  resumeAll() {
    for (const j of this.jobs) if (j.status === 'paused') this.resume(j.key);
  }

  /** Cancel: removes the job and the partial files it created (files of a completed install are untouched). */
  cancel(key: string) {
    const j = this.jobs.find((x) => x.key === key);
    if (!j) return;
    if (this.active?.key === key) this.active.ctrl.abort();
    this.store.set((d) => {
      d.jobs = d.jobs.filter((x) => x.key !== key);
    });
    if (j.status !== 'done' && !this.core.installed(j.kind, j.id)) {
      for (const f of j.files) {
        try {
          const dest = safeJoin(j.targetDir, f.path);
          fs.rmSync(dest + PART, { force: true });
          fs.rmSync(dest, { force: true });
        } catch {
          /* ignore */
        }
      }
      removeEmptyDirs(j.targetDir);
    }
    this.persist();
    this.pump();
  }

  clearFinished() {
    this.store.set((d) => {
      d.jobs = d.jobs.filter((j) => j.status !== 'done');
    });
    this.persist();
  }

  private pump() {
    if (this.active || !this.core.session) return;
    const next = this.jobs.find((j) => j.status === 'queued');
    if (!next) return;
    void this.run(next);
  }

  private async run(job: DownloadJob) {
    const ctrl = new AbortController();
    this.active = { key: job.key, ctrl };
    job.status = 'downloading';
    job.error = undefined;
    this.persist();
    const base = job.kind === 'game' ? 'games' : 'emulators';
    try {
      fs.mkdirSync(job.targetDir, { recursive: true });
      // Bytes already on disk (complete files and partial files).
      const doneBytes = () =>
        job.files.reduce((a, f) => {
          const dest = safeJoin(job.targetDir, f.path);
          try {
            const st = fs.statSync(dest);
            if (st.size === f.size) return a + f.size;
          } catch {
            /* not there */
          }
          try {
            return a + Math.min(fs.statSync(dest + PART).size, f.size);
          } catch {
            return a;
          }
        }, 0);
      job.receivedBytes = doneBytes();
      let windowStart = Date.now();
      let windowBytes = 0;

      for (const f of job.files) {
        if (ctrl.signal.aborted) break;
        const dest = safeJoin(job.targetDir, f.path);
        const part = dest + PART;
        try {
          if (fs.statSync(dest).size === f.size) continue;
        } catch {
          /* need to download */
        }
        fs.mkdirSync(path.dirname(dest), { recursive: true });
        let start = 0;
        try {
          start = fs.statSync(part).size;
          if (start > f.size) {
            fs.rmSync(part);
            start = 0;
          }
        } catch {
          start = 0;
        }
        job.currentFile = f.path;
        if (start < f.size || f.size === 0) {
          const res = await this.core.api.raw(`/${base}/${job.id}/download?path=${encodeURIComponent(f.path)}`, {
            headers: start > 0 ? { Range: `bytes=${start}-` } : {},
            signal: ctrl.signal,
          });
          if (!res.ok && res.status !== 206) throw new Error(`Download of ${f.path} failed: HTTP ${res.status}`);
          if (start > 0 && res.status !== 206) {
            // Server ignored the range - start over for this file.
            job.receivedBytes -= start;
            start = 0;
          }
          const out = fs.createWriteStream(part, { flags: start > 0 ? 'a' : 'w' });
          const body = Readable.fromWeb(res.body as never);
          try {
            for await (const chunk of body) {
              const buf = chunk as Buffer;
              if (!out.write(buf)) await new Promise<void>((r) => out.once('drain', () => r()));
              job.receivedBytes += buf.length;
              windowBytes += buf.length;
              const t = Date.now();
              if (t - windowStart >= 1000) {
                job.speed = (windowBytes * 1000) / (t - windowStart);
                windowStart = t;
                windowBytes = 0;
              }
              this.emitUpdate();
            }
          } finally {
            await new Promise<void>((r) => out.end(() => r()));
          }
        }
        if (ctrl.signal.aborted) break;
        const size = fs.statSync(part).size;
        if (size !== f.size) throw new Error(`Incomplete download of ${f.path} (${size}/${f.size} bytes)`);
        fs.renameSync(part, dest);
        try {
          fs.utimesSync(dest, new Date(), new Date(f.mtime));
        } catch {
          /* ignore */
        }
      }

      if (!ctrl.signal.aborted) {
        const item: InstalledItem = {
          kind: job.kind,
          id: job.id,
          name: job.name,
          platform: job.platform,
          folder: job.folder,
          location: job.location,
          path: job.targetDir,
          files: job.files,
          sizeBytes: job.totalBytes,
          installedAt: new Date().toISOString(),
        };
        if (job.kind === 'emulator' && this.core.os === 'linux') this.makeExecutable(item);
        fs.writeFileSync(path.join(job.targetDir, MANIFEST), JSON.stringify({ ...item, files: undefined, server: this.core.session?.serverUrl }, null, 2));
        this.core.registerInstalled(item);
        job.status = 'done';
        job.receivedBytes = job.totalBytes;
        job.finishedAt = new Date().toISOString();
        job.currentFile = undefined;
        job.speed = 0;
        this.core.toast(`${job.name} is installed`);
        this.emit('installed', item);
      }
    } catch (e) {
      const err = e as Error;
      if (ctrl.signal.aborted || err.name === 'AbortError') {
        /* paused or cancelled */
      } else {
        job.status = 'error';
        job.error = err.message;
        job.speed = 0;
        this.core.toast(`Download of ${job.name} failed: ${err.message}`, true);
      }
    } finally {
      this.active = null;
      this.persist();
      this.pump();
    }
  }

  private makeExecutable(item: InstalledItem) {
    const emu = this.core.emulator(item.id);
    const exe = emu?.config?.systems.linux?.executable;
    const candidates = item.files.filter((f) => /\.(AppImage|sh|x86_64)$/i.test(f.path) || (exe && f.path === exe.replace(/^\.\//, '')));
    for (const f of candidates) {
      try {
        fs.chmodSync(safeJoin(item.path, f.path), 0o755);
      } catch {
        /* ignore */
      }
    }
  }
}

export function removeEmptyDirs(dir: string) {
  try {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) if (e.isDirectory()) removeEmptyDirs(path.join(dir, e.name));
    if (fs.readdirSync(dir).length === 0) fs.rmdirSync(dir);
  } catch {
    /* ignore */
  }
}
