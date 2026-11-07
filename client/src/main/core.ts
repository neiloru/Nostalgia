import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { EventEmitter } from 'node:events';
import type {
  AuthResponse,
  EmulatorSummary,
  GameDetail,
  GameMetadata,
  GameSummary,
  LibraryState,
  PlaySession,
  PlaytimeEntry,
  SaveVersion,
  ServerEvent,
} from '@nostalgia/shared';
import { Api, ApiError, normalizeServerUrl } from './api.js';
import { JsonStore } from './store.js';
import type { ClientSettings, ClientState, GameSettings, InstalledItem, RunningGame, SaveSyncState, Session } from './types.js';

export interface CoreOptions {
  userData: string;
  home: string;
  documents: string;
  appData: string;
  version: string;
}

interface Cache {
  games: GameSummary[];
  emulators: EmulatorSummary[];
  library: LibraryState;
  playtime: PlaytimeEntry[];
  latestSaves: SaveVersion[];
  fetchedAt?: string;
}

const OS: 'windows' | 'linux' = process.platform === 'win32' ? 'windows' : 'linux';

/**
 * Holds all client state, talks to the server and persists everything needed to work offline.
 * Emits `state` whenever the UI should re-render, `toast` for notifications.
 */
export class Core extends EventEmitter {
  readonly os = OS;
  readonly api: Api;
  readonly device: string;
  readonly deviceId: string;

  private sessionStore: JsonStore<{ session: Session | null }>;
  readonly settingsStore: JsonStore<ClientSettings>;
  readonly installedStore: JsonStore<{ games: Record<number, InstalledItem>; emulators: Record<number, InstalledItem> }>;
  readonly gameSettingsStore: JsonStore<Record<number, GameSettings>>;
  /** Per user id -> emulator id -> sync state */
  readonly saveSyncStore: JsonStore<Record<string, Record<number, SaveSyncState>>>;
  readonly pendingStore: JsonStore<{ sessions: (PlaySession & { userId: number })[]; active: (RunningGame & { userId: number; heartbeat: string }) | null }>;
  private cacheStore: JsonStore<Record<string, Cache>>;

  online = false;
  serverReachable = false;
  running: RunningGame | null = null;
  fullscreen = false;
  lastError?: string;
  busySaves = new Set<number>();
  private healthTimer: NodeJS.Timeout | null = null;
  private sse: AbortController | null = null;
  private emitTimer: NodeJS.Timeout | null = null;
  ready = false;

  constructor(readonly opts: CoreOptions) {
    super();
    const ud = opts.userData;
    const deviceStore = new JsonStore<{ id?: string }>(path.join(ud, 'device.json'), {});
    if (!deviceStore.data.id) {
      deviceStore.set({ id: crypto.randomUUID() });
      deviceStore.saveNow();
    }
    this.deviceId = deviceStore.data.id!;
    this.device = `${os.hostname()} (${OS})`;
    this.sessionStore = new JsonStore(path.join(ud, 'session.json'), { session: null });
    this.settingsStore = new JsonStore<ClientSettings>(path.join(ud, 'settings.json'), {
      locations: [path.join(opts.home, 'Games', 'Nostalgia')],
      startFullscreen: process.env.SteamDeck === '1' || process.env.XDG_CURRENT_DESKTOP === 'gamescope',
      autoSyncSaves: true,
      minimizeOnLaunch: OS === 'windows',
      installEmulatorWithGame: true,
    });
    this.installedStore = new JsonStore(path.join(ud, 'installed.json'), { games: {}, emulators: {} });
    this.gameSettingsStore = new JsonStore(path.join(ud, 'game-settings.json'), {});
    this.saveSyncStore = new JsonStore(path.join(ud, 'save-sync.json'), {});
    this.pendingStore = new JsonStore(path.join(ud, 'pending.json'), { sessions: [], active: null });
    this.cacheStore = new JsonStore(path.join(ud, 'cache.json'), {});
    this.api = new Api(
      () => this.session,
      this.device,
      () => this.setOnline(false),
      () => this.handleUnauthorized(),
    );
    this.verifyInstalled();
  }

  // ------------------------------------------------------------------ paths
  get userData() {
    return this.opts.userData;
  }
  get mediaCacheDir() {
    return path.join(this.opts.userData, 'media-cache');
  }
  get detailCacheDir() {
    return path.join(this.opts.userData, 'details');
  }
  get saveBackupDir() {
    return path.join(this.opts.userData, 'save-backups');
  }

  // ------------------------------------------------------------------ session
  get session(): Session | null {
    return this.sessionStore.data.session;
  }
  get userId(): number | null {
    return this.session?.user.id ?? null;
  }
  private get cacheKey() {
    const s = this.session;
    return s ? `${s.serverUrl}|${s.user.id}` : '';
  }
  get cache(): Cache {
    return this.cacheStore.data[this.cacheKey] ?? { games: [], emulators: [], library: { games: [], emulators: [] }, playtime: [], latestSaves: [] };
  }
  private setCache(update: Partial<Cache>) {
    const key = this.cacheKey;
    if (!key) return;
    this.cacheStore.set((c) => {
      c[key] = { ...this.cache, ...update };
    });
  }

  get settings(): ClientSettings {
    return this.settingsStore.data;
  }

  async start() {
    this.recoverActiveSession();
    this.ready = true;
    if (this.session) {
      await this.checkHealth();
    }
    this.scheduleHealth();
    this.emitState();
  }

  async login(serverUrl: string, username: string, password: string, register = false) {
    const url = normalizeServerUrl(serverUrl);
    const health = await this.api.json<{ ok: boolean; name: string }>('/health', { serverUrl: url, timeoutMs: 8000 }).catch((e: ApiError) => {
      throw new ApiError(0, e.network ? `Cannot reach ${url}` : e.message);
    });
    if (health?.name !== 'nostalgia') throw new ApiError(0, `${url} is not a Nostalgia server`);
    const res = await this.api.json<AuthResponse>(register ? '/auth/register' : '/auth/login', {
      method: 'POST',
      serverUrl: url,
      body: { username, password },
    });
    this.sessionStore.set({ session: { serverUrl: url, token: res.token, user: res.user } });
    this.sessionStore.saveNow();
    this.setOnline(true);
    await this.refreshAll();
    this.emit('login');
    this.emitState();
  }

  async logout() {
    const s = this.session;
    if (s && this.online) await this.api.json('/auth/logout', { method: 'POST' }).catch(() => undefined);
    this.stopEvents();
    this.sessionStore.set({ session: null });
    this.sessionStore.saveNow();
    this.online = false;
    this.emitState();
  }

  private handleUnauthorized() {
    if (!this.session) return;
    this.toast('Your session has expired - please sign in again', true);
    this.stopEvents();
    this.sessionStore.set({ session: null });
    this.online = false;
    this.emitState();
  }

  async changePassword(oldPassword: string, newPassword: string) {
    await this.api.json('/me/password', { method: 'POST', body: { oldPassword, newPassword } });
    this.toast('Password changed');
  }

  // ------------------------------------------------------------------ online / offline
  setOnline(v: boolean) {
    if (this.online === v) return;
    this.online = v;
    this.serverReachable = v;
    if (v) {
      this.emit('online');
      this.startEvents();
    } else {
      this.stopEvents();
      this.emit('offline');
    }
    this.scheduleHealth();
    this.emitState();
  }

  private scheduleHealth() {
    if (this.healthTimer) clearTimeout(this.healthTimer);
    this.healthTimer = setTimeout(
      async () => {
        await this.checkHealth();
        this.scheduleHealth();
      },
      this.online ? 30_000 : 10_000,
    );
  }

  async checkHealth(): Promise<boolean> {
    const s = this.session;
    if (!s) return false;
    try {
      await this.api.json('/health', { timeoutMs: 6000 });
      // Validate the token too.
      await this.api.json('/me', { timeoutMs: 6000 });
      const wasOffline = !this.online;
      this.setOnline(true);
      if (wasOffline) await this.refreshAll();
      return true;
    } catch (e) {
      if (e instanceof ApiError && !e.network && e.status !== 401) {
        // Server reachable but erroring - treat as online.
        this.setOnline(true);
        return true;
      }
      this.setOnline(false);
      return false;
    }
  }

  async refreshAll() {
    if (!this.session) return;
    try {
      const [games, emulators, library, playtime, latestSaves] = await Promise.all([
        this.api.json<GameSummary[]>('/games'),
        this.api.json<EmulatorSummary[]>('/emulators'),
        this.api.json<LibraryState>('/library'),
        this.api.json<PlaytimeEntry[]>('/playtime'),
        this.api.json<SaveVersion[]>('/saves'),
      ]);
      this.setCache({ games, emulators, library, playtime, latestSaves, fetchedAt: new Date().toISOString() });
      await this.flushPlaytime();
      this.emit('refreshed');
    } catch (e) {
      this.lastError = (e as Error).message;
    }
    this.emitState();
  }

  async refreshLatestSaves() {
    if (!this.online) return this.cache.latestSaves;
    const latestSaves = await this.api.json<SaveVersion[]>('/saves');
    this.setCache({ latestSaves });
    return latestSaves;
  }

  // ------------------------------------------------------------------ push events (SSE)
  private startEvents() {
    if (this.sse || !this.session) return;
    const ctrl = new AbortController();
    this.sse = ctrl;
    const run = async () => {
      let backoff = 2000;
      while (!ctrl.signal.aborted) {
        try {
          const res = await fetch(this.api.url(`/events?device=${encodeURIComponent(this.device)}`), {
            headers: this.api.headers({ Accept: 'text/event-stream' }),
            signal: ctrl.signal,
          });
          if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
          backoff = 2000;
          const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
          let buf = '';
          for (;;) {
            const { value, done } = await reader.read();
            if (done) break;
            buf += value;
            let idx: number;
            while ((idx = buf.indexOf('\n\n')) >= 0) {
              const chunk = buf.slice(0, idx);
              buf = buf.slice(idx + 2);
              const data = chunk
                .split('\n')
                .filter((l) => l.startsWith('data:'))
                .map((l) => l.slice(5).trim())
                .join('\n');
              if (data) {
                try {
                  this.handleEvent(JSON.parse(data) as ServerEvent);
                } catch {
                  /* ignore malformed */
                }
              }
            }
          }
        } catch {
          if (ctrl.signal.aborted) return;
        }
        await new Promise((r) => setTimeout(r, backoff));
        backoff = Math.min(backoff * 2, 60_000);
      }
    };
    void run();
  }

  private stopEvents() {
    this.sse?.abort();
    this.sse = null;
  }

  private handleEvent(ev: ServerEvent) {
    if (ev.type === 'library') {
      this.setCache({ library: ev.library });
      this.emitState();
    } else if (ev.type === 'catalog') {
      void this.refreshAll();
    } else if (ev.type === 'save') {
      const latest = this.cache.latestSaves.filter((s) => s.emulatorId !== ev.emulatorId);
      this.setCache({ latestSaves: [...latest, ev.version] });
      this.emit('remote-save', ev.emulatorId);
      this.emitState();
    }
  }

  // ------------------------------------------------------------------ catalog / library
  game(id: number): GameSummary | undefined {
    return this.cache.games.find((g) => g.id === id);
  }
  emulator(id: number): EmulatorSummary | undefined {
    return this.cache.emulators.find((e) => e.id === id);
  }

  async getGameDetail(id: number): Promise<GameDetail | null> {
    const file = path.join(this.detailCacheDir, `${id}.json`);
    if (this.online) {
      try {
        const d = await this.api.json<GameDetail>(`/games/${id}`);
        fs.mkdirSync(this.detailCacheDir, { recursive: true });
        fs.writeFileSync(file, JSON.stringify(d));
        return d;
      } catch {
        /* fall back to the offline copy */
      }
    }
    try {
      if (fs.existsSync(file)) return JSON.parse(fs.readFileSync(file, 'utf8')) as GameDetail;
    } catch {
      /* ignore */
    }
    const inst = this.installedStore.data.games[id];
    const summary = this.game(id);
    if (inst) {
      const metaFile = path.join(inst.path, 'igdb', 'metadata.json');
      let metadata: GameMetadata | null = null;
      try {
        metadata = JSON.parse(fs.readFileSync(metaFile, 'utf8')) as GameMetadata;
      } catch {
        /* none */
      }
      if (summary) return { ...summary, metadata };
    }
    return summary ? { ...summary, metadata: null } : null;
  }

  private async libraryCall(method: 'POST' | 'DELETE', kind: 'games' | 'emulators', id: number) {
    if (!this.online) throw new ApiError(0, 'You are offline - library changes need a connection to the server');
    const library = await this.api.json<LibraryState>(`/library/${kind}/${id}`, { method });
    this.setCache({ library });
    this.emitState();
  }
  addGame = (id: number) => this.libraryCall('POST', 'games', id);
  removeGame = (id: number) => this.libraryCall('DELETE', 'games', id);
  addEmulator = (id: number) => this.libraryCall('POST', 'emulators', id);
  removeEmulator = (id: number) => this.libraryCall('DELETE', 'emulators', id);

  // ------------------------------------------------------------------ installed items
  private verifyInstalled() {
    this.installedStore.set((d) => {
      for (const kind of ['games', 'emulators'] as const) {
        for (const [id, item] of Object.entries(d[kind])) {
          if (!fs.existsSync(item.path)) delete d[kind][Number(id)];
        }
      }
    });
  }

  installed(kind: 'game' | 'emulator', id: number): InstalledItem | undefined {
    return kind === 'game' ? this.installedStore.data.games[id] : this.installedStore.data.emulators[id];
  }

  registerInstalled(item: InstalledItem) {
    this.installedStore.set((d) => {
      (item.kind === 'game' ? d.games : d.emulators)[item.id] = item;
    });
    this.installedStore.saveNow();
    this.emitState();
  }

  unregisterInstalled(kind: 'game' | 'emulator', id: number) {
    this.installedStore.set((d) => {
      delete (kind === 'game' ? d.games : d.emulators)[id];
    });
    this.installedStore.saveNow();
    this.emitState();
  }

  // ------------------------------------------------------------------ play time
  recordSession(gameId: number, startedAt: string, seconds: number) {
    if (!this.userId || seconds < 1) return;
    this.pendingStore.set((d) => {
      d.sessions.push({ sessionId: crypto.randomUUID(), gameId, startedAt, seconds: Math.round(seconds), userId: this.userId! });
    });
    this.pendingStore.saveNow();
    // Optimistic local update so the UI shows the new time immediately.
    const pt = this.cache.playtime.filter((p) => p.gameId !== gameId);
    const cur = this.cache.playtime.find((p) => p.gameId === gameId) ?? { gameId, seconds: 0, sessions: 0 };
    this.setCache({
      playtime: [...pt, { gameId, seconds: cur.seconds + Math.round(seconds), sessions: cur.sessions + 1, lastPlayed: new Date().toISOString() }],
    });
    void this.flushPlaytime();
  }

  async flushPlaytime() {
    if (!this.online || !this.userId) return;
    const mine = this.pendingStore.data.sessions.filter((s) => s.userId === this.userId);
    if (!mine.length) return;
    try {
      await this.api.json('/playtime', { method: 'POST', body: mine.map(({ userId: _u, ...s }) => s) });
      const sent = new Set(mine.map((s) => s.sessionId));
      this.pendingStore.set((d) => {
        d.sessions = d.sessions.filter((s) => !sent.has(s.sessionId));
      });
      this.pendingStore.saveNow();
      const playtime = await this.api.json<PlaytimeEntry[]>('/playtime');
      this.setCache({ playtime });
      this.emitState();
    } catch {
      /* retried on next online transition */
    }
  }

  /** Heartbeat of the running game, so play time survives a crash of the launcher. */
  heartbeat(r: RunningGame | null) {
    this.pendingStore.set((d) => {
      d.active = r && this.userId ? { ...r, userId: this.userId, heartbeat: new Date().toISOString() } : null;
    });
    this.pendingStore.saveNow();
  }

  private recoverActiveSession() {
    const a = this.pendingStore.data.active;
    if (!a) return;
    const seconds = (Date.parse(a.heartbeat) - Date.parse(a.startedAt)) / 1000;
    if (seconds > 5) {
      this.pendingStore.set((d) => {
        d.sessions.push({ sessionId: crypto.randomUUID(), gameId: a.gameId, startedAt: a.startedAt, seconds: Math.round(seconds), userId: a.userId });
        d.active = null;
      });
    } else this.pendingStore.set({ active: null });
    this.pendingStore.saveNow();
  }

  // ------------------------------------------------------------------ save sync state
  saveState(emulatorId: number): SaveSyncState {
    const uid = String(this.userId ?? 0);
    return this.saveSyncStore.data[uid]?.[emulatorId] ?? {};
  }

  setSaveState(emulatorId: number, update: Partial<SaveSyncState>) {
    const uid = String(this.userId ?? 0);
    this.saveSyncStore.set((d) => {
      d[uid] ??= {};
      d[uid][emulatorId] = { ...(d[uid][emulatorId] ?? {}), ...update };
    });
    this.saveSyncStore.saveNow();
    this.emitState();
  }

  // ------------------------------------------------------------------ ui state
  toast(message: string, error = false) {
    this.emit('toast', { message, error });
  }

  emitState() {
    if (this.emitTimer) return;
    this.emitTimer = setTimeout(() => {
      this.emitTimer = null;
      this.emit('state', this.snapshot());
    }, 30);
  }

  snapshot(): ClientState {
    const c = this.cache;
    const s = this.session;
    const saveSync: ClientState['saveSync'] = {};
    const uid = String(this.userId ?? 0);
    for (const e of c.emulators) {
      saveSync[e.id] = {
        ...(this.saveSyncStore.data[uid]?.[e.id] ?? {}),
        busy: this.busySaves.has(e.id),
        latest: c.latestSaves.find((v) => v.emulatorId === e.id),
      };
    }
    return {
      ready: this.ready,
      session: s ? { serverUrl: s.serverUrl, user: s.user } : null,
      online: this.online,
      serverReachable: this.serverReachable,
      lastError: this.lastError,
      catalog: { games: c.games, emulators: c.emulators, fetchedAt: c.fetchedAt },
      library: c.library,
      installed: this.installedStore.data,
      playtime: Object.fromEntries(c.playtime.map((p) => [p.gameId, p])),
      settings: this.settings,
      gameSettings: this.gameSettingsStore.data,
      running: this.running,
      saveSync,
      pendingSessions: this.pendingStore.data.sessions.filter((x) => x.userId === this.userId).length,
      fullscreen: this.fullscreen,
      os: this.os,
      device: this.device,
      version: this.opts.version,
    };
  }

  flush() {
    for (const s of [this.sessionStore, this.settingsStore, this.installedStore, this.gameSettingsStore, this.saveSyncStore, this.pendingStore, this.cacheStore]) {
      s.saveNow();
    }
  }
}
