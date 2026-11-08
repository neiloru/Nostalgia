import fs from 'node:fs';
import path from 'node:path';
import { app, BrowserWindow, dialog, ipcMain, net, protocol, shell } from 'electron';
import { pathToFileURL } from 'node:url';
import { Core } from './core.js';
import { DownloadManager, MANIFEST } from './downloads.js';
import { SaveSync } from './savesync.js';
import { Launcher } from './launcher.js';
import type { ClientSettings, GameSettings, InstalledItem, ItemKind } from './types.js';

protocol.registerSchemesAsPrivileged([
  { scheme: 'nostalgia', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true } },
]);

// Optional portable mode: a "portable" file next to the executable keeps all data beside it.
const exeDir = path.dirname(app.getPath('exe'));
if (fs.existsSync(path.join(exeDir, 'portable')) || process.env.NOSTALGIA_DATA_DIR) {
  app.setPath('userData', process.env.NOSTALGIA_DATA_DIR ?? path.join(exeDir, 'nostalgia-data'));
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
}

let win: BrowserWindow | null = null;
let core: Core;
let downloads: DownloadManager;
let saves: SaveSync;
let launcher: Launcher;

function send(channel: string, payload: unknown) {
  if (win && !win.isDestroyed()) win.webContents.send(channel, payload);
}

function createWindow() {
  const args = process.argv;
  const fullscreen = args.includes('--fullscreen') || (!args.includes('--windowed') && core.settings.startFullscreen);
  win = new BrowserWindow({
    width: 1440,
    height: 880,
    minWidth: 960,
    minHeight: 600,
    show: false,
    fullscreen,
    backgroundColor: '#0f1116',
    autoHideMenuBar: true,
    title: 'Nostalgia',
    icon: path.join(app.getAppPath(), 'resources', 'icon.png'),
    webPreferences: {
      preload: path.join(__dirname, '../preload/index.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      backgroundThrottling: false,
    },
  });
  win.setMenuBarVisibility(false);
  core.fullscreen = fullscreen;
  win.on('enter-full-screen', () => {
    core.fullscreen = true;
    core.emitState();
  });
  win.on('leave-full-screen', () => {
    core.fullscreen = false;
    core.emitState();
  });
  win.once('ready-to-show', () => win?.show());
  win.webContents.on('before-input-event', (e, input) => {
    if (input.type === 'keyDown' && input.key === 'F11') {
      win?.setFullScreen(!win.isFullScreen());
      e.preventDefault();
    }
    if (input.type === 'keyDown' && input.key === 'F12' && !app.isPackaged) win?.webContents.toggleDevTools();
  });
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:/.test(url)) void shell.openExternal(url);
    return { action: 'deny' };
  });
  win.on('closed', () => (win = null));

  if (process.env.ELECTRON_RENDERER_URL) void win.loadURL(process.env.ELECTRON_RENDERER_URL);
  else void win.loadFile(path.join(__dirname, '../renderer/index.html'));
}

/** `nostalgia://media/<gameId>/<file>`: local install -> local cache -> server (cached). */
function registerMediaProtocol() {
  protocol.handle('nostalgia', async (req) => {
    const url = new URL(req.url);
    const [, idStr, name] = url.pathname.split('/');
    const id = Number(url.hostname === 'media' ? idStr : NaN);
    if (!Number.isInteger(id) || !name || !/^[\w.-]+\.(jpg|jpeg|png|webp)$/i.test(name)) return new Response('bad request', { status: 400 });
    const inst = core.installed('game', id);
    const candidates = [inst ? path.join(inst.path, 'igdb', name) : null, path.join(core.mediaCacheDir, String(id), name)].filter(Boolean) as string[];
    for (const f of candidates) if (fs.existsSync(f)) return net.fetch(pathToFileURL(f).toString());
    if (!core.online) return new Response('offline', { status: 404 });
    try {
      const res = await core.api.raw(`/games/${id}/media/${name}`, { timeoutMs: 30_000 });
      if (!res.ok) return new Response('not found', { status: 404 });
      const buf = Buffer.from(await res.arrayBuffer());
      const dir = path.join(core.mediaCacheDir, String(id));
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, name), buf);
      return new Response(buf, { headers: { 'Content-Type': res.headers.get('content-type') ?? 'image/jpeg' } });
    } catch {
      return new Response('error', { status: 502 });
    }
  });
}

function isInside(child: string, parent: string) {
  const rel = path.relative(parent, child);
  return !!rel && !rel.startsWith('..') && !path.isAbsolute(rel);
}

async function uninstall(kind: ItemKind, id: number) {
  const item = core.installed(kind, id);
  if (!item) return;
  if (kind === 'emulator') {
    if (core.running?.emulatorId === id) throw new Error('The emulator is running');
    await saves.syncUp(id, 'before uninstall').catch(() => undefined);
  } else if (core.running?.gameId === id) throw new Error('The game is running');
  // Only remove folders created by Nostalgia inside a download location.
  const ok = fs.existsSync(path.join(item.path, MANIFEST)) && isInside(item.path, item.location);
  if (!ok) throw new Error(`Refusing to delete ${item.path}`);
  await fs.promises.rm(item.path, { recursive: true, force: true });
  core.unregisterInstalled(kind, id);
  core.toast(`${item.name} was uninstalled`);
}

type Handler = (...args: never[]) => unknown;

function handlers(): Record<string, Handler> {
  return {
    getState: () => core.snapshot(),
    getDownloads: () => downloads.list(),
    login: (server: string, user: string, pass: string) => core.login(server, user, pass, false),
    register: (server: string, user: string, pass: string) => core.login(server, user, pass, true),
    logout: async () => {
      if (launcher.isRunning()) throw new Error('Close the running game first');
      downloads.pauseAll();
      await core.logout();
    },
    changePassword: (o: string, n: string) => core.changePassword(o, n),
    refresh: async () => {
      const ok = await core.checkHealth();
      if (ok) await core.refreshAll();
      return ok;
    },
    getGameDetail: (id: number) => core.getGameDetail(id),
    addGame: async (id: number) => {
      await core.addGame(id);
    },
    removeGame: (id: number) => core.removeGame(id),
    addEmulator: (id: number) => core.addEmulator(id),
    removeEmulator: (id: number) => core.removeEmulator(id),
    install: async (kind: ItemKind, id: number, location?: string) => {
      if (kind === 'game' && !core.cache.library.games.includes(id)) await core.addGame(id).catch(() => undefined);
      if (kind === 'emulator' && !core.cache.library.emulators.includes(id)) await core.addEmulator(id).catch(() => undefined);
      await downloads.enqueue(kind, id, location);
      if (kind === 'game' && core.settings.installEmulatorWithGame) {
        const emu = launcher.pickEmulator(id);
        if (emu && !core.installed('emulator', emu.id)) await downloads.enqueue('emulator', emu.id, location).catch(() => undefined);
      }
    },
    uninstall: (kind: ItemKind, id: number) => uninstall(kind, id),
    openFolder: (kind: ItemKind, id: number) => {
      const it = core.installed(kind, id);
      if (it) void shell.openPath(it.path);
    },
    pauseDownload: (key: string) => downloads.pause(key),
    resumeDownload: (key: string) => downloads.resume(key),
    cancelDownload: (key: string) => downloads.cancel(key),
    pauseAll: () => downloads.pauseAll(),
    resumeAll: () => downloads.resumeAll(),
    clearFinished: () => downloads.clearFinished(),
    play: (id: number) => launcher.launch(id),
    stopGame: () => launcher.stop(),
    launchOptions: (id: number) => launcher.launchOptions(id),
    setGameSettings: (id: number, s: GameSettings) => {
      core.gameSettingsStore.set((d) => {
        d[id] = { ...(d[id] ?? {}), ...s };
      });
      core.emitState();
      return launcher.launchOptions(id);
    },
    listSaves: (emulatorId: number) => saves.listVersions(emulatorId),
    restoreSave: (emulatorId: number, versionId: number) => saves.restore(emulatorId, versionId),
    syncSaves: (emulatorId: number) => saves.syncNow(emulatorId),
    savePaths: (emulatorId: number) => saves.savePaths(emulatorId),
    openSaveBackups: () => {
      fs.mkdirSync(core.saveBackupDir, { recursive: true });
      void shell.openPath(core.saveBackupDir);
    },
    setSettings: (s: Partial<ClientSettings>) => {
      core.settingsStore.set(s);
      core.emitState();
    },
    addLocation: async () => {
      const r = await dialog.showOpenDialog(win!, { title: 'Choose a download location', properties: ['openDirectory', 'createDirectory'] });
      if (r.canceled || !r.filePaths[0]) return null;
      const p = r.filePaths[0];
      core.settingsStore.set((s) => {
        if (!s.locations.includes(p)) s.locations.push(p);
      });
      core.emitState();
      return p;
    },
    removeLocation: (p: string) => {
      const used = Object.values({ ...core.installedStore.data.games, ...core.installedStore.data.emulators }).some((i) => i.location === p);
      if (used) throw new Error('This location still contains installed games or emulators');
      core.settingsStore.set((s) => {
        s.locations = s.locations.filter((l) => l !== p);
        if (s.defaultLocation === p) s.defaultLocation = undefined;
      });
      core.emitState();
    },
    locationInfo: async () => {
      const out: { path: string; free?: number; exists: boolean }[] = [];
      for (const p of core.settings.locations) {
        try {
          fs.mkdirSync(p, { recursive: true });
          const st = await fs.promises.statfs(p);
          out.push({ path: p, free: st.bavail * st.bsize, exists: true });
        } catch {
          out.push({ path: p, exists: false });
        }
      }
      return out;
    },
    toggleFullscreen: () => win?.setFullScreen(!win.isFullScreen()),
    minimize: () => win?.minimize(),
    quit: () => app.quit(),
    openExternal: (url: string) => {
      if (/^https?:\/\//.test(url)) void shell.openExternal(url);
    },
  };
}

app.whenReady().then(async () => {
  core = new Core({
    userData: app.getPath('userData'),
    home: app.getPath('home'),
    documents: app.getPath('documents'),
    appData: app.getPath('appData'),
    version: app.getVersion(),
  });
  downloads = new DownloadManager(core);
  saves = new SaveSync(core);
  launcher = new Launcher(core, saves, downloads, {
    beforeLaunch: () => {
      if (core.settings.minimizeOnLaunch && win && !win.isFullScreen()) win.minimize();
    },
    afterExit: () => {
      if (win) {
        if (win.isMinimized()) win.restore();
        win.focus();
      }
    },
  });

  const h = handlers();
  ipcMain.handle('invoke', async (_e, method: string, args: unknown[]) => {
    const fn = h[method];
    if (!fn) throw new Error(`Unknown method ${method}`);
    try {
      return { ok: true, value: await (fn as (...a: unknown[]) => unknown)(...args) };
    } catch (err) {
      return { ok: false, error: (err as Error).message };
    }
  });

  core.on('state', (s) => send('state', s));
  core.on('toast', (t) => send('toast', t));
  downloads.on('update', (jobs) => send('downloads', jobs));
  core.on('login', () => void saves.syncAll('login'));
  // A freshly installed emulator gets the newest cloud save right away, not only on the next start.
  downloads.on('installed', (item: InstalledItem) => {
    if (item.kind === 'emulator' && core.settings.autoSyncSaves) void saves.syncDown(item.id, 'install').catch(() => undefined);
  });

  registerMediaProtocol();
  createWindow();
  await core.start();
  // Startup: fetch the newest cloud saves of all installed emulators.
  void saves.syncAll('startup');
  void saves.retryPending();

  app.on('second-instance', () => {
    if (win) {
      if (win.isMinimized()) win.restore();
      win.focus();
    }
  });
});

app.on('window-all-closed', () => {
  core?.flush();
  app.quit();
});

app.on('before-quit', () => core?.flush());
