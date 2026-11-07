import type { EmuConfig, EmulatorSystemConfig } from './types.js';

export type OsKey = 'windows' | 'linux';

/** Validate an already-parsed emu.yaml document. Throws with a readable message. */
export function validateEmuConfig(doc: unknown): EmuConfig {
  if (!doc || typeof doc !== 'object') throw new Error('emu.yaml must be a mapping');
  const d = doc as Record<string, unknown>;
  const systems = d.systems as Record<string, unknown> | undefined;
  if (!systems || typeof systems !== 'object') throw new Error('emu.yaml: "systems" with at least "windows" or "linux" is required');
  const out: EmuConfig = { systems: {} };
  if (d.name != null) out.name = String(d.name);
  if (d.description != null) out.description = String(d.description);
  if (d.version != null) out.version = String(d.version);
  if (d.extensions != null) {
    if (!Array.isArray(d.extensions)) throw new Error('emu.yaml: "extensions" must be a list');
    out.extensions = d.extensions.map((e) => String(e).replace(/^\./, '').toLowerCase());
  }
  for (const os of ['windows', 'linux'] as const) {
    const s = systems[os] as Record<string, unknown> | undefined;
    if (s == null) continue;
    if (typeof s !== 'object') throw new Error(`emu.yaml: systems.${os} must be a mapping`);
    if (!s.executable) throw new Error(`emu.yaml: systems.${os}.executable is required`);
    const sys: EmulatorSystemConfig = { executable: String(s.executable) };
    if (s.arguments != null) {
      sys.arguments = Array.isArray(s.arguments) ? s.arguments.map(String) : String(s.arguments);
    }
    if (s.workingDirectory != null) sys.workingDirectory = String(s.workingDirectory);
    const saves: string[] = [];
    if (Array.isArray(s.saves)) saves.push(...s.saves.map(String));
    else if (typeof s.saves === 'string') saves.push(s.saves);
    if (s.saveFolder != null) saves.push(String(s.saveFolder));
    if (saves.length) sys.saves = [...new Set(saves)];
    if (s.env && typeof s.env === 'object') {
      sys.env = Object.fromEntries(Object.entries(s.env as Record<string, unknown>).map(([k, v]) => [k, String(v)]));
    }
    out.systems[os] = sys;
  }
  if (!out.systems.windows && !out.systems.linux) throw new Error('emu.yaml: define systems.windows and/or systems.linux');
  return out;
}

/** Split a command line string into arguments, honouring double and single quotes. */
export function splitArgs(input: string): string[] {
  const out: string[] = [];
  let cur = '';
  let quote: string | null = null;
  let has = false;
  for (let i = 0; i < input.length; i++) {
    const c = input[i];
    if (quote) {
      if (c === quote) quote = null;
      else if (c === '\\' && quote === '"' && input[i + 1] === '"') { cur += '"'; i++; }
      else cur += c;
    } else if (c === '"' || c === "'") { quote = c; has = true; }
    else if (/\s/.test(c)) { if (has || cur) out.push(cur); cur = ''; has = false; }
    else { cur += c; has = true; }
  }
  if (has || cur) out.push(cur);
  return out;
}

export interface ExpandContext {
  vars: Record<string, string>; // {rom}, {emulatorDir}, ...
  env: Record<string, string | undefined>;
  home: string;
  os: OsKey;
}

/**
 * Expand `{var}` placeholders, `%ENV%` (windows style), `$ENV` / `${ENV}` and a leading `~`.
 * Unknown placeholders are left untouched.
 */
export function expandString(value: string, ctx: ExpandContext): string {
  let s = value.replace(/\{([a-zA-Z]+)\}/g, (m, k: string) => (k in ctx.vars ? ctx.vars[k] : k === 'home' ? ctx.home : m));
  const envGet = (k: string): string | undefined => {
    if (ctx.os === 'windows') {
      const hit = Object.keys(ctx.env).find((e) => e.toLowerCase() === k.toLowerCase());
      return hit ? ctx.env[hit] : undefined;
    }
    return ctx.env[k];
  };
  s = s.replace(/%([A-Za-z_][A-Za-z0-9_]*)%/g, (m, k: string) => envGet(k) ?? (k.toUpperCase() === 'USERPROFILE' || k.toUpperCase() === 'HOME' ? ctx.home : m));
  s = s.replace(/\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)/g, (m, a?: string, b?: string) => {
    const k = (a ?? b)!;
    return envGet(k) ?? (k === 'HOME' ? ctx.home : m);
  });
  if (s === '~' || s.startsWith('~/') || s.startsWith('~\\')) s = ctx.home + s.slice(1);
  return s;
}

export function buildArguments(sys: EmulatorSystemConfig, ctx: ExpandContext): string[] {
  const raw = sys.arguments ?? ['{rom}'];
  // Split before expanding so paths with spaces stay one argument.
  const parts = Array.isArray(raw) ? raw : splitArgs(raw);
  return parts.map((p) => expandString(p, ctx));
}

const DISC_RE = /[([\s_-](?:disc|disk|cd)\s*[-_ ]?(\d+|[a-z])\b/i;

/** Natural sort compare, so "Disc 2" < "Disc 10". */
export function naturalCompare(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
}

export function discLabel(file: string): string {
  const base = file.split('/').pop()!;
  const m = DISC_RE.exec(base);
  return m ? `Disc ${m[1].toUpperCase()}` : base;
}

/**
 * Pick launchable files from a game's file list.
 * Returns all files having the highest-priority extension found (e.g. all `.cue` of a multi disc game).
 * Files inside the `igdb/` asset folder are ignored.
 */
export function launchCandidates(files: string[], extensions: string[]): string[] {
  const usable = files.filter((f) => !f.startsWith('igdb/') && !f.split('/').pop()!.startsWith('.'));
  const byExt = (ext: string) => usable.filter((f) => f.toLowerCase().endsWith('.' + ext));
  for (const ext of extensions) {
    const hit = byExt(ext.toLowerCase());
    if (hit.length) return hit.sort(naturalCompare);
  }
  return usable.sort(naturalCompare);
}
