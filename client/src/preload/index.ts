import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron';

const api = {
  async call<T = unknown>(method: string, ...args: unknown[]): Promise<T> {
    const r = (await ipcRenderer.invoke('invoke', method, args)) as { ok: boolean; value?: T; error?: string };
    if (!r.ok) throw new Error(r.error ?? 'Unknown error');
    return r.value as T;
  },
  on(channel: 'state' | 'downloads' | 'toast', cb: (payload: unknown) => void): () => void {
    const listener = (_e: IpcRendererEvent, payload: unknown) => cb(payload);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
  },
  platform: process.platform,
};

contextBridge.exposeInMainWorld('nostalgia', api);

export type NostalgiaBridge = typeof api;
