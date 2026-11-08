import { Fragment, useEffect, useMemo, useState } from 'react';
import { platformInfo, type SaveVersion } from '@nostalgia/shared';
import { act, call } from '../bridge';
import { ActionButtons, Cover, OfflineBanner, itemStatus, type AnyItem } from '../common';
import { Confirm, Icon, Modal, Select, Toggle, fmtBytes, fmtDate, fmtDuration, fmtRelative } from '../ui';
import { heroImage, useGameDetail } from './StorePage';
import type { Navigate } from '../App';
import type { ClientState, DownloadJob, ItemKind, LaunchOptions } from '../../../main/types';

export function Library({
  state,
  jobs,
  nav,
  selected,
  setSelected,
}: {
  state: ClientState;
  jobs: DownloadJob[];
  nav: Navigate;
  selected: { kind: ItemKind; id: number } | null;
  setSelected: (s: { kind: ItemKind; id: number } | null) => void;
}) {
  const [q, setQ] = useState('');
  const [showEmulators, setShowEmulators] = useState(() => localStorage.getItem('libEmus') === '1');
  const [installedOnly, setInstalledOnly] = useState(() => localStorage.getItem('libInstalled') === '1');
  useEffect(() => {
    try {
      localStorage.setItem('libEmus', showEmulators ? '1' : '0');
      localStorage.setItem('libInstalled', installedOnly ? '1' : '0');
    } catch {
      /* ignore */
    }
  }, [showEmulators, installedOnly]);

  const items = useMemo(() => {
    const games: AnyItem[] = state.library.games
      .map((id) => state.catalog.games.find((g) => g.id === id))
      .filter((g): g is NonNullable<typeof g> => !!g);
    const emus: AnyItem[] = showEmulators
      ? state.library.emulators.map((id) => state.catalog.emulators.find((e) => e.id === id)).filter((e): e is NonNullable<typeof e> => !!e)
      : [];
    const ql = q.trim().toLowerCase();
    return [...games, ...emus]
      .filter((it) => !ql || it.name.toLowerCase().includes(ql))
      .filter((it) => !installedOnly || itemStatus(state, jobs, it.kind, it.id).installed)
      .sort((a, b) => {
        if (a.kind !== b.kind) return a.kind === 'game' ? -1 : 1;
        // Offline: installed first.
        if (!state.online) {
          const ia = Number(!!itemStatus(state, jobs, a.kind, a.id).installed);
          const ib = Number(!!itemStatus(state, jobs, b.kind, b.id).installed);
          if (ia !== ib) return ib - ia;
        }
        return a.name.localeCompare(b.name);
      });
  }, [state, jobs, q, showEmulators, installedOnly]);

  // Names that appear on more than one platform get the short platform name appended.
  const duplicates = useMemo(() => {
    const platforms = new Map<string, Set<string>>();
    const add = (kind: ItemKind, name: string, platform: string) => {
      const key = `${kind}:${name.toLowerCase()}`;
      platforms.set(key, (platforms.get(key) ?? new Set()).add(platform));
    };
    for (const id of state.library.games) {
      const g = state.catalog.games.find((x) => x.id === id);
      if (g) add('game', g.name, g.platform);
    }
    for (const id of state.library.emulators) {
      const e = state.catalog.emulators.find((x) => x.id === id);
      if (e) add('emulator', e.name, e.platform);
    }
    return new Set([...platforms].filter(([, p]) => p.size > 1).map(([k]) => k));
  }, [state.library, state.catalog]);

  const current = selected && items.find((i) => i.kind === selected.kind && i.id === selected.id) ? selected : items[0] ? { kind: items[0].kind, id: items[0].id } : null;

  return (
    <div className="library">
      <aside className="lib-list">
        <div className="search">
          <Icon name="search" size={16} />
          <input type="search" placeholder="Search library…" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <div className="lib-toggles">
          <Toggle label="Show emulators" checked={showEmulators} onChange={setShowEmulators} />
          <Toggle label="Installed only" checked={installedOnly} onChange={setInstalledOnly} />
        </div>
        <div className="lib-items">
          {items.map((it, i) => {
            const st = itemStatus(state, jobs, it.kind, it.id);
            const active = current?.kind === it.kind && current.id === it.id;
            const unavailable = !state.online && !st.installed;
            const label = duplicates.has(`${it.kind}:${it.name.toLowerCase()}`) ? `${it.name} (${platformInfo(it.platform).short})` : it.name;
            return (
              <Fragment key={`${it.kind}:${it.id}`}>
                {it.kind === 'emulator' && items[i - 1]?.kind !== 'emulator' && <div className="lib-section">Emulators</div>}
                <button
                  className={`lib-item ${active ? 'active' : ''} ${unavailable ? 'unavailable' : ''} ${st.installed ? 'installed' : ''}`}
                  data-autofocus={active ? true : undefined}
                  title={label}
                  onClick={() => setSelected({ kind: it.kind, id: it.id })}
                  onFocus={() => setSelected({ kind: it.kind, id: it.id })}
                >
                  <span className="lib-name">{label}</span>
                  {st.job && <span className="lib-progress">{Math.round((st.job.receivedBytes / Math.max(1, st.job.totalBytes)) * 100)}%</span>}
                  {state.running?.gameId === it.id && it.kind === 'game' && <span className="pulse" />}
                </button>
              </Fragment>
            );
          })}
          {!items.length && (
            <div className="empty small">
              {state.library.games.length ? 'Nothing matches.' : 'Your library is empty. Add games from the vault.'}
              {!state.library.games.length && (
                <button className="btn primary" onClick={() => nav.toTab('vault')}>
                  Open the vault
                </button>
              )}
            </div>
          )}
        </div>
      </aside>
      <section className="lib-detail">
        {!state.online && <OfflineBanner />}
        {current?.kind === 'game' && <GameDetailPanel key={`g${current.id}`} state={state} jobs={jobs} id={current.id} nav={nav} />}
        {current?.kind === 'emulator' && <EmulatorPanel key={`e${current.id}`} state={state} jobs={jobs} id={current.id} nav={nav} />}
      </section>
    </div>
  );
}

function GameDetailPanel({ state, jobs, id, nav }: { state: ClientState; jobs: DownloadJob[]; id: number; nav: Navigate }) {
  const g = state.catalog.games.find((x) => x.id === id)!;
  const detail = useGameDetail(id, state.online);
  const { installed } = itemStatus(state, jobs, 'game', id);
  const [opts, setOpts] = useState<LaunchOptions | null>(null);
  const [confirmUninstall, setConfirmUninstall] = useState(false);
  const [args, setArgs] = useState('');
  const pt = state.playtime[id];

  useEffect(() => {
    void call<LaunchOptions>('launchOptions', id).then((o) => {
      setOpts(o);
      setArgs(o.extraArgs ?? '');
    });
  }, [id, installed, state.installed.emulators, state.catalog.emulators]);

  const emuId = opts?.emulatorId;
  const emu = state.catalog.emulators.find((e) => e.id === emuId);
  const hero = heroImage(detail);
  const unavailable = !state.online && !installed;

  const update = async (s: { emulatorId?: number; disc?: string; extraArgs?: string }) => {
    const o = await act(() => call<LaunchOptions>('setGameSettings', id, s));
    if (o) setOpts(o);
  };

  return (
    <div className="detail">
      <div className="detail-hero" style={hero ? { backgroundImage: `url("${hero}")` } : undefined}>
        <div className="hero-shade" />
        <div className="detail-hero-content">
          <div className="detail-cover">
            <Cover item={g} />
          </div>
          <div>
            <h1>{g.name}</h1>
            <div className="meta-row">
              <span className="plat">{g.platformName}</span>
              {g.year && <span>{g.year}</span>}
              {g.developers[0] && <span>{g.developers[0]}</span>}
            </div>
          </div>
        </div>
      </div>

      <div className="detail-bar">
        {unavailable ? (
          <div className="muted">This game is not installed and you are offline.</div>
        ) : (
          <ActionButtons state={state} jobs={jobs} kind="game" id={id} size={g.sizeBytes} showLibrary={false} />
        )}
        <div className="stats">
          <Stat label="Play time" value={fmtDuration(pt?.seconds ?? 0)} />
          <Stat label="Last played" value={fmtRelative(pt?.lastPlayed)} />
          <Stat label="Size" value={fmtBytes(g.sizeBytes)} />
        </div>
      </div>

      <div className="detail-grid">
        <section className="panel">
          <h2>
            <Icon name="cloud" /> Cloud saves
          </h2>
          {emu ? <SaveBox state={state} emulatorId={emu.id} /> : <p className="muted">No emulator available for this platform.</p>}
        </section>

        <section className="panel">
          <h2>
            <Icon name="gear" /> Game settings <small className="muted">(this device only)</small>
          </h2>
          {opts && (
            <div className="form">
              <Select
                label="Emulator"
                value={opts.emulatorId ?? 0}
                options={opts.emulators.map((e) => ({
                  value: e.id,
                  label: e.name,
                  hint: !e.supported ? `not available for ${state.os}` : e.installed ? 'installed' : 'not installed',
                }))}
                onChange={(v) => update({ emulatorId: v })}
              />
              {opts.discs.length > 1 && (
                <Select label="Disc" value={opts.disc ?? ''} options={opts.discs.map((d) => ({ value: d.file, label: d.label, hint: d.file }))} onChange={(v) => update({ disc: v })} />
              )}
              {opts.discs.length === 1 && <div className="muted small">Launch file: {opts.discs[0].file}</div>}
              {!installed && <div className="muted small">Install the game to choose discs.</div>}
              <label className="field">
                Extra emulator arguments
                <input value={args} placeholder="optional" onChange={(e) => setArgs(e.target.value)} onBlur={() => args !== (opts.extraArgs ?? '') && update({ extraArgs: args })} />
              </label>
              {emu && !state.installed.emulators[emu.id] && (
                <button className="btn" onClick={() => act(() => call('install', 'emulator', emu.id))} disabled={!state.online}>
                  <Icon name="download" /> Install {emu.name}
                </button>
              )}
            </div>
          )}
        </section>

        <section className="panel">
          <h2>Manage</h2>
          <div className="button-col">
            <button className="btn" onClick={() => nav.toStore('game', id)}>
              Store page
            </button>
            {installed && (
              <>
                <button className="btn" onClick={() => call('openFolder', 'game', id)}>
                  <Icon name="folder" /> Open folder
                </button>
                <button className="btn danger" onClick={() => setConfirmUninstall(true)} disabled={state.running?.gameId === id}>
                  <Icon name="trash" /> Uninstall
                </button>
              </>
            )}
            <RemoveFromLibrary state={state} kind="game" id={id} />
          </div>
          {installed && <div className="muted small path">{installed.path}</div>}
        </section>
      </div>

      {confirmUninstall && (
        <Confirm
          title={`Uninstall ${g.name}?`}
          message="The game files are removed from this device. Your cloud saves and play time stay on the server."
          confirm="Uninstall"
          danger
          onConfirm={() => act(() => call('uninstall', 'game', id))}
          onClose={() => setConfirmUninstall(false)}
        />
      )}
    </div>
  );
}

function RemoveFromLibrary({ state, kind, id }: { state: ClientState; kind: ItemKind; id: number }) {
  const [confirm, setConfirm] = useState(false);
  return (
    <>
      <button className="btn ghost" onClick={() => setConfirm(true)} disabled={!state.online}>
        Remove from library
      </button>
      {confirm && (
        <Confirm
          title="Remove from library?"
          message="Installed files stay on this device. Cloud saves are kept on the server."
          confirm="Remove"
          danger
          onConfirm={() => act(() => call(kind === 'game' ? 'removeGame' : 'removeEmulator', id))}
          onClose={() => setConfirm(false)}
        />
      )}
    </>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <small>{label}</small>
      <strong>{value}</strong>
    </div>
  );
}

export function SaveBox({ state, emulatorId }: { state: ClientState; emulatorId: number }) {
  const emu = state.catalog.emulators.find((e) => e.id === emulatorId);
  const s = state.saveSync[emulatorId];
  const installed = state.installed.emulators[emulatorId];
  const [versions, setVersions] = useState<SaveVersion[] | null>(null);
  const [restore, setRestore] = useState<SaveVersion | null>(null);
  const [paths, setPaths] = useState<string[]>([]);

  useEffect(() => {
    void call<string[]>('savePaths', emulatorId).then(setPaths);
  }, [emulatorId, installed]);

  const openVersions = async () => {
    const v = await act(() => call<SaveVersion[]>('listSaves', emulatorId));
    if (v) setVersions(v);
  };

  return (
    <div className="savebox">
      <div className="kv">
        <span>Emulator</span>
        <span>{emu?.name}</span>
        <span>Newest on server</span>
        <span>{s?.latest ? `v${s.latest.version} · ${fmtRelative(s.latest.createdAt)}${s.latest.device ? ` · ${s.latest.device}` : ''}` : 'none yet'}</span>
        <span>On this device</span>
        <span>
          {!installed ? 'emulator not installed' : s?.version ? `v${s.version}${s.pendingUpload ? ' + local changes (upload pending)' : ''}` : s?.pendingUpload ? 'upload pending' : 'not synced yet'}
        </span>
        {s?.syncedAt && (
          <>
            <span>Last sync</span>
            <span>
              {fmtRelative(s.syncedAt)}
              {s.lastResult ? ` (${s.lastResult})` : ''}
            </span>
          </>
        )}
      </div>
      {s?.lastError && <div className="form-error small">{s.lastError}</div>}
      {paths.length > 0 && (
        <details className="small muted">
          <summary>Save folders</summary>
          {paths.map((p) => (
            <div key={p} className="mono path">
              {p}
            </div>
          ))}
        </details>
      )}
      <div className="row">
        <button className="btn" disabled={!state.online || !installed || s?.busy} onClick={() => act(() => call('syncSaves', emulatorId))}>
          <Icon name="refresh" size={16} /> {s?.busy ? 'Syncing…' : 'Sync now'}
        </button>
        <button className="btn" disabled={!state.online} onClick={openVersions}>
          <Icon name="history" size={16} /> Older versions
        </button>
      </div>
      {versions && (
        <Modal title={`Cloud saves – ${emu?.name}`} onClose={() => setVersions(null)} wide>
          {!versions.length && <p className="muted">No cloud saves yet.</p>}
          <div className="option-list">
            {versions.map((v, i) => (
              <button key={v.id} className="option version" onClick={() => setRestore(v)} disabled={!installed || state.running?.emulatorId === emulatorId}>
                <strong>v{v.version}</strong>
                <span>{fmtDate(v.createdAt)}</span>
                <span className="muted">{v.device ?? ''}</span>
                <span className="muted">{v.note ?? ''}</span>
                <span className="muted">{fmtBytes(v.size)}</span>
                {i === 0 && <span className="badge ok small">newest</span>}
                {v.id === s?.versionId && <span className="badge info small">on device</span>}
              </button>
            ))}
          </div>
          <p className="muted small">
            Restoring copies the version to this device (your current local saves are backed up first) and uploads it as the newest version. No cloud save is
            ever deleted.
          </p>
          <button className="btn ghost" onClick={() => call('openSaveBackups')}>
            <Icon name="folder" size={16} /> Open local backups
          </button>
        </Modal>
      )}
      {restore && (
        <Confirm
          title={`Restore v${restore.version}?`}
          message={`Your current local ${emu?.name} saves are backed up and replaced by version ${restore.version} from ${fmtDate(restore.createdAt)}.`}
          confirm="Restore"
          onConfirm={() => {
            setVersions(null);
            void act(() => call('restoreSave', emulatorId, restore.id));
          }}
          onClose={() => setRestore(null)}
        />
      )}
    </div>
  );
}

function EmulatorPanel({ state, jobs, id, nav }: { state: ClientState; jobs: DownloadJob[]; id: number; nav: Navigate }) {
  const e = state.catalog.emulators.find((x) => x.id === id)!;
  const { installed } = itemStatus(state, jobs, 'emulator', id);
  const [confirmUninstall, setConfirmUninstall] = useState(false);
  const games = state.catalog.games.filter((g) => g.platform === e.platform && state.library.games.includes(g.id));
  return (
    <div className="detail">
      <div className="detail-hero emu-hero">
        <div className="hero-shade" />
        <div className="detail-hero-content">
          <div className="detail-cover">
            <Cover item={e} />
          </div>
          <div>
            <h1>{e.name}</h1>
            <div className="meta-row">
              <span className="plat">{e.platformName}</span>
              <span>Emulator</span>
            </div>
          </div>
        </div>
      </div>
      <div className="detail-bar">
        {!state.online && !installed ? (
          <div className="muted">Not installed and you are offline.</div>
        ) : (
          <ActionButtons state={state} jobs={jobs} kind="emulator" id={id} size={e.sizeBytes} showLibrary={false} />
        )}
        <div className="stats">
          <Stat label="Size" value={fmtBytes(e.sizeBytes)} />
          <Stat label="Games in library" value={String(games.length)} />
        </div>
      </div>
      <div className="detail-grid">
        <section className="panel">
          <h2>
            <Icon name="cloud" /> Cloud saves
          </h2>
          <SaveBox state={state} emulatorId={id} />
        </section>
        <section className="panel">
          <h2>Your games</h2>
          <div className="link-list">
            {games.map((g) => (
              <button key={g.id} className="link-row" onClick={() => nav.toLibrary('game', g.id)}>
                {g.name}
              </button>
            ))}
            {!games.length && <span className="muted">No games for this emulator in your library.</span>}
          </div>
        </section>
        <section className="panel">
          <h2>Manage</h2>
          <div className="button-col">
            <button className="btn" onClick={() => nav.toStore('emulator', id)}>
              Store page
            </button>
            {installed && (
              <>
                <button className="btn" onClick={() => call('openFolder', 'emulator', id)}>
                  <Icon name="folder" /> Open folder
                </button>
                <button className="btn danger" onClick={() => setConfirmUninstall(true)} disabled={state.running?.emulatorId === id}>
                  <Icon name="trash" /> Uninstall
                </button>
              </>
            )}
            <RemoveFromLibrary state={state} kind="emulator" id={id} />
          </div>
          {installed && <div className="muted small path">{installed.path}</div>}
        </section>
      </div>
      {confirmUninstall && (
        <Confirm
          title={`Uninstall ${e.name}?`}
          message="Local saves are uploaded first. Save folders outside of the emulator folder are not touched."
          confirm="Uninstall"
          danger
          onConfirm={() => act(() => call('uninstall', 'emulator', id))}
          onClose={() => setConfirmUninstall(false)}
        />
      )}
    </div>
  );
}
