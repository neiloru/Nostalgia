import { useState } from 'react';
import { call } from '../bridge';
import type { ClientState } from '../../../main/types';

export function Login({ state }: { state: ClientState }) {
  const [server, setServer] = useState(() => localStorage.getItem('lastServer') ?? 'http://localhost:8080');
  const [username, setUsername] = useState(() => localStorage.getItem('lastUser') ?? '');
  const [password, setPassword] = useState('');
  const [password2, setPassword2] = useState('');
  const [register, setRegister] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    if (register && password !== password2) {
      setError('Passwords do not match');
      return;
    }
    setBusy(true);
    try {
      await call(register ? 'register' : 'login', server, username, password);
      try {
        localStorage.setItem('lastServer', server);
        localStorage.setItem('lastUser', username);
      } catch {
        /* ignore */
      }
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="login-screen">
      <div className="login-art" aria-hidden>
        {Array.from({ length: 24 }).map((_, i) => (
          <span key={i} style={{ animationDelay: `${(i % 8) * -1.7}s`, left: `${(i * 37) % 100}%`, top: `${(i * 53) % 100}%` }} />
        ))}
      </div>
      <form className="login-card" onSubmit={submit}>
        <div className="brand big">
          <span className="logo">N</span>
          <span>Nostalgia</span>
        </div>
        <p className="muted">{register ? 'Create your account' : 'Sign in to your game server'}</p>
        <label>
          Server
          <input value={server} onChange={(e) => setServer(e.target.value)} placeholder="http://192.168.1.10:8080" required />
        </label>
        <label>
          Username
          <input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" required data-autofocus />
        </label>
        <label>
          Password
          <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete={register ? 'new-password' : 'current-password'} required />
        </label>
        {register && (
          <label>
            Repeat password
            <input type="password" value={password2} onChange={(e) => setPassword2(e.target.value)} autoComplete="new-password" required />
          </label>
        )}
        {error && <div className="form-error">{error}</div>}
        <button className="btn primary big" type="submit" disabled={busy}>
          {busy ? 'Please wait…' : register ? 'Create account' : 'Sign in'}
        </button>
        <button type="button" className="btn ghost" onClick={() => setRegister(!register)}>
          {register ? 'I already have an account' : 'Create a new account'}
        </button>
        <div className="login-foot muted">
          v{state.version} · {state.device}
        </div>
      </form>
    </div>
  );
}
