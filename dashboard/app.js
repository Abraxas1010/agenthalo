/* Agent H.A.L.O. Dashboard — Fallout Terminal Theme */
'use strict';

const $ = (sel, ctx) => (ctx || document).querySelector(sel);
const $$ = (sel, ctx) => [...(ctx || document).querySelectorAll(sel)];
const content = $('#content');
const PROVIDER_INFO = {
  openrouter: {
    name: 'OpenRouter (Required)',
    envVar: 'OPENROUTER_API_KEY',
    keyUrl: 'https://openrouter.ai/settings/keys',
    category: 'llm',
    required: true,
    description: 'Sole LLM inference upstream — all model requests route through OpenRouter.',
  },
  anthropic: {
    name: 'Anthropic (Direct)',
    envVar: 'ANTHROPIC_API_KEY',
    keyUrl: 'https://console.anthropic.com/settings/keys',
    category: 'llm',
    required: false,
    description: 'Optional direct access (operator only). Customer traffic uses OpenRouter.',
  },
  openai: {
    name: 'OpenAI (Direct)',
    envVar: 'OPENAI_API_KEY',
    keyUrl: 'https://platform.openai.com/api-keys',
    category: 'llm',
    required: false,
    description: 'Optional direct access (operator only). Customer traffic uses OpenRouter.',
  },
  google: {
    name: 'Google AI (Direct)',
    envVar: 'GOOGLE_API_KEY',
    keyUrl: 'https://aistudio.google.com/app/apikey',
    category: 'llm',
    required: false,
    description: 'Optional direct access (operator only). Customer traffic uses OpenRouter.',
  },
  pinata: {
    name: 'Pinata (IPFS Storage)',
    envVar: 'PINATA_JWT',
    keyUrl: 'https://app.pinata.cloud/developers/api-keys',
    category: 'storage',
    required: false,
    description: 'Immutable 3rd-party storage. Customer IPFS pins route through your Pinata account.',
  },
  agentpmt: {
    name: 'AgentPMT (Tool Proxy)',
    envVar: 'AGENTPMT_API_KEY',
    keyUrl: 'https://www.agentpmt.com',
    category: 'tooling',
    required: false,
    description: 'Third-party MCP tool routing. Required for live agentpmt/* tool execution.',
  },
};

// -- Routing ------------------------------------------------------------------
const pages = { overview: renderOverview, sessions: renderSessions, costs: renderCosts,
  config: renderConfig, setup: renderSetup, trust: renderTrust, nucleusdb: renderNucleusDB,
  cockpit: renderCockpit, deploy: renderDeploy };

// Setup-first gate: cached setup state
let _setupState = null;
let _setupStateFetchedAt = 0;
let _lastSetupComplete = null;
const SETUP_CACHE_MS = 5000;

async function fetchSetupState(force) {
  const now = Date.now();
  if (!force && _setupState && (now - _setupStateFetchedAt) < SETUP_CACHE_MS) return _setupState;
  try {
    const cfg = await api('/config');
    _setupState = cfg.setup_complete || { identity: false, agentpmt: false, llm: false, complete: false };
    _setupStateFetchedAt = now;
  } catch (_e) {
    // Fail closed: keep users in setup if we cannot verify state.
    _setupState = { identity: false, agentpmt: false, llm: false, complete: false };
    _setupStateFetchedAt = now;
  }
  updateNavLockState();
  return _setupState;
}

function updateNavLockState() {
  if (!_setupState) return;
  const complete = _setupState.complete;
  const justUnlocked = _lastSetupComplete === false && complete === true;
  $$('.nav-link').forEach(a => {
    const page = a.dataset.page;
    if (page === 'setup') {
      a.classList.remove('nav-locked');
      a.classList.toggle('setup-incomplete', !complete);
      a.classList.remove('nav-unlocked');
    } else {
      a.classList.toggle('nav-locked', !complete);
      a.classList.remove('setup-incomplete');
      if (!complete) a.classList.remove('nav-unlocked');
    }
  });

  if (justUnlocked) {
    $$('.nav-link').forEach(a => {
      if (a.dataset.page !== 'setup') a.classList.add('nav-unlocked');
    });
    setTimeout(() => {
      $$('.nav-link.nav-unlocked').forEach(a => a.classList.remove('nav-unlocked'));
    }, 900);
  }

  // Update progress indicator
  const prog = document.getElementById('setup-progress');
  if (prog) {
    if (complete) {
      prog.style.display = 'none';
    } else {
      const steps = [_setupState.identity, _setupState.agentpmt, _setupState.llm];
      const done = steps.filter(Boolean).length;
      prog.style.display = 'block';
      prog.innerHTML = `Setup: ${done}/${steps.length}
        <div class="progress-bar"><div class="progress-fill" style="width:${Math.round(done/steps.length*100)}%"></div></div>`;
    }
  }
  _lastSetupComplete = complete;
}

// Invalidate setup cache (called after setup actions)
window._invalidateSetupState = function() {
  _setupState = null;
  _setupStateFetchedAt = 0;
};

async function route() {
  // Clean up particle animation when leaving NucleusDB page
  if (window._destroyHeroParticles) window._destroyHeroParticles();
  const hash = location.hash.replace('#/', '') || 'setup';
  const page = hash.split('/')[0];
  const arg = hash.split('/').slice(1).join('/');

  // Fetch setup state and gate navigation
  const ss = await fetchSetupState();
  if (!ss.complete && page !== 'setup') {
    location.hash = '#/setup';
    return;
  }

  $$('.nav-link').forEach(a => a.classList.toggle('active', a.dataset.page === page));
  if (pages[page]) pages[page](arg);
  else content.innerHTML = '<div class="loading">Page not found</div>';
}

window.addEventListener('hashchange', route);
window.addEventListener('DOMContentLoaded', route);

// -- CRT Effects Toggle -------------------------------------------------------
function toggleCRT() {
  document.body.classList.toggle('no-crt');
  const on = !document.body.classList.contains('no-crt');
  localStorage.setItem('crt', on ? 'on' : 'off');
  const btn = $('#crt-toggle');
  if (btn) {
    btn.textContent = on ? 'CRT' : 'CRT:OFF';
    btn.classList.toggle('crt-on', on);
  }
}
window.toggleCRT = toggleCRT;

// Restore CRT preference
if (localStorage.getItem('crt') === 'off') {
  document.body.classList.add('no-crt');
  const btn = document.getElementById('crt-toggle');
  if (btn) btn.textContent = 'CRT:OFF';
} else {
  const btn = document.getElementById('crt-toggle');
  if (btn) btn.classList.add('crt-on');
}

// -- API helpers --------------------------------------------------------------
async function api(path) {
  const res = await fetch('/api' + path);
  if (!res.ok) throw await toApiError(res, path);
  return res.json();
}

async function apiPost(path, body) {
  const res = await fetch('/api' + path, {
    method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify(body)
  });
  if (!res.ok) throw await toApiError(res, path);
  return res.json();
}

async function apiDelete(path) {
  const res = await fetch('/api' + path, { method: 'DELETE' });
  if (!res.ok) throw await toApiError(res, path);
  return res.json();
}

async function toApiError(res, path) {
  const raw = await res.text();
  let body = null;
  try { body = raw ? JSON.parse(raw) : null; } catch (_e) {}
  const message = (body && body.error) || raw || `API error: ${res.status}`;
  const err = new Error(message);
  err.status = res.status;
  err.path = path;
  err.body = body;
  return err;
}

// -- HTML escaping (XSS prevention) -------------------------------------------
function esc(s) {
  if (s == null) return '';
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;');
}
window.__escapeHtml = esc;
window.__providerInfo = PROVIDER_INFO;

function parseProviderList(v) {
  if (Array.isArray(v)) {
    return [...new Set(v.map(x => String(x || '').trim().toLowerCase()).filter(Boolean))];
  }
  if (typeof v === 'string') {
    return [...new Set(v.split(',').map(x => x.trim().toLowerCase()).filter(Boolean))];
  }
  return [];
}

window.openSetupGuide = function openSetupGuide(context) {
  const payload = Object.assign({ ts: Date.now() }, context || {});
  localStorage.setItem('halo_setup_context', JSON.stringify(payload));
  location.hash = '#/setup';
};

function consumeSetupContext() {
  const raw = localStorage.getItem('halo_setup_context');
  if (!raw) return {};
  localStorage.removeItem('halo_setup_context');
  try { return JSON.parse(raw) || {}; } catch (_e) { return {}; }
}

window.copySetupText = async function copySetupText(value) {
  try {
    await navigator.clipboard.writeText(String(value || ''));
    alert('Copied to clipboard');
  } catch (_e) {
    alert('Copy failed. Please copy manually.');
  }
};

window.openSetupProviderConfig = function openSetupProviderConfig(provider) {
  localStorage.setItem('halo_setup_open_provider', String(provider || '').toLowerCase());
  location.hash = '#/config';
};

window.trySetupRedirect = function trySetupRedirect(err, agent, from) {
  const message = String(err && err.message || '');
  const lower = message.toLowerCase();
  const status = Number(err && err.status);
  const body = (err && err.body && typeof err.body === 'object') ? err.body : null;

  let reason = null;
  let providers = [];
  if (body && body.code === 'auth_required') {
    reason = 'auth_required';
  } else if (status === 401 || lower.includes('authentication required') || lower.includes('agenthalo_api_key')) {
    reason = 'auth_required';
  } else if (body && Array.isArray(body.missing_keys) && body.missing_keys.length > 0) {
    reason = 'provider_keys_missing';
    providers = parseProviderList(body.missing_keys);
  } else {
    const match = message.match(/missing API keys?:\s*(.+)$/i);
    if (match) {
      reason = 'provider_keys_missing';
      providers = parseProviderList(match[1]);
    }
  }

  if (!reason) return false;
  const context = {
    reason,
    from: from || 'dashboard',
    agent: agent || null,
    providers,
  };

  if (typeof window.openSetupGuide === 'function') {
    window.openSetupGuide(context);
  } else {
    location.hash = '#/config';
  }
  return true;
};

// -- Format helpers -----------------------------------------------------------
function fmtCost(v) { return '$' + (v || 0).toFixed(2); }
function fmtTokens(v) { return (v || 0).toLocaleString(); }
function fmtDuration(secs) {
  if (!secs) return '0s';
  const h = Math.floor(secs / 3600), m = Math.floor((secs % 3600) / 60), s = secs % 60;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}
function fmtTime(ts) {
  if (!ts) return '';
  return new Date(ts * 1000).toLocaleString();
}
function truncate(s, n) { return s && s.length > n ? s.slice(0, n) + '...' : s; }
function typeBadge(type) {
  return `<span class="type-badge type-${type || 'integer'}">${esc(type || 'integer')}</span>`;
}
function renderTypedValue(row) {
  const type = row.type || 'integer';
  const val = row.value;
  const display = row.display != null ? row.display : String(val);
  switch (type) {
    case 'null':
      return `<span class="ndb-value-display val-null">NULL</span>`;
    case 'bool':
      return `<span class="ndb-value-display val-bool">${val ? 'true' : 'false'}</span>`;
    case 'integer':
    case 'float':
      return `<span class="ndb-value-display">${esc(display)}</span>`;
    case 'text':
      return `<span class="ndb-value-display val-text">"${esc(truncate(display, 60))}"</span>`;
    case 'json': {
      const preview = typeof val === 'object' ? JSON.stringify(val) : display;
      return `<button type="button" class="ndb-value-display val-json ndb-json-toggle" title="Click to expand" data-key="${esc(row.key)}">${esc(truncate(preview, 60))}</button>`;
    }
    case 'vector': {
      const dims = Array.isArray(val) ? val : [];
      return `<span class="ndb-value-display val-vector">[${dims.length}d] ${esc(truncate(display, 50))}</span>`;
    }
    case 'bytes':
      return `<span class="ndb-value-display val-bytes">${esc(truncate(display, 60))}</span>`;
    default:
      return `<span class="ndb-value-display">${esc(truncate(display, 60))}</span>`;
  }
}
function statusBadge(status) {
  const cls = status === 'completed' ? 'badge-ok' : status === 'failed' ? 'badge-err' :
    status === 'running' ? 'badge-info' : 'badge-muted';
  return `<span class="badge ${cls}">${esc(status)}</span>`;
}
function eventTypeBadge(type) {
  const colors = { assistant: '#00ee00', tool_call: '#c49bff', tool_result: '#c49bff',
    mcp_tool_call: '#ffb830', file_change: '#00ff41', bash_command: '#ff8c00',
    error: '#ff3030', thinking: '#3a7a2a' };
  const c = colors[type] || '#3a7a2a';
  return `<span class="event-type" style="background:${c}18;color:${c}">${esc(type)}</span>`;
}
function formatBytes(bytes) {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let val = bytes;
  while (val >= 1024 && i < units.length - 1) { val /= 1024; i++; }
  return val.toFixed(i === 0 ? 0 : 1) + ' ' + units[i];
}

// -- Fallout chart palette ----------------------------------------------------
const chartGreen = '#00ee00';
const chartAmber = '#ffb830';
const chartGreenBg = 'rgba(0, 238, 0, 0.15)';
const chartAmberBg = 'rgba(255, 184, 48, 0.3)';
const chartGridColor = 'rgba(26, 51, 18, 0.6)';
const chartPalette = ['#00ee00', '#ffb830', '#ff8c00', '#c49bff', '#ff3030', '#00ff41'];

function falloutChartDefaults() {
  if (typeof Chart === 'undefined') return;
  Chart.defaults.color = '#3a7a2a';
  Chart.defaults.borderColor = 'rgba(26, 51, 18, 0.4)';
  Chart.defaults.font.family = "'Share Tech Mono', 'Courier New', monospace";
}
falloutChartDefaults();

// -- SSE live updates ---------------------------------------------------------
let evtSource;
function initSSE() {
  if (evtSource) evtSource.close();
  evtSource = new EventSource('/events');
  evtSource.addEventListener('session_update', (e) => {
    const data = JSON.parse(e.data);
    const countEl = $('#live-session-count');
    if (countEl) countEl.textContent = data.session_count;
  });
}
initSSE();

// =============================================================================
// PAGE: Overview
// =============================================================================
async function renderOverview() {
  content.innerHTML = '<div class="loading">Loading...</div>';
  try {
    const [status, sessions, costs] = await Promise.all([
      api('/status'), api('/sessions?limit=5'), api('/costs?monthly=true')
    ]);

    const s = status;
    const recentSessions = (sessions.sessions || []).slice(0, 5);

    content.innerHTML = `
      <div class="page-title">Overview</div>
      <div class="card-grid">
        <div class="card">
          <div class="card-label">Status</div>
          <div class="card-value" style="font-size:14px">${s.authenticated ? '<span class="badge badge-ok">Authenticated</span>' : '<span class="badge badge-warn">Not Auth</span>'}</div>
          <div class="card-sub">x402: ${s.x402_enabled ? 'Enabled' : 'Disabled'} | Proxy: ${s.tool_proxy_enabled ? 'On' : 'Off'}</div>
        </div>
        <div class="card">
          <div class="card-label">Sessions</div>
          <div class="card-value" id="live-session-count">${s.session_count}</div>
          <div class="card-sub">Total recorded sessions</div>
        </div>
        <div class="card">
          <div class="card-label">Total Cost</div>
          <div class="card-value">${fmtCost(s.total_cost_usd)}</div>
          <div class="card-sub">${fmtTokens(s.total_tokens)} tokens</div>
        </div>
        <div class="card">
          <div class="card-label">Agent Wrapping</div>
          <div class="card-value" style="font-size:12px">
            Claude: ${s.wrapping?.claude ? '<span class="badge badge-ok">ON</span>' : '<span class="badge badge-muted">OFF</span>'}
            Codex: ${s.wrapping?.codex ? '<span class="badge badge-ok">ON</span>' : '<span class="badge badge-muted">OFF</span>'}
            Gemini: ${s.wrapping?.gemini ? '<span class="badge badge-ok">ON</span>' : '<span class="badge badge-muted">OFF</span>'}
          </div>
          <div class="card-sub">PQ Wallet: ${s.pq_wallet ? 'Present' : 'Not created'}</div>
        </div>
      </div>

      ${costs.buckets && costs.buckets.length > 0 ? `
        <div class="chart-container">
          <div class="chart-title">Cost Over Time</div>
          <canvas id="cost-chart" height="200"></canvas>
        </div>
      ` : ''}

      <div class="section-header">Recent Sessions</div>
      ${recentSessions.length > 0 ? `
        <div class="table-wrap"><table>
          <thead><tr><th>Session</th><th>Agent</th><th>Model</th><th>Tokens</th><th>Cost</th><th>Duration</th><th>Status</th></tr></thead>
          <tbody>
            ${recentSessions.map(item => {
              const ss = item.session, sm = item.summary || {};
              const tokens = (sm.total_input_tokens || 0) + (sm.total_output_tokens || 0);
              return `<tr class="clickable" onclick="location.hash='#/sessions/${encodeURIComponent(ss.session_id)}'">
                <td style="font-size:11px">${esc(truncate(ss.session_id, 24))}</td>
                <td>${esc(ss.agent)}</td>
                <td>${esc(truncate(ss.model || 'unknown', 20))}</td>
                <td>${fmtTokens(tokens)}</td>
                <td>${fmtCost(sm.estimated_cost_usd)}</td>
                <td>${fmtDuration(sm.duration_secs)}</td>
                <td>${statusBadge(ss.status)}</td>
              </tr>`;
            }).join('')}
          </tbody>
        </table></div>
      ` : '<div style="color:var(--text-muted)">No sessions recorded yet. Run <code style="color:var(--accent)">agenthalo run claude ...</code> to start.</div>'}
    `;

    // Render cost chart
    if (costs.buckets && costs.buckets.length > 0) {
      const ctx = $('#cost-chart');
      if (ctx && typeof Chart !== 'undefined') {
        new Chart(ctx, {
          type: 'bar',
          data: {
            labels: costs.buckets.map(b => b.label),
            datasets: [{
              label: 'Cost (USD)',
              data: costs.buckets.map(b => b.cost_usd),
              backgroundColor: chartAmberBg,
              borderColor: chartAmber,
              borderWidth: 1,
            }]
          },
          options: {
            responsive: true,
            plugins: { legend: { display: false } },
            scales: {
              y: { beginAtZero: true, ticks: { callback: v => '$' + v.toFixed(2) },
                grid: { color: chartGridColor } },
              x: { grid: { display: false } }
            }
          }
        });
      }
    }
  } catch (e) {
    content.innerHTML = `<div class="loading">Error loading dashboard: ${esc(e.message)}</div>`;
  }
}

// =============================================================================
// PAGE: Sessions
// =============================================================================
async function renderSessions(sessionId) {
  if (sessionId) return renderSessionDetail(sessionId);

  content.innerHTML = '<div class="loading">Loading sessions...</div>';
  try {
    const data = await api('/sessions');
    const items = data.sessions || [];

    content.innerHTML = `
      <div class="page-title">Sessions</div>
      <div class="filter-bar">
        <input type="text" id="filter-agent" placeholder="Filter by agent..." oninput="filterSessions()">
        <input type="text" id="filter-model" placeholder="Filter by model..." oninput="filterSessions()">
        <span style="color:var(--text-muted);font-size:12px">${items.length} sessions</span>
      </div>
      <div class="table-wrap"><table>
        <thead><tr><th>Session ID</th><th>Agent</th><th>Model</th><th>Tokens</th><th>Cost</th><th>Duration</th><th>Started</th><th>Status</th></tr></thead>
        <tbody id="sessions-tbody">
          ${items.map(item => sessionRow(item)).join('')}
        </tbody>
      </table></div>
    `;

    window._sessionItems = items;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

function sessionRow(item) {
  const ss = item.session, sm = item.summary || {};
  const tokens = (sm.total_input_tokens || 0) + (sm.total_output_tokens || 0);
  return `<tr class="clickable session-row" data-agent="${esc(ss.agent)}" data-model="${esc(ss.model || '')}"
    onclick="location.hash='#/sessions/${encodeURIComponent(ss.session_id)}'">
    <td style="font-size:11px">${esc(truncate(ss.session_id, 28))}</td>
    <td>${esc(ss.agent)}</td>
    <td>${esc(truncate(ss.model || 'unknown', 22))}</td>
    <td>${fmtTokens(tokens)}</td>
    <td>${fmtCost(sm.estimated_cost_usd)}</td>
    <td>${fmtDuration(sm.duration_secs)}</td>
    <td style="font-size:11px">${fmtTime(ss.started_at)}</td>
    <td>${statusBadge(ss.status)}</td>
  </tr>`;
}

window.filterSessions = function() {
  const agent = ($('#filter-agent')?.value || '').toLowerCase();
  const model = ($('#filter-model')?.value || '').toLowerCase();
  const items = window._sessionItems || [];
  const filtered = items.filter(item => {
    const s = item.session;
    if (agent && !s.agent.toLowerCase().includes(agent)) return false;
    if (model && !(s.model || '').toLowerCase().includes(model)) return false;
    return true;
  });
  const tbody = $('#sessions-tbody');
  if (tbody) tbody.innerHTML = filtered.map(sessionRow).join('');
};

async function renderSessionDetail(id) {
  content.innerHTML = '<div class="loading">Loading session...</div>';
  try {
    const data = await api('/sessions/' + encodeURIComponent(id));
    const ss = data.session, sm = data.summary || {}, events = data.events || [];
    const tokens = (sm.total_input_tokens || 0) + (sm.total_output_tokens || 0);

    content.innerHTML = `
      <a href="#/sessions" class="back-link">&larr; Back to Sessions</a>
      <div class="page-title">${esc(truncate(ss.session_id, 32))} ${statusBadge(ss.status)}</div>

      <div class="card-grid">
        <div class="card">
          <div class="card-label">Agent</div>
          <div class="card-value" style="font-size:16px">${esc(ss.agent)}</div>
          <div class="card-sub">${esc(ss.model || 'unknown')}</div>
        </div>
        <div class="card">
          <div class="card-label">Tokens</div>
          <div class="card-value" style="font-size:16px">${fmtTokens(tokens)}</div>
          <div class="card-sub">In: ${fmtTokens(sm.total_input_tokens)} / Out: ${fmtTokens(sm.total_output_tokens)}</div>
        </div>
        <div class="card">
          <div class="card-label">Cost</div>
          <div class="card-value" style="font-size:16px">${fmtCost(sm.estimated_cost_usd)}</div>
          <div class="card-sub">${fmtDuration(sm.duration_secs)}</div>
        </div>
        <div class="card">
          <div class="card-label">Activity</div>
          <div class="card-value" style="font-size:12px">
            ${sm.tool_calls || 0} tools | ${sm.bash_commands || 0} cmds | ${sm.files_modified || 0} files
          </div>
          <div class="card-sub">MCP: ${sm.mcp_tool_calls || 0} | Errors: ${sm.errors || 0}</div>
        </div>
      </div>

      <div style="margin-bottom:12px;display:flex;gap:8px">
        <button class="btn" data-session-id="${encodeURIComponent(ss.session_id)}" onclick="exportSessionByButton(this)">Export JSON</button>
        <button class="btn btn-primary" data-session-id="${encodeURIComponent(ss.session_id)}" onclick="attestSessionByButton(this)">Attest</button>
      </div>

      <div class="section-header">Event Timeline (${events.length} events)</div>
      <div class="event-timeline">
        ${events.map(ev => `
          <div class="event-item">
            <span class="event-seq">#${ev.seq}</span>
            ${eventTypeBadge(ev.event_type)}
            <span class="event-content">${esc(truncate(JSON.stringify(ev.content), 100))}</span>
            ${ev.input_tokens ? `<span style="color:var(--text-dim);font-size:10px;margin-left:8px">in:${ev.input_tokens} out:${ev.output_tokens || 0}</span>` : ''}
          </div>
        `).join('')}
      </div>
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.exportSession = async function(id) {
  try {
    const data = await api('/sessions/' + encodeURIComponent(id) + '/export');
    const blob = new Blob([JSON.stringify(data, null, 2)], {type: 'application/json'});
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `session_${id}.json`;
    a.click();
  } catch (e) { alert('Export failed: ' + e.message); }
};

window.exportSessionByButton = function(btn) {
  const encoded = btn?.dataset?.sessionId || '';
  if (!encoded) return;
  try {
    exportSession(decodeURIComponent(encoded));
  } catch (_e) {
    alert('Invalid session id');
  }
};

window.attestSession = async function(id) {
  if (!confirm('Create attestation for this session?')) return;
  try {
    const data = await apiPost('/sessions/' + encodeURIComponent(id) + '/attest', {});
    alert('Attestation created!\nDigest: ' + (data.attestation?.attestation_digest || 'unknown'));
  } catch (e) { alert('Attestation failed: ' + e.message); }
};

window.attestSessionByButton = function(btn) {
  const encoded = btn?.dataset?.sessionId || '';
  if (!encoded) return;
  try {
    attestSession(decodeURIComponent(encoded));
  } catch (_e) {
    alert('Invalid session id');
  }
};

// =============================================================================
// PAGE: Costs & Analytics
// =============================================================================
async function renderCosts() {
  content.innerHTML = '<div class="loading">Loading costs...</div>';
  try {
    const [daily, byAgent, byModel, paid] = await Promise.all([
      api('/costs/daily'), api('/costs/by-agent'), api('/costs/by-model'), api('/costs/paid')
    ]);

    const dailyItems = daily.daily || [];
    const totalCost = dailyItems.reduce((sum, d) => sum + d.cost_usd, 0);

    content.innerHTML = `
      <div class="page-title">Costs &amp; Analytics</div>

      <div class="card-grid">
        <div class="card">
          <div class="card-label">Total Spend</div>
          <div class="card-value">${fmtCost(totalCost)}</div>
        </div>
        <div class="card">
          <div class="card-label">Sessions</div>
          <div class="card-value">${dailyItems.reduce((s, d) => s + d.sessions, 0)}</div>
        </div>
        <div class="card">
          <div class="card-label">Total Tokens</div>
          <div class="card-value">${fmtTokens(dailyItems.reduce((s, d) => s + d.tokens, 0))}</div>
        </div>
      </div>

      <div class="chart-row">
        <div class="chart-container">
          <div class="chart-title">Daily Cost</div>
          <canvas id="daily-cost-chart" height="250"></canvas>
        </div>
        <div class="chart-container">
          <div class="chart-title">Cost by Agent</div>
          <canvas id="agent-cost-chart" height="250"></canvas>
        </div>
      </div>

      <div class="chart-container">
        <div class="chart-title">Cost by Model</div>
        <canvas id="model-cost-chart" height="200"></canvas>
      </div>

      ${(paid.by_type || []).length > 0 ? `
        <div class="section-header">Paid Operations</div>
        <div class="table-wrap"><table>
          <thead><tr><th>Operation</th><th>Count</th><th>Credits</th><th>USD</th></tr></thead>
          <tbody>
            ${(paid.by_type || []).map(op => `
              <tr><td>${esc(op.operation)}</td><td>${op.count}</td><td>${fmtTokens(op.credits_spent)}</td><td>${fmtCost(op.usd_spent)}</td></tr>
            `).join('')}
          </tbody>
        </table></div>
      ` : ''}
    `;

    if (typeof Chart === 'undefined') return;

    // Daily cost line chart
    if (dailyItems.length > 0) {
      new Chart($('#daily-cost-chart'), {
        type: 'line',
        data: {
          labels: dailyItems.map(d => d.date),
          datasets: [{
            label: 'Cost (USD)',
            data: dailyItems.map(d => d.cost_usd),
            borderColor: chartGreen, backgroundColor: chartGreenBg,
            fill: true, tension: 0.3, pointRadius: 3,
          }]
        },
        options: {
          responsive: true,
          plugins: { legend: { display: false } },
          scales: {
            y: { beginAtZero: true, ticks: { callback: v => '$' + v.toFixed(2) },
              grid: { color: chartGridColor } },
            x: { grid: { display: false } }
          }
        }
      });
    }

    // Agent pie chart
    const agents = byAgent.by_agent || [];
    if (agents.length > 0) {
      new Chart($('#agent-cost-chart'), {
        type: 'doughnut',
        data: {
          labels: agents.map(a => a.agent),
          datasets: [{ data: agents.map(a => a.cost_usd), backgroundColor: chartPalette }]
        },
        options: { responsive: true, plugins: { legend: { position: 'bottom' } } }
      });
    }

    // Model bar chart
    const models = byModel.by_model || [];
    if (models.length > 0) {
      new Chart($('#model-cost-chart'), {
        type: 'bar',
        data: {
          labels: models.map(m => m.model),
          datasets: [{ label: 'Cost (USD)', data: models.map(m => m.cost_usd),
            backgroundColor: chartAmberBg, borderColor: chartAmber, borderWidth: 1 }]
        },
        options: {
          responsive: true, indexAxis: 'y',
          plugins: { legend: { display: false } },
          scales: {
            x: { beginAtZero: true, ticks: { callback: v => '$' + v.toFixed(2) },
              grid: { color: chartGridColor } },
            y: { grid: { display: false } }
          }
        }
      });
    }
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

// =============================================================================
// PAGE: Configuration
// =============================================================================
async function renderConfig() {
  content.innerHTML = '<div class="loading">Loading config...</div>';
  try {
    const cfg = await api('/config');
    let vaultResp = { keys: [] };
    let vaultKeysAuthRequired = false;
    try {
      vaultResp = await api('/vault/keys');
    } catch (e) {
      vaultKeysAuthRequired = Number(e && e.status) === 401;
    }
    const vaultKeys = vaultResp.keys || [];
    let pmtToolsResp = { count: 0, tools: [] };
    let pmtToolsError = '';
    try {
      pmtToolsResp = await api('/agentpmt/tools');
    } catch (e) {
      pmtToolsError = String(e && e.message || 'failed to load AgentPMT tool catalog');
    }

    content.innerHTML = `
      <div class="page-title">Configuration</div>

      <div class="section-header">Authentication</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">Status</div>
            <div class="config-desc">OAuth or API key authentication</div>
          </div>
          ${cfg.authentication.authenticated
            ? '<span class="badge badge-ok">Authenticated</span>'
            : '<span class="badge badge-warn">Not Authenticated</span>'}
        </div>
      </div>

      <div class="section-header">Agent Wrapping</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        ${['claude', 'codex', 'gemini'].map(agent => `
          <div class="config-row">
            <div>
              <div class="config-label">${agent.charAt(0).toUpperCase() + agent.slice(1)}</div>
              <div class="config-desc">Wrap ${agent} commands through H.A.L.O.</div>
            </div>
            <button class="toggle ${cfg.wrapping[agent] ? 'on' : ''}"
              onclick="toggleWrap('${agent}', ${!cfg.wrapping[agent]})"></button>
          </div>
        `).join('')}
        <div class="config-row">
          <div class="config-desc">Shell RC: ${esc(cfg.wrapping.shell_rc)}</div>
        </div>
      </div>

      <div class="section-header">x402 Payments</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">x402direct Integration</div>
            <div class="config-desc">Stablecoin payments for AI agents</div>
          </div>
          <button class="toggle ${cfg.x402.enabled ? 'on' : ''}"
            onclick="toggleX402(${!cfg.x402.enabled})"></button>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Network</div>
            <div class="config-desc">${cfg.x402.network}</div>
          </div>
          <span class="badge badge-info">${cfg.x402.network}</span>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Max Auto-Approve</div>
            <div class="config-desc">${fmtCost(cfg.x402.max_auto_approve_usd)} USDC</div>
          </div>
        </div>
      </div>

      <div class="section-header">AgentPMT</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">Tool Proxy</div>
            <div class="config-desc">Third-party tool access via AgentPMT</div>
          </div>
          <span class="badge ${cfg.agentpmt.enabled ? 'badge-ok' : 'badge-muted'}">
            ${cfg.agentpmt.enabled ? 'Enabled' : 'Disabled'}</span>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Budget Tag</div>
            <div class="config-desc">${esc(cfg.agentpmt.budget_tag || '(none)')}</div>
          </div>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">MCP Endpoint</div>
            <div class="config-desc" style="font-size:10px">${esc(cfg.agentpmt.endpoint || '(default)')}</div>
          </div>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Credential Status</div>
            <div class="config-desc">${cfg.agentpmt.auth_configured ? 'Configured' : 'Missing'}</div>
          </div>
          <span class="badge ${cfg.agentpmt.auth_configured ? 'badge-ok' : 'badge-warn'}">
            ${cfg.agentpmt.auth_configured ? 'Ready' : 'Needs Key'}</span>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Tool Catalog</div>
            <div class="config-desc">
              ${Number(pmtToolsResp.count || 0)} tools discovered
              (${esc(String(pmtToolsResp.source || 'cache'))}${pmtToolsResp.stale ? ', stale' : ', fresh'})
            </div>
            ${pmtToolsResp.refresh_attempted
              ? `<div class="config-desc" style="font-size:10px">Live refresh attempted this request</div>`
              : ''}
            ${pmtToolsError ? `<div class="config-desc" style="color:var(--danger);font-size:10px">Catalog error: ${esc(pmtToolsError)}</div>` : ''}
          </div>
          <div style="display:flex;gap:6px;align-items:center">
            <button class="btn btn-sm" onclick="refreshAgentPmtCatalog()">Refresh</button>
          </div>
        </div>
        ${Array.isArray(pmtToolsResp.tools) && pmtToolsResp.tools.length ? `
          <div class="config-row">
            <div>
              <div class="config-label">Tools</div>
              <div class="config-desc" style="font-size:10px">
                ${pmtToolsResp.tools.slice(0, 8).map(t => esc(String(t.name || ''))).join(', ')}
                ${pmtToolsResp.tools.length > 8 ? ` ... (+${pmtToolsResp.tools.length - 8} more)` : ''}
              </div>
            </div>
          </div>
        ` : ''}
      </div>

      <div class="section-header">On-Chain</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">Chain</div>
            <div class="config-desc">${esc(cfg.onchain.chain_name || 'Not configured')} (ID: ${esc(cfg.onchain.chain_id)})</div>
          </div>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Contract</div>
            <div class="config-desc" style="font-size:10px">${esc(cfg.onchain.contract_address || '(not deployed)')}</div>
          </div>
        </div>
      </div>

      <div class="section-header">Add-ons</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">p2pclaw</div>
            <div class="config-desc">Marketplace integration</div>
          </div>
          <span class="badge ${cfg.addons.p2pclaw ? 'badge-ok' : 'badge-muted'}">
            ${cfg.addons.p2pclaw ? 'Enabled' : 'Disabled'}</span>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">AgentPMT Workflows</div>
            <div class="config-desc">Challenge and workflow extensions</div>
          </div>
          <span class="badge ${cfg.addons.agentpmt_workflows ? 'badge-ok' : 'badge-muted'}">
            ${cfg.addons.agentpmt_workflows ? 'Enabled' : 'Disabled'}</span>
        </div>
      </div>

      <div class="section-header">API Keys &amp; Services</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        ${cfg.vault?.available ? `
          ${vaultKeysAuthRequired ? `
            <div class="config-row">
              <div>
                <div class="config-label">Authentication required</div>
                <div class="config-desc">Unlock sensitive controls first, then add provider API keys.</div>
              </div>
              <button class="btn btn-sm btn-primary" onclick="location.hash='#/setup'">Open Setup</button>
            </div>
          ` : ''}
          ${vaultKeys.map(k => {
            const pi = PROVIDER_INFO[String(k.provider || '').toLowerCase()] || {};
            const isRequired = pi.required;
            const desc = pi.description || '';
            const catLabel = pi.category === 'storage'
              ? 'Storage'
              : pi.category === 'llm'
                ? 'LLM'
                : pi.category === 'tooling'
                  ? 'Tooling'
                  : '';
            return `
            <div class="config-row">
              <div>
                <div class="config-label">
                  ${esc(pi.name || k.provider)}
                  ${isRequired ? '<span class="badge badge-warn" style="font-size:9px;margin-left:6px">REQUIRED</span>' : ''}
                  ${catLabel ? '<span class="badge badge-info" style="font-size:9px;margin-left:4px">' + esc(catLabel) + '</span>' : ''}
                </div>
                <div class="config-desc">${esc(k.env_var)} · ${k.configured ? 'Configured' : 'Missing'}${k.tested ? ' · Tested' : ''}</div>
                ${desc ? '<div class="config-desc" style="font-size:10px;margin-top:2px">' + esc(desc) + '</div>' : ''}
              </div>
              <div style="display:flex;gap:6px;align-items:center">
                <button class="btn btn-sm" onclick="vaultSetKey('${esc(k.provider)}','${esc(k.env_var)}')">Set Key</button>
                <button class="btn btn-sm" onclick="vaultTestKey('${esc(k.provider)}')">Test</button>
                <button class="btn btn-sm" onclick="vaultRemoveKey('${esc(k.provider)}')">Remove</button>
              </div>
            </div>
            `;
          }).join('')}
        ` : `
          <div class="config-row">
            <div>
              <div class="config-label">Vault unavailable</div>
              <div class="config-desc">Create/import a PQ wallet to enable encrypted API key storage.</div>
            </div>
            <button class="btn btn-sm btn-primary" onclick="location.hash='#/setup'">Open Setup</button>
          </div>
        `}
      </div>

      <div class="section-header">Paths</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row"><div><div class="config-label">Home</div><div class="config-desc" style="font-size:10px">${esc(cfg.paths.home)}</div></div></div>
        <div class="config-row"><div><div class="config-label">Database</div><div class="config-desc" style="font-size:10px">${esc(cfg.paths.db)}</div></div></div>
        <div class="config-row"><div><div class="config-label">PQ Wallet</div><div class="config-desc">${cfg.pq_wallet ? 'Present (ML-DSA-65)' : 'Not created'}</div></div></div>
      </div>
    `;

    const autoOpenProvider = localStorage.getItem('halo_setup_open_provider');
    if (autoOpenProvider) {
      localStorage.removeItem('halo_setup_open_provider');
      const providerEntry = vaultKeys.find(k => String(k.provider || '').toLowerCase() === autoOpenProvider);
      if (providerEntry) {
        openVaultModal(providerEntry.provider, providerEntry.env_var || providerDefaultEnv(providerEntry.provider));
      }
    }
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.refreshAgentPmtCatalog = async function refreshAgentPmtCatalog() {
  try {
    const resp = await apiPost('/agentpmt/refresh', {});
    alert(`AgentPMT catalog refreshed (${Number(resp.count || 0)} tools).`);
    renderConfig();
  } catch (e) {
    alert(`AgentPMT refresh failed: ${String(e && e.message || e)}`);
  }
};

function providerDefaultEnv(provider) {
  const key = String(provider || '').toLowerCase();
  return (PROVIDER_INFO[key] && PROVIDER_INFO[key].envVar) || `${key.toUpperCase()}_API_KEY`;
}

async function renderSetup() {
  const ctx = consumeSetupContext();

  // Fetch live state
  let vaultKeys = [];
  let vaultAvailable = false;
  try {
    const vr = await api('/vault/keys');
    vaultKeys = vr.keys || [];
    vaultAvailable = true;
  } catch (_e) { /* vault locked or unavailable */ }

  let cfg = null;
  try { cfg = await api('/config'); } catch (_e) {}
  const isAuthenticated = cfg && cfg.authentication && cfg.authentication.authenticated;
  const hasWallet = cfg && cfg.pq_wallet;
  const ss = (cfg && cfg.setup_complete) || { identity: false, agentpmt: false, llm: false, complete: false };

  // Build status lookup from vault keys
  const keyStatus = {};
  vaultKeys.forEach(k => { keyStatus[String(k.provider || '').toLowerCase()] = k; });

  function providerStatus(provider) {
    const v = keyStatus[provider];
    if (!v) return { configured: false, tested: false };
    return { configured: !!v.configured, tested: !!v.tested };
  }

  function statusBadgeHtml(provider) {
    const s = providerStatus(provider);
    if (s.tested) return '<span class="badge badge-ok">Verified</span>';
    if (s.configured) return '<span class="badge badge-warn">Configured (untested)</span>';
    return '<span class="badge badge-muted">Not configured</span>';
  }

  function providerCard(provider) {
    const info = PROVIDER_INFO[provider] || { name: provider, envVar: providerDefaultEnv(provider), keyUrl: '#', description: '' };
    const s = providerStatus(provider);
    const docsLink = info.keyUrl && info.keyUrl !== '#'
      ? `<a class="btn btn-sm" href="${esc(info.keyUrl)}" target="_blank" rel="noopener noreferrer">Get Key</a>`
      : '';
    return `
      <div style="padding:10px 0;border-bottom:1px solid var(--border);display:flex;align-items:center;justify-content:space-between;gap:8px;flex-wrap:wrap">
        <div style="flex:1;min-width:180px">
          <div style="font-size:13px">${esc(info.name)}</div>
          <div style="font-size:10px;color:var(--text-dim);margin-top:2px">${esc(info.envVar)}</div>
          ${info.description ? `<div style="font-size:11px;color:var(--text-dim);margin-top:4px">${esc(info.description)}</div>` : ''}
        </div>
        <div style="display:flex;gap:6px;align-items:center">
          ${statusBadgeHtml(provider)}
          ${docsLink}
          <button class="btn btn-sm btn-primary setup-provider-config-btn" data-provider="${esc(provider)}">Set Key</button>
          ${s.configured ? `<button class="btn btn-sm setup-provider-test-btn" data-provider="${esc(provider)}">Test</button>` : ''}
          ${s.configured ? `<button class="btn btn-sm setup-provider-disconnect-btn" data-provider="${esc(provider)}" title="Remove this key">Disconnect</button>` : ''}
        </div>
      </div>
    `;
  }

  const requiredProviders = Object.keys(PROVIDER_INFO).filter(p => PROVIDER_INFO[p].required);
  const optionalLLM = Object.keys(PROVIDER_INFO).filter(p => !PROVIDER_INFO[p].required && PROVIDER_INFO[p].category === 'llm');
  const optionalStorage = Object.keys(PROVIDER_INFO).filter(p => !PROVIDER_INFO[p].required && PROVIDER_INFO[p].category === 'storage');
  const optionalTooling = Object.keys(PROVIDER_INFO).filter(p => !PROVIDER_INFO[p].required && PROVIDER_INFO[p].category === 'tooling');

  // Step states
  const step1Done = ss.agentpmt;
  const step2Done = ss.llm;
  const identityDone = ss.identity;
  const allDone = ss.complete;

  const pmtEnabled = cfg && cfg.agentpmt && cfg.agentpmt.enabled;
  const pmtAuth = cfg && cfg.agentpmt && cfg.agentpmt.auth_configured;
  const pmtToolCount = (cfg && cfg.agentpmt && cfg.agentpmt.tool_count) || 0;
  const orStatus = providerStatus('openrouter');

  // Stepper step classes
  const s1c = step1Done ? 's-done' : 's-active';
  const s2c = step1Done ? (step2Done ? 's-done' : 's-active') : '';
  const s3c = allDone ? 's-done' : (step1Done && step2Done ? 's-active' : '');

  // Card classes
  const c1c = step1Done ? 'card-done' : 'card-active';
  const c2c = step1Done ? (step2Done ? 'card-done' : 'card-active') : 'card-locked';
  const c3c = allDone ? 'card-done card-celebrate' : 'card-locked';

  content.innerHTML = `

    <!-- Hero -->
    <div class="setup-hero">
      <div class="setup-hero-emoji">${allDone ? '&#127881;' : '&#128075;'}</div>
      <h1>${allDone ? 'You\'re All Set!' : 'Welcome to Agent H.A.L.O.'}</h1>
      <p>${allDone
        ? 'Your dashboard is fully operational. Go build something amazing.'
        : 'Get your agent economy running in just a couple of steps. It only takes a minute.'}</p>
    </div>

    <!-- Stepper -->
    <div class="setup-stepper">
      <div class="setup-stepper-step ${s1c}">
        <div class="setup-stepper-dot">${step1Done ? '&#10003;' : '1'}</div>
        <div class="setup-stepper-label">Account</div>
      </div>
      <div class="setup-stepper-step ${s2c}">
        <div class="setup-stepper-dot">${step2Done ? '&#10003;' : '2'}</div>
        <div class="setup-stepper-label">LLM Key</div>
      </div>
      <div class="setup-stepper-step ${s3c}">
        <div class="setup-stepper-dot">${allDone ? '&#10003;' : '3'}</div>
        <div class="setup-stepper-label">Ready</div>
      </div>
    </div>

    <!-- STEP 1: Connect Your Account -->
    <div class="setup-card-v2 ${c1c}" id="setup-step-1">
      <div class="card-header">
        <div class="card-icon">&#9883;</div>
        <div>
          <div class="card-title">Verify Your Identity</div>
          <div class="card-desc">Connect to AgentPMT to unlock ${pmtToolCount > 0 ? pmtToolCount + '+' : ''} tools, workflows, and budget management</div>
        </div>
      </div>

      ${step1Done ? `
        <!-- Connected state -->
        <div class="setup-success-banner">
          <span class="success-icon">&#10003;</span>
          <span>AgentPMT connected${pmtToolCount > 0 ? ' &mdash; <strong>' + pmtToolCount + ' tools</strong> ready to use' : ''}</span>
        </div>
        <div style="margin-top:16px;display:flex;gap:10px;align-items:center;flex-wrap:wrap">
          <button class="btn btn-sm" id="setup-disconnect-agentpmt" style="border-color:var(--red);color:var(--red)">
            Disconnect My Account
          </button>
          <span style="font-size:11px;color:var(--text-dim)">Removes your token and disables the tool proxy</span>
        </div>
      ` : `
        <!-- Not connected — two paths -->

        <!-- Path A: Sign up or sign in at AgentPMT -->
        <div class="setup-recommended">
          <div class="setup-recommended-label">Recommended</div>
          <p style="font-size:14px;color:var(--text-muted);line-height:1.6;margin-bottom:16px">
            AgentPMT is your gateway to 100+ third-party tools, budget controls, and workflow automation.
            Create a free account (or sign in), then grab your Bearer Token.
          </p>

          <div style="display:flex;gap:12px;flex-wrap:wrap;margin-bottom:20px">
            <a class="setup-cta-big" href="https://www.agentpmt.com" target="_blank" rel="noopener noreferrer" id="setup-agentpmt-signup">
              Create Free Account &#8599;
            </a>
            <a class="setup-cta-big" href="https://www.agentpmt.com/login" target="_blank" rel="noopener noreferrer" style="background:transparent;border-color:var(--border);color:var(--text-muted)">
              I Already Have an Account &#8599;
            </a>
          </div>

          <!-- Embedded signup iframe (opens when user clicks) -->
          <div id="setup-agentpmt-iframe-wrap" style="display:none;margin-bottom:18px">
            <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:8px">
              <span style="font-size:12px;color:var(--text-muted)">AgentPMT &mdash; complete signup, then grab your Bearer Token below</span>
              <button class="btn btn-sm" id="setup-close-iframe" style="font-size:11px">Close</button>
            </div>
            <iframe id="setup-agentpmt-iframe" style="width:100%;height:560px;border:1px solid var(--border);border-radius:8px;background:#fff" sandbox="allow-scripts allow-same-origin allow-forms allow-popups"></iframe>
          </div>

          <div style="border:1px solid var(--border);border-radius:8px;padding:18px 20px;margin-bottom:16px;background:rgba(255,106,0,0.02)">
            <div style="font-size:13px;font-weight:700;color:var(--accent);margin-bottom:12px">How to get your Bearer Token:</div>
            <ol class="setup-steps-friendly" style="margin:0">
              <li>
                <span class="step-circle">1</span>
                <span>Sign in at <a href="https://www.agentpmt.com/login" target="_blank" rel="noopener noreferrer" style="color:var(--accent)">agentpmt.com</a></span>
              </li>
              <li>
                <span class="step-circle">2</span>
                <span>Click <strong>Dashboard</strong> at the top (defaults to AU Budgets tab)</span>
              </li>
              <li>
                <span class="step-circle">3</span>
                <span>Click the <strong>Display Bearer Token</strong> button</span>
              </li>
              <li>
                <span class="step-circle">4</span>
                <span>Copy the token and paste it below</span>
              </li>
            </ol>
          </div>

          <div class="setup-token-area" style="border-top:none;padding-top:0">
            <label for="setup-agentpmt-token">Your AgentPMT Bearer Token</label>
            <div class="setup-token-row">
              <input id="setup-agentpmt-token" type="password" placeholder="Paste your bearer token here..."
                     autocomplete="off" spellcheck="false">
              <button class="btn btn-primary" id="setup-save-agentpmt" style="padding:10px 20px;font-size:13px;border-radius:6px">Save &amp; Connect</button>
              <button class="btn" id="setup-test-agentpmt" style="padding:10px 16px;font-size:13px;border-radius:6px" ${!pmtAuth ? 'disabled' : ''}>
                Test
              </button>
            </div>
            <div id="setup-agentpmt-status" class="setup-token-status"></div>
          </div>
        </div>

        <!-- Path B: Anonymous Agent Wallet -->
        <details class="setup-alt-path" style="margin-top:16px">
          <summary>Skip signup &mdash; create an anonymous agent wallet instead</summary>
          <div class="alt-body">
            <p style="font-size:13px;color:var(--text-muted);line-height:1.6;margin-bottom:14px">
              Don't want to create an account right now? We can generate an anonymous agent wallet via the AgentPMT API.
              You'll get a bearer token automatically &mdash; no email or signup required.
            </p>
            <div class="setup-info-box" style="margin-top:0;margin-bottom:14px">
              <span class="info-icon">&#9432;</span>
              <span>Anonymous wallets have limited budgets. You can upgrade to a full account anytime.</span>
            </div>
            <div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap">
              <button class="btn btn-primary" id="setup-create-anon-wallet" style="border-radius:6px;padding:10px 20px;font-size:13px">
                Create Anonymous Wallet
              </button>
              <span id="setup-anon-wallet-status" style="font-size:12px;color:var(--text-dim)"></span>
            </div>
          </div>
        </details>

        <!-- Path C: Local PQ wallet -->
        <details class="setup-alt-path">
          <summary>Advanced: Use local PQ wallet identity instead</summary>
          <div class="alt-body">
            <p style="font-size:12px;color:var(--text-dim);line-height:1.6;margin-bottom:12px">
              Creates a local cryptographic identity for vault storage and dashboard auth.
              Does <strong>not</strong> provide marketplace access &mdash; for tools and workflows, use AgentPMT above.
            </p>
            <div style="border:1px solid var(--border);border-radius:6px;padding:12px 14px;margin-bottom:10px">
              <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:6px">
                <span style="font-size:13px;color:var(--text)">Dashboard auth</span>
                ${isAuthenticated
                  ? '<span class="badge badge-ok">Authenticated</span>'
                  : '<span class="badge badge-muted">Not set up</span>'}
              </div>
              <div style="display:flex;justify-content:space-between;align-items:center">
                <span style="font-size:13px;color:var(--text)">PQ wallet</span>
                ${hasWallet
                  ? '<span class="badge badge-ok">Present</span>'
                  : '<span class="badge badge-muted">Not created</span>'}
              </div>
            </div>
            ${!identityDone ? `
              <div class="setup-cmd">
                <code>agenthalo keygen --pq</code>
                <button class="btn btn-sm" onclick="copySetupText('agenthalo keygen --pq')">Copy</button>
              </div>
              <div class="setup-cmd">
                <code>agenthalo login</code>
                <button class="btn btn-sm" onclick="copySetupText('agenthalo login')">Copy</button>
              </div>
            ` : `
              <div class="setup-success-banner" style="font-size:12px">
                <span class="success-icon">&#10003;</span>
                <span>Local identity configured. Add AgentPMT above for full marketplace access.</span>
              </div>
            `}
          </div>
        </details>
      `}
    </div>

    <!-- STEP 2: OpenRouter LLM Key -->
    <div class="setup-card-v2 ${c2c}" id="setup-step-2">
      <div class="card-header">
        <div class="card-icon">&#9889;</div>
        <div>
          <div class="card-title">Add Your LLM Key</div>
          <div class="card-desc">Power your agents with OpenRouter &mdash; one key for 200+ models</div>
        </div>
      </div>

      ${step2Done ? `
        <!-- Connected state with verified badge in proper context -->
        <div class="setup-success-banner">
          <span class="success-icon">&#10003;</span>
          <span>
            OpenRouter ${orStatus.tested ? '<strong>verified</strong>' : 'connected'} &mdash; LLM proxy is live
          </span>
        </div>
        <div style="margin-top:12px;display:flex;gap:10px;align-items:center;flex-wrap:wrap">
          <button class="btn btn-sm setup-provider-config-btn" data-provider="openrouter" style="border-radius:6px">Change Key</button>
          <button class="btn btn-sm setup-provider-test-btn" data-provider="openrouter" style="border-radius:6px">Re-test</button>
          <button class="btn btn-sm setup-provider-disconnect-btn" data-provider="openrouter" style="border-color:var(--red);color:var(--red);border-radius:6px">
            Disconnect
          </button>
        </div>
      ` : `
        ${requiredProviders.map(p => {
          const info = PROVIDER_INFO[p] || { name: p, envVar: providerDefaultEnv(p), keyUrl: '#', description: '' };
          const s = providerStatus(p);
          return `
            <div class="setup-provider-card">
              <div class="provider-info">
                <div class="provider-name">${esc(info.name)}</div>
                <div class="provider-env">${esc(info.envVar)}</div>
                ${info.description ? '<div class="provider-desc">' + esc(info.description) + '</div>' : ''}
              </div>
              <div class="provider-actions">
                ${statusBadgeHtml(p)}
                ${info.keyUrl && info.keyUrl !== '#' ? '<a class="btn btn-sm" href="' + esc(info.keyUrl) + '" target="_blank" rel="noopener noreferrer">Get Key</a>' : ''}
                <button class="btn btn-sm btn-primary setup-provider-config-btn" data-provider="${esc(p)}">Set Key</button>
                ${s.configured ? '<button class="btn btn-sm setup-provider-test-btn" data-provider="' + esc(p) + '">Test</button>' : ''}
              </div>
            </div>
          `;
        }).join('')}

        ${step1Done ? `
          <div class="setup-info-box" style="margin-top:14px">
            <span class="info-icon">&#9888;</span>
            <span>Add your OpenRouter key so customers can use LLM inference through your agents.</span>
          </div>
        ` : ''}
      `}
    </div>

    <!-- STEP 3: Dashboard Unlocked -->
    <div class="setup-card-v2 ${c3c}" id="setup-step-3">
      <div class="card-header">
        <div class="card-icon">&#127919;</div>
        <div>
          <div class="card-title">${allDone ? 'Dashboard Unlocked!' : 'Almost There'}</div>
          <div class="card-desc">${allDone ? 'All systems go. Explore your dashboard.' : 'Complete the steps above to unlock everything.'}</div>
        </div>
      </div>
      ${allDone ? `
        <div class="setup-unlocked-actions">
          <a class="btn btn-primary" href="#/overview" style="border-radius:6px">Explore Overview</a>
          <a class="btn" href="#/cockpit" style="border-radius:6px">Open Cockpit</a>
          <a class="btn" href="#/deploy" style="border-radius:6px">Deploy Agents</a>
        </div>
      ` : `
        <p style="color:var(--text-dim);font-size:13px;line-height:1.6;margin-top:4px">
          Once you connect your account and add an LLM key, all tabs unlock automatically.
        </p>
      `}
    </div>

    <!-- Optional Integrations -->
    <details class="setup-optional-section">
      <summary>Optional Integrations (${optionalStorage.length + optionalLLM.length + optionalTooling.length} services)</summary>
      <div class="optional-body">
        <div style="margin-top:12px">
          <div style="font-size:14px;font-weight:700;color:var(--accent);margin-bottom:6px">Immutable Storage</div>
          <div style="font-size:12px;color:var(--text-dim);margin-bottom:10px;line-height:1.5">
            IPFS-based storage for agent traces, attestations, and customer data.
          </div>
          ${optionalStorage.map(p => providerCard(p)).join('')}
        </div>
        <div style="margin-top:18px">
          <div style="font-size:14px;font-weight:700;color:var(--accent);margin-bottom:6px">Direct LLM Keys</div>
          <div style="font-size:12px;color:var(--text-dim);margin-bottom:10px;line-height:1.5">
            Operator-side diagnostics and fallback. Customer traffic uses OpenRouter.
          </div>
          ${optionalLLM.map(p => providerCard(p)).join('')}
        </div>
        <div style="margin-top:18px">
          <div style="font-size:14px;font-weight:700;color:var(--accent);margin-bottom:6px">Additional Tools</div>
          ${optionalTooling.map(p => providerCard(p)).join('')}
        </div>
      </div>
    </details>

    <!-- Funding & Monetization -->
    <div class="setup-card-v2" style="margin-top:20px;border-color:rgba(255,106,0,0.25)">
      <div class="card-header">
        <div class="card-icon">&#128176;</div>
        <div>
          <div class="card-title">Funding &amp; Monetization</div>
          <div class="card-desc">Two verified channels for customer balance top-ups</div>
        </div>
      </div>
      <div class="setup-funding-channel">
        <div class="channel-icon">&#127968;</div>
        <div>
          <div class="channel-name">AgentPMT Token Purchase</div>
          <div class="channel-desc">Customers buy tokens at AgentPMT.com. Signed receipts verified via HMAC-SHA256.</div>
        </div>
      </div>
      <div class="setup-funding-channel">
        <div class="channel-icon">&#9939;</div>
        <div>
          <div class="channel-name">x402direct (USDC on Base L2)</div>
          <div class="channel-desc">Direct USDC stablecoin payment. Transaction hash verified on-chain.</div>
        </div>
      </div>
      <p style="font-size:11px;color:var(--text-dim);margin-top:8px;line-height:1.5">
        All tools, workflows, and agent configurations are accessible exclusively through AgentPMT MCP.
      </p>
    </div>
  `;

  // ---- Wire up interactive elements ----

  // AgentPMT token save
  const saveBtn = document.getElementById('setup-save-agentpmt');
  const testBtn = document.getElementById('setup-test-agentpmt');
  const tokenInput = document.getElementById('setup-agentpmt-token');
  const statusEl = document.getElementById('setup-agentpmt-status');

  if (saveBtn && tokenInput) {
    saveBtn.addEventListener('click', async () => {
      const token = (tokenInput.value || '').trim();
      if (!token) {
        if (statusEl) statusEl.innerHTML = '<span style="color:var(--red)">Please paste your bearer token first.</span>';
        return;
      }
      saveBtn.disabled = true;
      saveBtn.textContent = 'Connecting...';
      try {
        // Store the token in the vault under the agentpmt provider
        await apiPost('/vault/keys/agentpmt', { key: token, env_var: 'AGENTPMT_API_KEY' });
        // Enable the tool proxy
        await apiPost('/agentpmt/enable', {});
        if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Token saved. Testing connection...</span>';
        // Auto-test: refresh catalog
        try {
          const refreshResp = await apiPost('/agentpmt/refresh', {});
          const count = Number(refreshResp.count || 0);
          if (statusEl) statusEl.innerHTML = `<span style="color:var(--green)">&#10003; Connected! ${count} tools available.</span>`;
        } catch (re) {
          if (statusEl) statusEl.innerHTML = `<span style="color:var(--yellow)">Token saved but catalog refresh failed: ${esc(String(re.message || re))}</span>`;
        }
        // Invalidate setup state cache and re-render
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        if (statusEl) statusEl.innerHTML = `<span style="color:var(--red)">Save failed: ${esc(String(e.message || e))}</span>`;
        saveBtn.disabled = false;
        saveBtn.textContent = 'Save & Connect';
      }
    });
  }

  if (testBtn) {
    testBtn.addEventListener('click', async () => {
      testBtn.disabled = true;
      testBtn.textContent = 'Testing...';
      try {
        const resp = await apiPost('/agentpmt/refresh', {});
        const count = Number(resp.count || 0);
        if (statusEl) statusEl.innerHTML = `<span style="color:var(--green)">&#10003; Connection OK &mdash; ${count} tools loaded.</span>`;
      } catch (e) {
        if (statusEl) statusEl.innerHTML = `<span style="color:var(--red)">Test failed: ${esc(String(e.message || e))}</span>`;
      }
      testBtn.disabled = false;
      testBtn.textContent = 'Test';
    });
  }

  // AgentPMT disconnect
  const disconnectPmtBtn = document.getElementById('setup-disconnect-agentpmt');
  if (disconnectPmtBtn) {
    disconnectPmtBtn.addEventListener('click', async () => {
      if (!confirm('Disconnect your AgentPMT account? This removes your token and disables the tool proxy.')) return;
      disconnectPmtBtn.disabled = true;
      disconnectPmtBtn.textContent = 'Disconnecting...';
      try {
        await apiPost('/agentpmt/disconnect', {});
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        alert('Disconnect failed: ' + (e.message || e));
        disconnectPmtBtn.disabled = false;
        disconnectPmtBtn.textContent = 'Disconnect My Account';
      }
    });
  }

  // Anonymous wallet creation
  const anonWalletBtn = document.getElementById('setup-create-anon-wallet');
  const anonWalletStatus = document.getElementById('setup-anon-wallet-status');
  if (anonWalletBtn) {
    anonWalletBtn.addEventListener('click', async () => {
      anonWalletBtn.disabled = true;
      anonWalletBtn.textContent = 'Creating wallet...';
      if (anonWalletStatus) anonWalletStatus.innerHTML = '<span style="color:var(--text-muted)">Requesting anonymous wallet from AgentPMT...</span>';
      try {
        const resp = await apiPost('/agentpmt/anonymous-wallet', {});
        if (resp.token_saved) {
          if (anonWalletStatus) anonWalletStatus.innerHTML = '<span style="color:var(--green)">&#10003; Wallet created and connected!</span>';
          window._invalidateSetupState();
          await fetchSetupState(true);
          await renderSetup();
          updateNavLockState();
        } else {
          if (anonWalletStatus) anonWalletStatus.innerHTML = '<span style="color:var(--yellow)">Wallet created but no token was returned. You may need to enter it manually.</span>';
          anonWalletBtn.disabled = false;
          anonWalletBtn.textContent = 'Create Anonymous Wallet';
        }
      } catch (e) {
        if (anonWalletStatus) anonWalletStatus.innerHTML = `<span style="color:var(--red)">Failed: ${esc(String(e.message || e))}</span>`;
        anonWalletBtn.disabled = false;
        anonWalletBtn.textContent = 'Create Anonymous Wallet';
      }
    });
  }

  // Iframe embed logic: try opening AgentPMT in iframe when CTA is clicked
  const signupBtn = document.getElementById('setup-agentpmt-signup');
  const iframeWrap = document.getElementById('setup-agentpmt-iframe-wrap');
  const iframe = document.getElementById('setup-agentpmt-iframe');
  const closeIframeBtn = document.getElementById('setup-close-iframe');
  if (signupBtn && iframeWrap && iframe) {
    signupBtn.addEventListener('click', (e) => {
      // Try embedding; if it fails the link still opens in new tab (target=_blank)
      e.preventDefault();
      iframe.src = 'https://www.agentpmt.com/login';
      iframeWrap.style.display = 'block';
      iframeWrap.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
      // Also open in new tab as fallback
      window.open('https://www.agentpmt.com', '_blank', 'noopener,noreferrer');
    });
  }
  if (closeIframeBtn && iframeWrap && iframe) {
    closeIframeBtn.addEventListener('click', () => {
      iframeWrap.style.display = 'none';
      iframe.src = '';
    });
  }

  // Provider "Set Key" buttons
  content.querySelectorAll('.setup-provider-config-btn[data-provider]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const provider = btn.dataset.provider || '';
      const info = PROVIDER_INFO[provider] || {};
      openVaultModal(provider, info.envVar || providerDefaultEnv(provider));
    });
  });

  // Provider "Test" buttons
  content.querySelectorAll('.setup-provider-test-btn[data-provider]').forEach((btn) => {
    btn.addEventListener('click', () => {
      window.vaultTestKey(btn.dataset.provider || '');
    });
  });

  // Provider "Disconnect" buttons
  content.querySelectorAll('.setup-provider-disconnect-btn[data-provider]').forEach((btn) => {
    btn.addEventListener('click', () => {
      window.vaultRemoveKey(btn.dataset.provider || '');
    });
  });

  // Auto-open provider modal if redirected from config
  const autoOpenProvider = localStorage.getItem('halo_setup_open_provider');
  if (autoOpenProvider) {
    localStorage.removeItem('halo_setup_open_provider');
    const info = PROVIDER_INFO[autoOpenProvider];
    if (info) openVaultModal(autoOpenProvider, info.envVar || providerDefaultEnv(autoOpenProvider));
  }
}

window.toggleWrap = async function(agent, enable) {
  try {
    await apiPost('/config/wrap', { agent, enable });
    renderConfig();
  } catch (e) { alert('Failed: ' + e.message); }
};

window.toggleX402 = async function(enable) {
  try {
    await apiPost('/config/x402', { enabled: enable });
    renderConfig();
  } catch (e) { alert('Failed: ' + e.message); }
};

function openVaultModal(provider, envVar) {
  const old = document.getElementById('vault-key-modal');
  if (old) old.remove();
  const wrap = document.createElement('div');
  wrap.id = 'vault-key-modal';
  wrap.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.65);display:flex;align-items:center;justify-content:center;z-index:1200';
  wrap.innerHTML = `
    <div style="width:min(520px,92vw);background:var(--bg-card);border:1px solid var(--accent);padding:16px;border-radius:6px">
      <div style="font-size:14px;color:var(--accent);margin-bottom:6px">Set API Key: ${esc(provider)}</div>
      <div style="font-size:11px;color:var(--text-dim);margin-bottom:10px">${esc(envVar)}</div>
      <input id="vault-key-input" type="password" placeholder="Paste API key" style="width:100%;padding:8px 10px;font-size:12px;margin-bottom:10px">
      <div style="display:flex;gap:8px;justify-content:flex-end">
        <button class="btn btn-sm" id="vault-key-cancel">Cancel</button>
        <button class="btn btn-sm btn-primary" id="vault-key-save">Save</button>
      </div>
    </div>
  `;
  document.body.appendChild(wrap);
  const input = document.getElementById('vault-key-input');
  input?.focus();
  wrap.querySelector('#vault-key-cancel').addEventListener('click', () => wrap.remove());
  wrap.querySelector('#vault-key-save').addEventListener('click', async () => {
    const key = input?.value || '';
    if (!key.trim()) return;
    try {
      await apiPost(`/vault/keys/${encodeURIComponent(provider)}`, { key, env_var: envVar });
      wrap.remove();
      window._invalidateSetupState();
      await fetchSetupState(true);
      // Re-render current page
      const curPage = (location.hash.replace('#/', '') || 'setup').split('/')[0];
      if (pages[curPage]) await pages[curPage]();
      updateNavLockState();
    } catch (e) {
      alert('Set key failed: ' + e.message);
    }
  });
}

window.vaultSetKey = function(provider, envVar) {
  openVaultModal(provider, envVar);
};

window.vaultTestKey = async function(provider) {
  try {
    const res = await apiPost(`/vault/test/${encodeURIComponent(provider)}`, {});
    if (res.ok) alert(`${provider}: key validated successfully`);
    else alert(`${provider}: ${res.error || 'validation failed'}`);
    window._invalidateSetupState();
    await fetchSetupState(true);
    const curPage = (location.hash.replace('#/', '') || 'setup').split('/')[0];
    if (pages[curPage]) await pages[curPage]();
    updateNavLockState();
  } catch (e) {
    alert('Test key failed: ' + e.message);
  }
};

window.vaultRemoveKey = async function(provider) {
  if (!confirm(`Remove key for ${provider}?`)) return;
  try {
    await apiDelete(`/vault/keys/${encodeURIComponent(provider)}`);
    window._invalidateSetupState();
    await fetchSetupState(true);
    const curPage = (location.hash.replace('#/', '') || 'setup').split('/')[0];
    if (pages[curPage]) await pages[curPage]();
    updateNavLockState();
  } catch (e) {
    alert('Remove key failed: ' + e.message);
  }
};

// =============================================================================
// PAGE: Cockpit
// =============================================================================
function renderCockpit() {
  content.innerHTML = `
    <div class="page-header">
      <h1>Cockpit</h1>
      <p class="subtitle">Agent orchestration terminal</p>
    </div>
    <div id="cockpit-root" style="margin-top:10px"></div>
  `;

  const root = document.getElementById('cockpit-root');
  if (window.CockpitPage && typeof window.CockpitPage.mount === 'function') {
    window.CockpitPage.mount(root);
  } else {
    root.innerHTML = `
      <div class="card" style="padding:2rem;text-align:center;color:var(--amber);">
        <p style="font-size:1.5rem;">&#9654; Cockpit unavailable</p>
        <p style="margin-top:1rem;color:var(--text-dim);">cockpit.js failed to load.</p>
      </div>`;
  }
}

// =============================================================================
// PAGE: Deploy
// =============================================================================
function renderDeploy() {
  content.innerHTML = `
    <div class="page-header">
      <h1>Deploy</h1>
      <p class="subtitle">Launch and manage agents</p>
    </div>
    <div id="deploy-root"></div>
  `;

  const root = document.getElementById('deploy-root');
  if (window.DeployPage && typeof window.DeployPage.init === 'function') {
    window.DeployPage.init(root);
  } else {
    root.innerHTML = `
      <div class="card" style="padding:2rem;text-align:center;color:var(--amber);">
        <p style="font-size:1.5rem;">&#9732; Deploy unavailable</p>
        <p style="margin-top:1rem;color:var(--text-dim);">deploy.js failed to load.</p>
      </div>`;
  }
}

// =============================================================================
// PAGE: Trust & Attestations
// =============================================================================
async function renderTrust() {
  content.innerHTML = '<div class="loading">Loading attestations...</div>';
  try {
    const data = await api('/attestations');
    const attestations = data.attestations || [];

    content.innerHTML = `
      <div class="page-title">Trust &amp; Attestations</div>

      <div class="card-grid">
        <div class="card">
          <div class="card-label">Attestations</div>
          <div class="card-value">${attestations.length}</div>
          <div class="card-sub">Total created</div>
        </div>
        <div class="card">
          <div class="card-label">On-Chain</div>
          <div class="card-value">${attestations.filter(a => a.tx_hash).length}</div>
          <div class="card-sub">Posted to blockchain</div>
        </div>
      </div>

      <div class="section-header">Verify Attestation</div>
      <div style="display:flex;gap:8px;margin-bottom:20px">
        <input type="text" id="verify-digest" placeholder="Paste attestation digest..." style="flex:1;padding:8px 12px;font-size:12px">
        <button class="btn btn-primary" onclick="verifyDigest()">Verify</button>
      </div>
      <div id="verify-result"></div>

      <div class="section-header">Attestation History</div>
      ${attestations.length > 0 ? `
        <div class="table-wrap"><table>
          <thead><tr><th>Digest</th><th>Proof Type</th><th>Session</th><th>TX Hash</th></tr></thead>
          <tbody>
            ${attestations.map(a => `
              <tr>
                <td style="font-size:10px">${esc(truncate(a.attestation_digest || '', 32))}</td>
                <td><span class="badge badge-info">${esc(a.proof_type || 'merkle')}</span></td>
                <td style="font-size:10px">${esc(truncate(a.session_id || '', 24))}</td>
                <td style="font-size:10px">${a.tx_hash ? esc(truncate(a.tx_hash, 24)) : '-'}</td>
              </tr>
            `).join('')}
          </tbody>
        </table></div>
      ` : '<div style="color:var(--text-muted)">No attestations created yet.</div>'}
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.verifyDigest = async function() {
  const digest = ($('#verify-digest')?.value || '').trim();
  if (!digest) return;
  const el = $('#verify-result');
  el.innerHTML = '<div style="color:var(--text-muted)">Checking...</div>';
  try {
    const data = await apiPost('/attestations/verify', { digest });
    if (data.verified) {
      el.innerHTML = `<div class="card" style="border-color:var(--green)">
        <div class="card-label" style="color:var(--green)">CRYPTOGRAPHICALLY VERIFIED</div>
        <div class="card-sub">Merkle root recomputed from session events matches stored attestation.
          ${data.checks ? `<br>Digest: ${data.checks.digest_match ? 'OK' : 'MISMATCH'} |
          Root: ${data.checks.merkle_root_match ? 'OK' : 'MISMATCH'} |
          Events: ${data.checks.event_count_match ? 'OK' : 'MISMATCH'}` : ''}
          ${data.event_count ? `<br>${data.event_count} events verified` : ''}
        </div></div>`;
    } else if (data.found) {
      el.innerHTML = `<div class="card" style="border-color:var(--red)">
        <div class="card-label" style="color:var(--red)">VERIFICATION FAILED</div>
        <div class="card-sub">${esc(data.reason || 'Recomputed attestation does not match stored digest.')}
          ${data.checks ? `<br>Digest: ${data.checks.digest_match ? 'OK' : 'MISMATCH'} |
          Root: ${data.checks.merkle_root_match ? 'OK' : 'MISMATCH'} |
          Events: ${data.checks.event_count_match ? 'OK' : 'MISMATCH'}` : ''}
        </div></div>`;
    } else {
      el.innerHTML = '<div class="card" style="border-color:var(--yellow)"><div class="card-label" style="color:var(--yellow)">NOT FOUND</div><div class="card-sub">No attestation with this digest in local store</div></div>';
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Verification failed: ${esc(e.message)}</div>`;
  }
};

// =============================================================================
// PAGE: NucleusDB — Full Database Browser (Redesigned)
// =============================================================================

// NucleusDB sub-tab state
const ndb = {
  tab: 'browse',
  page: 0,
  pageSize: 50,
  prefix: '',
  sort: 'key',
  order: 'asc',
  editingKey: null,
};

const ndbSharing = {
  includeRevoked: false,
};

// Backend description map
const backendInfo = {
  binary_merkle: { name: 'BinaryMerkle', algo: 'SHA-256', type: 'Post-Quantum', proof: 'O(log n)', setup: 'None' },
  ipa: { name: 'IPA', algo: 'Pedersen', type: 'Binding', proof: 'O(n)', setup: 'None' },
  kzg: { name: 'KZG', algo: 'BLS12-381', type: 'Pairing', proof: 'O(1)', setup: 'Trusted' },
};

async function renderNucleusDB(subtab) {
  ndb.tab = subtab || ndb.tab || 'browse';
  content.innerHTML = '<div class="loading">Initializing NucleusDB...</div>';

  try {
    const [status, stats] = await Promise.all([
      api('/nucleusdb/status'),
      api('/nucleusdb/stats').catch(() => null)
    ]);

    const keyCount = stats?.key_count || 0;
    const commitCount = stats?.commit_count || 0;
    const dbSize = stats?.db_size_bytes || 0;
    const backend = status.backend || 'binary_merkle';
    const bi = backendInfo[backend] || backendInfo.binary_merkle;
    const chainOk = status.exists && commitCount > 0;

    content.innerHTML = `
      <div class="ndb-hero">
        <canvas class="ndb-hero-canvas" id="hero-particles"></canvas>
        <div class="ndb-hero-grid">
          <div class="ndb-hero-logo-wrap">
            <img src="img/nucleus_db_hero.png" alt="NucleusDB" onerror="this.style.display='none'">
          </div>
          <div class="ndb-hero-copy">
            <div class="ndb-hero-kicker">Agent H.A.L.O. // Containment Node</div>
            <div class="ndb-hero-title">NucleusDB</div>
            <div class="ndb-hero-subtitle">Proof-Carrying Algebraic Database</div>
            <div class="ndb-hero-separator"></div>
          </div>
        </div>
      </div>

      <div class="card-grid" style="margin-bottom:14px">
        <div class="card">
          <div class="card-label">Keys</div>
          <div class="card-value">${keyCount.toLocaleString()}</div>
          <div class="card-sub">${stats?.type_distribution ? Object.keys(stats.type_distribution).length + ' types' : 'No data'}</div>
        </div>
        <div class="card">
          <div class="card-label">Commits</div>
          <div class="card-value">${commitCount.toLocaleString()}</div>
          <div class="card-sub">${formatBytes(dbSize)}</div>
        </div>
        <div class="card">
          <div class="card-label">Backend</div>
          <div class="card-value" style="font-size:14px">${esc(bi.name)}</div>
          <div class="card-sub">${esc(bi.algo)} | ${esc(bi.type)}</div>
        </div>
        <div class="card">
          <div class="card-label">Chain</div>
          <div class="card-value" style="font-size:14px">${chainOk
            ? '<span class="badge badge-ok">HEALTHY</span>'
            : status.exists ? '<span class="badge badge-warn">EMPTY</span>' : '<span class="badge badge-muted">NO DB</span>'}</div>
          <div class="card-sub">${chainOk ? 'Seal #' + commitCount : status.exists ? 'No commits yet' : 'Create database first'}</div>
        </div>
      </div>

      <div class="ndb-tabs">
        <button class="ndb-tab ${ndb.tab === 'browse' ? 'active' : ''}" onclick="ndbSwitchTab('browse')">F1:DATA</button>
        <button class="ndb-tab ${ndb.tab === 'sql' ? 'active' : ''}" onclick="ndbSwitchTab('sql')">F2:SQL</button>
        <button class="ndb-tab ${ndb.tab === 'vectors' ? 'active' : ''}" onclick="ndbSwitchTab('vectors')">F3:VEC</button>
        <button class="ndb-tab ${ndb.tab === 'commits' ? 'active' : ''}" onclick="ndbSwitchTab('commits')">F4:CHAIN</button>
        <button class="ndb-tab ${ndb.tab === 'proofs' ? 'active' : ''}" onclick="ndbSwitchTab('proofs')">F5:PROOF</button>
        <button class="ndb-tab ${ndb.tab === 'sharing' ? 'active' : ''}" onclick="ndbSwitchTab('sharing')">F6:SHARE</button>
        <button class="ndb-tab ${ndb.tab === 'config' ? 'active' : ''}" onclick="ndbSwitchTab('config')">F7:CFG</button>
      </div>
      <div id="ndb-content"></div>
    `;

    // Store stats for sub-tabs
    window._ndbStats = stats;
    window._ndbStatus = status;

    // Start particle network animation
    if (window._initHeroParticles) window._initHeroParticles();

    // Render active sub-tab
    switch (ndb.tab) {
      case 'browse': await ndbRenderBrowse(); break;
      case 'sql': ndbRenderSQL(); break;
      case 'vectors': await ndbRenderVectors(); break;
      case 'commits': await ndbRenderCommits(); break;
      case 'proofs': ndbRenderProofs(); break;
      case 'sharing': await ndbRenderSharing(); break;
      case 'config': await ndbRenderConfig(); break;
    }
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.ndbSwitchTab = function(tab) {
  ndb.tab = tab;
  renderNucleusDB(tab);
};

// -- Browse Sub-Tab -----------------------------------------------------------
async function ndbRenderBrowse() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading data...</div>';

  try {
    const data = await api(`/nucleusdb/browse?page=${ndb.page}&page_size=${ndb.pageSize}&prefix=${encodeURIComponent(ndb.prefix)}&sort=${ndb.sort}&order=${ndb.order}`);
    const rows = data.rows || [];
    const total = data.total || 0;
    const totalPages = data.total_pages || 1;

    const sortIcon = (field) => {
      if (ndb.sort !== field) return '<span style="opacity:0.3">&#8597;</span>';
      return ndb.order === 'asc' ? '&#9650;' : '&#9660;';
    };

    el.innerHTML = `
      <div class="ndb-toolbar">
        <div style="display:flex;gap:8px;align-items:center;flex:1">
          <input type="text" id="ndb-search" placeholder="Filter by key prefix..." value="${esc(ndb.prefix)}"
            style="width:260px;padding:6px 10px;font-size:12px">
          <button class="btn btn-sm" onclick="ndbSearch()">Filter</button>
          ${ndb.prefix ? `<button class="btn btn-sm" onclick="ndbClearSearch()">Clear</button>` : ''}
          <span class="ndb-count">${total} key${total !== 1 ? 's' : ''}</span>
        </div>
        <div style="display:flex;gap:6px">
          <button class="btn btn-sm btn-primary" onclick="ndbNewKey()">+ New Key</button>
          <button class="btn btn-sm" onclick="ndbExport('json')">Export JSON</button>
          <button class="btn btn-sm" onclick="ndbExport('csv')">Export CSV</button>
        </div>
      </div>

      ${rows.length > 0 ? `
        <div class="table-wrap"><table class="ndb-table">
          <thead><tr>
            <th class="ndb-sortable" onclick="ndbSort('key')">Key ${sortIcon('key')}</th>
            <th style="width:70px">Type</th>
            <th class="ndb-sortable" onclick="ndbSort('value')">Value ${sortIcon('value')}</th>
            <th style="width:50px">Idx</th>
            <th style="width:140px;text-align:center">Actions</th>
          </tr></thead>
          <tbody>
            ${rows.map(row => `
              <tr data-key="${esc(row.key)}">
                <td class="ndb-key">${esc(row.key)}</td>
                <td>${typeBadge(row.type)}</td>
                <td class="ndb-value ndb-value-cell" data-key="${esc(row.key)}">${renderTypedValue(row)}</td>
                <td style="color:var(--text-dim);font-size:11px">${row.index}</td>
                <td class="ndb-actions">
                  <button class="btn-icon" data-ndb-action="verify" data-key="${esc(row.key)}" title="Verify Merkle proof">&#128737;</button>
                  <button class="btn-icon" data-ndb-action="history" data-key="${esc(row.key)}" title="Key history">&#128339;</button>
                  <button class="btn-icon" data-ndb-action="edit" data-key="${esc(row.key)}" title="Edit value">&#9998;</button>
                  <button class="btn-icon btn-icon-danger" data-ndb-action="delete" data-key="${esc(row.key)}" title="Delete">&#128465;</button>
                </td>
              </tr>
            `).join('')}
          </tbody>
        </table></div>

        <div class="ndb-pagination">
          <button class="btn btn-sm" onclick="ndbPageNav(0)" ${ndb.page === 0 ? 'disabled' : ''}>&#171; First</button>
          <button class="btn btn-sm" onclick="ndbPageNav(${ndb.page - 1})" ${ndb.page === 0 ? 'disabled' : ''}>&#8249; Prev</button>
          <span class="ndb-page-info">Page ${ndb.page + 1} of ${totalPages}</span>
          <button class="btn btn-sm" onclick="ndbPageNav(${ndb.page + 1})" ${ndb.page >= totalPages - 1 ? 'disabled' : ''}>Next &#8250;</button>
          <button class="btn btn-sm" onclick="ndbPageNav(${totalPages - 1})" ${ndb.page >= totalPages - 1 ? 'disabled' : ''}>Last &#187;</button>
          <select class="ndb-page-size" onchange="ndbChangePageSize(this.value)">
            ${[25, 50, 100, 200].map(n => `<option value="${n}" ${ndb.pageSize === n ? 'selected' : ''}>${n} / page</option>`).join('')}
          </select>
        </div>
      ` : `
        <div class="ndb-empty">
          <div style="font-size:36px;margin-bottom:12px;color:var(--accent)">&#9762;</div>
          <div style="font-size:14px;margin-bottom:8px;color:var(--accent)">No data stored yet</div>
          <div style="color:var(--text-muted);margin-bottom:16px;font-size:12px">Insert your first key-value pair to get started.</div>
          <button class="btn btn-primary" onclick="ndbNewKey()">+ Insert First Key</button>
          <button class="btn btn-sm" style="margin-left:8px" onclick="ndbSwitchTab('sql')">Open SQL Console</button>
        </div>
      `}

      <div id="ndb-detail-panel"></div>
    `;

    // Store rows for edit flow
    window._ndbRows = rows;

    // Bind Enter key on search input
    const searchInput = $('#ndb-search');
    if (searchInput) {
      searchInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') ndbSearch();
      });
    }

    const table = el.querySelector('.ndb-table');
    if (table) {
      table.addEventListener('dblclick', (e) => {
        const cell = e.target.closest('.ndb-value-cell');
        if (!cell) return;
        const key = cell.dataset.key || '';
        if (key) ndbStartEditTyped(key);
      });
      table.addEventListener('click', (e) => {
        const jsonToggle = e.target.closest('.ndb-json-toggle');
        if (jsonToggle) {
          ndbExpandJson(jsonToggle);
          return;
        }
        const btn = e.target.closest('[data-ndb-action]');
        if (!btn) return;
        const key = btn.dataset.key || '';
        if (!key) return;
        const action = btn.dataset.ndbAction;
        if (action === 'verify') ndbVerifyKey(key);
        else if (action === 'history') ndbKeyHistory(key);
        else if (action === 'edit') ndbStartEditTyped(key);
        else if (action === 'delete') ndbDeleteKey(key);
      });
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error loading data: ${esc(e.message)}</div>`;
  }
}

// JSON expand handler
window.ndbExpandJson = function(el, key) {
  const effectiveKey = key || el?.dataset?.key || '';
  if (!effectiveKey) return;
  const existing = el.parentElement.querySelector('.ndb-json-expanded');
  if (existing) { existing.remove(); return; }
  const row = (window._ndbRows || []).find(r => r.key === effectiveKey);
  if (!row) return;
  const div = document.createElement('div');
  div.className = 'ndb-json-expanded';
  div.textContent = typeof row.value === 'object' ? JSON.stringify(row.value, null, 2) : row.display;
  el.parentElement.appendChild(div);
};

window.ndbSearch = function() {
  ndb.prefix = ($('#ndb-search')?.value || '').trim();
  ndb.page = 0;
  ndbRenderBrowse();
};

window.ndbClearSearch = function() {
  ndb.prefix = '';
  ndb.page = 0;
  ndbRenderBrowse();
};

window.ndbSort = function(field) {
  if (ndb.sort === field) {
    ndb.order = ndb.order === 'asc' ? 'desc' : 'asc';
  } else {
    ndb.sort = field;
    ndb.order = 'asc';
  }
  ndb.page = 0;
  ndbRenderBrowse();
};

window.ndbPageNav = function(page) {
  ndb.page = Math.max(0, page);
  ndbRenderBrowse();
};

window.ndbChangePageSize = function(size) {
  ndb.pageSize = parseInt(size) || 50;
  ndb.page = 0;
  ndbRenderBrowse();
};

// Typed edit
window.ndbStartEditTyped = function(key) {
  const row = (window._ndbRows || []).find(r => r.key === key);
  const type = row?.type || 'integer';
  const val = row?.value;
  const panel = $('#ndb-detail-panel');

  let valueInput;
  switch (type) {
    case 'integer':
    case 'float':
      valueInput = `<input type="number" id="ndb-edit-value" value="${val != null ? val : 0}" step="${type === 'float' ? 'any' : '1'}"
        style="width:260px;padding:6px 10px;font-size:13px">`;
      break;
    case 'bool':
      valueInput = `<select id="ndb-edit-value" class="ndb-type-select" style="width:120px">
        <option value="true" ${val ? 'selected' : ''}>true</option>
        <option value="false" ${!val ? 'selected' : ''}>false</option>
      </select>`;
      break;
    case 'null':
      valueInput = `<span style="color:var(--text-muted);font-style:italic">NULL (no editable value)</span>
        <input type="hidden" id="ndb-edit-value" value="null">`;
      break;
    case 'text':
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px">${esc(val || '')}</textarea>`;
      break;
    case 'json':
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px;min-height:120px">${esc(typeof val === 'object' ? JSON.stringify(val, null, 2) : String(val))}</textarea>`;
      break;
    case 'vector': {
      const arrStr = Array.isArray(val) ? val.join(', ') : '';
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px" placeholder="0.1, 0.2, 0.3, ...">${esc(arrStr)}</textarea>
        <div style="color:var(--text-dim);font-size:10px;margin-top:2px">${Array.isArray(val) ? val.length + ' dimensions' : ''} &mdash; comma-separated floats</div>`;
      break;
    }
    case 'bytes':
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px" placeholder="hex bytes: 0a1b2c...">${esc(val || '')}</textarea>`;
      break;
    default:
      valueInput = `<input type="text" id="ndb-edit-value" value="${esc(String(val || ''))}"
        style="width:260px;padding:6px 10px;font-size:13px">`;
  }

  panel.innerHTML = `
    <div class="ndb-edit-panel">
      <div class="section-header">Edit Key</div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
        <label style="font-weight:600;min-width:50px;font-size:12px">Key:</label>
        <span style="color:var(--accent)">${esc(key)}</span>
        ${typeBadge(type)}
      </div>
      <div style="display:flex;gap:8px;align-items:flex-start;margin-bottom:12px">
        <label style="font-weight:600;min-width:50px;margin-top:6px;font-size:12px">Value:</label>
        <div>${valueInput}</div>
      </div>
      <div style="display:flex;gap:8px">
        <button class="btn btn-primary btn-sm" id="ndb-save-edit-btn" data-key="${esc(key)}" data-type="${esc(type)}">Save &amp; Commit</button>
        <button class="btn btn-sm" onclick="$('#ndb-detail-panel').innerHTML=''">Cancel</button>
      </div>
      <div id="ndb-edit-result" style="margin-top:8px"></div>
    </div>
  `;
  const saveBtn = $('#ndb-save-edit-btn');
  if (saveBtn) {
    saveBtn.addEventListener('click', () => {
      ndbSaveEditTyped(saveBtn.dataset.key || '', saveBtn.dataset.type || 'integer');
    });
  }
  const inp = $('#ndb-edit-value');
  if (inp && inp.focus) { inp.focus(); if (inp.select) inp.select(); }
};

window.ndbSaveEditTyped = async function(key, type) {
  const raw = $('#ndb-edit-value')?.value;
  let value;
  try {
    switch (type) {
      case 'integer': value = parseInt(raw); if (isNaN(value)) throw new Error('Invalid integer'); break;
      case 'float': value = parseFloat(raw); if (isNaN(value)) throw new Error('Invalid float'); break;
      case 'bool': value = raw === 'true'; break;
      case 'null': value = null; break;
      case 'text': value = raw; break;
      case 'json': value = JSON.parse(raw); break;
      case 'vector': {
        const nums = raw.split(',').map(s => parseFloat(s.trim())).filter(n => !isNaN(n));
        if (nums.length === 0) throw new Error('Vector must have at least one dimension');
        value = nums;
        break;
      }
      case 'bytes': value = raw; break;
      default: value = raw;
    }
  } catch (e) {
    $('#ndb-edit-result').innerHTML = `<div style="color:var(--red)">Invalid value: ${esc(e.message)}</div>`;
    return;
  }
  try {
    const res = await apiPost('/nucleusdb/edit', { key, type, value });
    if (res.error) {
      $('#ndb-edit-result').innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      const typeLabel = res.type ? ` (${res.type})` : '';
      $('#ndb-detail-panel').innerHTML = `<div style="color:var(--green);padding:8px;text-shadow:var(--glow-green)">Saved ${esc(key)}${typeLabel} and committed.</div>`;
      setTimeout(() => ndbRenderBrowse(), 800);
    }
  } catch (e) {
    $('#ndb-edit-result').innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

window.ndbNewKey = function() {
  const panel = $('#ndb-detail-panel');
  panel.innerHTML = `
    <div class="ndb-edit-panel">
      <div class="section-header">New Key-Value Pair</div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
        <label style="font-weight:600;min-width:50px;font-size:12px">Key:</label>
        <input type="text" id="ndb-new-key" placeholder="my_key" style="width:260px;padding:6px 10px;font-size:13px">
      </div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
        <label style="font-weight:600;min-width:50px;font-size:12px">Type:</label>
        <select id="ndb-new-type" class="ndb-type-select" onchange="ndbNewKeyTypeChanged()">
          <option value="integer">Integer</option>
          <option value="float">Float</option>
          <option value="text">Text</option>
          <option value="json">JSON</option>
          <option value="bool">Boolean</option>
          <option value="vector">Vector</option>
          <option value="null">Null</option>
        </select>
      </div>
      <div id="ndb-new-value-wrap" style="display:flex;gap:8px;align-items:flex-start;margin-bottom:12px">
        <label style="font-weight:600;min-width:50px;margin-top:6px;font-size:12px">Value:</label>
        <div id="ndb-new-value-input">
          <input type="number" id="ndb-new-value" value="0" style="width:260px;padding:6px 10px;font-size:13px">
        </div>
      </div>
      <div style="display:flex;gap:8px">
        <button class="btn btn-primary btn-sm" onclick="ndbInsertNew()">Insert &amp; Commit</button>
        <button class="btn btn-sm" onclick="$('#ndb-detail-panel').innerHTML=''">Cancel</button>
      </div>
      <div id="ndb-new-result" style="margin-top:8px"></div>
    </div>
  `;
  $('#ndb-new-key').focus();
};

window.ndbNewKeyTypeChanged = function() {
  const type = $('#ndb-new-type')?.value || 'integer';
  const wrap = $('#ndb-new-value-input');
  if (!wrap) return;
  switch (type) {
    case 'integer':
      wrap.innerHTML = `<input type="number" id="ndb-new-value" value="0" step="1" style="width:260px;padding:6px 10px;font-size:13px">`;
      break;
    case 'float':
      wrap.innerHTML = `<input type="number" id="ndb-new-value" value="0.0" step="any" style="width:260px;padding:6px 10px;font-size:13px">`;
      break;
    case 'text':
      wrap.innerHTML = `<textarea id="ndb-new-value" class="ndb-value-textarea" style="width:400px" placeholder="Enter text..."></textarea>`;
      break;
    case 'json':
      wrap.innerHTML = `<textarea id="ndb-new-value" class="ndb-value-textarea" style="width:400px;min-height:100px" placeholder='{"key": "value"}'>{}</textarea>`;
      break;
    case 'bool':
      wrap.innerHTML = `<select id="ndb-new-value" class="ndb-type-select" style="width:120px">
        <option value="true">true</option><option value="false">false</option></select>`;
      break;
    case 'vector':
      wrap.innerHTML = `<textarea id="ndb-new-value" class="ndb-value-textarea" style="width:400px" placeholder="0.1, 0.2, 0.3, ..."></textarea>
        <div style="color:var(--text-dim);font-size:10px;margin-top:2px">Comma-separated float values</div>`;
      break;
    case 'null':
      wrap.innerHTML = `<span style="color:var(--text-muted);font-style:italic">NULL &mdash; no value</span>
        <input type="hidden" id="ndb-new-value" value="null">`;
      break;
  }
};

window.ndbInsertNew = async function() {
  const key = ($('#ndb-new-key')?.value || '').trim();
  const type = ($('#ndb-new-type')?.value || 'integer');
  const raw = ($('#ndb-new-value')?.value || '').trim();

  if (!key) {
    $('#ndb-new-result').innerHTML = '<div style="color:var(--red)">Key cannot be empty</div>';
    return;
  }

  let value;
  try {
    switch (type) {
      case 'integer': value = parseInt(raw); if (isNaN(value)) throw new Error('Invalid integer'); break;
      case 'float': value = parseFloat(raw); if (isNaN(value)) throw new Error('Invalid float'); break;
      case 'bool': value = raw === 'true'; break;
      case 'null': value = null; break;
      case 'text': value = raw; break;
      case 'json': value = JSON.parse(raw); break;
      case 'vector': {
        const nums = raw.split(',').map(s => parseFloat(s.trim())).filter(n => !isNaN(n));
        if (nums.length === 0) throw new Error('Enter at least one number');
        value = nums;
        break;
      }
      default: value = raw;
    }
  } catch (e) {
    $('#ndb-new-result').innerHTML = `<div style="color:var(--red)">Invalid value: ${esc(e.message)}</div>`;
    return;
  }

  try {
    const res = await apiPost('/nucleusdb/edit', { key, type, value });
    if (res.error) {
      $('#ndb-new-result').innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      const typeLabel = res.type ? ` (${res.type})` : '';
      $('#ndb-detail-panel').innerHTML = `<div style="color:var(--green);padding:8px;text-shadow:var(--glow-green)">Inserted ${esc(key)}${typeLabel} and committed.</div>`;
      setTimeout(() => ndbRenderBrowse(), 800);
    }
  } catch (e) {
    $('#ndb-new-result').innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

window.ndbDeleteKey = async function(key) {
  if (!confirm(`Delete key '${key}'? This queues a tombstone (value=0) and commits.`)) return;
  try {
    const res = await apiPost('/nucleusdb/edit', { key, type: 'integer', value: 0 });
    if (res.error) {
      alert('Delete failed: ' + res.error);
    } else {
      ndbRenderBrowse();
    }
  } catch (e) {
    alert('Delete failed: ' + e.message);
  }
};

window.ndbVerifyKey = async function(key) {
  const panel = $('#ndb-detail-panel');
  panel.innerHTML = '<div style="color:var(--text-muted);padding:8px">Verifying Merkle proof...</div>';
  try {
    const res = await api(`/nucleusdb/verify/${encodeURIComponent(key)}`);
    if (!res.found) {
      panel.innerHTML = `<div class="ndb-verify-panel"><span class="badge badge-err">Key not found</span></div>`;
      return;
    }
    panel.innerHTML = `
      <div class="ndb-verify-panel">
        <div class="section-header">Merkle Proof Verification</div>
        <div class="ndb-verify-grid">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Key</span><span class="ndb-mono" style="color:var(--accent)">${esc(res.key)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Type</span>${typeBadge(res.type || 'integer')}</div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Value</span><span class="ndb-mono">${esc(res.display || String(res.value))}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Index</span><span class="ndb-mono">${res.index}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Backend</span><span class="ndb-mono">${esc(res.backend)}</span></div>
          ${res.blob_verified != null ? `
          <div class="ndb-verify-row"><span class="ndb-verify-label">Blob</span>
            <span>${res.blob_verified
              ? '<span class="badge badge-ok">Blob Verified</span>'
              : '<span class="badge badge-warn">No Blob</span>'}</span>
          </div>` : ''}
          <div class="ndb-verify-row">
            <span class="ndb-verify-label">Verified</span>
            <span>${res.verified
              ? '<span class="badge badge-ok" style="font-size:13px">&#10003; VERIFIED</span>'
              : '<span class="badge badge-err" style="font-size:13px">&#10007; FAILED</span>'
            }</span>
          </div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Root Hash</span><span class="ndb-mono ndb-hash">${esc(res.root_hash)}</span></div>
        </div>
        <button class="btn btn-sm" style="margin-top:8px" onclick="$('#ndb-detail-panel').innerHTML=''">Close</button>
      </div>
    `;
  } catch (e) {
    panel.innerHTML = `<div style="color:var(--red);padding:8px">Verify error: ${esc(e.message)}</div>`;
  }
};

window.ndbKeyHistory = async function(key) {
  const panel = $('#ndb-detail-panel');
  panel.innerHTML = '<div style="color:var(--text-muted);padding:8px">Loading history...</div>';
  try {
    const res = await api(`/nucleusdb/key-history/${encodeURIComponent(key)}`);
    if (!res.found) {
      panel.innerHTML = `<div class="ndb-verify-panel"><span class="badge badge-err">Key not found</span></div>`;
      return;
    }
    const typeTag = res.type || 'integer';
    const currentDisplay = res.current_display != null
      ? String(res.current_display)
      : String(res.current_value ?? '');
    const typedValue = res.current_typed_value !== undefined
      ? res.current_typed_value
      : res.current_value;
    const typedJson = JSON.stringify(typedValue, null, 2);

    panel.innerHTML = `
      <div class="ndb-verify-panel">
        <div class="section-header">Key History: ${esc(key)}</div>
        <div class="ndb-verify-grid" style="margin-bottom:12px">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Type</span>${typeBadge(typeTag)}</div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Display</span><span class="ndb-mono">${esc(currentDisplay)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Typed Value</span><span class="ndb-mono">${esc(truncate(typedJson, 120))}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Raw Value</span><span class="ndb-mono">${res.current_value}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Index</span><span class="ndb-mono">${res.index}</span></div>
        </div>
        ${typedJson.length > 120 ? `
          <details style="margin-bottom:10px">
            <summary style="cursor:pointer;color:var(--text-muted);font-size:12px">Show full typed value JSON</summary>
            <pre class="ndb-json-expanded">${esc(typedJson)}</pre>
          </details>
        ` : ''}
        ${res.commits && res.commits.length > 0 ? `
          <div style="font-size:12px;font-weight:600;margin-bottom:6px;color:var(--text-muted);text-transform:uppercase;letter-spacing:1px">Commits (${res.commits.length})</div>
          <div class="table-wrap"><table>
            <thead><tr><th>Height</th><th>State Root</th><th>Timestamp</th></tr></thead>
            <tbody>${res.commits.map(c => `
              <tr>
                <td style="color:var(--accent)">${c.height}</td>
                <td class="ndb-mono ndb-hash">${esc(c.state_root)}</td>
                <td style="font-size:11px">${c.timestamp_unix ? fmtTime(c.timestamp_unix) : 'n/a'}</td>
              </tr>
            `).join('')}</tbody>
          </table></div>
          ${res.note ? `<div style="color:var(--text-dim);font-size:11px;margin-top:4px">${esc(res.note)}</div>` : ''}
        ` : '<div style="color:var(--text-muted)">No commits yet.</div>'}
        <button class="btn btn-sm" style="margin-top:8px" onclick="$('#ndb-detail-panel').innerHTML=''">Close</button>
      </div>
    `;
  } catch (e) {
    panel.innerHTML = `<div style="color:var(--red);padding:8px">History error: ${esc(e.message)}</div>`;
  }
};

window.ndbExport = async function(fmt) {
  try {
    const res = await api(`/nucleusdb/export?format=${fmt}`);
    const text = fmt === 'csv' ? res.content : JSON.stringify(res.content, null, 2);
    const blob = new Blob([text], { type: fmt === 'csv' ? 'text/csv' : 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `nucleusdb_export.${fmt}`;
    a.click();
    URL.revokeObjectURL(url);
  } catch (e) {
    alert('Export failed: ' + e.message);
  }
};

// -- SQL Sub-Tab --------------------------------------------------------------
function ndbRenderSQL() {
  const el = $('#ndb-content');
  el.innerHTML = `
    <div style="margin:12px 0">
      <div style="display:flex;gap:8px;margin-bottom:8px">
        <input type="text" id="sql-input" placeholder="Enter SQL (e.g. SELECT * FROM data)"
          style="flex:1;padding:8px 12px;font-size:12px">
        <button class="btn btn-primary" onclick="runSQL()">Execute</button>
      </div>
      <div class="config-desc" style="margin-bottom:4px">
        <strong style="color:var(--accent)">Supported:</strong> SELECT, INSERT, UPDATE, DELETE, COMMIT, VERIFY, SHOW STATUS/HISTORY/MODE/TYPES, VECTOR_SEARCH, SET MODE APPEND_ONLY, EXPORT
      </div>
      <div class="ndb-sql-presets">
        <span style="color:var(--text-dim);font-size:11px">Quick:</span>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SELECT * FROM data')">All Data</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW TYPES')">Types</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW STATUS')">Status</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW HISTORY')">History</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('EXPORT')">Export</button>
      </div>
      <div class="ndb-sql-presets" style="margin-top:2px">
        <span style="color:var(--text-dim);font-size:11px">Insert:</span>
        <button class="btn btn-xs" onclick="ndbSQLPreset(&quot;INSERT INTO data (key, value) VALUES ('mykey', 'hello world')&quot;)">Text</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset(&quot;INSERT INTO data (key, value) VALUES ('mykey', '{\\&quot;name\\&quot;:\\&quot;Alice\\&quot;}')&quot;)">JSON</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset(&quot;INSERT INTO data (key, value) VALUES ('mykey', VECTOR(0.1, 0.2, 0.3))&quot;)">Vector</button>
      </div>
      <div id="sql-result" style="margin-top:12px"></div>
    </div>
  `;
  const inp = $('#sql-input');
  if (inp) inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') runSQL(); });
}

window.ndbSQLPreset = function(sql) {
  const inp = $('#sql-input');
  if (inp) { inp.value = sql; runSQL(); }
};

window.runSQL = async function() {
  const query = ($('#sql-input')?.value || '').trim();
  if (!query) return;
  const el = $('#sql-result');
  el.innerHTML = '<div style="color:var(--text-muted)">Executing...</div>';
  try {
    const data = await apiPost('/nucleusdb/sql', { query });
    if (data.error) {
      el.innerHTML = `<div style="color:var(--red)">Error: ${esc(data.error)}</div>`;
    } else if (data.columns && data.rows) {
      if (data.rows.length === 0) {
        el.innerHTML = `<div style="color:var(--text-muted)">No rows returned.</div>`;
      } else {
        el.innerHTML = `<div class="table-wrap"><table>
          <thead><tr>${data.columns.map(c => `<th>${esc(c)}</th>`).join('')}</tr></thead>
          <tbody>${data.rows.map(row =>
            `<tr>${row.map(cell => `<td style="font-size:11px">${esc(cell)}</td>`).join('')}</tr>`
          ).join('')}</tbody>
        </table></div>
        <div style="color:var(--text-muted);font-size:11px;margin-top:4px">${data.rows.length} row(s)</div>`;
      }
    } else if (data.message) {
      el.innerHTML = `<div style="color:var(--green);text-shadow:var(--glow-green)">${esc(data.message)}</div>`;
    } else {
      el.innerHTML = `<pre class="ndb-json-expanded">${esc(JSON.stringify(data, null, 2))}</pre>`;
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

// -- Vectors Sub-Tab ----------------------------------------------------------
async function ndbRenderVectors() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading vector index...</div>';
  try {
    const stats = window._ndbStats || await api('/nucleusdb/stats');
    const vecCount = stats.vector_count || 0;
    const vecDims = stats.vector_dims || 0;

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="card-grid">
          <div class="card">
            <div class="card-label">Vectors Indexed</div>
            <div class="card-value">${vecCount}</div>
          </div>
          <div class="card">
            <div class="card-label">Dimensions</div>
            <div class="card-value">${vecDims || 'n/a'}</div>
          </div>
          <div class="card">
            <div class="card-label">Blob Storage</div>
            <div class="card-value" style="font-size:14px">${formatBytes(stats.blob_total_bytes || 0)}</div>
            <div class="card-sub">${stats.blob_count || 0} objects</div>
          </div>
        </div>

        <div class="section-header">Similarity Search</div>
        <div class="ndb-vector-search">
          <div style="display:flex;gap:8px;align-items:flex-start;margin-bottom:8px">
            <label style="font-weight:600;min-width:60px;margin-top:6px;font-size:12px">Query:</label>
            <textarea id="ndb-vec-query" class="ndb-value-textarea" style="width:400px;min-height:40px"
              placeholder="0.1, 0.2, 0.3, ...${vecDims ? ' (' + vecDims + ' dims)' : ''}"></textarea>
          </div>
          <div style="display:flex;gap:12px;align-items:center;margin-bottom:12px">
            <label style="font-weight:600;min-width:60px;font-size:12px">Metric:</label>
            <select id="ndb-vec-metric" class="ndb-type-select">
              <option value="cosine">Cosine</option>
              <option value="l2">L2 (Euclidean)</option>
              <option value="inner_product">Inner Product</option>
            </select>
            <label style="font-weight:500;margin-left:8px;font-size:12px">k:</label>
            <input type="number" id="ndb-vec-k" value="10" min="1" max="100"
              style="width:60px;padding:6px 10px;font-size:12px">
            <button class="btn btn-primary btn-sm" onclick="ndbVectorSearch()">Search</button>
          </div>
          ${vecCount === 0 ? `<div style="color:var(--text-muted);font-size:12px">No vectors in the index yet. Insert vectors via the Browse tab or SQL console.</div>` : ''}
        </div>
        <div id="ndb-vec-results"></div>

        <div class="section-header">Insert Vector</div>
        <div class="ndb-vector-search">
          <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
            <label style="font-weight:600;min-width:60px;font-size:12px">Key:</label>
            <input type="text" id="ndb-vec-insert-key" placeholder="doc:embedding:1"
              style="width:260px;padding:6px 10px;font-size:12px">
          </div>
          <div style="display:flex;gap:8px;align-items:flex-start;margin-bottom:8px">
            <label style="font-weight:600;min-width:60px;margin-top:6px;font-size:12px">Dims:</label>
            <textarea id="ndb-vec-insert-dims" class="ndb-value-textarea" style="width:400px;min-height:40px"
              placeholder="0.1, 0.2, 0.3, ..."></textarea>
          </div>
          <div style="display:flex;gap:8px">
            <button class="btn btn-primary btn-sm" onclick="ndbVectorInsert()">Insert &amp; Commit</button>
          </div>
          <div id="ndb-vec-insert-result" style="margin-top:8px"></div>
        </div>
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

window.ndbVectorSearch = async function() {
  const raw = ($('#ndb-vec-query')?.value || '').trim();
  const metric = $('#ndb-vec-metric')?.value || 'cosine';
  const k = parseInt($('#ndb-vec-k')?.value) || 10;
  const el = $('#ndb-vec-results');

  if (!raw) { el.innerHTML = '<div style="color:var(--red)">Enter a query vector</div>'; return; }

  const query = raw.split(',').map(s => parseFloat(s.trim())).filter(n => !isNaN(n));
  if (query.length === 0) { el.innerHTML = '<div style="color:var(--red)">Invalid vector</div>'; return; }

  el.innerHTML = '<div style="color:var(--text-muted)">Searching...</div>';
  try {
    const res = await apiPost('/nucleusdb/vector-search', { query, k, metric });
    if (res.error) {
      el.innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
      return;
    }
    const results = res.results || [];
    const totalVectors = res.total_vectors ?? res.vector_count ?? 0;
    if (results.length === 0) {
      el.innerHTML = `<div style="color:var(--text-muted)">No results found. ${totalVectors === 0 ? 'Index is empty.' : ''}</div>`;
      return;
    }
    el.innerHTML = `
      <div class="section-header" style="margin-top:12px">Results (${results.length} nearest, ${esc(metric)})</div>
      <div class="ndb-vector-results">
        ${results.map((r, i) => `
          <div class="ndb-vector-result-item">
            <span class="ndb-vector-rank">#${i + 1}</span>
            <span class="ndb-key" style="flex:1">${esc(r.key)}</span>
            <span class="ndb-vector-dist">${typeof r.distance === 'number' ? r.distance.toFixed(6) : r.distance}</span>
            <button class="btn-icon ndb-vec-verify-btn" data-key="${esc(r.key)}" title="Verify">&#128737;</button>
          </div>
        `).join('')}
      </div>
    `;
    $$('.ndb-vec-verify-btn', el).forEach((btn) => {
      btn.addEventListener('click', () => {
        const key = btn.dataset.key || '';
        if (key) ndbVerifyKey(key);
      });
    });
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Search error: ${esc(e.message)}</div>`;
  }
};

window.ndbVectorInsert = async function() {
  const key = ($('#ndb-vec-insert-key')?.value || '').trim();
  const raw = ($('#ndb-vec-insert-dims')?.value || '').trim();
  const el = $('#ndb-vec-insert-result');

  if (!key) { el.innerHTML = '<div style="color:var(--red)">Key cannot be empty</div>'; return; }
  const nums = raw.split(',').map(s => parseFloat(s.trim())).filter(n => !isNaN(n));
  if (nums.length === 0) { el.innerHTML = '<div style="color:var(--red)">Enter at least one dimension</div>'; return; }

  try {
    const res = await apiPost('/nucleusdb/edit', { key, type: 'vector', value: nums });
    if (res.error) {
      el.innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      el.innerHTML = `<div style="color:var(--green);text-shadow:var(--glow-green)">Inserted ${esc(key)} &mdash; ${nums.length}d vector committed.</div>`;
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

// -- Commits Sub-Tab (Seal Chain Visualization) -------------------------------
async function ndbRenderCommits() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading seal chain...</div>';
  try {
    const history = await api('/nucleusdb/history');
    const commits = history.commits?.rows || [];
    const columns = history.commits?.columns || [];

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="seal-chain-status ${commits.length > 0 ? 'ok' : ''}">
          <span class="seal-chain-indicator">${commits.length > 0
            ? '&#10003; SEAL CHAIN UNBROKEN'
            : '&#9888; NO COMMITS'}</span>
          <span style="color:var(--text-dim);font-size:11px;margin-left:auto">${commits.length} commit${commits.length !== 1 ? 's' : ''}</span>
        </div>

        ${commits.length > 0 ? `
          <div class="seal-chain">
            ${commits.slice().reverse().slice(0, 20).map((row, i) => {
              const height = row[0];
              const rootHash = row[1] || '';
              const timestamp = row[2] || '';
              return `
                <div class="seal-node">
                  <div class="seal-height">Commit #${esc(String(height))}</div>
                  <div class="seal-detail"><span>Root:</span> ${esc(truncate(rootHash, 48))}</div>
                  <div class="seal-detail"><span>Seal:</span> SHA-256(seal_${height > 0 ? height - 1 : 0} | kv_digest)</div>
                  <div class="seal-detail"><span>Time:</span> ${esc(timestamp)}</div>
                </div>
                ${i < Math.min(commits.length, 20) - 1 ? '<div class="seal-connector"></div>' : ''}
              `;
            }).join('')}
          </div>
          ${commits.length > 20 ? `<div style="color:var(--text-dim);font-size:11px;margin-top:8px;text-align:center">Showing 20 of ${commits.length} commits</div>` : ''}
        ` : '<div style="color:var(--text-muted);padding:24px;text-align:center">No commits yet. Insert data and COMMIT to create the first seal.</div>'}
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

// -- Proofs Sub-Tab (NEW) -----------------------------------------------------
function ndbRenderProofs() {
  const el = $('#ndb-content');
  const stats = window._ndbStats || {};
  const status = window._ndbStatus || {};
  const backend = status.backend || 'binary_merkle';
  const bi = backendInfo[backend] || backendInfo.binary_merkle;

  el.innerHTML = `
    <div style="margin:12px 0">
      <div class="proof-section">
        <div class="proof-section-title">Active Backend</div>
        <div class="ndb-verify-grid">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Engine</span><span class="ndb-mono" style="color:var(--accent)">${esc(bi.name)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Algorithm</span><span class="ndb-mono">${esc(bi.algo)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Security</span><span class="ndb-mono">${esc(bi.type)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Proof Size</span><span class="ndb-mono">${esc(bi.proof)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Setup</span><span class="ndb-mono">${esc(bi.setup)}</span></div>
        </div>
        <div style="color:var(--text-dim);font-size:11px;margin-top:8px">
          Every key has a position in a Merkle tree. A proof is a path from the leaf to the root.
          If ANY value changes, the root changes. Verification: ${esc(bi.proof)} hashes.
        </div>
      </div>

      ${stats.sth ? `
      <div class="proof-section">
        <div class="proof-section-title">Certificate Transparency (RFC 6962)</div>
        <div class="ndb-verify-grid">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Tree Size</span><span class="ndb-mono" style="color:var(--accent)">${stats.sth.tree_size}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Root Hash</span><span class="ndb-mono ndb-hash">${esc(stats.sth.root_hash)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Timestamp</span><span class="ndb-mono">${stats.sth.timestamp_unix ? fmtTime(stats.sth.timestamp_unix) : 'n/a'}</span></div>
        </div>
      </div>
      ` : ''}

      <div class="proof-section">
        <div class="proof-section-title">Verify a Key</div>
        <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
          <input type="text" id="ndb-proof-key" placeholder="Enter key to verify..." style="flex:1;padding:8px 12px;font-size:12px">
          <button class="btn btn-primary btn-sm" onclick="ndbProofVerify()">Verify</button>
        </div>
        <div id="ndb-proof-result"></div>
      </div>

      <div class="proof-section">
        <div class="proof-section-title">Backend Comparison</div>
        <div class="backend-comparison">
          <div class="backend-card ${backend === 'binary_merkle' ? 'active' : ''}">
            <div class="backend-card-name">BinaryMerkle</div>
            <div class="backend-card-detail">SHA-256</div>
            <div class="backend-card-detail">Post-Quantum</div>
            <div class="backend-card-detail">O(log n) proof</div>
            <div class="backend-card-detail">No trusted setup</div>
            <div style="margin-top:6px">${backend === 'binary_merkle' ? '<span class="badge badge-ok">ACTIVE</span>' : '<span class="badge badge-muted">Available</span>'}</div>
          </div>
          <div class="backend-card ${backend === 'ipa' ? 'active' : ''}">
            <div class="backend-card-name">IPA</div>
            <div class="backend-card-detail">Pedersen</div>
            <div class="backend-card-detail">Binding</div>
            <div class="backend-card-detail">O(n) proof*</div>
            <div class="backend-card-detail">No trusted setup</div>
            <div style="margin-top:6px">${backend === 'ipa' ? '<span class="badge badge-ok">ACTIVE</span>' : '<span class="badge badge-muted">Available</span>'}</div>
          </div>
          <div class="backend-card ${backend === 'kzg' ? 'active' : ''}">
            <div class="backend-card-name">KZG</div>
            <div class="backend-card-detail">BLS12-381</div>
            <div class="backend-card-detail">Pairing</div>
            <div class="backend-card-detail">O(1) proof**</div>
            <div class="backend-card-detail">Trusted setup</div>
            <div style="margin-top:6px">${backend === 'kzg' ? '<span class="badge badge-ok">ACTIVE</span>' : '<span class="badge badge-muted">Available</span>'}</div>
          </div>
        </div>
        <div style="color:var(--text-dim);font-size:10px;margin-top:8px">
          * IPA currently carries full vector (P1.3 planned) &nbsp;&nbsp;
          ** KZG requires consumer to have same trusted setup
        </div>
      </div>
    </div>
  `;

  const inp = $('#ndb-proof-key');
  if (inp) inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') ndbProofVerify(); });
}

window.ndbProofVerify = async function() {
  const key = ($('#ndb-proof-key')?.value || '').trim();
  if (!key) return;
  const el = $('#ndb-proof-result');
  el.innerHTML = '<div style="color:var(--text-muted)">Verifying...</div>';
  try {
    const res = await api(`/nucleusdb/verify/${encodeURIComponent(key)}`);
    if (!res.found) {
      el.innerHTML = `<span class="badge badge-err">Key not found</span>`;
      return;
    }
    el.innerHTML = `
      <div class="ndb-verify-grid" style="margin-top:8px">
        <div class="ndb-verify-row"><span class="ndb-verify-label">Key</span><span class="ndb-mono" style="color:var(--accent)">${esc(res.key)}</span></div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Type</span>${typeBadge(res.type || 'integer')}</div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Value</span><span class="ndb-mono">${esc(res.display || String(res.value))}</span></div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Backend</span><span class="ndb-mono">${esc(res.backend)}</span></div>
        <div class="ndb-verify-row">
          <span class="ndb-verify-label">Status</span>
          <span>${res.verified
            ? '<span class="badge badge-ok" style="font-size:12px">&#10003; VERIFIED</span>'
            : '<span class="badge badge-err" style="font-size:12px">&#10007; FAILED</span>'
          }</span>
        </div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Root Hash</span><span class="ndb-mono ndb-hash">${esc(res.root_hash)}</span></div>
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

// -- Sharing Sub-Tab (NucleusPOD) ---------------------------------------------
function ndbGrantShortHex(hex) {
  if (!hex || hex.length <= 24) return hex || '';
  return `${hex.slice(0, 14)}...${hex.slice(-8)}`;
}

function ndbGrantFormatExpiry(expiresAt) {
  if (!expiresAt) return 'No expiry';
  return `Expires ${new Date(expiresAt * 1000).toLocaleString()}`;
}

async function ndbRenderSharing() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading sharing controls...</div>';
  try {
    const modeQuery = ndbSharing.includeRevoked ? 'include_revoked=true' : 'active=true';
    const [stats, grantResp] = await Promise.all([
      api('/nucleusdb/stats'),
      api(`/nucleusdb/grants?${modeQuery}`),
    ]);
    window._ndbStats = stats;
    const grants = grantResp?.grants || [];
    const activeGrants = stats?.grant_active_count ?? grantResp?.active_total ?? 0;
    const totalGrants = stats?.grant_count ?? grantResp?.total ?? grants.length;

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="proof-section">
          <div class="proof-section-title">NucleusPOD &mdash; Proof-Carrying Data Sharing</div>
          <div style="color:var(--text-dim);font-size:12px;margin-bottom:12px">
            Share verified records with other agents. Each shared item carries its own cryptographic proof &mdash;
            the recipient verifies independently without trusting the sender.
          </div>
          <div style="display:flex;gap:12px;flex-wrap:wrap">
            <div class="card" style="flex:1;min-width:140px">
              <div class="card-label">Proof Envelopes</div>
              <div class="card-value" style="font-size:16px;color:var(--text-muted)">0</div>
              <div class="card-sub">Self-contained proofs</div>
            </div>
            <div class="card" style="flex:1;min-width:140px">
              <div class="card-label">Access Grants</div>
              <div class="card-value" style="font-size:16px">${Number(activeGrants).toLocaleString()}</div>
              <div class="card-sub">${Number(totalGrants).toLocaleString()} total</div>
            </div>
          </div>
        </div>

        <div class="proof-section">
          <div class="proof-section-title">Access Grants</div>
          <div style="color:var(--text-dim);font-size:12px;margin-bottom:12px">
            Grant per-key read/write/append access to specific agents. PUF identifiers are 32-byte hex fingerprints.
          </div>

          <div class="grant-form-grid">
            <input id="ndb-grant-grantor" type="text" placeholder="Grantor PUF (0x + 64 hex chars)" class="input">
            <input id="ndb-grant-grantee" type="text" placeholder="Grantee PUF (0x + 64 hex chars)" class="input">
            <input id="ndb-grant-pattern" type="text" placeholder="Key pattern (examples: docs/*, report:2026, *)" class="input">
            <input id="ndb-grant-expiry" type="datetime-local" class="input">
          </div>

          <div class="grant-toolbar">
            <label><input id="ndb-grant-read" type="checkbox" checked> READ</label>
            <label><input id="ndb-grant-write" type="checkbox"> WRITE</label>
            <label><input id="ndb-grant-append" type="checkbox"> APPEND</label>
            <button class="btn btn-sm" onclick="ndbCreateGrant()">Create Grant</button>
            <button class="btn btn-sm" onclick="ndbRefreshGrants()">Refresh</button>
            <label><input id="ndb-grant-show-revoked" type="checkbox" ${ndbSharing.includeRevoked ? 'checked' : ''} onchange="ndbToggleRevoked(this.checked)"> Show revoked/expired</label>
          </div>

          <div id="ndb-grant-status" style="color:var(--text-dim);font-size:11px;margin:8px 0 2px">Loaded ${grants.length} grant(s).</div>

          <div id="ndb-grant-list">
            ${grants.length === 0
              ? `<div class="grant-empty">No grants to display.</div>`
              : grants.map(g => `
                <div class="grant-card">
                  <div class="grant-header">
                    <div class="grant-id">${esc(g.grant_id_hex || '')}</div>
                    <div>
                      ${g.active
                        ? '<span class="badge badge-ok">ACTIVE</span>'
                        : (g.revoked ? '<span class="badge badge-err">REVOKED</span>' : '<span class="badge badge-warn">EXPIRED</span>')
                      }
                      ${g.revoked ? '' : `<button class="btn-icon btn-icon-danger" title="Revoke grant" onclick="ndbRevokeGrant('${g.grant_id_hex}')">&#10005;</button>`}
                    </div>
                  </div>
                  <div class="grant-detail"><span>Key Pattern:</span> <code>${esc(g.key_pattern || '')}</code></div>
                  <div class="grant-detail"><span>Grantor:</span> <code>${esc(ndbGrantShortHex(g.grantor_puf_hex || ''))}</code> &nbsp; <span>Grantee:</span> <code>${esc(ndbGrantShortHex(g.grantee_puf_hex || ''))}</code></div>
                  <div class="grant-detail">
                    <span>Permissions:</span>
                    <span class="grant-perm ${g.permissions?.read ? 'active' : ''}">READ</span>
                    <span class="grant-perm ${g.permissions?.write ? 'active' : ''}">WRITE</span>
                    <span class="grant-perm ${g.permissions?.append ? 'active' : ''}">APPEND</span>
                    &nbsp; <span>${esc(ndbGrantFormatExpiry(g.expires_at))}</span>
                  </div>
                </div>
              `).join('')
            }
          </div>
        </div>

        <div class="proof-section">
          <div class="proof-section-title">How It Works</div>
          <div class="ndb-verify-grid">
            <div class="ndb-verify-row"><span class="ndb-verify-label">Envelope</span><span style="color:var(--text-dim);font-size:11px">Self-contained proof unit: data + Merkle proof + metadata + author PUF</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Grants</span><span style="color:var(--text-dim);font-size:11px">Per-key access control: grantor PUF + grantee PUF + key pattern + permissions + expiry</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Discovery</span><span style="color:var(--text-dim);font-size:11px">.well-known/nucleus-pod &mdash; JSON capabilities doc for agent discovery</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Verify</span><span style="color:var(--text-dim);font-size:11px">Recipients verify proofs locally &mdash; no trust in sender required</span></div>
          </div>
        </div>
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Sharing tab load failed: ${esc(e.message)}</div>`;
  }
}

window.ndbToggleRevoked = async function(on) {
  ndbSharing.includeRevoked = !!on;
  await ndbRenderSharing();
};

window.ndbRefreshGrants = async function() {
  await ndbRenderSharing();
};

window.ndbCreateGrant = async function() {
  const statusEl = $('#ndb-grant-status');
  const grantorRaw = ($('#ndb-grant-grantor')?.value || '').trim();
  const granteeRaw = ($('#ndb-grant-grantee')?.value || '').trim();
  const keyPattern = ($('#ndb-grant-pattern')?.value || '').trim();
  const expiryRaw = ($('#ndb-grant-expiry')?.value || '').trim();
  const read = !!($('#ndb-grant-read') && $('#ndb-grant-read').checked);
  const write = !!($('#ndb-grant-write') && $('#ndb-grant-write').checked);
  const append = !!($('#ndb-grant-append') && $('#ndb-grant-append').checked);

  const normalizeHex = (v) => {
    const s = v.toLowerCase().replace(/^0x/, '');
    return s.length === 64 && /^[0-9a-f]+$/.test(s) ? `0x${s}` : null;
  };

  const grantor = normalizeHex(grantorRaw);
  const grantee = normalizeHex(granteeRaw);
  if (!grantor || !grantee) {
    if (statusEl) statusEl.innerHTML = '<span style="color:var(--red)">Grantor and grantee must be 32-byte hex PUF values.</span>';
    return;
  }
  if (!keyPattern) {
    if (statusEl) statusEl.innerHTML = '<span style="color:var(--red)">Key pattern is required.</span>';
    return;
  }
  if (!read && !write && !append) {
    if (statusEl) statusEl.innerHTML = '<span style="color:var(--red)">Enable at least one permission.</span>';
    return;
  }

  let expiresAt = null;
  if (expiryRaw) {
    const ms = Date.parse(expiryRaw);
    if (!Number.isFinite(ms)) {
      if (statusEl) statusEl.innerHTML = '<span style="color:var(--red)">Invalid expiry date/time.</span>';
      return;
    }
    expiresAt = Math.floor(ms / 1000);
  }

  if (statusEl) statusEl.innerHTML = '<span style="color:var(--text-muted)">Creating grant...</span>';
  try {
    await apiPost('/nucleusdb/grants', {
      grantor_puf_hex: grantor,
      grantee_puf_hex: grantee,
      key_pattern: keyPattern,
      permissions: { read, write, append },
      expires_at: expiresAt,
    });
    if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">Grant created.</span>';
    await ndbRenderSharing();
  } catch (e) {
    if (statusEl) statusEl.innerHTML = `<span style="color:var(--red)">Create failed: ${esc(e.message)}</span>`;
  }
};

window.ndbRevokeGrant = async function(grantIdHex) {
  if (!grantIdHex) return;
  const statusEl = $('#ndb-grant-status');
  if (statusEl) statusEl.innerHTML = '<span style="color:var(--text-muted)">Revoking grant...</span>';
  try {
    await apiPost(`/nucleusdb/grants/${encodeURIComponent(grantIdHex)}/revoke`, {});
    if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">Grant revoked.</span>';
    await ndbRenderSharing();
  } catch (e) {
    if (statusEl) statusEl.innerHTML = `<span style="color:var(--red)">Revoke failed: ${esc(e.message)}</span>`;
  }
};

// -- Config Sub-Tab (Merged Schema + Settings) --------------------------------
async function ndbRenderConfig() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading config...</div>';
  try {
    const stats = window._ndbStats || await api('/nucleusdb/stats');
    const prefixes = stats.top_prefixes || [];

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="card-grid">
          <div class="card">
            <div class="card-label">Total Keys</div>
            <div class="card-value">${stats.key_count}</div>
          </div>
          <div class="card">
            <div class="card-label">Commits</div>
            <div class="card-value">${stats.commit_count}</div>
          </div>
          <div class="card">
            <div class="card-label">Write Mode</div>
            <div class="card-value" style="font-size:13px">${esc(stats.write_mode)}</div>
          </div>
          <div class="card">
            <div class="card-label">DB Size</div>
            <div class="card-value" style="font-size:14px">${formatBytes(stats.db_size_bytes)}</div>
          </div>
        </div>

        ${stats.type_distribution ? `
          <div class="section-header">Type Distribution</div>
          <div class="ndb-type-dist">
            ${Object.entries(stats.type_distribution).sort((a,b) => b[1] - a[1]).map(([t, count]) =>
              `<div class="ndb-type-dist-item">${typeBadge(t)} <span class="ndb-type-dist-count">${count.toLocaleString()}</span></div>`
            ).join('')}
          </div>
        ` : ''}

        <div class="section-header">Storage</div>
        <div class="card-grid">
          <div class="card">
            <div class="card-label">Blob Objects</div>
            <div class="card-value">${stats.blob_count || 0}</div>
            <div class="card-sub">${formatBytes(stats.blob_total_bytes || 0)} stored</div>
          </div>
          <div class="card">
            <div class="card-label">Vectors</div>
            <div class="card-value">${stats.vector_count || 0}</div>
            <div class="card-sub">${stats.vector_dims ? stats.vector_dims + ' dimensions' : 'No vectors yet'}</div>
          </div>
        </div>

        ${prefixes.length > 0 ? `
          <div class="section-header">Key Prefix Distribution</div>
          <div class="ndb-prefix-list">
            ${prefixes.map(p => `
              <div class="ndb-prefix-item">
                <span class="ndb-prefix-name clickable" data-prefix="${esc(p.prefix)}">${esc(p.prefix)}</span>
                <div class="ndb-prefix-bar-wrap">
                  <div class="ndb-prefix-bar" style="width:${Math.max(4, (p.count / (prefixes[0]?.count || 1)) * 100)}%"></div>
                </div>
                <span style="color:var(--text-muted);font-size:12px">${p.count}</span>
              </div>
            `).join('')}
          </div>
        ` : ''}

        <div class="section-header">Write Mode</div>
        <div style="display:flex;align-items:center;gap:12px;margin-bottom:12px">
          <span class="badge ${stats.write_mode === 'AppendOnly' ? 'badge-warn' : 'badge-ok'}" style="font-size:12px">
            ${esc(stats.write_mode)}
          </span>
          ${stats.write_mode !== 'AppendOnly' ? `
            <button class="btn btn-sm" onclick="ndbSetAppendOnly()">Lock to Append-Only</button>
            <span style="color:var(--text-dim);font-size:11px">INSERT only. UPDATE/DELETE disabled. Irreversible.</span>
          ` : `
            <span style="color:var(--text-dim);font-size:11px">Database is locked. INSERT only.</span>
          `}
        </div>

        <div class="section-header">Export</div>
        <div style="display:flex;gap:8px;margin-bottom:12px">
          <button class="btn btn-sm" onclick="ndbExport('json')">Export JSON</button>
          <button class="btn btn-sm" onclick="ndbExport('csv')">Export CSV</button>
        </div>

        <div class="section-header">Database Path</div>
        <div style="color:var(--text-dim);font-size:11px">${esc((window._ndbStatus || {}).db_path || 'unknown')}</div>
      </div>
    `;

    $$('.ndb-prefix-name.clickable', el).forEach((node) => {
      node.addEventListener('click', () => {
        ndb.prefix = node.dataset.prefix || '';
        ndb.page = 0;
        ndbSwitchTab('browse');
      });
    });
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

window.ndbSetAppendOnly = async function() {
  if (!confirm('Lock database to AppendOnly mode? This is IRREVERSIBLE. UPDATE and DELETE will be permanently disabled.')) return;
  try {
    const res = await apiPost('/nucleusdb/sql', { query: 'SET MODE APPEND_ONLY' });
    if (res.error) {
      alert('Failed: ' + res.error);
    } else {
      ndbRenderConfig();
    }
  } catch (e) {
    alert('Failed: ' + e.message);
  }
};

// =============================================================================
// Particle Network — amber constellation mesh (inspired by apoth3osis.io banner)
// =============================================================================
(function() {
  let _raf = 0;
  const PARTICLE_COUNT = 80;
  const CONNECT_DIST = 110;
  const SPEED = 0.12;

  function initParticles(canvasId) {
    const canvas = document.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    // Cancel any prior animation loop for this canvas
    if (_raf) cancelAnimationFrame(_raf);

    const rect = canvas.parentElement.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    canvas.style.width = rect.width + 'px';
    canvas.style.height = rect.height + 'px';
    ctx.scale(dpr, dpr);

    const W = rect.width;
    const H = rect.height;

    // Create particles
    const particles = [];
    for (let i = 0; i < PARTICLE_COUNT; i++) {
      particles.push({
        x: Math.random() * W,
        y: Math.random() * H,
        vx: (Math.random() - 0.5) * SPEED * 2,
        vy: (Math.random() - 0.5) * SPEED * 2,
        r: Math.random() * 1.6 + 0.6,          // radius 0.6 – 2.2
        brightness: Math.random() * 0.5 + 0.3   // 0.3 – 0.8
      });
    }

    function draw() {
      ctx.clearRect(0, 0, W, H);

      // Subtle background gradient (dark, barely visible)
      const bg = ctx.createRadialGradient(W * 0.3, H * 0.4, 0, W * 0.5, H * 0.5, W * 0.8);
      bg.addColorStop(0, 'rgba(255, 106, 0, 0.04)');
      bg.addColorStop(0.5, 'rgba(255, 159, 42, 0.02)');
      bg.addColorStop(1, 'transparent');
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, W, H);

      // Update positions
      for (const p of particles) {
        p.x += p.vx;
        p.y += p.vy;
        if (p.x < 0 || p.x > W) p.vx *= -1;
        if (p.y < 0 || p.y > H) p.vy *= -1;
        p.x = Math.max(0, Math.min(W, p.x));
        p.y = Math.max(0, Math.min(H, p.y));
      }

      // Draw connections
      for (let i = 0; i < particles.length; i++) {
        for (let j = i + 1; j < particles.length; j++) {
          const dx = particles[i].x - particles[j].x;
          const dy = particles[i].y - particles[j].y;
          const dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < CONNECT_DIST) {
            const alpha = (1 - dist / CONNECT_DIST) * 0.25;
            ctx.strokeStyle = `rgba(255, 140, 20, ${alpha})`;
            ctx.lineWidth = 0.5;
            ctx.beginPath();
            ctx.moveTo(particles[i].x, particles[i].y);
            ctx.lineTo(particles[j].x, particles[j].y);
            ctx.stroke();
          }
        }
      }

      // Draw nodes
      for (const p of particles) {
        // Outer glow
        const glow = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, p.r * 4);
        glow.addColorStop(0, `rgba(255, 140, 20, ${p.brightness * 0.35})`);
        glow.addColorStop(1, 'transparent');
        ctx.fillStyle = glow;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r * 4, 0, Math.PI * 2);
        ctx.fill();

        // Core dot
        ctx.fillStyle = `rgba(255, 159, 42, ${p.brightness})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fill();
      }

      _raf = requestAnimationFrame(draw);
    }

    draw();
  }

  // Expose for use after NucleusDB tab renders
  window._initHeroParticles = function() {
    // Small delay to let DOM settle
    setTimeout(() => initParticles('hero-particles'), 50);
  };

  // Clean up on page navigation
  window._destroyHeroParticles = function() {
    if (_raf) { cancelAnimationFrame(_raf); _raf = 0; }
  };
})();
