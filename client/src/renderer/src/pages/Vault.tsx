import { useEffect, useMemo, useRef, useState } from 'react';
import { GameCard, OfflineBanner, type AnyItem } from '../common';
import { Icon, Select, Toggle } from '../ui';
import { focusEl, focusFirst, pushBack } from '../nav';
import { StorePage } from './StorePage';
import type { Navigate } from '../App';
import type { ClientState, DownloadJob, ItemKind } from '../../../main/types';

type Sort = 'name' | 'year' | 'rating' | 'added';

interface Filters {
  q: string;
  platform: string;
  genre: string;
  theme: string;
  mode: string;
  developer: string;
  decade: string;
  sort: Sort;
  showEmulators: boolean;
  installedOnly: boolean;
  libraryOnly: boolean;
}

const DEFAULT: Filters = {
  q: '',
  platform: '',
  genre: '',
  theme: '',
  mode: '',
  developer: '',
  decade: '',
  sort: 'name',
  showEmulators: false,
  installedOnly: false,
  libraryOnly: false,
};

const load = (): Filters => {
  try {
    return { ...DEFAULT, ...JSON.parse(localStorage.getItem('vaultFilters') ?? '{}'), q: '' };
  } catch {
    return DEFAULT;
  }
};

const uniq = (xs: string[]) => [...new Set(xs.filter(Boolean))].sort((a, b) => a.localeCompare(b));

export function Vault({
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
  const [f, setF] = useState<Filters>(load);
  const searchRef = useRef<HTMLInputElement>(null);
  const lastOpened = useRef<string | null>(null);
  const set = (u: Partial<Filters>) => setF((cur) => ({ ...cur, ...u }));

  useEffect(() => {
    try {
      localStorage.setItem('vaultFilters', JSON.stringify(f));
    } catch {
      /* ignore */
    }
  }, [f]);

  useEffect(() => {
    const onNav = (e: Event) => {
      if ((e as CustomEvent).detail === 'secondary' && !selected && !document.querySelector('[data-nav-scope]')) searchRef.current?.focus();
    };
    window.addEventListener('nav', onNav);
    return () => window.removeEventListener('nav', onNav);
  }, [selected]);

  // Coming back from a store page: restore focus to the card.
  useEffect(() => {
    if (selected) return pushBack(() => setSelected(null));
    const t = setTimeout(() => {
      const el = lastOpened.current ? document.querySelector<HTMLElement>(`[data-card="${lastOpened.current}"] .card`) : null;
      if (el) focusEl(el);
      else focusFirst(document.querySelector<HTMLElement>('.vault-grid'));
    }, 30);
    return () => clearTimeout(t);
  }, [selected, setSelected]);

  const { games, emulators } = state.catalog;
  const options = useMemo(
    () => ({
      platforms: uniq([...games.map((g) => g.platform), ...(f.showEmulators ? emulators.map((e) => e.platform) : [])]).map((p) => ({
        value: p,
        label: games.find((g) => g.platform === p)?.platformName ?? emulators.find((e) => e.platform === p)?.platformName ?? p,
      })),
      genres: uniq(games.flatMap((g) => g.genres)),
      themes: uniq(games.flatMap((g) => g.themes)),
      modes: uniq(games.flatMap((g) => g.gameModes)),
      developers: uniq(games.flatMap((g) => g.developers)),
      decades: uniq(games.map((g) => (g.year ? `${Math.floor(g.year / 10) * 10}s` : ''))),
    }),
    [games, emulators, f.showEmulators],
  );

  const items = useMemo(() => {
    const q = f.q.trim().toLowerCase();
    const list: AnyItem[] = [...games, ...(f.showEmulators ? emulators : [])];
    const filtered = list.filter((it) => {
      if (q && !it.name.toLowerCase().includes(q) && !it.platformName.toLowerCase().includes(q)) return false;
      if (f.platform && it.platform !== f.platform) return false;
      const installed = it.kind === 'game' ? state.installed.games[it.id] : state.installed.emulators[it.id];
      if (f.installedOnly && !installed) return false;
      if (f.libraryOnly && !(it.kind === 'game' ? state.library.games : state.library.emulators).includes(it.id)) return false;
      if (it.kind === 'game') {
        if (f.genre && !it.genres.includes(f.genre)) return false;
        if (f.theme && !it.themes.includes(f.theme)) return false;
        if (f.mode && !it.gameModes.includes(f.mode)) return false;
        if (f.developer && !it.developers.includes(f.developer) && !it.publishers.includes(f.developer)) return false;
        if (f.decade && !(it.year && `${Math.floor(it.year / 10) * 10}s` === f.decade)) return false;
      } else if (f.genre || f.theme || f.mode || f.developer || f.decade) return false;
      return true;
    });
    const by = (it: AnyItem) => it.name.toLowerCase();
    filtered.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === 'game' ? -1 : 1;
      if (f.sort === 'year' && a.kind === 'game' && b.kind === 'game') return (b.year ?? 0) - (a.year ?? 0) || by(a).localeCompare(by(b));
      if (f.sort === 'rating' && a.kind === 'game' && b.kind === 'game') return (b.rating ?? 0) - (a.rating ?? 0) || by(a).localeCompare(by(b));
      if (f.sort === 'added') return b.addedAt.localeCompare(a.addedAt);
      return by(a).localeCompare(by(b));
    });
    // Offline: installed first.
    if (!state.online) {
      filtered.sort((a, b) => Number(!!isInstalled(state, b)) - Number(!!isInstalled(state, a)));
    }
    return filtered;
  }, [games, emulators, f, state]);

  if (selected) {
    return (
      <StorePage
        state={state}
        jobs={jobs}
        kind={selected.kind}
        id={selected.id}
        onBack={() => setSelected(null)}
        onOpenLibrary={() => nav.toLibrary(selected.kind, selected.id)}
        onOpen={(kind, id) => setSelected({ kind, id })}
      />
    );
  }

  const activeFilters = [f.platform, f.genre, f.theme, f.mode, f.developer, f.decade].filter(Boolean).length + (f.installedOnly ? 1 : 0) + (f.libraryOnly ? 1 : 0);
  const all = (label: string) => ({ value: '', label });

  return (
    <div className="vault">
      <aside className="filters" data-nav-group data-nav-remember>
        <div className="search">
          <Icon name="search" size={16} />
          <input ref={searchRef} type="search" placeholder="Search the vault…" value={f.q} onChange={(e) => set({ q: e.target.value })} />
        </div>
        <Select label="Platform" value={f.platform} options={[all('All platforms'), ...options.platforms]} onChange={(v) => set({ platform: v })} />
        <Select label="Genre" value={f.genre} options={[all('Any genre'), ...options.genres.map((g) => ({ value: g, label: g }))]} onChange={(v) => set({ genre: v })} />
        <Select label="Theme" value={f.theme} options={[all('Any theme'), ...options.themes.map((g) => ({ value: g, label: g }))]} onChange={(v) => set({ theme: v })} />
        <Select label="Game mode" value={f.mode} options={[all('Any mode'), ...options.modes.map((g) => ({ value: g, label: g }))]} onChange={(v) => set({ mode: v })} />
        <Select label="Era" value={f.decade} options={[all('Any year'), ...options.decades.map((g) => ({ value: g, label: g }))]} onChange={(v) => set({ decade: v })} />
        <Select
          label="Developer / publisher"
          value={f.developer}
          options={[all('Anyone'), ...options.developers.map((g) => ({ value: g, label: g }))]}
          onChange={(v) => set({ developer: v })}
        />
        <Select<Sort>
          label="Sort by"
          value={f.sort}
          options={[
            { value: 'name', label: 'Name' },
            { value: 'year', label: 'Release year' },
            { value: 'rating', label: 'Rating' },
            { value: 'added', label: 'Recently added' },
          ]}
          onChange={(v) => set({ sort: v })}
        />
        <div className="filter-toggles">
          <Toggle label="Show emulators" checked={f.showEmulators} onChange={(v) => set({ showEmulators: v })} />
          <Toggle label="Installed only" checked={f.installedOnly} onChange={(v) => set({ installedOnly: v })} />
          <Toggle label="In my library only" checked={f.libraryOnly} onChange={(v) => set({ libraryOnly: v })} />
        </div>
        {activeFilters > 0 && (
          <button className="btn ghost" onClick={() => setF({ ...DEFAULT, showEmulators: f.showEmulators, sort: f.sort })}>
            Clear {activeFilters} filter{activeFilters > 1 ? 's' : ''}
          </button>
        )}
      </aside>
      <section className="vault-main" data-nav-group data-nav-remember>
        {!state.online && <OfflineBanner />}
        <div className="section-head">
          <h1>Vault</h1>
          <span className="muted">
            {items.length} {items.length === 1 ? 'title' : 'titles'}
          </span>
        </div>
        {items.length ? (
          <div className="vault-grid">
            {items.map((it, i) => (
              <div key={`${it.kind}:${it.id}`} data-card={`${it.kind}:${it.id}`} data-autofocus={i === 0 ? true : undefined}>
                <GameCard
                  item={it}
                  state={state}
                  jobs={jobs}
                  onOpen={() => {
                    lastOpened.current = `${it.kind}:${it.id}`;
                    setSelected({ kind: it.kind, id: it.id });
                  }}
                />
              </div>
            ))}
          </div>
        ) : (
          <div className="empty">
            {games.length ? 'Nothing matches these filters.' : state.online ? 'The server has no games yet.' : 'No cached catalog available offline.'}
          </div>
        )}
      </section>
    </div>
  );
}

function isInstalled(state: ClientState, it: AnyItem) {
  return it.kind === 'game' ? state.installed.games[it.id] : state.installed.emulators[it.id];
}
