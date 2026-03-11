/* AgentHALO Containers */
'use strict';

(function() {
  const state = {
    sessions: [],
    selectedSessionId: null,
  };

  function esc(v) {
    const s = String(v == null ? '' : v);
    return s
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }

  async function apiGet(path) {
    if (typeof window.api === 'function') return window.api(path);
    const res = await fetch('/api' + path);
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.error || ('HTTP ' + res.status));
    return body;
  }

  async function apiPost(path, payload) {
    if (typeof window.apiPost === 'function') return window.apiPost(path, payload);
    const res = await fetch('/api' + path, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload || {}),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.error || ('HTTP ' + res.status));
    return body;
  }

  async function apiDelete(path) {
    const res = await fetch('/api' + path, { method: 'DELETE' });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.error || ('HTTP ' + res.status));
    return body;
  }

  function selectedSession() {
    return state.sessions.find((session) => session.session_id === state.selectedSessionId) || null;
  }

  function currentReusePolicy() {
    const el = document.querySelector('#containers-reuse-policy');
    return String(el?.value || 'reusable');
  }

  function buildHookupPayload() {
    const kind = String(document.querySelector('#containers-hookup-kind')?.value || 'cli');
    if (kind === 'api') {
      return {
        kind: 'api',
        provider: String(document.querySelector('#containers-provider')?.value || 'openrouter'),
        model: String(document.querySelector('#containers-model')?.value || '').trim(),
        api_key_source: String(document.querySelector('#containers-api-key-source')?.value || 'vault:openrouter').trim(),
      };
    }
    if (kind === 'local_model') {
      return {
        kind: 'local_model',
        model_id: String(document.querySelector('#containers-model')?.value || '').trim(),
      };
    }
    return {
      kind: 'cli',
      cli_name: String(document.querySelector('#containers-cli-name')?.value || 'codex').trim(),
      model: String(document.querySelector('#containers-model')?.value || '').trim() || null,
    };
  }

  function badge(text, cls) {
    return `<span class="badge ${cls || ''}">${esc(text)}</span>`;
  }

  function renderTableRows() {
    if (!state.sessions.length) {
      return '<tr><td colspan="8" class="muted">No tracked containers yet.</td></tr>';
    }
    return state.sessions.map((session) => {
      const selected = session.session_id === state.selectedSessionId ? ' style="background:rgba(255,255,255,0.04)"' : '';
      return `
        <tr${selected}>
          <td><code>${esc(session.session_id || '')}</code></td>
          <td><code>${esc(session.container_id || '')}</code></td>
          <td>${esc(session.agent_id || '')}</td>
          <td>${session.lock_state ? badge(session.lock_state, session.lock_state === 'locked' ? 'badge-ok' : '') : badge('unknown', 'badge-muted')}</td>
          <td>${esc(session.reuse_policy || '-')}</td>
          <td>${session.mesh_port ? esc(String(session.mesh_port)) : '-'}</td>
          <td>${new Date((Number(session.started_at_unix || 0)) * 1000).toLocaleString()}</td>
          <td>
            <button class="btn btn-sm" data-action="select" data-session-id="${esc(session.session_id || '')}">Details</button>
            <button class="btn btn-sm" data-action="initialize" data-session-id="${esc(session.session_id || '')}">Initialize</button>
            <button class="btn btn-sm" data-action="deinitialize" data-session-id="${esc(session.session_id || '')}">Deinit</button>
            <button class="btn btn-sm" data-action="destroy" data-session-id="${esc(session.session_id || '')}">Destroy</button>
          </td>
        </tr>
      `;
    }).join('');
  }

  async function fetchLogs(sessionId) {
    if (!sessionId) return '';
    try {
      const result = await apiGet(`/containers/${encodeURIComponent(sessionId)}/logs`);
      return String(result.logs || '');
    } catch (err) {
      return `Unable to load logs: ${String(err?.message || err || 'unknown error')}`;
    }
  }

  async function renderContainers() {
    const host = document.querySelector('#content');
    host.innerHTML = '<div class="loading">Loading containers...</div>';
    try {
      const persistedSelected = localStorage.getItem('containers_selected_session');
      if (persistedSelected) {
        try {
          state.selectedSessionId = JSON.parse(persistedSelected);
        } catch (_e) {
          state.selectedSessionId = persistedSelected;
        }
      }
      const list = await apiGet('/containers');
      state.sessions = Array.isArray(list.sessions) ? list.sessions : [];
      if (!state.selectedSessionId && state.sessions.length) {
        state.selectedSessionId = state.sessions[0].session_id;
      }
      const selected = selectedSession();
      const logs = await fetchLogs(selected?.session_id);
      host.innerHTML = `
        <div class="page-header">
          <h1>Containers</h1>
          <p class="subtitle">Provision EMPTY runtimes, initialize agent hookups, and manage lock state.</p>
        </div>

        <section class="card" style="margin-bottom:16px">
          <h3>Provision Container</h3>
          <div class="orchestrator-form" style="display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:10px">
            <label>Image <input id="containers-image" class="input" value="${esc(localStorage.getItem('containers_image') || 'agenthalo:latest')}"></label>
            <label>Agent ID <input id="containers-agent-id" class="input" placeholder="container-agent"></label>
            <label>Admission
              <select id="containers-admission-mode" class="input">
                <option value="warn">warn</option>
                <option value="block">block</option>
                <option value="force">force</option>
              </select>
            </label>
          </div>
          <div style="margin-top:12px">
            <button class="btn btn-primary" id="containers-provision-btn">Provision EMPTY Container</button>
          </div>
        </section>

        <section class="card" style="margin-bottom:16px">
          <h3>Container List</h3>
          <div class="table-wrap">
            <table>
              <thead>
                <tr><th>Session</th><th>Container</th><th>Agent ID</th><th>Lock</th><th>Reuse</th><th>Mesh Port</th><th>Started</th><th>Actions</th></tr>
              </thead>
              <tbody id="containers-table-body">${renderTableRows()}</tbody>
            </table>
          </div>
        </section>

        <section class="card">
          <h3>Detail</h3>
          ${selected ? `
            <div class="orchestrator-form" style="display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:10px">
              <label>Hookup Kind
                <select id="containers-hookup-kind" class="input">
                  <option value="cli">CLI</option>
                  <option value="api">API</option>
                  <option value="local_model">Local Model</option>
                </select>
              </label>
              <label>CLI Name <input id="containers-cli-name" class="input" value="codex"></label>
              <label>Provider <input id="containers-provider" class="input" value="openrouter"></label>
              <label>Model / Repo ID <input id="containers-model" class="input" placeholder="claude-opus-4-6 or Qwen/Qwen2.5-Coder-7B"></label>
              <label>API Key Source <input id="containers-api-key-source" class="input" value="vault:openrouter"></label>
              <label>Reuse Policy
                <select id="containers-reuse-policy" class="input">
                  <option value="reusable">reusable</option>
                  <option value="single_use">single_use</option>
                </select>
              </label>
            </div>
            <div style="margin-top:12px;display:flex;gap:8px;flex-wrap:wrap">
              <button class="btn btn-primary" id="containers-init-selected">Initialize Selected</button>
              <button class="btn" id="containers-deinit-selected">Deinitialize Selected</button>
              <button class="btn" id="containers-destroy-selected">Destroy Selected</button>
            </div>
            <div style="margin-top:14px" class="table-wrap">
              <table>
                <tbody>
                  <tr><th>Session</th><td><code>${esc(selected.session_id)}</code></td></tr>
                  <tr><th>Container</th><td><code>${esc(selected.container_id || '')}</code></td></tr>
                  <tr><th>Agent</th><td>${esc(selected.agent_id || '')}</td></tr>
                  <tr><th>Lock</th><td>${esc(selected.lock_state || 'unknown')}</td></tr>
                  <tr><th>Reuse</th><td>${esc(selected.reuse_policy || '-')}</td></tr>
                </tbody>
              </table>
            </div>
            <h4 style="margin-top:14px">Logs</h4>
            <pre class="network-briefing" style="max-height:300px;overflow:auto">${esc(logs || 'No logs yet.')}</pre>
          ` : '<div class="muted">Select a container to inspect it.</div>'}
          <div id="containers-msg" class="networking-msg" style="margin-top:12px"></div>
        </section>
      `;

      document.querySelector('#containers-admission-mode').value = localStorage.getItem('containers_admission_mode') || 'warn';
      document.querySelector('#containers-provision-btn')?.addEventListener('click', async () => {
        try {
          const image = String(document.querySelector('#containers-image')?.value || 'agenthalo:latest').trim();
          const agentId = String(document.querySelector('#containers-agent-id')?.value || '').trim();
          const admissionMode = String(document.querySelector('#containers-admission-mode')?.value || 'warn');
          localStorage.setItem('containers_image', image);
          localStorage.setItem('containers_admission_mode', admissionMode);
          const result = await apiPost('/containers/provision', {
            image,
            agent_id: agentId || null,
            admission_mode: admissionMode,
          });
          state.selectedSessionId = result.session_id || null;
          localStorage.setItem('containers_selected_session', JSON.stringify(state.selectedSessionId));
          await renderContainers();
        } catch (err) {
          const msg = document.querySelector('#containers-msg');
          if (msg) msg.textContent = String(err?.message || err || 'provision failed');
        }
      });

      document.querySelectorAll('button[data-action]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const sessionId = String(btn.dataset.sessionId || '');
          const action = String(btn.dataset.action || '');
          state.selectedSessionId = sessionId;
          localStorage.setItem('containers_selected_session', JSON.stringify(sessionId));
          if (action === 'select') {
            await renderContainers();
            return;
          }
          try {
            if (action === 'initialize') {
              await apiPost('/containers/initialize', {
                session_id: sessionId,
                hookup: buildHookupPayload(),
                reuse_policy: currentReusePolicy(),
              });
            } else if (action === 'deinitialize') {
              await apiPost('/containers/deinitialize', { session_id: sessionId });
            } else if (action === 'destroy') {
              if (!window.confirm(`Destroy container session ${sessionId}?`)) return;
              await apiDelete(`/containers/${encodeURIComponent(sessionId)}`);
              if (state.selectedSessionId === sessionId) state.selectedSessionId = null;
              localStorage.removeItem('containers_selected_session');
            }
            await renderContainers();
          } catch (err) {
            const msg = document.querySelector('#containers-msg');
            if (msg) msg.textContent = String(err?.message || err || `${action} failed`);
          }
        });
      });

      document.querySelector('#containers-init-selected')?.addEventListener('click', async () => {
        const selectedNow = selectedSession();
        if (!selectedNow) return;
        try {
          await apiPost('/containers/initialize', {
            session_id: selectedNow.session_id,
            hookup: buildHookupPayload(),
            reuse_policy: currentReusePolicy(),
          });
          await renderContainers();
        } catch (err) {
          const msg = document.querySelector('#containers-msg');
          if (msg) msg.textContent = String(err?.message || err || 'initialize failed');
        }
      });

      document.querySelector('#containers-deinit-selected')?.addEventListener('click', async () => {
        const selectedNow = selectedSession();
        if (!selectedNow) return;
        try {
          await apiPost('/containers/deinitialize', { session_id: selectedNow.session_id });
          await renderContainers();
        } catch (err) {
          const msg = document.querySelector('#containers-msg');
          if (msg) msg.textContent = String(err?.message || err || 'deinitialize failed');
        }
      });

      document.querySelector('#containers-destroy-selected')?.addEventListener('click', async () => {
        const selectedNow = selectedSession();
        if (!selectedNow) return;
        if (!window.confirm(`Destroy container session ${selectedNow.session_id}?`)) return;
        try {
          await apiDelete(`/containers/${encodeURIComponent(selectedNow.session_id)}`);
          state.selectedSessionId = null;
          localStorage.removeItem('containers_selected_session');
          await renderContainers();
        } catch (err) {
          const msg = document.querySelector('#containers-msg');
          if (msg) msg.textContent = String(err?.message || err || 'destroy failed');
        }
      });
    } catch (err) {
      host.innerHTML = `<div class="loading">Container dashboard unavailable: ${esc(String(err?.message || err || 'unknown error'))}</div>`;
    }
  }

  window.renderContainers = renderContainers;
})();
