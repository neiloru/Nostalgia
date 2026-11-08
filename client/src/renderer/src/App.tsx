import { useEffect, useMemo, useRef, useState } from 'react';
import { act, call, onToast, useClientState, useDownloads, type Toast } from './bridge';
import { focusFirst, focusGroup, inputMode, onInputMode } from './nav';
import { Icon, fmtClock } from './ui';
import { Login } from './pages/Login';
import { Vault } from './pages/Vault';
import { Library } from './pages/Library';
import { Downloads } from './pages/Downloads';
import { Settings } from './pages/Settings';
import type { ItemKind } from '../../main/types';

export type Tab = 'vault' | 'library' | 'downloads' | 'settings';
const TABS: { id: Tab; label: string; icon: 'grid' | 'library' | 'download' | 'gear' }[] = [
  { id: 'vault', label: 'Vault', icon: 'grid' },
  { id: 'library', label: 'Library', icon: 'library' },
  { id: 'downloads', label: 'Downloads', icon: 'download' },
  { id: 'settings', label: 'Settings', icon: 'gear' },
];

export interface Navigate {
  /** Open the library tab with an item selected. */
  toLibrary: (kind: ItemKind, id: number) => void;
  /** Open the store page of an item in the vault. */
  toStore: (kind: ItemKind, id: number) => void;
  toTab: (t: Tab) => void;
}

export function App() {
  const state = useClientState();
  const jobs = useDownloads();
  const [tab, setTab] = useState<Tab>('vault');
  const [libSel, setLibSel] = useState<{ kind: ItemKind; id: number } | null>(null);
  const [storeSel, setStoreSel] = useState<{ kind: ItemKind; id: number } | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [mode, setMode] = useState(inputMode);
  const contentRef = useRef<HTMLDivElement>(null);
  const railRef = useRef<HTMLElement>(null);

  useEffect(() => onInputMode(setMode), []);
  useEffect(
    () =>
      onToast((t) => {
        setToasts((l) => [...l.slice(-3), t]);
        setTimeout(() => setToasts((l) => l.filter((x) => x.id !== t.id)), t.error ? 7000 : 4000);
      }),
    [],
  );

  // LB / RB, Q / E switch tabs.
  useEffect(() => {
    const onNav = (e: Event) => {
      const a = (e as CustomEvent).detail as string;
      if (document.querySelector('[data-nav-scope]')) return;
      if (a === 'tabPrev' || a === 'tabNext') {
        const i = TABS.findIndex((t) => t.id === tab);
        const next = TABS[(i + (a === 'tabNext' ? 1 : TABS.length - 1)) % TABS.length].id;
        setTab(next);
      } else if (a === 'menu') {
        setTab('settings');
      } else if (a === 'back') {
        // B with nothing to close: go to the sidebar.
        if (!railRef.current?.contains(document.activeElement)) focusGroup(railRef.current);
      }
    };
    window.addEventListener('nav', onNav);
    return () => window.removeEventListener('nav', onNav);
  }, [tab]);

  // Focus the content when the tab changes.
  useEffect(() => {
    const t = setTimeout(() => focusFirst(contentRef.current), 30);
    return () => clearTimeout(t);
  }, [tab]);

  const nav: Navigate = useMemo(
    () => ({
      toLibrary: (kind, id) => {
        setLibSel({ kind, id });
        setTab('library');
      },
      toStore: (kind, id) => {
        setStoreSel({ kind, id });
        setTab('vault');
      },
      toTab: setTab,
    }),
    [],
  );

  const activeDownloads = jobs.filter((j) => j.status === 'downloading' || j.status === 'queued');
  const dlProgress = activeDownloads.length
    ? activeDownloads.reduce((a, j) => a + j.receivedBytes, 0) / Math.max(1, activeDownloads.reduce((a, j) => a + j.totalBytes, 0))
    : 0;

  const selectTab = (t: Tab) => {
    if (t === tab) focusFirst(contentRef.current);
    else setTab(t);
  };

  if (!state) return <div className="splash" />;
  if (!state.session) return <Login state={state} />;

  return (
    <div className={`shell input-${mode}`}>
      <nav className="rail" ref={railRef} data-nav-group role="tablist" aria-orientation="vertical">
        <div className="rail-tabs">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={tab === t.id}
              className={`rail-item ${tab === t.id ? 'active' : ''}`}
              data-nav-default={tab === t.id ? true : undefined}
              onClick={() => selectTab(t.id)}
            >
              <Icon name={t.icon} size={20} />
              <span className="rail-label">{t.label}</span>
              {t.id === 'downloads' && activeDownloads.length > 0 && <span className="rail-badge">{Math.round(dlProgress * 100)}%</span>}
            </button>
          ))}
        </div>
        <div className="rail-foot">
          {!state.online && (
            <button className="rail-item offline" onClick={() => act(() => call('refresh'))} title="Try to reconnect">
              <Icon name="wifiOff" size={20} />
              <span className="rail-label">Offline</span>
            </button>
          )}
          <button className="rail-item" title="Toggle fullscreen (F11)" onClick={() => call('toggleFullscreen')}>
            <Icon name="expand" size={20} />
            <span className="rail-label">{state.fullscreen ? 'Window' : 'Fullscreen'}</span>
          </button>
          <div className="rail-user" title={state.session.serverUrl}>
            <Icon name="user" size={20} />
            <span className="rail-label">
              <span className="ellipsis">{state.session.user.username}</span>
              <small className="ellipsis">{state.session.serverUrl.replace(/^https?:\/\//, '')}</small>
            </span>
          </div>
        </div>
      </nav>

      <div className="main-col">
        {state.running && <RunningBanner name={state.running.name} startedAt={state.running.startedAt} />}

        <main className="content" ref={contentRef} data-nav-group data-nav-remember>
          {tab === 'vault' && <Vault state={state} jobs={jobs} nav={nav} selected={storeSel} setSelected={setStoreSel} />}
          {tab === 'library' && <Library state={state} jobs={jobs} nav={nav} selected={libSel} setSelected={setLibSel} />}
          {tab === 'downloads' && <Downloads state={state} jobs={jobs} />}
          {tab === 'settings' && <Settings state={state} />}
        </main>

        <footer className={`hintbar ${mode === 'mouse' ? 'dim' : ''}`}>
          <span><kbd>A</kbd> Select</span>
          <span><kbd>B</kbd> Back</span>
          {((tab === 'vault' && !storeSel) || tab === 'library') && <span><kbd>Y</kbd> Search</span>}
          <span><kbd>LB</kbd><kbd>RB</kbd> Switch section</span>
          <span className="spacer" />
          {state.pendingSessions > 0 && <span>{state.pendingSessions} play session{state.pendingSessions > 1 ? 's' : ''} waiting to sync</span>}
          <span>{state.online ? 'Connected' : 'Offline'}</span>
        </footer>
      </div>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.error ? 'error' : ''}`}>
            {t.message}
          </div>
        ))}
      </div>
    </div>
  );
}

function RunningBanner({ name, startedAt }: { name: string; startedAt: string }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="running-banner">
      <span className="live-dot" />
      <span>
        Playing <strong>{name}</strong>
      </span>
      <span className="mono">{fmtClock((now - Date.parse(startedAt)) / 1000)}</span>
      <span className="spacer" />
      <button className="btn small danger" onClick={() => call('stopGame')}>
        <Icon name="x" size={14} /> Force quit
      </button>
    </div>
  );
}
