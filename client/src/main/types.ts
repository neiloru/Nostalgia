import type { EmulatorSummary, GameSummary, LibraryState, PlaySession, PlaytimeEntry, RemoteFile, SaveVersion, User } from '@nostalgia/shared';

export type ItemKind = 'game' | 'emulator';

export interface Session {
  serverUrl: string;
  token: string;
  user: User;
}

export interface InstalledItem {
  kind: ItemKind;
  id: number;
  name: string;
  platform: string;
  folder: string;
  location: string;
  path: string;
  files: RemoteFile[];
  sizeBytes: number;
  installedAt: string;
}

export interface GameSettings {
  emulatorId?: number;
  disc?: string;
  extraArgs?: string;
}

export interface ClientSettings {
  locations: string[];
  defaultLocation?: string;
  startFullscreen: boolean;
  /** UI scale factor, e.g. 1.5; 0 = automatic (by window size, 1920x1080 = 100%). */
  uiScale: number;
  autoSyncSaves: boolean;
  minimizeOnLaunch: boolean;
  installEmulatorWithGame: boolean;
}

export interface SaveSyncState {
  versionId?: number;
  version?: number;
  fingerprint?: string;
  syncedAt?: string;
  pendingUpload?: boolean;
  lastError?: string;
  lastResult?: string;
}

export type DownloadStatus = 'queued' | 'downloading' | 'paused' | 'error' | 'done';

export interface DownloadJob {
  key: string; // `${kind}:${id}`
  kind: ItemKind;
  id: number;
  name: string;
  platform: string;
  folder: string;
  location: string;
  targetDir: string;
  files: RemoteFile[];
  totalBytes: number;
  receivedBytes: number;
  status: DownloadStatus;
  error?: string;
  speed: number; // bytes/s
  currentFile?: string;
  addedAt: string;
  finishedAt?: string;
}

export interface RunningGame {
  gameId: number;
  emulatorId: number;
  name: string;
  startedAt: string;
  pid?: number;
}

export interface ClientState {
  ready: boolean;
  session: { serverUrl: string; user: User } | null;
  online: boolean;
  serverReachable: boolean;
  lastError?: string;
  catalog: { games: GameSummary[]; emulators: EmulatorSummary[]; fetchedAt?: string };
  library: LibraryState;
  installed: { games: Record<number, InstalledItem>; emulators: Record<number, InstalledItem> };
  playtime: Record<number, PlaytimeEntry>;
  settings: ClientSettings;
  gameSettings: Record<number, GameSettings>;
  running: RunningGame | null;
  saveSync: Record<number, SaveSyncState & { busy?: boolean; latest?: SaveVersion }>;
  pendingSessions: number;
  fullscreen: boolean;
  /** Zoom factor currently applied to the UI. */
  uiZoom: number;
  os: 'windows' | 'linux';
  device: string;
  version: string;
}

export interface LaunchOptions {
  emulators: { id: number; name: string; installed: boolean; supported: boolean }[];
  emulatorId?: number;
  discs: { file: string; label: string }[];
  disc?: string;
  extraArgs?: string;
}

export type { PlaySession };
