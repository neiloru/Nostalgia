import fs from 'node:fs';
import path from 'node:path';
import type { GameMetadata } from '@nostalgia/shared';
import { platformInfo } from '@nostalgia/shared';
import type { LoadedSettings } from './config.js';
import { log } from './log.js';

const IMG = 'https://images.igdb.com/igdb/image/upload';
const MAX_ARTWORKS = 6;
const MAX_SCREENSHOTS = 8;

interface Named {
  name: string;
}
interface Img {
  image_id: string;
}
interface IgdbGame {
  id: number;
  name: string;
  slug?: string;
  summary?: string;
  storyline?: string;
  url?: string;
  first_release_date?: number;
  rating?: number;
  rating_count?: number;
  total_rating?: number;
  total_rating_count?: number;
  aggregated_rating?: number;
  genres?: Named[];
  themes?: Named[];
  game_modes?: Named[];
  player_perspectives?: Named[];
  franchises?: Named[];
  collection?: Named;
  platforms?: Named[];
  involved_companies?: { company?: Named; developer?: boolean; publisher?: boolean }[];
  cover?: Img;
  artworks?: Img[];
  screenshots?: Img[];
  videos?: { name?: string; video_id: string }[];
}

export interface IgdbSearchHit {
  igdbId: number;
  name: string;
  year?: number;
  platforms: string[];
  coverUrl?: string;
}

export class Igdb {
  private token: { value: string; expires: number } | null = null;
  private lastRequest = 0;
  private chain: Promise<unknown> = Promise.resolve();

  constructor(private settings: LoadedSettings) {}

  get configured() {
    return !!(this.settings.igdbClientId && this.settings.igdbClientSecret);
  }

  private async getToken(): Promise<string> {
    if (this.token && this.token.expires > Date.now() + 60_000) return this.token.value;
    const url = new URL('https://id.twitch.tv/oauth2/token');
    url.searchParams.set('client_id', this.settings.igdbClientId);
    url.searchParams.set('client_secret', this.settings.igdbClientSecret);
    url.searchParams.set('grant_type', 'client_credentials');
    const res = await fetch(url, { method: 'POST' });
    if (!res.ok) throw new Error(`IGDB authentication failed (${res.status}): ${await res.text()}`);
    const json = (await res.json()) as { access_token: string; expires_in: number };
    this.token = { value: json.access_token, expires: Date.now() + json.expires_in * 1000 };
    return this.token.value;
  }

  /** IGDB allows 4 requests / second - requests are serialised with spacing. */
  query<T>(endpoint: string, body: string): Promise<T> {
    const run = async (): Promise<T> => {
      if (!this.configured) throw new Error('IGDB credentials are not configured in the settings file');
      const wait = this.lastRequest + 280 - Date.now();
      if (wait > 0) await new Promise((r) => setTimeout(r, wait));
      this.lastRequest = Date.now();
      for (let attempt = 0; ; attempt++) {
        const res = await fetch(`https://api.igdb.com/v4/${endpoint}`, {
          method: 'POST',
          headers: { 'Client-ID': this.settings.igdbClientId, Authorization: `Bearer ${await this.getToken()}`, Accept: 'application/json' },
          body,
        });
        if (res.status === 429 && attempt < 5) {
          await new Promise((r) => setTimeout(r, 1000 * (attempt + 1)));
          continue;
        }
        if (res.status === 401 && attempt < 1) {
          this.token = null;
          continue;
        }
        if (!res.ok) throw new Error(`IGDB ${endpoint} failed (${res.status}): ${await res.text()}`);
        return (await res.json()) as T;
      }
    };
    const p = this.chain.then(run, run);
    this.chain = p.catch(() => undefined);
    return p;
  }

  async search(q: string, platform?: string): Promise<IgdbSearchHit[]> {
    const term = q.replace(/"/g, '').trim();
    if (!term) return [];
    const idMatch = /^\d+$/.test(term);
    const pid = platform ? platformInfo(platform).igdb : undefined;
    const fields = 'fields name,first_release_date,platforms.name,cover.image_id;';
    let body: string;
    if (idMatch) body = `${fields} where id = ${term};`;
    else body = `search "${term}"; ${fields} ${pid ? `where platforms = (${pid});` : ''} limit 25;`;
    let rows = await this.query<IgdbGame[]>('games', body);
    if (!rows.length && pid && !idMatch) {
      rows = await this.query<IgdbGame[]>('games', `search "${term}"; ${fields} limit 25;`);
    }
    return rows.map((g) => ({
      igdbId: g.id,
      name: g.name,
      year: g.first_release_date ? new Date(g.first_release_date * 1000).getUTCFullYear() : undefined,
      platforms: (g.platforms ?? []).map((p) => p.name),
      coverUrl: g.cover ? `${IMG}/t_cover_small/${g.cover.image_id}.jpg` : undefined,
    }));
  }

  async fetchGame(igdbId: number): Promise<IgdbGame> {
    const rows = await this.query<IgdbGame[]>(
      'games',
      `fields name,slug,summary,storyline,url,first_release_date,rating,rating_count,total_rating,total_rating_count,aggregated_rating,
        genres.name,themes.name,game_modes.name,player_perspectives.name,franchises.name,collection.name,platforms.name,
        involved_companies.company.name,involved_companies.developer,involved_companies.publisher,
        cover.image_id,artworks.image_id,screenshots.image_id,videos.name,videos.video_id;
       where id = ${igdbId};`,
    );
    if (!rows.length) throw new Error(`IGDB game ${igdbId} not found`);
    return rows[0];
  }

  /**
   * Download metadata + cover + artworks + screenshots into `<gameDir>/igdb/`.
   * Existing files are overwritten in place (never deleted).
   */
  async downloadInto(gameDir: string, igdbId: number): Promise<GameMetadata> {
    const g = await this.fetchGame(igdbId);
    const dir = path.join(gameDir, 'igdb');
    // Never recreate a game folder that was renamed or moved while the download was queued.
    if (!fs.existsSync(gameDir)) throw new Error(`Game folder no longer exists: ${gameDir}`);
    fs.mkdirSync(dir, { recursive: true });

    const cover = g.cover ? await this.saveImage(`${IMG}/t_cover_big_2x/${g.cover.image_id}.jpg`, path.join(dir, 'cover.jpg')) : false;
    const artworks: string[] = [];
    for (const [i, a] of (g.artworks ?? []).slice(0, MAX_ARTWORKS).entries()) {
      const name = `artwork-${String(i + 1).padStart(2, '0')}.jpg`;
      if (await this.saveImage(`${IMG}/t_1080p/${a.image_id}.jpg`, path.join(dir, name))) artworks.push(name);
    }
    const screenshots: string[] = [];
    for (const [i, s] of (g.screenshots ?? []).slice(0, MAX_SCREENSHOTS).entries()) {
      const name = `screenshot-${String(i + 1).padStart(2, '0')}.jpg`;
      if (await this.saveImage(`${IMG}/t_1080p/${s.image_id}.jpg`, path.join(dir, name))) screenshots.push(name);
    }

    const companies = g.involved_companies ?? [];
    const meta: GameMetadata = {
      igdbId: g.id,
      name: g.name,
      slug: g.slug,
      summary: g.summary,
      storyline: g.storyline,
      url: g.url,
      releaseDate: g.first_release_date ? new Date(g.first_release_date * 1000).toISOString().slice(0, 10) : undefined,
      year: g.first_release_date ? new Date(g.first_release_date * 1000).getUTCFullYear() : undefined,
      rating: g.total_rating != null ? Math.round(g.total_rating) : g.rating != null ? Math.round(g.rating) : undefined,
      ratingCount: g.total_rating_count ?? g.rating_count,
      aggregatedRating: g.aggregated_rating != null ? Math.round(g.aggregated_rating) : undefined,
      genres: (g.genres ?? []).map((x) => x.name),
      themes: (g.themes ?? []).map((x) => x.name),
      gameModes: (g.game_modes ?? []).map((x) => x.name),
      perspectives: (g.player_perspectives ?? []).map((x) => x.name),
      developers: companies.filter((c) => c.developer && c.company).map((c) => c.company!.name),
      publishers: companies.filter((c) => c.publisher && c.company).map((c) => c.company!.name),
      franchises: [...(g.franchises ?? []).map((x) => x.name), ...(g.collection ? [g.collection.name] : [])].filter(
        (v, i, a) => a.indexOf(v) === i,
      ),
      platforms: (g.platforms ?? []).map((x) => x.name),
      videos: (g.videos ?? []).map((v) => ({ name: v.name, youtubeId: v.video_id })),
      cover: cover ? 'cover.jpg' : undefined,
      artworks,
      screenshots,
      fetchedAt: new Date().toISOString(),
    };
    fs.writeFileSync(path.join(dir, 'igdb-raw.json'), JSON.stringify(g, null, 2));
    fs.writeFileSync(path.join(dir, 'metadata.json'), JSON.stringify(meta, null, 2));
    return meta;
  }

  private async saveImage(url: string, file: string): Promise<boolean> {
    try {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const buf = Buffer.from(await res.arrayBuffer());
      const tmp = `${file}.tmp`;
      fs.writeFileSync(tmp, buf);
      fs.renameSync(tmp, file);
      return true;
    } catch (e) {
      log.warn(`Image download failed ${url}: ${(e as Error).message}`);
      return fs.existsSync(file);
    }
  }
}
