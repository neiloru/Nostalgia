import type { Session } from './types.js';

export class ApiError extends Error {
  constructor(public status: number, message: string, public network = false) {
    super(message);
  }
}

export function normalizeServerUrl(input: string): string {
  let s = input.trim();
  if (!s) throw new ApiError(0, 'Server address is required');
  if (!/^https?:\/\//i.test(s)) s = `http://${s}`;
  return s.replace(/\/+$/, '');
}

/** HTTP client for the Nostalgia server. Network failures are reported via `onNetworkError`. */
export class Api {
  constructor(
    private getSession: () => Session | null,
    private device: string,
    private onNetworkError: (e: Error) => void,
    private onUnauthorized: () => void,
  ) {}

  url(path: string, serverUrl?: string): string {
    const base = serverUrl ?? this.getSession()?.serverUrl;
    if (!base) throw new ApiError(0, 'Not connected to a server');
    return `${base}/api${path}`;
  }

  headers(extra: Record<string, string> = {}): Record<string, string> {
    const s = this.getSession();
    return { 'X-Device': this.device, ...(s ? { Authorization: `Bearer ${s.token}` } : {}), ...extra };
  }

  async raw(path: string, init: RequestInit & { serverUrl?: string; timeoutMs?: number } = {}): Promise<Response> {
    const { serverUrl, timeoutMs, ...rest } = init;
    let res: Response;
    try {
      res = await fetch(this.url(path, serverUrl), {
        ...rest,
        headers: { ...this.headers(), ...((rest.headers as Record<string, string>) ?? {}) },
        signal: rest.signal ?? (timeoutMs ? AbortSignal.timeout(timeoutMs) : undefined),
      });
    } catch (e) {
      const err = e as Error;
      if (err.name === 'AbortError' && rest.signal?.aborted) throw err;
      this.onNetworkError(err);
      throw new ApiError(0, `Server unreachable (${err.message})`, true);
    }
    if (res.status === 401 && !path.startsWith('/auth/')) this.onUnauthorized();
    return res;
  }

  async json<T>(path: string, init: Omit<RequestInit, 'body'> & { serverUrl?: string; body?: unknown; timeoutMs?: number } = {}): Promise<T> {
    const { body, ...rest } = init;
    const isJson = body !== undefined && !(body instanceof Uint8Array) && typeof body !== 'string';
    const res = await this.raw(path, {
      timeoutMs: 20_000,
      ...rest,
      headers: { ...(isJson ? { 'Content-Type': 'application/json' } : {}), ...((rest.headers as Record<string, string>) ?? {}) },
      body: isJson ? JSON.stringify(body) : (body as BodyInit | undefined),
    });
    const text = await res.text();
    let data: unknown = null;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      data = text;
    }
    if (!res.ok) throw new ApiError(res.status, (data as { error?: string })?.error ?? `HTTP ${res.status}`);
    return data as T;
  }
}
