import crypto from 'node:crypto';
import fs from 'node:fs';
import zlib from 'node:zlib';
import { log } from './log.js';

const API = 'https://hasheous.org/api/v1/Lookup/ByHash';

export interface FileHashes {
  md5: string;
  sha1: string;
  crc32: string;
}

/** Hash a file with md5, sha1 and crc32 in a single streaming pass. */
export function hashFile(file: string, onProgress?: (bytes: number) => void): Promise<FileHashes> {
  return new Promise((resolve, reject) => {
    const md5 = crypto.createHash('md5');
    const sha1 = crypto.createHash('sha1');
    let crc = 0;
    const stream = fs.createReadStream(file, { highWaterMark: 4 * 1024 * 1024 });
    stream.on('data', (chunk) => {
      const b = chunk as Buffer;
      md5.update(b);
      sha1.update(b);
      crc = zlib.crc32(b, crc);
      onProgress?.(b.length);
    });
    stream.on('error', reject);
    stream.on('end', () =>
      resolve({
        md5: md5.digest('hex'),
        sha1: sha1.digest('hex'),
        crc32: (crc >>> 0).toString(16).padStart(8, '0'),
      }),
    );
  });
}

export interface HasheousResult {
  status: 'match' | 'nomatch' | 'error';
  name?: string;
  platform?: string;
  igdbId?: number;
  romName?: string;
  source?: string;
  error?: string;
  raw?: unknown;
}

interface HasheousResponse {
  name?: string;
  platform?: { name?: string };
  signature?: { rom?: { name?: string; signatureSource?: string } };
  metadata?: { source?: string; id?: string; status?: string }[];
}

async function query(body: Record<string, string>, timeoutMs: number): Promise<HasheousResult> {
  const res = await fetch(API, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (res.status === 404) return { status: 'nomatch' };
  if (!res.ok) return { status: 'error', error: `HTTP ${res.status}` };
  const j = (await res.json()) as HasheousResponse;
  const igdb = j.metadata?.find((m) => m.source === 'IGDB' && m.id);
  return {
    status: 'match',
    name: j.name,
    platform: j.platform?.name,
    igdbId: igdb?.id ? Number(igdb.id) : undefined,
    romName: j.signature?.rom?.name,
    source: j.signature?.rom?.signatureSource,
    raw: { name: j.name, platform: j.platform?.name, rom: j.signature?.rom, metadata: j.metadata },
  };
}

/**
 * Look a file up by hash. MD5 alone is answered quickly by hasheous.org, combined
 * hash queries can be very slow - so MD5 first and SHA1 only as a fallback.
 */
export async function lookupHasheous(h: FileHashes): Promise<HasheousResult> {
  try {
    const first = await query({ md5: h.md5 }, 120_000);
    if (first.status !== 'nomatch') return first;
    try {
      return await query({ sha1: h.sha1 }, 120_000);
    } catch {
      return first;
    }
  } catch (e) {
    log.warn(`Hasheous lookup failed: ${(e as Error).message}`);
    return { status: 'error', error: (e as Error).message };
  }
}

/** Files that are worth hashing (skip cue sheets, text, our own asset folder). */
export function shouldHash(rel: string): boolean {
  if (rel.startsWith('igdb/')) return false;
  return !/\.(cue|m3u|txt|nfo|jpg|png|json|yaml|yml|sfv|md5|dat)$/i.test(rel);
}
