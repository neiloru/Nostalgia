import crypto from 'node:crypto';
import type { NextFunction, Request, Response } from 'express';
import type { User } from '@nostalgia/shared';
import { type DB, now } from './db.js';
import type { LoadedSettings } from './config.js';

const SCRYPT = { N: 16384, r: 8, p: 1 };

export function hashPassword(password: string): string {
  const salt = crypto.randomBytes(16);
  const key = crypto.scryptSync(password, salt, 32, SCRYPT);
  return `scrypt$${salt.toString('base64')}$${key.toString('base64')}`;
}

export function verifyPassword(password: string, stored: string): boolean {
  const [alg, saltB64, keyB64] = stored.split('$');
  if (alg !== 'scrypt' || !saltB64 || !keyB64) return false;
  const key = Buffer.from(keyB64, 'base64');
  const test = crypto.scryptSync(password, Buffer.from(saltB64, 'base64'), key.length, SCRYPT);
  return crypto.timingSafeEqual(key, test);
}

export function safeEqual(a: string, b: string): boolean {
  const ha = crypto.createHash('sha256').update(a).digest();
  const hb = crypto.createHash('sha256').update(b).digest();
  return crypto.timingSafeEqual(ha, hb);
}

const sha = (s: string) => crypto.createHash('sha256').update(s).digest('hex');

export function validateUsername(u: unknown): string {
  const s = String(u ?? '').trim();
  if (!/^[A-Za-z0-9_.-]{3,32}$/.test(s)) throw new HttpError(400, 'Username must be 3-32 characters (letters, digits, _ . -)');
  return s;
}

export function validatePassword(p: unknown): string {
  const s = String(p ?? '');
  if (s.length < 6) throw new HttpError(400, 'Password must be at least 6 characters');
  if (s.length > 256) throw new HttpError(400, 'Password too long');
  return s;
}

export class HttpError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

interface UserRow {
  id: number;
  username: string;
  created_at: string;
  disabled: number;
}

export const toUser = (r: UserRow): User => ({ id: r.id, username: r.username, createdAt: r.created_at });

export class Auth {
  private adminTokens = new Map<string, number>(); // token -> expiry

  constructor(private db: DB, private settings: LoadedSettings) {}

  createSession(userId: number, device?: string): string {
    const token = crypto.randomBytes(32).toString('base64url');
    this.db
      .prepare('INSERT INTO sessions (token_hash, user_id, device, created_at, last_seen) VALUES (?, ?, ?, ?, ?)')
      .run(sha(token), userId, device ?? null, now(), now());
    return token;
  }

  revokeSession(token: string) {
    this.db.prepare('UPDATE sessions SET revoked = 1 WHERE token_hash = ?').run(sha(token));
  }

  revokeAllForUser(userId: number, exceptToken?: string) {
    this.db
      .prepare('UPDATE sessions SET revoked = 1 WHERE user_id = ? AND token_hash != ?')
      .run(userId, exceptToken ? sha(exceptToken) : '');
  }

  userForToken(token: string): User | null {
    const row = this.db
      .prepare(
        `SELECT u.id, u.username, u.created_at, u.disabled, s.last_seen FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ? AND s.revoked = 0`,
      )
      .get(sha(token)) as (UserRow & { last_seen: string }) | undefined;
    if (!row || row.disabled) return null;
    // Touch at most once a minute.
    if (Date.now() - Date.parse(row.last_seen) > 60_000) {
      this.db.prepare('UPDATE sessions SET last_seen = ? WHERE token_hash = ?').run(now(), sha(token));
    }
    return toUser(row);
  }

  register(username: string, password: string): User {
    const u = validateUsername(username);
    const p = validatePassword(password);
    const exists = this.db.prepare('SELECT id FROM users WHERE username = ?').get(u);
    if (exists) throw new HttpError(409, 'Username already taken');
    const res = this.db
      .prepare('INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)')
      .run(u, hashPassword(p), now());
    return this.getUser(Number(res.lastInsertRowid))!;
  }

  login(username: string, password: string): User {
    const row = this.db
      .prepare('SELECT id, username, created_at, disabled, password_hash FROM users WHERE username = ?')
      .get(String(username ?? '').trim()) as (UserRow & { password_hash: string }) | undefined;
    if (!row || !verifyPassword(String(password ?? ''), row.password_hash)) throw new HttpError(401, 'Invalid username or password');
    if (row.disabled) throw new HttpError(403, 'This account is disabled');
    return toUser(row);
  }

  setPassword(userId: number, password: string) {
    const p = validatePassword(password);
    this.db.prepare('UPDATE users SET password_hash = ? WHERE id = ?').run(hashPassword(p), userId);
  }

  checkPassword(userId: number, password: string): boolean {
    const row = this.db.prepare('SELECT password_hash FROM users WHERE id = ?').get(userId) as { password_hash: string } | undefined;
    return !!row && verifyPassword(password, row.password_hash);
  }

  getUser(id: number): User | null {
    const row = this.db.prepare('SELECT id, username, created_at, disabled FROM users WHERE id = ?').get(id) as UserRow | undefined;
    return row ? toUser(row) : null;
  }

  // ---- admin (credentials come from the settings file) ----
  adminLogin(username: string, password: string): string {
    const okUser = safeEqual(String(username ?? ''), this.settings.adminUsername);
    const okPass = safeEqual(String(password ?? ''), this.settings.adminPassword);
    if (!okUser || !okPass) throw new HttpError(401, 'Invalid admin credentials');
    const token = crypto.randomBytes(32).toString('base64url');
    this.adminTokens.set(token, Date.now() + 12 * 3600_000);
    return token;
  }

  adminLogout(token: string) {
    this.adminTokens.delete(token);
  }

  isAdmin(token: string | undefined): boolean {
    if (!token) return false;
    const exp = this.adminTokens.get(token);
    if (!exp) return false;
    if (exp < Date.now()) {
      this.adminTokens.delete(token);
      return false;
    }
    return true;
  }
}

export function bearer(req: Request): string | undefined {
  const h = req.headers.authorization;
  if (h?.startsWith('Bearer ')) return h.slice(7).trim();
  const q = req.query.t;
  return typeof q === 'string' ? q : undefined;
}

export function cookie(req: Request, name: string): string | undefined {
  const raw = req.headers.cookie;
  if (!raw) return undefined;
  for (const part of raw.split(';')) {
    const [k, ...v] = part.trim().split('=');
    if (k === name) return decodeURIComponent(v.join('='));
  }
  return undefined;
}

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Request {
      user?: User;
      token?: string;
    }
  }
}

export function requireUser(auth: Auth) {
  return (req: Request, res: Response, next: NextFunction) => {
    const token = bearer(req);
    const user = token ? auth.userForToken(token) : null;
    if (!user) {
      res.status(401).json({ error: 'Not authenticated' });
      return;
    }
    req.user = user;
    req.token = token;
    next();
  };
}

export const ADMIN_COOKIE = 'nostalgia_admin';

export function requireAdmin(auth: Auth) {
  return (req: Request, res: Response, next: NextFunction) => {
    if (!auth.isAdmin(cookie(req, ADMIN_COOKIE))) {
      res.status(401).json({ error: 'Admin login required' });
      return;
    }
    next();
  };
}
