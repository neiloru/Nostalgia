import { act, call } from '../bridge';
import { Cover, OfflineBanner } from '../common';
import { Icon, Progress, fmtBytes, fmtDate } from '../ui';
import type { ClientState, DownloadJob } from '../../../main/types';

const eta = (j: DownloadJob) => {
  if (!j.speed) return '';
  const s = (j.totalBytes - j.receivedBytes) / j.speed;
  if (s > 3600) return `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} min left`;
  if (s > 60) return `${Math.round(s / 60)} min left`;
  return `${Math.round(s)} s left`;
};

const STATUS: Record<DownloadJob['status'], string> = {
  queued: 'Queued',
  downloading: 'Downloading',
  paused: 'Paused',
  error: 'Failed',
  done: 'Installed',
};

export function Downloads({ state, jobs }: { state: ClientState; jobs: DownloadJob[] }) {
  const active = jobs.filter((j) => j.status !== 'done');
  const done = jobs.filter((j) => j.status === 'done').sort((a, b) => (b.finishedAt ?? '').localeCompare(a.finishedAt ?? ''));
  const running = active.some((j) => j.status === 'downloading' || j.status === 'queued');
  const paused = active.some((j) => j.status === 'paused');

  const item = (j: DownloadJob) =>
    j.kind === 'game' ? state.catalog.games.find((g) => g.id === j.id) : state.catalog.emulators.find((e) => e.id === j.id);

  return (
    <div className="downloads page">
      {!state.online && <OfflineBanner />}
      <div className="section-head">
        <h1>Downloads</h1>
        <span className="spacer" />
        {running && (
          <button className="btn" onClick={() => call('pauseAll')}>
            <Icon name="pause" size={16} /> Pause all
          </button>
        )}
        {paused && (
          <button className="btn" onClick={() => call('resumeAll')} disabled={!state.online}>
            <Icon name="download" size={16} /> Resume all
          </button>
        )}
        {done.length > 0 && (
          <button className="btn ghost" onClick={() => call('clearFinished')}>
            Clear finished
          </button>
        )}
      </div>

      {!active.length && !done.length && <div className="empty">No downloads. Install games from the vault or your library.</div>}

      <div className="dl-list">
        {active.map((j) => {
          const it = item(j);
          return (
            <div key={j.key} className={`dl-item ${j.status}`}>
              <div className="dl-thumb">{it ? <Cover item={it} /> : null}</div>
              <div className="dl-info">
                <div className="dl-title">
                  <strong>{j.name}</strong>
                  <span className="muted">
                    {j.kind === 'emulator' ? 'Emulator · ' : ''}
                    {j.platform.toUpperCase()}
                  </span>
                  <span className={`status ${j.status}`}>{STATUS[j.status]}</span>
                </div>
                <Progress value={j.receivedBytes} total={j.totalBytes} paused={j.status !== 'downloading'} />
                <div className="dl-meta muted">
                  <span>
                    {fmtBytes(j.receivedBytes)} / {fmtBytes(j.totalBytes)}
                  </span>
                  {j.status === 'downloading' && (
                    <>
                      <span>{fmtBytes(j.speed)}/s</span>
                      <span>{eta(j)}</span>
                    </>
                  )}
                  {j.currentFile && j.status === 'downloading' && <span className="ellipsis">{j.currentFile}</span>}
                  {j.error && <span className="form-error">{j.error}</span>}
                </div>
                <div className="muted small ellipsis">{j.targetDir}</div>
              </div>
              <div className="dl-actions">
                {j.status === 'downloading' || j.status === 'queued' ? (
                  <button className="btn" onClick={() => call('pauseDownload', j.key)}>
                    <Icon name="pause" size={16} /> Pause
                  </button>
                ) : (
                  <button className="btn primary" onClick={() => act(() => call('resumeDownload', j.key))} disabled={!state.online}>
                    <Icon name="download" size={16} /> Resume
                  </button>
                )}
                <button className="btn ghost" onClick={() => call('cancelDownload', j.key)} title="Cancel and remove partial files">
                  <Icon name="x" size={16} /> Cancel
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {done.length > 0 && (
        <>
          <h2 className="sub">Completed</h2>
          <div className="dl-list done">
            {done.map((j) => (
              <div key={j.key + j.finishedAt} className="dl-item done">
                <div className="dl-info">
                  <div className="dl-title">
                    <strong>{j.name}</strong>
                    <span className="muted">{fmtBytes(j.totalBytes)}</span>
                    <span className="muted">{fmtDate(j.finishedAt)}</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
