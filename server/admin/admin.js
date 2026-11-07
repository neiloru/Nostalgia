/* Nostalgia admin UI - dependency free. */
(() => {
  'use strict';
  const $ = (s, el = document) => el.querySelector(s);
  const $$ = (s, el = document) => [...el.querySelectorAll(s)];
  const esc = (v) => String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const fmtBytes = (n) => {
    if (!n) return '0 B';
    const u = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.min(u.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
    return `${(n / 1024 ** i).toFixed(i > 1 ? 1 : 0)} ${u[i]}`;
  };
  const fmtDur = (s) => {
    s = Number(s) || 0;
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
    return h ? `${h}h ${m}m` : `${m}m`;
  };
  const fmtDate = (d) => (d ? new Date(d).toLocaleString() : '-');

  let platforms = [];
  let current = 'dashboard';
  let pollTimer = null;

  async function api(path, opts = {}) {
    const res = await fetch(`/admin/api${path}`, {
      credentials: 'same-origin',
      headers: opts.body ? { 'Content-Type': 'application/json' } : {},
      ...opts,
      body: opts.body ? JSON.stringify(opts.body) : undefined,
    });
    if (res.status === 401 && path !== '/login') {
      showLogin();
      throw new Error('Not signed in');
    }
    const data = res.headers.get('content-type')?.includes('json') ? await res.json() : await res.text();
    if (!res.ok) throw new Error(data?.error || `HTTP ${res.status}`);
    return data;
  }

  function toast(msg, err = false) {
    const t = document.createElement('div');
    t.className = 'toast' + (err ? ' err' : '');
    t.textContent = msg;
    $('#toasts').append(t);
    setTimeout(() => t.remove(), err ? 7000 : 3500);
  }
  const guard = (fn) => async (...a) => {
    try {
      await fn(...a);
    } catch (e) {
      toast(e.message, true);
    }
  };

  function modal(html) {
    $('#modalBox').innerHTML = html;
    $('#modal').classList.remove('hidden');
    return $('#modalBox');
  }
  const closeModal = () => $('#modal').classList.add('hidden');
  $('#modal').addEventListener('mousedown', (e) => e.target.id === 'modal' && closeModal());
  document.addEventListener('keydown', (e) => e.key === 'Escape' && closeModal());

  function showLogin() {
    $('#app').classList.add('hidden');
    $('#login').classList.remove('hidden');
    clearInterval(pollTimer);
  }

  $('#loginForm').addEventListener('submit', async (e) => {
    e.preventDefault();
    const f = new FormData(e.target);
    try {
      await api('/login', { method: 'POST', body: { username: f.get('username'), password: f.get('password') } });
      $('#loginError').textContent = '';
      start();
    } catch (err) {
      $('#loginError').textContent = err.message;
    }
  });

  $('#logout').addEventListener('click', guard(async () => {
    await api('/logout', { method: 'POST' });
    showLogin();
  }));

  function platformOptions(sel, withAll = false) {
    return (withAll ? '<option value="">All platforms</option>' : '') +
      platforms.map((p) => `<option value="${esc(p.slug)}" ${p.slug === sel ? 'selected' : ''}>${esc(p.name)} (${esc(p.slug)})</option>`).join('');
  }

  // ---------------- job indicator ----------------
  async function pollJobs() {
    try {
      const j = await api('/jobs');
      const parts = [];
      if (j.scan.running) parts.push('Scanning library…');
      if (j.metadata.running) parts.push(`IGDB: ${esc(j.metadata.current || '')} (${j.metadata.queue} queued)`);
      if (j.hashing.running) {
        const pct = j.hashing.bytesTotal ? Math.round((j.hashing.bytesDone / j.hashing.bytesTotal) * 100) : 0;
        parts.push(`Hashing ${esc(j.hashing.current || '')} ${pct}%`);
      }
      $('#jobIndicator').innerHTML = parts.join('<br>');
      const badge = j.scan.last?.unsorted?.length || '';
      $('#unsortedBadge').textContent = badge;
      return j;
    } catch {
      return null;
    }
  }

  // ---------------- pages ----------------
  const pages = {
    async dashboard(main) {
      const o = await api('/overview');
      const s = o.jobs.scan.last;
      main.innerHTML = `
        <h1>Dashboard</h1>
        <div class="grid stats">
          ${[
            ['Games', o.games], ['Library size', fmtBytes(o.librarySize)], ['Emulators', o.emulators], ['Users', o.users],
            ['Cloud saves', `${o.saves} · ${fmtBytes(o.saveBytes)}`], ['Total play time', fmtDur(o.playSeconds)],
            ['Clients online', `${o.connectedClients} (${o.onlineUsers} users)`], ['Unsorted files', o.unsorted],
          ].map(([l, v]) => `<div class="card stat"><div class="l">${l}</div><div class="v">${esc(v)}</div></div>`).join('')}
        </div>
        <div class="grid" style="grid-template-columns: repeat(auto-fit, minmax(380px, 1fr))">
          <div class="card grid">
            <h2>Library scan</h2>
            <p class="muted" style="margin:0">Finds new games (<span class="mono">games/{platform}/{Name} [igdbid-ID]/</span>) and emulators,
              downloads IGDB metadata in the background. The optional Hasheous check hashes every file and compares it with known dumps (admin info only).</p>
            <label class="check"><input type="checkbox" id="scanHash" /> Also check file hashes with Hasheous (slow for large libraries)</label>
            <div class="row"><button class="primary" id="scanBtn" ${o.jobs.scan.running ? 'disabled' : ''}>Scan now</button>
              <span class="muted small-text">${s ? `Last scan ${fmtDate(s.finishedAt)} – ${s.newGames} new games, ${s.newEmulators} new emulators` : 'No scan yet'}</span></div>
            <div id="jobBox"></div>
            ${s?.errors?.length ? `<div><h2>Warnings</h2>${s.errors.map((e) => `<div class="small-text error">${esc(e)}</div>`).join('')}</div>` : ''}
          </div>
          <div class="card grid">
            <h2>Server</h2>
            <div class="kv small-text">
              <div>Settings file</div><div class="mono">${esc(o.settingsFile)}</div>
              <div>Library</div><div class="mono">${esc(o.libraryPath)}</div>
              <div>Data / saves</div><div class="mono">${esc(o.dataPath)}</div>
              <div>IGDB</div><div>${o.igdbConfigured ? '<span class="pill ok">configured</span>' : '<span class="pill err">missing credentials</span>'}</div>
              <div>Registration</div><div>${o.registration ? '<span class="pill ok">open</span>' : '<span class="pill">closed</span>'}</div>
              <div>Metadata pending</div><div>${o.metadataMissing}</div>
              <div>Missing games</div><div>${o.missingGames}</div>
            </div>
          </div>
        </div>`;
      $('#scanBtn').onclick = guard(async () => {
        await api('/scan', { method: 'POST', body: { hash: $('#scanHash').checked } });
        toast('Scan started');
        $('#scanBtn').disabled = true;
      });
      const renderJobs = (j) => {
        const box = $('#jobBox');
        if (!box || !j) return;
        const h = j.hashing;
        box.innerHTML = `
          ${j.metadata.running || j.metadata.queue ? `<div class="small-text">IGDB metadata: ${esc(j.metadata.current || '')} – ${j.metadata.queue} queued, ${j.metadata.done} done, ${j.metadata.failed} failed</div>` : ''}
          ${h.running ? `<div class="small-text">Hashing ${esc(h.current || '')} (${h.filesDone} files done)</div>
            <div class="progress"><div style="width:${h.bytesTotal ? (h.bytesDone / h.bytesTotal) * 100 : 0}%"></div></div>` : ''}`;
        if (!j.scan.running && $('#scanBtn')) $('#scanBtn').disabled = false;
      };
      renderJobs(o.jobs);
      return renderJobs;
    },

    async games(main) {
      const games = await api('/games');
      const state = { q: '', platform: '', filter: '' };
      main.innerHTML = `
        <h1>Games</h1>
        <div class="toolbar">
          <input id="gq" placeholder="Search…" />
          <select id="gp">${platformOptions('', true)}</select>
          <select id="gf">
            <option value="">All</option><option value="meta">Metadata missing / failed</option>
            <option value="hash">Hash not matched</option><option value="hidden">Hidden</option><option value="missing">Missing on disk</option>
          </select>
          <span class="spacer"></span><span class="muted" id="gcount"></span>
        </div>
        <div class="card" style="padding:0"><table><thead><tr>
          <th></th><th>Name</th><th>Platform</th><th>IGDB</th><th>Size</th><th>Metadata</th><th>Hasheous</th><th>Libraries</th><th></th>
        </tr></thead><tbody id="gbody"></tbody></table></div>`;
      const render = () => {
        const q = state.q.toLowerCase();
        const rows = games.filter((g) =>
          (!q || g.name.toLowerCase().includes(q) || g.folder.toLowerCase().includes(q)) &&
          (!state.platform || g.platform === state.platform) &&
          (state.filter !== 'meta' || g.metadataStatus !== 'ok') &&
          (state.filter !== 'hash' || (g.hash && g.hash.matched < g.hash.files)) &&
          (state.filter !== 'hidden' || g.hidden) &&
          (state.filter !== 'missing' || g.missing));
        $('#gcount').textContent = `${rows.length} of ${games.length}`;
        $('#gbody').innerHTML = rows.length ? rows.map((g) => `
          <tr data-id="${g.id}">
            <td>${g.hasCover ? `<img class="thumb" loading="lazy" src="/admin/api/games/${g.id}/media/cover.jpg">` : '<div class="thumb"></div>'}</td>
            <td><div>${esc(g.name)} ${g.hidden ? '<span class="pill">hidden</span>' : ''} ${g.missing ? '<span class="pill err">missing</span>' : ''}</div>
              <div class="muted small-text mono">${esc(g.relPath)}</div></td>
            <td><span class="pill">${esc(g.platform)}</span></td>
            <td><a href="https://www.igdb.com/games/${g.igdbId}" target="_blank" rel="noreferrer" class="mono">${g.igdbId}</a></td>
            <td>${fmtBytes(g.sizeBytes)}<div class="muted small-text">${g.fileCount} files</div></td>
            <td>${g.metadataStatus === 'ok' ? '<span class="pill ok">ok</span>' : g.metadataStatus === 'error'
              ? `<span class="pill err" title="${esc(g.metadataError)}">error</span>` : '<span class="pill warn">pending</span>'}</td>
            <td>${!g.hash ? '<span class="pill">not checked</span>' : g.hash.matched === g.hash.files
              ? `<span class="pill ok">${g.hash.matched}/${g.hash.files} known</span>`
              : `<span class="pill warn">${g.hash.matched}/${g.hash.files} known</span>`}</td>
            <td>${g.inLibraries}</td>
            <td><div class="row end">
              <button class="small" data-act="details">Details</button>
              <button class="small" data-act="match">Re-match</button>
            </div></td>
          </tr>`).join('') : '<tr><td colspan="9" class="empty">No games</td></tr>';
      };
      $('#gq').oninput = (e) => { state.q = e.target.value; render(); };
      $('#gp').onchange = (e) => { state.platform = e.target.value; render(); };
      $('#gf').onchange = (e) => { state.filter = e.target.value; render(); };
      $('#gbody').onclick = guard(async (e) => {
        const btn = e.target.closest('button');
        if (!btn) return;
        const g = games.find((x) => x.id === Number(btn.closest('tr').dataset.id));
        if (btn.dataset.act === 'details') return gameDetails(g.id);
        if (btn.dataset.act === 'match') {
          igdbPicker({
            title: `Re-match "${g.name}"`, query: g.name, platform: g.platform, name: g.name,
            note: 'The game folder will be renamed to the new IGDB id. No files are deleted.',
            onPick: async (hit, name) => {
              await api(`/games/${g.id}/match`, { method: 'POST', body: { igdbId: hit.igdbId, name } });
              toast('Game re-matched, downloading metadata…');
              go('games');
            },
          });
        }
      });
      render();
    },

    async import(main) {
      const [items, jobs] = await Promise.all([api('/unsorted'), api('/jobs')]);
      const byPlatform = {};
      for (const it of items) (byPlatform[it.platform] ||= []).push(it);
      main.innerHTML = `
        <h1>Import</h1>
        <p class="muted">Files and folders placed directly in <span class="mono">games/{platform}/</span> (or folders without an
          <span class="mono">[igdbid-…]</span> tag) appear here. Select the files that belong to one game (e.g. all discs and cue/bin files),
          pick the IGDB entry and they will be moved into <span class="mono">{Name} [igdbid-ID]/</span>. Nothing is ever deleted.</p>
        ${jobs.scan.last ? '' : '<p class="muted">No scan has run yet.</p>'}
        <div class="row" style="margin-bottom:14px"><button id="rescan">Rescan library</button></div>
        <div class="grid">
        ${Object.keys(byPlatform).length ? Object.entries(byPlatform).map(([p, list]) => `
          <div class="card" data-platform="${esc(p)}">
            <div class="row"><h2 style="margin:0">${esc(list[0].platformName)} <span class="muted mono">(${esc(p)})</span></h2><span class="spacer"></span>
              <button class="small" data-act="auto">Auto-group by title</button>
              <button class="primary small" data-act="import">Import selected…</button></div>
            <table><thead><tr><th style="width:30px"></th><th>Entry</th><th>Size</th><th>Suggested title</th><th></th></tr></thead><tbody>
            ${list.map((it) => `<tr data-path="${esc(it.path)}" data-suggest="${esc(it.suggestion)}">
              <td><input type="checkbox" class="pick" /></td>
              <td class="mono">${it.isDir ? '📁 ' : ''}${esc(it.path)}</td>
              <td>${fmtBytes(it.size)}</td><td>${esc(it.suggestion)}</td>
              <td><div class="row end"><button class="small" data-act="identify">Identify (Hasheous)</button></div></td>
            </tr>`).join('')}
            </tbody></table>
          </div>`).join('') : '<div class="card empty">Nothing to import – all files are organised. 🎉</div>'}
        </div>`;
      $('#rescan').onclick = guard(async () => {
        await api('/scan', { method: 'POST', body: {} });
        toast('Scan started');
        setTimeout(() => go('import'), 1500);
      });
      $$('.card[data-platform]').forEach((card) => {
        const platform = card.dataset.platform;
        card.addEventListener('click', guard(async (e) => {
          const btn = e.target.closest('button');
          if (!btn) return;
          if (btn.dataset.act === 'auto') {
            // Select all entries that share the suggestion of the first checked (or first) row.
            const rows = $$('tr[data-path]', card);
            const first = rows.find((r) => $('.pick', r).checked) || rows[0];
            rows.forEach((r) => ($('.pick', r).checked = r.dataset.suggest === first.dataset.suggest));
            return;
          }
          if (btn.dataset.act === 'identify') {
            const tr = btn.closest('tr');
            btn.disabled = true;
            btn.textContent = 'Hashing…';
            try {
              const r = await api('/unsorted/identify', { method: 'POST', body: { platform, path: tr.dataset.path } });
              tr.dataset.igdb = r.igdbId || '';
              tr.dataset.hname = r.name || '';
              btn.textContent = r.status === 'match' ? `✓ ${r.name}${r.igdbId ? ` (IGDB ${r.igdbId})` : ''}` : r.status === 'nomatch' ? 'Unknown dump' : 'Lookup failed';
            } finally {
              btn.disabled = false;
            }
            return;
          }
          if (btn.dataset.act === 'import') {
            const rows = $$('tr[data-path]', card).filter((r) => $('.pick', r).checked);
            if (!rows.length) return toast('Select at least one entry', true);
            const hint = rows.find((r) => r.dataset.igdb);
            igdbPicker({
              title: `Import ${rows.length} entr${rows.length === 1 ? 'y' : 'ies'}`,
              query: hint?.dataset.igdb || rows[0].dataset.suggest, platform, name: hint?.dataset.hname || rows[0].dataset.suggest,
              note: rows.map((r) => r.dataset.path).join(', '),
              onPick: async (hit, name) => {
                await api('/import', { method: 'POST', body: { platform, entries: rows.map((r) => r.dataset.path), igdbId: hit.igdbId, name } });
                toast(`Imported "${name}"`);
                go('import');
              },
            });
          }
        }));
      });
    },

    async emulators(main) {
      const emus = await api('/emulators');
      main.innerHTML = `
        <h1>Emulators</h1>
        <p class="muted">Emulators live in <span class="mono">emulators/{platform}/{Name}/</span> and need an <span class="mono">emu.yaml</span>
          describing how to start games and where saves are stored. Saves are synced per emulator.</p>
        <div class="card" style="padding:0"><table><thead><tr><th>Name</th><th>Platform</th><th>Systems</th><th>Size</th><th>emu.yaml</th><th>Libraries</th><th></th></tr></thead>
        <tbody>${emus.length ? emus.map((e) => `<tr data-id="${e.id}">
          <td>${esc(e.name)} ${e.missing ? '<span class="pill err">missing</span>' : ''}<div class="muted small-text mono">${esc(e.relPath)}</div></td>
          <td><span class="pill">${esc(e.platform)}</span></td>
          <td>${e.supports.map((s) => `<span class="pill ok">${s}</span>`).join(' ') || '-'}</td>
          <td>${fmtBytes(e.sizeBytes)}<div class="muted small-text">${e.fileCount} files</div></td>
          <td>${e.configError ? `<span class="pill err" title="${esc(e.configError)}">${esc(e.configError)}</span>` : '<span class="pill ok">valid</span>'}</td>
          <td>${e.inLibraries}</td>
          <td><div class="row end"><button class="small" data-act="yaml">Edit emu.yaml</button></div></td>
        </tr>`).join('') : '<tr><td colspan="7" class="empty">No emulators found</td></tr>'}</tbody></table></div>`;
      main.querySelector('tbody').onclick = guard(async (e) => {
        const btn = e.target.closest('button');
        if (!btn) return;
        const emu = emus.find((x) => x.id === Number(btn.closest('tr').dataset.id));
        const y = await api(`/emulators/${emu.id}/yaml`);
        const text = y.exists ? y.text : EMU_TEMPLATE.replace('{name}', emu.name);
        const box = modal(`<h2>emu.yaml – ${esc(emu.name)}</h2><div class="muted small-text mono">${esc(y.path)}</div>
          <textarea id="yamlText" spellcheck="false">${esc(text)}</textarea>
          <details class="small-text muted"><summary>Reference</summary><pre class="mono">${esc(EMU_HELP)}</pre></details>
          <p class="error" id="yamlErr"></p>
          <div class="row end"><button id="cancel">Cancel</button><button class="primary" id="save">Save</button></div>`);
        $('#cancel', box).onclick = closeModal;
        $('#save', box).onclick = guard(async () => {
          try {
            await api(`/emulators/${emu.id}/yaml`, { method: 'PUT', body: { text: $('#yamlText', box).value } });
            closeModal();
            toast('emu.yaml saved');
            go('emulators');
          } catch (err) {
            $('#yamlErr', box).textContent = err.message;
          }
        });
      });
    },

    async users(main) {
      const users = await api('/users');
      main.innerHTML = `
        <h1>Users</h1>
        <div class="toolbar"><span class="muted">Users can register themselves from the client. Accounts are never deleted (cloud saves are kept), but can be disabled.</span>
          <span class="spacer"></span><button class="primary" id="newUser">New user</button></div>
        <div class="card" style="padding:0"><table><thead><tr><th>User</th><th>Created</th><th>Last seen</th><th>Games</th><th>Play time</th><th>Cloud saves</th><th></th></tr></thead>
        <tbody>${users.length ? users.map((u) => `<tr data-id="${u.id}">
          <td>${esc(u.username)} ${u.online ? '<span class="pill ok">online</span>' : ''} ${u.disabled ? '<span class="pill err">disabled</span>' : ''}</td>
          <td>${fmtDate(u.createdAt)}</td><td>${fmtDate(u.lastSeen)}</td><td>${u.games}</td><td>${fmtDur(u.playSeconds)}</td>
          <td>${u.saves.count} · ${fmtBytes(u.saves.bytes)}</td>
          <td><div class="row end">
            <button class="small" data-act="details">Details</button>
            <button class="small" data-act="pw">Reset password</button>
            <button class="small ${u.disabled ? '' : 'danger'}" data-act="toggle">${u.disabled ? 'Enable' : 'Disable'}</button>
          </div></td></tr>`).join('') : '<tr><td colspan="7" class="empty">No users yet</td></tr>'}</tbody></table></div>`;
      $('#newUser').onclick = () => {
        const box = modal(`<h2>New user</h2><label>Username <input id="nu" /></label><label>Password <input id="np" type="password" /></label>
          <p class="error" id="nerr"></p><div class="row end"><button id="c">Cancel</button><button class="primary" id="s">Create</button></div>`);
        $('#c', box).onclick = closeModal;
        $('#s', box).onclick = async () => {
          try {
            await api('/users', { method: 'POST', body: { username: $('#nu', box).value, password: $('#np', box).value } });
            closeModal();
            toast('User created');
            go('users');
          } catch (e) {
            $('#nerr', box).textContent = e.message;
          }
        };
      };
      main.querySelector('tbody').onclick = guard(async (e) => {
        const btn = e.target.closest('button');
        if (!btn) return;
        const u = users.find((x) => x.id === Number(btn.closest('tr').dataset.id));
        if (btn.dataset.act === 'toggle') {
          await api(`/users/${u.id}/disabled`, { method: 'POST', body: { disabled: !u.disabled } });
          go('users');
        } else if (btn.dataset.act === 'pw') {
          const box = modal(`<h2>Reset password – ${esc(u.username)}</h2><label>New password <input id="np" type="password" /></label>
            <p class="error" id="perr"></p><div class="row end"><button id="c">Cancel</button><button class="primary" id="s">Set password</button></div>`);
          $('#c', box).onclick = closeModal;
          $('#s', box).onclick = async () => {
            try {
              await api(`/users/${u.id}/password`, { method: 'POST', body: { password: $('#np', box).value } });
              closeModal();
              toast('Password changed, user signed out everywhere');
            } catch (err) {
              $('#perr', box).textContent = err.message;
            }
          };
        } else if (btn.dataset.act === 'details') {
          const d = await api(`/users/${u.id}`);
          modal(`<h2>${esc(d.user.username)}</h2>
            <h2>Library (${d.games.length})</h2>
            <table><thead><tr><th>Game</th><th>Platform</th><th>Play time</th><th>Added</th></tr></thead><tbody>
            ${d.games.map((g) => `<tr><td>${esc(g.name)}</td><td>${esc(g.platform)}</td><td>${fmtDur(g.seconds)}</td><td>${fmtDate(g.addedAt)}</td></tr>`).join('') || '<tr><td colspan="4" class="muted">Empty</td></tr>'}
            </tbody></table>
            <h2>Cloud saves</h2>
            <table><thead><tr><th>Emulator</th><th>Version</th><th>Size</th><th>Device</th><th>Date</th><th></th></tr></thead><tbody>
            ${d.saves.map((s) => `<tr><td>${esc(s.emulator)} <span class="muted">(${esc(s.platform)})</span></td><td>v${s.version}</td><td>${fmtBytes(s.size)}</td>
              <td>${esc(s.device || '')}${s.note ? `<div class="muted small-text">${esc(s.note)}</div>` : ''}</td><td>${fmtDate(s.createdAt)}</td>
              <td><a href="/admin/api/saves/${s.id}/download">Download</a></td></tr>`).join('') || '<tr><td colspan="6" class="muted">No saves</td></tr>'}
            </tbody></table>
            <div class="row end"><button onclick="document.getElementById('modal').classList.add('hidden')">Close</button></div>`);
        }
      });
    },
  };

  async function gameDetails(id) {
    const g = await api(`/games/${id}`);
    const m = g.metadata;
    const box = modal(`
      <div class="row" style="align-items:flex-start;gap:18px">
        ${g.hasCover ? `<img src="/admin/api/games/${g.id}/media/cover.jpg" style="width:140px;border-radius:8px">` : ''}
        <div class="grid" style="flex:1;gap:8px">
          <h2 style="margin:0">${esc(g.name)}</h2>
          <div class="muted mono small-text">${esc(g.relPath)}</div>
          <div class="kv small-text">
            <div>IGDB</div><div><a target="_blank" rel="noreferrer" href="https://www.igdb.com/games/${g.igdbId}">${g.igdbId}</a></div>
            <div>Release</div><div>${esc(m?.releaseDate || '-')}</div>
            <div>Genres</div><div>${esc(m?.genres?.join(', ') || '-')}</div>
            <div>Developer</div><div>${esc(m?.developers?.join(', ') || '-')}</div>
            <div>Metadata</div><div>${esc(g.metadataStatus)} ${g.metadataError ? `<span class="error">${esc(g.metadataError)}</span>` : ''}</div>
          </div>
        </div>
      </div>
      ${m?.summary ? `<p class="small-text">${esc(m.summary)}</p>` : ''}
      <h2>Files &amp; Hasheous</h2>
      <table><thead><tr><th>File</th><th>Size</th><th>MD5</th><th>Hasheous</th></tr></thead><tbody>
        ${g.hashes.length ? g.hashes.map((h) => `<tr><td class="mono small-text">${esc(h.path)}</td><td>${fmtBytes(h.size)}</td><td class="mono small-text">${esc(h.md5)}</td>
          <td>${h.status === 'match' ? `<span class="pill ok">${esc(h.name)}</span>` : h.status === 'nomatch' ? '<span class="pill warn">unknown</span>' : `<span class="pill err">${esc(h.name || 'error')}</span>`}</td></tr>`).join('')
          : '<tr><td colspan="4" class="muted">Not checked yet</td></tr>'}
      </tbody></table>
      <div class="row end">
        <button id="hide">${g.hidden ? 'Show in vault' : 'Hide from vault'}</button>
        <button id="hash">Check hashes</button>
        <button id="refresh">Refresh metadata</button>
        <button class="primary" id="close">Close</button>
      </div>`);
    $('#close', box).onclick = closeModal;
    $('#refresh', box).onclick = guard(async () => { await api(`/games/${id}/refresh`, { method: 'POST' }); toast('Metadata refresh queued'); });
    $('#hash', box).onclick = guard(async () => { await api(`/games/${id}/hash`, { method: 'POST' }); toast('Hash check queued'); });
    $('#hide', box).onclick = guard(async () => { await api(`/games/${id}/hidden`, { method: 'POST', body: { hidden: !g.hidden } }); closeModal(); go('games'); });
  }

  function igdbPicker({ title, query, platform, name, note, onPick }) {
    let selected = null;
    const box = modal(`<h2>${esc(title)}</h2>
      ${note ? `<div class="muted small-text mono">${esc(note)}</div>` : ''}
      <div class="row"><input id="iq" value="${esc(query)}" placeholder="Search IGDB or enter an IGDB id" style="flex:1;width:auto" />
        <select id="ip" style="width:auto">${platformOptions(platform, true)}</select><button id="is">Search</button></div>
      <div class="results" id="ir"><div class="muted">Searching…</div></div>
      <label>Folder / game name <input id="iname" value="${esc(name)}" /></label>
      <div class="row end"><button id="ic">Cancel</button><button class="primary" id="ok" disabled>Use selected game</button></div>`);
    const search = guard(async () => {
      $('#ir', box).innerHTML = '<div class="muted">Searching…</div>';
      const hits = await api(`/igdb/search?q=${encodeURIComponent($('#iq', box).value)}&platform=${encodeURIComponent($('#ip', box).value)}`);
      $('#ir', box).innerHTML = hits.length ? hits.map((h, i) => `<div class="result" data-i="${i}">
          ${h.coverUrl ? `<img src="${esc(h.coverUrl)}">` : '<img>'}
          <div><div>${esc(h.name)} ${h.year ? `<span class="muted">(${h.year})</span>` : ''}</div>
          <div class="muted small-text">IGDB ${h.igdbId} · ${esc(h.platforms.join(', '))}</div></div></div>`).join('') : '<div class="muted">No results</div>';
      $$('.result', box).forEach((el) => (el.onclick = () => {
        $$('.result', box).forEach((x) => x.classList.remove('sel'));
        el.classList.add('sel');
        selected = hits[Number(el.dataset.i)];
        $('#iname', box).value = selected.name;
        $('#ok', box).disabled = false;
      }));
    });
    $('#is', box).onclick = search;
    $('#iq', box).onkeydown = (e) => e.key === 'Enter' && search();
    $('#ic', box).onclick = closeModal;
    $('#ok', box).onclick = guard(async () => {
      $('#ok', box).disabled = true;
      try {
        await onPick(selected, $('#iname', box).value.trim() || selected.name);
        closeModal();
      } finally {
        $('#ok', box).disabled = false;
      }
    });
    search();
  }

  const EMU_TEMPLATE = `# How the Nostalgia client starts this emulator and where it keeps its saves.
name: {name}
# Launchable file extensions, in priority order (cue is preferred over bin, ...)
extensions: [m3u, chd, cue, iso, bin]
systems:
  windows:
    executable: emulator.exe
    arguments: ["-fullscreen", "{rom}"]
    saves:
      - "%USERPROFILE%/Documents/Emulator/saves"
  linux:
    executable: emulator.AppImage
    arguments: ["-fullscreen", "{rom}"]
    saves:
      - "~/.config/emulator/saves"
`;
  const EMU_HELP = `Placeholders:
  {rom}          absolute path of the selected game file / disc
  {romDir}       folder of that file
  {gameDir}      installed game folder
  {emulatorDir}  installed emulator folder
  {home}         user home folder
Environment variables: %USERPROFILE%, %APPDATA%, $HOME, \${XDG_CONFIG_HOME}, ~
arguments may be a list or a single string ("-nogui -fullscreen \\"{rom}\\"").
saves: one or more folders - their content is uploaded as a versioned cloud save
       after the emulator closes (only if something changed).
Optional per system: workingDirectory, env (map).`;

  async function go(page) {
    current = page;
    location.hash = page;
    $$('#nav a').forEach((a) => a.classList.toggle('active', a.dataset.page === page));
    const main = $('#main');
    main.innerHTML = '<div class="muted">Loading…</div>';
    try {
      const onJobs = await pages[page](main);
      clearInterval(pollTimer);
      pollTimer = setInterval(async () => {
        const j = await pollJobs();
        if (typeof onJobs === 'function' && current === page) onJobs(j);
      }, 2000);
      pollJobs();
    } catch (e) {
      main.innerHTML = `<div class="error">${esc(e.message)}</div>`;
    }
  }

  $('#nav').addEventListener('click', (e) => {
    const a = e.target.closest('a');
    if (!a) return;
    e.preventDefault();
    go(a.dataset.page);
  });

  async function start() {
    try {
      platforms = await api('/platforms');
    } catch {
      return showLogin();
    }
    $('#login').classList.add('hidden');
    $('#app').classList.remove('hidden');
    const page = location.hash.slice(1);
    go(pages[page] ? page : 'dashboard');
  }

  start();
})();
