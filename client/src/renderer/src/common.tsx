import { useState } from 'react';
import type { EmulatorSummary, GameSummary } from '@nostalgia/shared';
import { act, call, media } from './bridge';
import { Confirm, Icon, Img, Modal, fmtBytes } from './ui';
import type { ClientState, DownloadJob, ItemKind } from '../../main/types';

export type AnyItem = GameSummary | EmulatorSummary;

export function itemStatus(state: ClientState, jobs: DownloadJob[], kind: ItemKind, id: number) {
  const installed = kind === 'game' ? state.installed.games[id] : state.installed.emulators[id];
  const job = jobs.find((j) => j.kind === kind && j.id === id && j.status !== 'done');
  const inLibrary = kind === 'game' ? state.library.games.includes(id) : state.library.emulators.includes(id);
  return { installed, job, inLibrary };
}

export function Cover({ item, className }: { item: AnyItem; className?: string }) {
  if (item.kind === 'emulator') {
    return (
      <div className={`emu-cover ${className ?? ''}`}>
        <Icon name="chip" size={42} />
        <strong>{item.name}</strong>
        <small>{item.platformName}</small>
      </div>
    );
  }
  return (
    <Img
      src={item.hasCover ? media(item.id, 'cover.jpg') : undefined}
      alt={item.name}
      className={className}
      fallback={
        <div className={`cover-fallback ${className ?? ''}`}>
          <strong>{item.name}</strong>
          <small>{item.platformName}</small>
        </div>
      }
    />
  );
}

export function GameCard({
  item,
  state,
  jobs,
  onOpen,
}: {
  item: AnyItem;
  state: ClientState;
  jobs: DownloadJob[];
  onOpen: () => void;
}) {
  const { installed, job, inLibrary } = itemStatus(state, jobs, item.kind, item.id);
  const disabled = !state.online && !installed;
  return (
    <button className={`card ${disabled ? 'unavailable' : ''}`} onClick={onOpen} title={disabled ? 'Not available offline' : item.name}>
      <div className="card-cover">
        <Cover item={item} />
        <div className="card-badges">
          {installed && <span className="badge ok" title="Installed"><Icon name="check" size={12} /></span>}
          {job && <span className="badge info">{job.status === 'paused' ? 'Paused' : `${Math.round((job.receivedBytes / Math.max(1, job.totalBytes)) * 100)}%`}</span>}
          {inLibrary && !installed && <span className="badge lib" title="In your library">In library</span>}
        </div>
        {job && (
          <div className="card-progress">
            <div style={{ width: `${(job.receivedBytes / Math.max(1, job.totalBytes)) * 100}%` }} />
          </div>
        )}
      </div>
      <div className="card-title">{item.name}</div>
      <div className="card-sub">
        <span className="plat">{item.platform.toUpperCase()}</span>
        {item.kind === 'game' && item.year ? <span>{item.year}</span> : null}
        {item.kind === 'emulator' && <span>Emulator</span>}
      </div>
    </button>
  );
}

/** Install / Play / progress button + library toggle used on the store page and in the library. */
export function ActionButtons({
  state,
  jobs,
  kind,
  id,
  size,
  showLibrary = true,
  onOpenLibrary,
}: {
  state: ClientState;
  jobs: DownloadJob[];
  kind: ItemKind;
  id: number;
  size: number;
  showLibrary?: boolean;
  onOpenLibrary?: () => void;
}) {
  const { installed, job, inLibrary } = itemStatus(state, jobs, kind, id);
  const [pickLocation, setPickLocation] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const running = state.running?.gameId === id && kind === 'game';

  const install = (location?: string) => act(() => call('install', kind, id, location));
  const startInstall = () => {
    if (state.settings.locations.length > 1) setPickLocation(true);
    else void install();
  };

  let main;
  if (running) {
    main = (
      <button className="btn primary big playing" onClick={() => call('stopGame')}>
        <span className="pulse" /> Playing – Quit
      </button>
    );
  } else if (installed && kind === 'game') {
    main = (
      <button className="btn play big" data-autofocus onClick={() => act(() => call('play', id))} disabled={!!state.running}>
        <Icon name="play" /> Play
      </button>
    );
  } else if (installed) {
    main = (
      <button className="btn big" disabled>
        <Icon name="check" /> Installed
      </button>
    );
  } else if (job) {
    const pct = Math.round((job.receivedBytes / Math.max(1, job.totalBytes)) * 100);
    main =
      job.status === 'paused' || job.status === 'error' ? (
        <button className="btn primary big" data-autofocus onClick={() => act(() => call('resumeDownload', job.key))}>
          <Icon name="download" /> Resume ({pct}%)
        </button>
      ) : (
        <button className="btn big downloading" data-autofocus onClick={() => act(() => call('pauseDownload', job.key))}>
          <span className="btn-progress" style={{ width: `${pct}%` }} />
          <Icon name="pause" /> {job.status === 'queued' ? 'Queued' : `Downloading ${pct}%`}
        </button>
      );
  } else {
    main = (
      <button className="btn primary big" data-autofocus onClick={startInstall} disabled={!state.online}>
        <Icon name="download" /> Install <small>{fmtBytes(size)}</small>
      </button>
    );
  }

  return (
    <div className="actions">
      {main}
      {showLibrary &&
        (inLibrary ? (
          <>
            {onOpenLibrary && (
              <button className="btn big" onClick={onOpenLibrary}>
                Open in library
              </button>
            )}
            <button className="btn big" onClick={() => setConfirmRemove(true)} disabled={!state.online}>
              <Icon name="check" /> In library
            </button>
          </>
        ) : (
          <button className="btn big" onClick={() => act(() => call(kind === 'game' ? 'addGame' : 'addEmulator', id), 'Added to your library')} disabled={!state.online}>
            <Icon name="plus" /> Add to library
          </button>
        ))}
      {pickLocation && (
        <Modal title="Install to…" onClose={() => setPickLocation(false)}>
          <div className="option-list">
            {state.settings.locations.map((l) => (
              <button
                key={l}
                className="option"
                data-autofocus={l === (state.settings.defaultLocation ?? state.settings.locations[0]) ? true : undefined}
                onClick={() => {
                  setPickLocation(false);
                  void install(l);
                }}
              >
                <Icon name="folder" /> <span>{l}</span>
              </button>
            ))}
          </div>
        </Modal>
      )}
      {confirmRemove && (
        <Confirm
          title="Remove from library?"
          message={installed ? 'The game stays installed on this device. Cloud saves are kept on the server.' : 'You can add it again at any time from the vault.'}
          confirm="Remove"
          danger
          onConfirm={() => act(() => call(kind === 'game' ? 'removeGame' : 'removeEmulator', id))}
          onClose={() => setConfirmRemove(false)}
        />
      )}
    </div>
  );
}

export function OfflineBanner() {
  return (
    <div className="offline-banner">
      <Icon name="wifiOff" />
      <div>
        <strong>You are offline.</strong> Only installed games are available. Play time and cloud saves are uploaded automatically when the server is
        reachable again.
      </div>
      <button className="btn small" onClick={() => act(() => call('refresh'))}>
        <Icon name="refresh" size={14} /> Retry
      </button>
    </div>
  );
}
