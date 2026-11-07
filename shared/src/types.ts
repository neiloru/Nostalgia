/** DTOs shared between server and client. */

export interface User {
  id: number;
  username: string;
  createdAt: string;
}

export interface AuthResponse {
  token: string;
  user: User;
}

/** Normalised IGDB metadata stored as `igdb/metadata.json` inside each game folder. */
export interface GameMetadata {
  igdbId: number;
  name: string;
  slug?: string;
  summary?: string;
  storyline?: string;
  url?: string;
  releaseDate?: string; // ISO date
  year?: number;
  rating?: number; // 0..100
  ratingCount?: number;
  aggregatedRating?: number;
  genres: string[];
  themes: string[];
  gameModes: string[];
  perspectives: string[];
  developers: string[];
  publishers: string[];
  franchises: string[];
  platforms: string[];
  videos: { name?: string; youtubeId: string }[];
  /** File names relative to the `igdb` folder. */
  cover?: string;
  artworks: string[];
  screenshots: string[];
  fetchedAt: string;
}

export interface GameSummary {
  id: number;
  kind: 'game';
  name: string;
  platform: string;
  platformName: string;
  folder: string;
  igdbId: number;
  sizeBytes: number;
  fileCount: number;
  hasCover: boolean;
  hasArtwork: boolean;
  year?: number;
  genres: string[];
  themes: string[];
  gameModes: string[];
  developers: string[];
  publishers: string[];
  rating?: number;
  addedAt: string;
}

export interface GameDetail extends GameSummary {
  metadata: GameMetadata | null;
}

export interface EmulatorSystemConfig {
  executable: string;
  arguments?: string | string[];
  workingDirectory?: string;
  /** Folders (one or more) holding the emulator's save data on the device. */
  saves?: string[];
  saveFolder?: string;
  env?: Record<string, string>;
}

/** Parsed `emu.yaml`. */
export interface EmuConfig {
  name?: string;
  description?: string;
  version?: string;
  /** Launchable extensions in priority order. */
  extensions?: string[];
  systems: Partial<Record<'windows' | 'linux', EmulatorSystemConfig>>;
}

export interface EmulatorSummary {
  id: number;
  kind: 'emulator';
  name: string;
  platform: string;
  platformName: string;
  folder: string;
  sizeBytes: number;
  fileCount: number;
  config: EmuConfig | null;
  configError?: string;
  /** OSes this emulator has a configuration for. */
  supports: ('windows' | 'linux')[];
  addedAt: string;
}

export interface RemoteFile {
  path: string; // relative, forward slashes
  size: number;
  mtime: number;
}

export interface FileListing {
  id: number;
  folder: string;
  files: RemoteFile[];
  totalBytes: number;
}

export interface LibraryState {
  games: number[];
  emulators: number[];
}

export interface SaveVersion {
  id: number;
  emulatorId: number;
  version: number;
  size: number;
  sha256: string;
  device?: string;
  note?: string;
  createdAt: string;
}

export interface PlaytimeEntry {
  gameId: number;
  seconds: number;
  sessions: number;
  lastPlayed?: string;
}

export interface PlaySession {
  sessionId: string;
  gameId: number;
  startedAt: string;
  seconds: number;
}

export type ServerEvent =
  | { type: 'hello'; serverTime: string }
  | { type: 'save'; emulatorId: number; version: SaveVersion; fromDevice?: string }
  | { type: 'library'; library: LibraryState }
  | { type: 'catalog' };

export interface ApiError {
  error: string;
}
