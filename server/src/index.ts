import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import express, { type NextFunction, type Request, type Response } from 'express';
import { loadSettings } from './config.js';
import { openDb } from './db.js';
import { Auth, HttpError } from './auth.js';
import { Library } from './library.js';
import { Igdb } from './igdb.js';
import { EventHub } from './events.js';
import { Jobs } from './jobs.js';
import { Saves } from './saves.js';
import { apiRouter } from './routes/api.js';
import { adminRouter } from './routes/admin.js';
import { log } from './log.js';

export async function createServer(settingsFile?: string) {
  const settings = loadSettings(settingsFile);
  const db = openDb(settings.dataPath);
  const auth = new Auth(db, settings);
  const library = new Library(db, settings);
  const igdb = new Igdb(settings);
  const events = new EventHub();
  const jobs = new Jobs(db, library, igdb, events);
  const saves = new Saves(db, settings.dataPath);

  library.ensureFolders();

  const app = express();
  app.disable('x-powered-by');
  app.set('trust proxy', true);

  app.use('/api', apiRouter({ db, auth, library, saves, events, settings }));
  app.use('/admin/api', adminRouter({ db, auth, library, saves, jobs, igdb, events, settings }));

  // Admin web UI (static). Works from src (dev) and dist (bundled).
  const here = path.dirname(fileURLToPath(import.meta.url));
  const adminDir = [path.join(here, 'admin'), path.join(here, '..', 'admin')].find((p) => fs.existsSync(path.join(p, 'index.html')));
  if (adminDir) {
    app.use('/admin', express.static(adminDir, { index: 'index.html' }));
  }
  app.get('/', (_req, res) => res.redirect('/admin/'));

  app.use((_req, res) => res.status(404).json({ error: 'Not found' }));
  app.use((err: unknown, _req: Request, res: Response, _next: NextFunction) => {
    if (res.headersSent) {
      res.end();
      return;
    }
    if (err instanceof HttpError) {
      res.status(err.status).json({ error: err.message });
      return;
    }
    const e = err as { status?: number; statusCode?: number; message?: string; type?: string };
    const status = e.status ?? e.statusCode ?? 500;
    if (status >= 500) log.error(err);
    res.status(status).json({ error: e.message ?? 'Internal error' });
  });

  return { app, settings, db, jobs, library, auth, events, saves };
}

async function main() {
  const { app, settings, jobs } = await createServer();
  const server = app.listen(settings.port, settings.host, () => {
    log.info(`Nostalgia server listening on http://${settings.host === '0.0.0.0' ? 'localhost' : settings.host}:${settings.port}`);
    log.info(`Admin UI: http://localhost:${settings.port}/admin  (library: ${settings.libraryPath})`);
    if (settings.adminPassword === 'change-me') log.warn('The admin password is still "change-me" - edit ' + settings.file);
    if (!settings.igdbClientId) log.warn('IGDB credentials missing - metadata, covers and artwork cannot be downloaded');
  });
  server.requestTimeout = 0; // large downloads
  server.headersTimeout = 65_000;

  await jobs.scan({ hash: false });
  if (settings.scanIntervalMinutes > 0) {
    setInterval(() => void jobs.scan({ hash: settings.hasheousOnScheduledScan }), settings.scanIntervalMinutes * 60_000).unref();
  }

  const stop = () => {
    log.info('Shutting down');
    server.close();
    process.exit(0);
  };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain || process.env.NOSTALGIA_MAIN === '1') {
  main().catch((e) => {
    log.error(e);
    process.exit(1);
  });
}
