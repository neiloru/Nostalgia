import { useEffect, useState } from 'react';
import type { ClientState, DownloadJob } from '../../main/types';

interface Bridge {
  call<T = unknown>(method: string, ...args: unknown[]): Promise<T>;
  on(channel: 'state' | 'downloads' | 'toast', cb: (payload: unknown) => void): () => void;
  platform: string;
}

declare global {
  interface Window {
    nostalgia: Bridge;
  }
}

export const bridge = window.nostalgia;
export const call = <T = unknown>(method: string, ...args: unknown[]) => bridge.call<T>(method, ...args);

let lastState: ClientState | null = null;
const stateListeners = new Set<(s: ClientState) => void>();
bridge.on('state', (s) => {
  lastState = s as ClientState;
  for (const l of stateListeners) l(lastState);
});
void call<ClientState>('getState').then((s) => {
  if (!lastState) {
    lastState = s;
    for (const l of stateListeners) l(s);
  }
});

export function useClientState(): ClientState | null {
  const [s, set] = useState<ClientState | null>(lastState);
  useEffect(() => {
    stateListeners.add(set);
    if (lastState) set(lastState);
    return () => {
      stateListeners.delete(set);
    };
  }, []);
  return s;
}

let lastJobs: DownloadJob[] = [];
const jobListeners = new Set<(j: DownloadJob[]) => void>();
bridge.on('downloads', (j) => {
  lastJobs = [...(j as DownloadJob[])];
  for (const l of jobListeners) l(lastJobs);
});
void call<DownloadJob[]>('getDownloads').then((j) => {
  lastJobs = j;
  for (const l of jobListeners) l(j);
});

export function useDownloads(): DownloadJob[] {
  const [j, set] = useState<DownloadJob[]>(lastJobs);
  useEffect(() => {
    jobListeners.add(set);
    set(lastJobs);
    return () => {
      jobListeners.delete(set);
    };
  }, []);
  return j;
}

// ------------------------------------------------------------ toasts
export interface Toast {
  id: number;
  message: string;
  error?: boolean;
}
let toastId = 0;
const toastListeners = new Set<(t: Toast) => void>();
export function toast(message: string, error = false) {
  const t = { id: ++toastId, message, error };
  for (const l of toastListeners) l(t);
}
bridge.on('toast', (t) => {
  const p = t as { message: string; error?: boolean };
  toast(p.message, p.error);
});
export function onToast(cb: (t: Toast) => void) {
  toastListeners.add(cb);
  return () => {
    toastListeners.delete(cb);
  };
}

/** Run an action, showing errors as toast. */
export async function act<T>(fn: () => Promise<T>, success?: string): Promise<T | undefined> {
  try {
    const r = await fn();
    if (success) toast(success);
    return r;
  } catch (e) {
    toast((e as Error).message.replace(/^Error invoking remote method '[^']+': (Error: )?/, ''), true);
    return undefined;
  }
}

export const media = (gameId: number, name: string) => `nostalgia://media/${gameId}/${encodeURIComponent(name)}`;
