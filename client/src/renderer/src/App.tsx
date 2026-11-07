import { useEffect, useMemo, useRef, useState } from 'react';
import { act, call, onToast, useClientState, useDownloads, type Toast } from './bridge';
import { focusFirst, inputMode, onInputMode } from './nav';
import { Icon, fmtClock } from './ui';
import { Login } from './pages/Login';
import { Vault } from './pages/Vault';
import { Library } from './pages/Library';
import { Downloads } from './pages/Downloads';
import { Settings } from './pages/Settings';
import type { ItemKind } from '../../main/types';

export type Tab = 'vault' | 'library' | 'downloads' | 'settings';
const TABS: { id: Tab; label: string }[] = [
  { id: 'vault', label: 'Vault' },
  { id: 'library', label: 'Library' },
  { id: 'downloads', label: 'Downloads' },
  { id: 'settings', label: 'Settings' },
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

  if (!state) return <div className="splash"><div className="logo big">N</div></div>;
  if (!state.session) return <Login state={state} />;

  return (
    <div className={`shell input-${mode}`}>
      <header className="topbar">
        <div className="brand">
          <span className="logo">N</span>
          <span>Nostalgia</span>
        </div>
        <nav className="tabs" role="tablist">
          <span className="pad-hint">LB</span>
          {TABS.map((t) => (
            <button key={t.id} role="tab" aria-selected={tab === t.id} className={`tab ${tab === t.id ? 'active' : ''}`} onClick={() => setTab(t.id)}>
              {t.label}
              {t.id === 'downloads' && activeDownloads.length > 0 && <span className="tab-badge">{Math.round(dlProgress * 100)}%</span>}
            </button>
          ))}
          <span className="pad-hint">RB</span>
        </nav>
        <div className="topbar-right">
          {!state.online && (
            <button className="offline-pill" onClick={() => act(() => call('refresh'))} title="Try to reconnect">
              <Icon name="wifiOff" size={15} /> Offline
            </button>
          )}
          <span className="user-chip" title={state.session.serverUrl}>
            {state.session.user.username}
          </span>
          <button className="icon-btn" title="Toggle fullscreen (F11)" onClick={() => call('toggleFullscreen')}>
            <Icon name="expand" />
          </button>
        </div>
      </header>

      {state.running && <RunningBanner name={state.running.name} startedAt={state.running.startedAt} />}

      <main className="content" ref={contentRef}>
        {tab === 'vault' && <Vault state={state} jobs={jobs} nav={nav} selected={storeSel} setSelected={setStoreSel} />}
        {tab === 'library' && <Library state={state} jobs={jobs} nav={nav} selected={libSel} setSelected={setLibSel} />}
        {tab === 'downloads' && <Downloads state={state} jobs={jobs} />}
        {tab === 'settings' && <Settings state={state} />}
      </main>

      <footer className={`hintbar ${mode === 'mouse' ? 'dim' : ''}`}>
        <span><kbd className="btn-a">A</kbd> Select</span>
        <span><kbd className="btn-b">B</kbd> Back</span>
        <span><kbd className="btn-y">Y</kbd> Search / filter</span>
        <span><kbd>LB</kbd><kbd>RB</kbd> Switch tabs</span>
        <span className="spacer" />
        {state.pendingSessions > 0 && <span className="muted">{state.pendingSessions} play session(s) waiting to sync</span>}
        <span className="muted">{state.online ? 'Connected' : 'Offline mode'}</span>
      </footer>

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
      <span className="pulse" />
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
