import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { buildArguments, discLabel, expandString, launchCandidates, platformInfo, splitArgs, type EmulatorSummary } from '@nostalgia/shared';
import type { Core } from './core.js';
import type { SaveSync } from './savesync.js';
import type { DownloadManager } from './downloads.js';
import type { LaunchOptions } from './types.js';

export class Launcher {
  private child: ChildProcess | null = null;
  private heartbeatTimer: NodeJS.Timeout | null = null;

  constructor(
    private core: Core,
    private saves: SaveSync,
    private downloads: DownloadManager,
    private hooks: { beforeLaunch?: () => void; afterExit?: () => void } = {},
  ) {}

  emulatorsFor(platform: string): EmulatorSummary[] {
    return this.core.cache.emulators.filter((e) => e.platform === platform);
  }

  /** Emulator used for a game: per-game choice, else an installed one, else the first available. */
  pickEmulator(gameId: number): EmulatorSummary | undefined {
    const g = this.core.game(gameId) ?? (this.core.installed('game', gameId) as { platform: string } | undefined);
    if (!g) return undefined;
    const list = this.emulatorsFor(g.platform).filter((e) => e.config?.systems[this.core.os]);
    const chosen = this.core.gameSettingsStore.data[gameId]?.emulatorId;
    return (
      list.find((e) => e.id === chosen) ?? list.find((e) => this.core.installed('emulator', e.id)) ?? list[0]
    );
  }

  launchOptions(gameId: number): LaunchOptions {
    const g = this.core.game(gameId) ?? this.core.installed('game', gameId);
    const settings = this.core.gameSettingsStore.data[gameId] ?? {};
    const emus = g ? this.emulatorsFor(g.platform) : [];
    const emu = this.pickEmulator(gameId);
    const inst = this.core.installed('game', gameId);
    const exts = emu?.config?.extensions ?? platformInfo(g?.platform ?? '').ext;
    const discs = inst ? launchCandidates(inst.files.map((f) => f.path), exts) : [];
    return {
      emulators: emus.map((e) => ({
        id: e.id,
        name: e.name,
        installed: !!this.core.installed('emulator', e.id),
        supported: !!e.config?.systems[this.core.os],
      })),
      emulatorId: emu?.id,
      discs: discs.map((file) => ({ file, label: discs.length > 1 ? discLabel(file) : file })),
      disc: settings.disc && discs.includes(settings.disc) ? settings.disc : discs[0],
      extraArgs: settings.extraArgs,
    };
  }

  isRunning() {
    return !!this.child;
  }

  async launch(gameId: number): Promise<void> {
    if (this.child) throw new Error(`${this.core.running?.name ?? 'A game'} is already running`);
    const inst = this.core.installed('game', gameId);
    if (!inst) throw new Error('The game is not installed');
    const emu = this.pickEmulator(gameId);
    if (!emu) throw new Error(`No emulator for ${platformInfo(inst.platform).name} is available on the server`);
    const sys = emu.config?.systems[this.core.os];
    if (!sys) throw new Error(`${emu.name} has no ${this.core.os} configuration in its emu.yaml`);
    const emuInst = this.core.installed('emulator', emu.id);
    if (!emuInst) {
      await this.downloads.enqueue('emulator', emu.id).catch(() => undefined);
      throw new Error(`${emu.name} is not installed yet - it was added to the downloads`);
    }

    const opts = this.launchOptions(gameId);
    if (!opts.disc) throw new Error('No launchable file found in the game folder');
    const rom = path.join(inst.path, ...opts.disc.split('/'));

    // Make sure we play with the newest cloud save.
    if (this.core.settings.autoSyncSaves && this.core.online) {
      try {
        await this.saves.syncDown(emu.id, 'launch');
      } catch (e) {
        this.core.toast(`Could not check cloud saves: ${(e as Error).message}`, true);
      }
    }
    for (const p of this.saves.savePaths(emu.id)) fs.mkdirSync(p, { recursive: true });

    const ctx = {
      vars: {
        rom,
        romDir: path.dirname(rom),
        gameDir: inst.path,
        emulatorDir: emuInst.path,
        home: this.core.opts.home,
        documents: this.core.opts.documents,
        appData: this.core.opts.appData,
        config: process.env.XDG_CONFIG_HOME ?? path.join(this.core.opts.home, '.config'),
        dataHome: process.env.XDG_DATA_HOME ?? path.join(this.core.opts.home, '.local', 'share'),
        disc: opts.disc,
      },
      env: process.env,
      home: this.core.opts.home,
      os: this.core.os,
    };
    const exe = path.resolve(emuInst.path, expandString(sys.executable, ctx));
    if (!fs.existsSync(exe)) throw new Error(`Emulator executable not found: ${exe}`);
    if (this.core.os === 'linux') {
      try {
        fs.chmodSync(exe, 0o755);
      } catch {
        /* ignore */
      }
    }
    const args = [...buildArguments(sys, ctx), ...(opts.extraArgs ? splitArgs(opts.extraArgs) : [])];
    const cwd = sys.workingDirectory ? path.resolve(emuInst.path, expandString(sys.workingDirectory, ctx)) : emuInst.path;
    const env = { ...process.env, ...Object.fromEntries(Object.entries(sys.env ?? {}).map(([k, v]) => [k, expandString(v, ctx)])) };
    // AppImages launched from inside an AppImage must not inherit our mount variables.
    delete env.APPIMAGE;
    delete env.APPDIR;

    const startedAt = new Date();
    const name = this.core.game(gameId)?.name ?? inst.name;
    console.log(`[launch] ${exe} ${args.map((a) => JSON.stringify(a)).join(' ')}`);
    const child = spawn(exe, args, { cwd, env, stdio: 'ignore', windowsHide: false });
    this.child = child;
    this.core.running = { gameId, emulatorId: emu.id, name, startedAt: startedAt.toISOString(), pid: child.pid };
    this.core.heartbeat(this.core.running);
    this.heartbeatTimer = setInterval(() => this.core.heartbeat(this.core.running), 30_000);
    this.core.emitState();
    this.hooks.beforeLaunch?.();

    // Do not block the caller until the emulator exits.
    void new Promise<void>((resolve) => {
      let finished = false;
      const done = async (code: number | null, err?: Error) => {
        if (finished) return;
        finished = true;
        if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
        this.heartbeatTimer = null;
        this.child = null;
        const seconds = (Date.now() - startedAt.getTime()) / 1000;
        this.core.running = null;
        this.core.heartbeat(null);
        this.core.emitState();
        this.hooks.afterExit?.();
        if (err) this.core.toast(`Could not start ${emu.name}: ${err.message}`, true);
        else if (seconds < 3 && code) this.core.toast(`${emu.name} exited immediately (code ${code})`, true);
        if (!err) {
          this.core.recordSession(gameId, startedAt.toISOString(), seconds);
          await this.saves.syncUp(emu.id, `${name} (${Math.round(seconds / 60)} min)`).catch(() => undefined);
        }
        resolve();
      };
      child.once('error', (e) => void done(null, e));
      child.once('exit', (code) => void done(code));
    });
  }

  stop() {
    if (!this.child) return;
    const pid = this.child.pid;
    if (this.core.os === 'windows' && pid) {
      // Kill the whole process tree on Windows.
      spawn('taskkill', ['/pid', String(pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
    } else this.child.kill('SIGTERM');
  }
}
