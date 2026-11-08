import { useEffect, useState } from 'react';
import { focusFirst } from '../nav';
import type { GameDetail } from '@nostalgia/shared';
import { call, media } from '../bridge';
import { ActionButtons, Cover, itemStatus } from '../common';
import { Icon, Img, Modal, fmtBytes, fmtDuration } from '../ui';
import type { ClientState, DownloadJob, ItemKind } from '../../../main/types';

export function useGameDetail(id: number | null, online: boolean) {
  const [detail, setDetail] = useState<GameDetail | null>(null);
  useEffect(() => {
    let alive = true;
    setDetail(null);
    if (id == null) return;
    void call<GameDetail | null>('getGameDetail', id).then((d) => alive && setDetail(d));
    return () => {
      alive = false;
    };
  }, [id, online]);
  return detail;
}

export function heroImage(d: GameDetail | null) {
  const m = d?.metadata;
  const name = m?.artworks?.[0] ?? m?.screenshots?.[0];
  return d && name ? media(d.id, name) : undefined;
}

export function StorePage({
  state,
  jobs,
  kind,
  id,
  onBack,
  onOpenLibrary,
  onOpen,
}: {
  state: ClientState;
  jobs: DownloadJob[];
  kind: ItemKind;
  id: number;
  onBack: () => void;
  onOpenLibrary: () => void;
  onOpen: (kind: ItemKind, id: number) => void;
}) {
  useEffect(() => {
    const t = setTimeout(() => focusFirst(document.querySelector<HTMLElement>('.store')), 60);
    document.querySelector('.store')?.scrollTo({ top: 0 });
    return () => clearTimeout(t);
  }, [kind, id]);
  if (kind === 'emulator') return <EmulatorPage state={state} jobs={jobs} id={id} onBack={onBack} onOpenLibrary={onOpenLibrary} onOpen={onOpen} />;
  return <GamePage state={state} jobs={jobs} id={id} onBack={onBack} onOpenLibrary={onOpenLibrary} onOpen={onOpen} />;
}

function GamePage({
  state,
  jobs,
  id,
  onBack,
  onOpenLibrary,
  onOpen,
}: {
  state: ClientState;
  jobs: DownloadJob[];
  id: number;
  onBack: () => void;
  onOpenLibrary: () => void;
  onOpen: (kind: ItemKind, id: number) => void;
}) {
  const summary = state.catalog.games.find((g) => g.id === id);
  const detail = useGameDetail(id, state.online);
  const [shot, setShot] = useState<number | null>(null);
  const g = detail ?? (summary ? { ...summary, metadata: null } : null);
  if (!g) {
    return (
      <div className="store">
        <button className="btn" onClick={onBack} data-autofocus>
          <Icon name="back" /> Back
        </button>
        <div className="empty">This game is not available.</div>
      </div>
    );
  }
  const m = g.metadata;
  const { installed, inLibrary } = itemStatus(state, jobs, 'game', id);
  const pt = state.playtime[id];
  const emus = state.catalog.emulators.filter((e) => e.platform === g.platform);
  const shots = [...(m?.screenshots ?? []), ...(m?.artworks ?? [])];
  const hero = heroImage(detail);
  const unavailable = !state.online && !installed;

  return (
    <div className="store">
      <div className="hero" style={hero ? { backgroundImage: `url("${hero}")` } : undefined}>
        <div className="hero-shade" />
        <button className="btn back-btn" onClick={onBack}>
          <Icon name="back" /> Vault
        </button>
        <div className="hero-content">
          <div className="hero-cover">
            <Cover item={g} />
          </div>
          <div className="hero-info">
            <h1>{g.name}</h1>
            <div className="meta-row">
              <span className="plat">{g.platformName}</span>
              {m?.releaseDate && <span>{new Date(m.releaseDate).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' })}</span>}
              {m?.developers?.[0] && <span>{m.developers[0]}</span>}
              {g.rating != null && (
                <span className="rating">
                  <Icon name="star" size={14} /> {g.rating}
                </span>
              )}
            </div>
            <div className="chips">
              {(m?.genres ?? g.genres).map((x) => (
                <span key={x} className="chip">
                  {x}
                </span>
              ))}
            </div>
            {unavailable ? (
              <div className="muted">Not available while offline.</div>
            ) : (
              <ActionButtons state={state} jobs={jobs} kind="game" id={id} size={g.sizeBytes} onOpenLibrary={inLibrary ? onOpenLibrary : undefined} />
            )}
            {pt && <div className="muted small">You played {fmtDuration(pt.seconds)}</div>}
          </div>
        </div>
      </div>

      <div className="store-body">
        <div className="store-main" data-nav-group>
          {m?.summary && (
            <section>
              <h2>About</h2>
              <p className="prose">{m.summary}</p>
            </section>
          )}
          {m?.storyline && (
            <section>
              <h2>Story</h2>
              <p className="prose">{m.storyline}</p>
            </section>
          )}
          {shots.length > 0 && (
            <section>
              <h2>Media</h2>
              <div className="shots">
                {shots.map((s, i) => (
                  <button key={s} className="shot" onClick={() => setShot(i)}>
                    <Img src={media(id, s)} alt={`Screenshot ${i + 1}`} />
                  </button>
                ))}
              </div>
            </section>
          )}
          {!m && <p className="muted">{state.online ? 'No details from IGDB yet.' : 'Details are not cached on this device.'}</p>}
        </div>
        <aside className="store-side" data-nav-group>
          <dl className="facts">
            <Fact label="Platform" value={g.platformName} />
            <Fact label="Developer" value={m?.developers?.join(', ')} />
            <Fact label="Publisher" value={m?.publishers?.join(', ')} />
            <Fact label="Release" value={m?.releaseDate} />
            <Fact label="Genres" value={m?.genres?.join(', ')} />
            <Fact label="Themes" value={m?.themes?.join(', ')} />
            <Fact label="Modes" value={m?.gameModes?.join(', ')} />
            <Fact label="Perspective" value={m?.perspectives?.join(', ')} />
            <Fact label="Series" value={m?.franchises?.join(', ')} />
            <Fact label="Size" value={`${fmtBytes(g.sizeBytes)} · ${g.fileCount} files`} />
            {installed && <Fact label="Installed in" value={installed.path} />}
          </dl>
          {emus.length > 0 && (
            <div className="side-box">
              <h3>Runs with</h3>
              {emus.map((e) => (
                <button key={e.id} className="link-row" onClick={() => onOpen('emulator', e.id)}>
                  <Icon name="chip" size={16} /> {e.name}
                  {state.installed.emulators[e.id] && <span className="badge ok small">installed</span>}
                </button>
              ))}
            </div>
          )}
          {m?.url && (
            <button className="btn ghost" onClick={() => call('openExternal', m.url)}>
              View on IGDB
            </button>
          )}
        </aside>
      </div>

      {shot != null && (
        <Modal onClose={() => setShot(null)} wide>
          <div className="lightbox">
            <button className="icon-btn" onClick={() => setShot((shot + shots.length - 1) % shots.length)} aria-label="Previous">
              <Icon name="back" />
            </button>
            <Img src={media(id, shots[shot])} alt="Screenshot" />
            <button className="icon-btn flip" onClick={() => setShot((shot + 1) % shots.length)} aria-label="Next" data-autofocus>
              <Icon name="back" />
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

function Fact({ label, value }: { label: string; value?: string | null }) {
  if (!value) return null;
  return (
    <>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </>
  );
}

function EmulatorPage({
  state,
  jobs,
  id,
  onBack,
  onOpenLibrary,
  onOpen,
}: {
  state: ClientState;
  jobs: DownloadJob[];
  id: number;
  onBack: () => void;
  onOpenLibrary: () => void;
  onOpen: (kind: ItemKind, id: number) => void;
}) {
  const e = state.catalog.emulators.find((x) => x.id === id);
  if (!e) {
    return (
      <div className="store">
        <button className="btn" onClick={onBack} data-autofocus>
          <Icon name="back" /> Back
        </button>
        <div className="empty">This emulator is not available.</div>
      </div>
    );
  }
  const { installed, inLibrary } = itemStatus(state, jobs, 'emulator', id);
  const games = state.catalog.games.filter((g) => g.platform === e.platform);
  const sys = e.config?.systems[state.os];
  const latest = state.saveSync[id]?.latest;
  return (
    <div className="store">
      <div className="hero emu-hero">
        <div className="hero-shade" />
        <button className="btn back-btn" onClick={onBack}>
          <Icon name="back" /> Vault
        </button>
        <div className="hero-content">
          <div className="hero-cover">
            <Cover item={e} />
          </div>
          <div className="hero-info">
            <h1>{e.name}</h1>
            <div className="meta-row">
              <span className="plat">{e.platformName}</span>
              <span>Emulator</span>
              <span>{e.supports.join(' · ') || 'no configuration'}</span>
            </div>
            {e.config?.description && <p className="prose">{e.config.description}</p>}
            {!sys && <div className="form-error">This emulator has no configuration for {state.os}.</div>}
            {!state.online && !installed ? (
              <div className="muted">Not available while offline.</div>
            ) : (
              <ActionButtons state={state} jobs={jobs} kind="emulator" id={id} size={e.sizeBytes} onOpenLibrary={inLibrary ? onOpenLibrary : undefined} />
            )}
          </div>
        </div>
      </div>
      <div className="store-body">
        <div className="store-main" data-nav-group>
          <section>
            <h2>Cloud saves</h2>
            <p className="prose muted">
              Saves are synchronised per emulator. {latest ? `Your newest cloud save is version ${latest.version}.` : 'You have no cloud saves for this emulator yet.'}
            </p>
            {sys?.saves && (
              <ul className="mono small">
                {sys.saves.map((s) => (
                  <li key={s}>{s}</li>
                ))}
              </ul>
            )}
          </section>
          <section>
            <h2>Games for {e.platformName}</h2>
            <div className="link-list">
              {games.map((g) => (
                <button key={g.id} className="link-row" onClick={() => onOpen('game', g.id)}>
                  {g.name}
                </button>
              ))}
              {!games.length && <span className="muted">No games for this platform yet.</span>}
            </div>
          </section>
        </div>
        <aside className="store-side" data-nav-group>
          <dl className="facts">
            <Fact label="Platform" value={e.platformName} />
            <Fact label="Version" value={e.config?.version} />
            <Fact label="Size" value={`${fmtBytes(e.sizeBytes)} · ${e.fileCount} files`} />
            <Fact label="Systems" value={e.supports.join(', ')} />
            {installed && <Fact label="Installed in" value={installed.path} />}
          </dl>
        </aside>
      </div>
    </div>
  );
}
