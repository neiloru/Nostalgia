import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { unzipSync, zipSync, type Zippable } from 'fflate';
import { expandString, type SaveVersion } from '@nostalgia/shared';
import type { Core } from './core.js';
import { ApiError } from './api.js';

interface LocalFile {
  rel: string; // "<index>/<path inside save folder>"
  abs: string;
  size: number;
  mtime: number;
  sha: string;
}

function listDir(dir: string, prefix: string, out: LocalFile[]) {
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const abs = path.join(dir, e.name);
    const rel = `${prefix}/${e.name}`;
    if (e.isDirectory()) listDir(abs, rel, out);
    else if (e.isFile()) {
      const st = fs.statSync(abs);
      out.push({ rel, abs, size: st.size, mtime: st.mtimeMs, sha: '' });
    }
  }
}

/**
 * Cloud saves are per emulator. A snapshot is a zip of all save folders listed in emu.yaml
 * (entry "0/..." = first folder, "1/..." = second, ...).
 *
 * Safety rules:
 *  - before local save files are replaced they are copied to <userData>/save-backups if they
 *    are not identical to a version that already exists on the server;
 *  - nothing on the server is ever deleted, restoring an old version uploads it as a new version.
 */
export class SaveSync {
  private retryTimer: NodeJS.Timeout | null = null;

  constructor(private core: Core) {
    core.on('online', () => void this.syncAll('online'));
    core.on('refreshed', () => void this.retryPending());
    core.on('remote-save', (emulatorId: number) => {
      if (this.core.settings.autoSyncSaves && this.core.running?.emulatorId !== emulatorId) void this.syncDown(emulatorId, 'remote').catch(() => undefined);
    });
  }

  /** Absolute save folders for an installed emulator on this OS. */
  savePaths(emulatorId: number): string[] {
    const emu = this.core.emulator(emulatorId);
    const inst = this.core.installed('emulator', emulatorId);
    const sys = emu?.config?.systems[this.core.os];
    if (!sys?.saves?.length) return [];
    const vars: Record<string, string> = {
      emulatorDir: inst?.path ?? '',
      home: this.core.opts.home,
      documents: this.core.opts.documents,
      appData: this.core.opts.appData,
      config: process.env.XDG_CONFIG_HOME ?? path.join(this.core.opts.home, '.config'),
      dataHome: process.env.XDG_DATA_HOME ?? path.join(this.core.opts.home, '.local', 'share'),
    };
    return sys.saves.map((s) =>
      path.resolve(inst?.path ?? this.core.opts.home, expandString(s, { vars, env: process.env, home: this.core.opts.home, os: this.core.os })),
    );
  }

  private scan(emulatorId: number): { files: LocalFile[]; fingerprint: string } {
    const files: LocalFile[] = [];
    this.savePaths(emulatorId).forEach((p, i) => listDir(p, String(i), files));
    files.sort((a, b) => a.rel.localeCompare(b.rel));
    const h = crypto.createHash('sha256');
    for (const f of files) {
      f.sha = crypto.createHash('sha256').update(fs.readFileSync(f.abs)).digest('hex');
      h.update(`${f.rel}\0${f.size}\0${f.sha}\n`);
    }
    return { files, fingerprint: files.length ? h.digest('hex') : 'empty' };
  }

  private buildZip(emulatorId: number, files: LocalFile[]): Uint8Array {
    const z: Zippable = {};
    for (const f of files) z[f.rel] = [fs.readFileSync(f.abs), { mtime: new Date(Math.floor(f.mtime / 2000) * 2000) }];
    z['nostalgia-save.json'] = [
      new TextEncoder().encode(JSON.stringify({ format: 1, folders: this.savePaths(emulatorId).length, files: files.map((f) => ({ path: f.rel, sha256: f.sha })) })),
      { mtime: new Date(2000, 0, 1) },
    ];
    return zipSync(z, { level: 6 });
  }

  private backup(emulatorId: number, files: LocalFile[], reason: string): string | null {
    if (!files.length) return null;
    const emu = this.core.emulator(emulatorId);
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    const dir = path.join(this.core.saveBackupDir, `${emu?.platform ?? 'emu'}-${(emu?.name ?? String(emulatorId)).replace(/[^\w.-]+/g, '_')}`, `${stamp}-${reason}`);
    for (const f of files) {
      const dest = path.join(dir, ...f.rel.split('/'));
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.copyFileSync(f.abs, dest);
    }
    return dir;
  }

  private async withBusy<T>(emulatorId: number, fn: () => Promise<T>): Promise<T> {
    if (this.core.busySaves.has(emulatorId)) throw new Error('A save sync for this emulator is already running');
    this.core.busySaves.add(emulatorId);
    this.core.emitState();
    try {
      return await fn();
    } finally {
      this.core.busySaves.delete(emulatorId);
      this.core.emitState();
    }
  }

  /** Install a server version into the local save folders (mirror). */
  private async applyVersion(emulatorId: number, v: SaveVersion, reason: string, localFiles: LocalFile[], localFp: string): Promise<Uint8Array> {
    const st = this.core.saveState(emulatorId);
    if (localFiles.length && localFp !== st.fingerprint) {
      const dir = this.backup(emulatorId, localFiles, reason);
      if (dir) this.core.toast(`Local saves backed up before ${reason === 'restore' ? 'restoring' : 'downloading'} (${path.basename(dir)})`);
    }
    const res = await this.core.api.raw(`/saves/version/${v.id}/download`, { timeoutMs: 120_000 });
    if (!res.ok) throw new ApiError(res.status, `Downloading save failed (HTTP ${res.status})`);
    const buf = new Uint8Array(await res.arrayBuffer());
    const sha = crypto.createHash('sha256').update(buf).digest('hex');
    if (sha !== v.sha256) throw new Error('Downloaded save is corrupted (checksum mismatch)');
    const entries = unzipSync(buf);
    const paths = this.savePaths(emulatorId);
    // Remove local files that are not part of the snapshot (they are backed up or identical to a server version).
    const incoming = new Set(Object.keys(entries).filter((k) => /^\d+\//.test(k) && !k.endsWith('/')));
    for (const f of localFiles) if (!incoming.has(f.rel)) fs.rmSync(f.abs, { force: true });
    for (const [name, data] of Object.entries(entries)) {
      const m = /^(\d+)\/(.+)$/.exec(name);
      if (!m || name.endsWith('/')) continue;
      const root = paths[Number(m[1])];
      if (!root) continue;
      const dest = path.resolve(root, ...m[2].split('/'));
      if (!dest.startsWith(path.resolve(root) + path.sep)) continue; // zip-slip guard
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.writeFileSync(dest, data);
    }
    return buf;
  }

  /**
   * Bring local saves up to date with the server:
   *  - server has a newer version -> download it (after backing up unsynced local changes)
   *  - local has unsynced changes and server has nothing newer -> upload
   */
  async syncDown(emulatorId: number, reason = 'sync'): Promise<string> {
    if (!this.core.installed('emulator', emulatorId) || !this.savePaths(emulatorId).length) return 'no-save-config';
    if (this.core.running?.emulatorId === emulatorId) return 'running';
    if (!this.core.online) return 'offline';
    return this.withBusy(emulatorId, async () => {
      const versions = await this.core.api.json<SaveVersion[]>(`/saves/${emulatorId}`);
      const latest = versions[0];
      const st = this.core.saveState(emulatorId);
      const { files, fingerprint } = this.scan(emulatorId);
      if (!latest) {
        if (files.length && fingerprint !== st.fingerprint) return this.upload(emulatorId, files, fingerprint, 'first upload');
        return 'nothing';
      }
      // Local saves vanished since the last sync (emulator reinstalled, folder deleted): download them again.
      const localMissing = !files.length && !!st.fingerprint && fingerprint !== st.fingerprint;
      if (latest.id === st.versionId && !localMissing) {
        if (fingerprint !== st.fingerprint && files.length) return this.upload(emulatorId, files, fingerprint, 'unsynced changes');
        this.core.setSaveState(emulatorId, { lastResult: 'up to date', lastError: undefined, syncedAt: new Date().toISOString() });
        return 'up-to-date';
      }
      await this.applyVersion(emulatorId, latest, reason, files, fingerprint);
      const after = this.scan(emulatorId);
      this.core.setSaveState(emulatorId, {
        versionId: latest.id,
        version: latest.version,
        fingerprint: after.fingerprint,
        syncedAt: new Date().toISOString(),
        pendingUpload: false,
        lastError: undefined,
        lastResult: `downloaded v${latest.version}`,
      });
      const emu = this.core.emulator(emulatorId);
      this.core.toast(`Cloud save v${latest.version} for ${emu?.name ?? 'emulator'} downloaded`);
      return 'downloaded';
    });
  }

  private async upload(emulatorId: number, files: LocalFile[], fingerprint: string, note: string, zip?: Uint8Array): Promise<string> {
    const body = zip ?? this.buildZip(emulatorId, files);
    const sha = crypto.createHash('sha256').update(body).digest('hex');
    try {
      const res = await this.core.api.json<{ version: SaveVersion; created: boolean }>(`/saves/${emulatorId}`, {
        method: 'POST',
        body,
        timeoutMs: 300_000,
        headers: { 'Content-Type': 'application/zip', 'X-Content-Sha256': sha, 'X-Save-Note': encodeURIComponent(note) },
      });
      this.core.setSaveState(emulatorId, {
        versionId: res.version.id,
        version: res.version.version,
        fingerprint,
        syncedAt: new Date().toISOString(),
        pendingUpload: false,
        lastError: undefined,
        lastResult: res.created ? `uploaded v${res.version.version}` : 'up to date',
      });
      await this.core.refreshLatestSaves().catch(() => undefined);
      if (res.created) this.core.toast(`Cloud save v${res.version.version} uploaded (${this.core.emulator(emulatorId)?.name ?? ''})`);
      return res.created ? 'uploaded' : 'up-to-date';
    } catch (e) {
      this.core.setSaveState(emulatorId, { pendingUpload: true, lastError: (e as Error).message });
      this.scheduleRetry();
      throw e;
    }
  }

  /** Called after the emulator closed: upload if the save folder changed. Failures are retried later. */
  async syncUp(emulatorId: number, note = 'after playing'): Promise<string> {
    if (!this.savePaths(emulatorId).length) return 'no-save-config';
    const { files, fingerprint } = this.scan(emulatorId);
    const st = this.core.saveState(emulatorId);
    if (fingerprint === st.fingerprint || !files.length) return 'unchanged';
    if (!this.core.online) {
      this.core.setSaveState(emulatorId, { pendingUpload: true, lastError: 'offline - will upload when back online' });
      return 'pending';
    }
    return this.withBusy(emulatorId, () => this.upload(emulatorId, files, fingerprint, note)).catch((e) => {
      this.core.toast(`Save upload failed, will retry: ${(e as Error).message}`, true);
      return 'pending';
    });
  }

  private scheduleRetry() {
    if (this.retryTimer) return;
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      void this.retryPending();
    }, 120_000);
  }

  async retryPending() {
    if (!this.core.online) return;
    for (const id of Object.keys(this.core.installedStore.data.emulators).map(Number)) {
      if (this.core.saveState(id).pendingUpload && this.core.running?.emulatorId !== id) {
        await this.syncUp(id, 'retry').catch(() => undefined);
      }
    }
  }

  /** On start / reconnect: newest cloud saves for every installed emulator. */
  async syncAll(reason: string) {
    if (!this.core.settings.autoSyncSaves || !this.core.online) return;
    for (const id of Object.keys(this.core.installedStore.data.emulators).map(Number)) {
      try {
        await this.syncDown(id, reason);
      } catch (e) {
        this.core.setSaveState(id, { lastError: (e as Error).message });
      }
    }
  }

  async listVersions(emulatorId: number): Promise<SaveVersion[]> {
    return this.core.api.json<SaveVersion[]>(`/saves/${emulatorId}`);
  }

  /** Restore an older version locally, then upload it as the newest version so all devices get it. */
  async restore(emulatorId: number, versionId: number): Promise<void> {
    if (this.core.running?.emulatorId === emulatorId) throw new Error('Close the emulator first');
    if (!this.core.installed('emulator', emulatorId)) throw new Error('Install the emulator first');
    await this.withBusy(emulatorId, async () => {
      const versions = await this.listVersions(emulatorId);
      const v = versions.find((x) => x.id === versionId);
      if (!v) throw new Error('Version not found');
      const { files, fingerprint } = this.scan(emulatorId);
      const zip = await this.applyVersion(emulatorId, v, 'restore', files, fingerprint);
      const after = this.scan(emulatorId);
      if (versions[0]?.id === v.id) {
        this.core.setSaveState(emulatorId, { versionId: v.id, version: v.version, fingerprint: after.fingerprint, pendingUpload: false, lastResult: `restored v${v.version}` });
      } else {
        await this.upload(emulatorId, after.files, after.fingerprint, `restored from v${v.version}`, zip);
      }
      this.core.toast(`Restored cloud save v${v.version}`);
    });
  }

  /** Manual "sync now". */
  async syncNow(emulatorId: number) {
    const r = await this.syncDown(emulatorId, 'manual');
    if (r === 'up-to-date' || r === 'nothing') {
      const up = await this.syncUp(emulatorId, 'manual sync');
      if (up === 'unchanged') this.core.toast('Saves are up to date');
    }
  }
}
