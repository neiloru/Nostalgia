import { useEffect, useState } from 'react';
import { act, call } from '../bridge';
import { Confirm, Icon, Select, Toggle, fmtBytes } from '../ui';
import type { ClientSettings, ClientState } from '../../../main/types';

export function Settings({ state }: { state: ClientState }) {
  const [oldPw, setOldPw] = useState('');
  const [newPw, setNewPw] = useState('');
  const [newPw2, setNewPw2] = useState('');
  const [pwError, setPwError] = useState('');
  const [confirmLogout, setConfirmLogout] = useState(false);
  const [locs, setLocs] = useState<{ path: string; free?: number; exists: boolean }[]>([]);
  const s = state.settings;
  const set = (u: Partial<ClientSettings>) => call('setSettings', u);

  useEffect(() => {
    void call<typeof locs>('locationInfo').then(setLocs);
  }, [s.locations]);

  const changePw = async () => {
    setPwError('');
    if (newPw !== newPw2) return setPwError('The new passwords do not match');
    if (newPw.length < 6) return setPwError('At least 6 characters');
    try {
      await call('changePassword', oldPw, newPw);
      setOldPw('');
      setNewPw('');
      setNewPw2('');
    } catch (e) {
      setPwError((e as Error).message);
    }
  };

  const installedCount = (p: string) =>
    [...Object.values(state.installed.games), ...Object.values(state.installed.emulators)].filter((i) => i.location === p).length;
  const defaultLoc = s.defaultLocation ?? s.locations[0];

  return (
    <div className="settings page">
      <h1>Settings</h1>
      <div className="settings-grid">
        <section className="panel" data-nav-group>
          <h2>Account</h2>
          <div className="kv">
            <span>User</span>
            <span>{state.session?.user.username}</span>
            <span>Server</span>
            <span className="mono">{state.session?.serverUrl}</span>
            <span>Status</span>
            <span>{state.online ? 'Connected' : 'Offline'}</span>
            <span>Device</span>
            <span>{state.device}</span>
          </div>
          <h3>Change password</h3>
          <div className="form">
            <label className="field">
              Current password
              <input type="password" value={oldPw} onChange={(e) => setOldPw(e.target.value)} autoComplete="current-password" />
            </label>
            <label className="field">
              New password
              <input type="password" value={newPw} onChange={(e) => setNewPw(e.target.value)} autoComplete="new-password" />
            </label>
            <label className="field">
              Repeat new password
              <input type="password" value={newPw2} onChange={(e) => setNewPw2(e.target.value)} autoComplete="new-password" />
            </label>
            {pwError && <div className="form-error">{pwError}</div>}
            <button className="btn primary" onClick={changePw} disabled={!state.online || !oldPw || !newPw}>
              Change password
            </button>
          </div>
          <div className="row" style={{ marginTop: 18 }}>
            <button className="btn danger" onClick={() => setConfirmLogout(true)}>
              <Icon name="logout" size={16} /> Log out
            </button>
          </div>
        </section>

        <section className="panel" data-nav-group>
          <h2>Download locations</h2>
          <p className="muted small">Games are installed to &lt;location&gt;/games/&lt;platform&gt;/ and emulators to &lt;location&gt;/emulators/&lt;platform&gt;/.</p>
          <div className="loc-list">
            {s.locations.map((p) => {
              const info = locs.find((l) => l.path === p);
              const count = installedCount(p);
              return (
                <div key={p} className="loc">
                  <Icon name="folder" />
                  <div className="loc-info">
                    <span className="mono">{p}</span>
                    <small className="muted">
                      {info?.free != null ? `${fmtBytes(info.free)} free` : info && !info.exists ? 'not accessible' : ''}
                      {count ? ` · ${count} installed` : ''}
                    </small>
                  </div>
                  {p === defaultLoc ? (
                    <span className="badge ok small">default</span>
                  ) : (
                    <button className="btn small" onClick={() => set({ defaultLocation: p })}>
                      Make default
                    </button>
                  )}
                  <button className="btn small ghost" onClick={() => act(() => call('removeLocation', p))} disabled={s.locations.length < 2 || count > 0} title={count ? 'Uninstall items first' : 'Remove'}>
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              );
            })}
          </div>
          <button className="btn" onClick={() => act(() => call('addLocation'))}>
            <Icon name="plus" size={16} /> Add location
          </button>
        </section>

        <section className="panel" data-nav-group>
          <h2>Cloud saves &amp; play</h2>
          <div className="toggle-list" data-autofocus>
            <Toggle label="Sync cloud saves automatically" hint="Download newest saves on start, upload after playing" checked={s.autoSyncSaves} onChange={(v) => set({ autoSyncSaves: v })} />
            <Toggle label="Install the emulator together with a game" checked={s.installEmulatorWithGame} onChange={(v) => set({ installEmulatorWithGame: v })} />
            <Toggle label="Minimize Nostalgia while playing" checked={s.minimizeOnLaunch} onChange={(v) => set({ minimizeOnLaunch: v })} />
          </div>
          <button className="btn ghost" onClick={() => call('openSaveBackups')}>
            <Icon name="folder" size={16} /> Open local save backups
          </button>
        </section>

        <section className="panel" data-nav-group>
          <h2>Display</h2>
          <div className="toggle-list">
            <Toggle label="Fullscreen" hint="F11" checked={state.fullscreen} onChange={() => call('toggleFullscreen')} />
            <Toggle label="Start in fullscreen" hint="Recommended for Steam Deck / TV" checked={s.startFullscreen} onChange={(v) => set({ startFullscreen: v })} />
          </div>
          <Select
            label="UI scale"
            value={s.uiScale ?? 0}
            options={[
              { value: 0, label: s.uiScale ? 'Automatic' : `Automatic (${Math.round(state.uiZoom * 100)} %)`, hint: 'By window size, 1080p = 100 %' },
              ...[0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3].map((z) => ({ value: z, label: `${Math.round(z * 100)} %` })),
            ]}
            onChange={(v) => set({ uiScale: v })}
          />
          <div className="muted small">Ctrl + / Ctrl − / Ctrl 0 change the scale with a keyboard.</div>
          <h3>Controls</h3>
          <div className="kv small">
            <span>Navigate</span>
            <span>D-pad / left stick · arrow keys · mouse</span>
            <span>Select</span>
            <span>A · Enter</span>
            <span>Back</span>
            <span>B · Esc / Backspace</span>
            <span>Switch tabs</span>
            <span>LB / RB · Q / E</span>
            <span>Search / filter</span>
            <span>Y · F</span>
            <span>Settings</span>
            <span>Start</span>
          </div>
          <h3>About</h3>
          <div className="muted small">Nostalgia v{state.version}</div>
          <button className="btn ghost" onClick={() => call('quit')}>
            Quit Nostalgia
          </button>
        </section>
      </div>
      {confirmLogout && (
        <Confirm
          title="Log out?"
          message="Installed games stay on this device. Running downloads are paused."
          confirm="Log out"
          danger
          onConfirm={() => act(() => call('logout'))}
          onClose={() => setConfirmLogout(false)}
        />
      )}
    </div>
  );
}
