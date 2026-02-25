/* Agent H.A.L.O. Dashboard — Embedded SPA */
'use strict';

const $ = (sel, ctx) => (ctx || document).querySelector(sel);
const $$ = (sel, ctx) => [...(ctx || document).querySelectorAll(sel)];
const content = $('#content');

// -- Routing ------------------------------------------------------------------
const pages = { overview: renderOverview, sessions: renderSessions, costs: renderCosts,
  config: renderConfig, trust: renderTrust, nucleusdb: renderNucleusDB };

function route() {
  const hash = location.hash.replace('#/', '') || 'overview';
  const page = hash.split('/')[0];
  const arg = hash.split('/').slice(1).join('/');
  $$('.nav-link').forEach(a => a.classList.toggle('active', a.dataset.page === page));
  if (pages[page]) pages[page](arg);
  else content.innerHTML = '<div class="loading">Page not found</div>';
}

window.addEventListener('hashchange', route);
window.addEventListener('DOMContentLoaded', route);

// -- Theme --------------------------------------------------------------------
function toggleTheme() {
  document.body.classList.toggle('light');
  localStorage.setItem('theme', document.body.classList.contains('light') ? 'light' : 'dark');
}
if (localStorage.getItem('theme') === 'light') document.body.classList.add('light');

// -- API helpers --------------------------------------------------------------
async function api(path) {
  const res = await fetch('/api' + path);
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

async function apiPost(path, body) {
  const res = await fetch('/api' + path, {
    method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify(body)
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

// -- HTML escaping (XSS prevention) -------------------------------------------
function esc(s) {
  if (s == null) return '';
  return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;');
}

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
function statusBadge(status) {
  const cls = status === 'completed' ? 'badge-ok' : status === 'failed' ? 'badge-err' :
    status === 'running' ? 'badge-info' : 'badge-muted';
  return `<span class="badge ${cls}">${esc(status)}</span>`;
}
function eventTypeBadge(type) {
  const colors = { assistant: '#58a6ff', tool_call: '#bc8cff', tool_result: '#bc8cff',
    mcp_tool_call: '#d29922', file_change: '#3fb950', bash_command: '#db6d28',
    error: '#f85149', thinking: '#8b949e' };
  const c = colors[type] || '#8b949e';
  return `<span class="event-type" style="background:${c}22;color:${c}">${esc(type)}</span>`;
}

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
          <div class="card-value">${s.authenticated ? '<span class="badge badge-ok">Authenticated</span>' : '<span class="badge badge-warn">Not Auth</span>'}</div>
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
          <div class="card-value" style="font-size:14px">
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
                <td style="font-family:var(--font-mono);font-size:12px">${esc(truncate(ss.session_id, 24))}</td>
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
      ` : '<div style="color:var(--text-muted)">No sessions recorded yet. Run <code>agenthalo run claude ...</code> to start.</div>'}
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
              backgroundColor: 'rgba(88, 166, 255, 0.5)',
              borderColor: '#58a6ff',
              borderWidth: 1,
            }]
          },
          options: {
            responsive: true,
            plugins: { legend: { display: false } },
            scales: {
              y: { beginAtZero: true, ticks: { callback: v => '$' + v.toFixed(2) },
                grid: { color: 'rgba(139,148,158,0.1)' } },
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
        <span style="color:var(--text-muted);font-size:13px">${items.length} sessions</span>
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
    <td style="font-family:var(--font-mono);font-size:12px">${esc(truncate(ss.session_id, 28))}</td>
    <td>${esc(ss.agent)}</td>
    <td>${esc(truncate(ss.model || 'unknown', 22))}</td>
    <td>${fmtTokens(tokens)}</td>
    <td>${fmtCost(sm.estimated_cost_usd)}</td>
    <td>${fmtDuration(sm.duration_secs)}</td>
    <td style="font-size:12px">${fmtTime(ss.started_at)}</td>
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
          <div class="card-value" style="font-size:18px">${esc(ss.agent)}</div>
          <div class="card-sub">${esc(ss.model || 'unknown')}</div>
        </div>
        <div class="card">
          <div class="card-label">Tokens</div>
          <div class="card-value" style="font-size:18px">${fmtTokens(tokens)}</div>
          <div class="card-sub">In: ${fmtTokens(sm.total_input_tokens)} / Out: ${fmtTokens(sm.total_output_tokens)}</div>
        </div>
        <div class="card">
          <div class="card-label">Cost</div>
          <div class="card-value" style="font-size:18px">${fmtCost(sm.estimated_cost_usd)}</div>
          <div class="card-sub">${fmtDuration(sm.duration_secs)}</div>
        </div>
        <div class="card">
          <div class="card-label">Activity</div>
          <div class="card-value" style="font-size:14px">
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
            ${ev.input_tokens ? `<span style="color:var(--text-muted);font-size:11px;margin-left:8px">in:${ev.input_tokens} out:${ev.output_tokens || 0}</span>` : ''}
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
      <div class="page-title">Costs & Analytics</div>

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
            borderColor: '#58a6ff', backgroundColor: 'rgba(88,166,255,0.1)',
            fill: true, tension: 0.3, pointRadius: 3,
          }]
        },
        options: {
          responsive: true,
          plugins: { legend: { display: false } },
          scales: {
            y: { beginAtZero: true, ticks: { callback: v => '$' + v.toFixed(2) },
              grid: { color: 'rgba(139,148,158,0.1)' } },
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
          datasets: [{ data: agents.map(a => a.cost_usd),
            backgroundColor: ['#58a6ff', '#3fb950', '#d29922', '#f85149', '#bc8cff', '#db6d28'] }]
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
            backgroundColor: 'rgba(188,140,255,0.5)', borderColor: '#bc8cff', borderWidth: 1 }]
        },
        options: {
          responsive: true, indexAxis: 'y',
          plugins: { legend: { display: false } },
          scales: {
            x: { beginAtZero: true, ticks: { callback: v => '$' + v.toFixed(2) },
              grid: { color: 'rgba(139,148,158,0.1)' } },
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
            <div class="config-desc" style="font-family:var(--font-mono);font-size:11px">${esc(cfg.onchain.contract_address || '(not deployed)')}</div>
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

      <div class="section-header">Paths</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row"><div><div class="config-label">Home</div><div class="config-desc" style="font-family:var(--font-mono);font-size:11px">${esc(cfg.paths.home)}</div></div></div>
        <div class="config-row"><div><div class="config-label">Database</div><div class="config-desc" style="font-family:var(--font-mono);font-size:11px">${esc(cfg.paths.db)}</div></div></div>
        <div class="config-row"><div><div class="config-label">PQ Wallet</div><div class="config-desc">${cfg.pq_wallet ? 'Present (ML-DSA-65)' : 'Not created'}</div></div></div>
      </div>
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
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

// =============================================================================
// PAGE: Trust & Attestations
// =============================================================================
async function renderTrust() {
  content.innerHTML = '<div class="loading">Loading attestations...</div>';
  try {
    const data = await api('/attestations');
    const attestations = data.attestations || [];

    content.innerHTML = `
      <div class="page-title">Trust & Attestations</div>

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
      <div style="display:flex;gap:8px;margin-bottom:24px">
        <input type="text" id="verify-digest" placeholder="Paste attestation digest..." style="flex:1;padding:8px 12px;border:1px solid var(--border);border-radius:6px;background:var(--bg-card);color:var(--text);font-family:var(--font-mono);font-size:13px">
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
                <td style="font-family:var(--font-mono);font-size:11px">${esc(truncate(a.attestation_digest || '', 32))}</td>
                <td><span class="badge badge-info">${esc(a.proof_type || 'merkle')}</span></td>
                <td style="font-family:var(--font-mono);font-size:11px">${esc(truncate(a.session_id || '', 24))}</td>
                <td style="font-family:var(--font-mono);font-size:11px">${a.tx_hash ? esc(truncate(a.tx_hash, 24)) : '-'}</td>
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
        <div class="card-sub">${esc(data.reason || 'Recomputed attestation does not match stored digest — possible tampering.')}
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
// PAGE: NucleusDB — Full Database Browser
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

async function renderNucleusDB(subtab) {
  ndb.tab = subtab || ndb.tab || 'browse';
  content.innerHTML = '<div class="loading">Loading NucleusDB...</div>';

  try {
    const status = await api('/nucleusdb/status');

    content.innerHTML = `
      <div class="page-title">NucleusDB</div>
      <div class="card-grid" style="margin-bottom:16px">
        <div class="card">
          <div class="card-label">Backend</div>
          <div class="card-value" style="font-size:16px">${esc(status.backend)}</div>
          <div class="card-sub">SHA-256 Merkle tree</div>
        </div>
        <div class="card">
          <div class="card-label">Sessions</div>
          <div class="card-value">${status.session_count}</div>
        </div>
        <div class="card">
          <div class="card-label">Database</div>
          <div class="card-value" style="font-size:14px">
            ${status.exists ? '<span class="badge badge-ok">Active</span>' : '<span class="badge badge-warn">Not Created</span>'}
          </div>
          <div class="card-sub" style="font-family:var(--font-mono);font-size:10px">${esc(status.db_path)}</div>
        </div>
      </div>

      <div class="ndb-tabs">
        <button class="ndb-tab ${ndb.tab === 'browse' ? 'active' : ''}" onclick="ndbSwitchTab('browse')">Browse</button>
        <button class="ndb-tab ${ndb.tab === 'sql' ? 'active' : ''}" onclick="ndbSwitchTab('sql')">SQL</button>
        <button class="ndb-tab ${ndb.tab === 'commits' ? 'active' : ''}" onclick="ndbSwitchTab('commits')">Commits</button>
        <button class="ndb-tab ${ndb.tab === 'schema' ? 'active' : ''}" onclick="ndbSwitchTab('schema')">Schema</button>
        <button class="ndb-tab ${ndb.tab === 'settings' ? 'active' : ''}" onclick="ndbSwitchTab('settings')">Settings</button>
      </div>
      <div id="ndb-content"></div>
    `;

    // Render active sub-tab
    switch (ndb.tab) {
      case 'browse': await ndbRenderBrowse(); break;
      case 'sql': ndbRenderSQL(); break;
      case 'commits': await ndbRenderCommits(); break;
      case 'schema': await ndbRenderSchema(); break;
      case 'settings': await ndbRenderSettings(); break;
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
            style="width:260px;padding:6px 10px;border:1px solid var(--border);border-radius:6px;background:var(--bg-card);color:var(--text);font-family:var(--font-mono);font-size:13px">
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
            <th class="ndb-sortable" onclick="ndbSort('value')" style="width:140px">Value ${sortIcon('value')}</th>
            <th style="width:70px">Index</th>
            <th style="width:180px;text-align:center">Actions</th>
          </tr></thead>
          <tbody>
            ${rows.map(row => `
              <tr data-key="${esc(row.key)}">
                <td class="ndb-key">${esc(row.key)}</td>
                <td class="ndb-value" ondblclick="ndbStartEdit(this, '${esc(row.key)}', ${row.value})">${row.value}</td>
                <td style="color:var(--text-muted);font-size:12px">${row.index}</td>
                <td class="ndb-actions">
                  <button class="btn-icon" onclick="ndbVerifyKey('${esc(row.key)}')" title="Verify Merkle proof">&#128737;</button>
                  <button class="btn-icon" onclick="ndbKeyHistory('${esc(row.key)}')" title="Key history">&#128339;</button>
                  <button class="btn-icon" onclick="ndbStartEdit(null, '${esc(row.key)}', ${row.value})" title="Edit value">&#9998;</button>
                  <button class="btn-icon btn-icon-danger" onclick="ndbDeleteKey('${esc(row.key)}')" title="Delete">&#128465;</button>
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
          <div style="font-size:48px;margin-bottom:12px">&#9683;</div>
          <div style="font-size:16px;margin-bottom:8px">No data stored yet</div>
          <div style="color:var(--text-muted);margin-bottom:16px">Insert your first key-value pair to get started.</div>
          <button class="btn btn-primary" onclick="ndbNewKey()">+ Insert First Key</button>
          <button class="btn btn-sm" style="margin-left:8px" onclick="ndbSwitchTab('sql')">Open SQL Console</button>
        </div>
      `}

      <div id="ndb-detail-panel"></div>
    `;

    // Bind Enter key on search input
    const searchInput = $('#ndb-search');
    if (searchInput) {
      searchInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') ndbSearch();
      });
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error loading data: ${esc(e.message)}</div>`;
  }
}

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

window.ndbStartEdit = function(cell, key, currentValue) {
  const panel = $('#ndb-detail-panel');
  panel.innerHTML = `
    <div class="ndb-edit-panel">
      <div class="section-header">Edit Key</div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
        <label style="font-weight:600;min-width:50px">Key:</label>
        <span style="font-family:var(--font-mono)">${esc(key)}</span>
      </div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:12px">
        <label style="font-weight:600;min-width:50px">Value:</label>
        <input type="number" id="ndb-edit-value" value="${currentValue}"
          style="width:200px;padding:6px 10px;border:1px solid var(--border);border-radius:6px;background:var(--bg-card);color:var(--text);font-family:var(--font-mono);font-size:14px">
      </div>
      <div style="display:flex;gap:8px">
        <button class="btn btn-primary btn-sm" onclick="ndbSaveEdit('${esc(key)}')">Save &amp; Commit</button>
        <button class="btn btn-sm" onclick="$('#ndb-detail-panel').innerHTML=''">Cancel</button>
      </div>
      <div id="ndb-edit-result" style="margin-top:8px"></div>
    </div>
  `;
  $('#ndb-edit-value').focus();
  $('#ndb-edit-value').select();
};

window.ndbSaveEdit = async function(key) {
  const value = parseInt($('#ndb-edit-value')?.value);
  if (isNaN(value)) {
    $('#ndb-edit-result').innerHTML = '<div style="color:var(--red)">Value must be an integer</div>';
    return;
  }
  try {
    const res = await apiPost('/nucleusdb/edit', { key, value });
    if (res.error) {
      $('#ndb-edit-result').innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      $('#ndb-detail-panel').innerHTML = `<div style="color:var(--green);padding:8px">Saved ${esc(key)} = ${value} and committed.</div>`;
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
        <label style="font-weight:600;min-width:50px">Key:</label>
        <input type="text" id="ndb-new-key" placeholder="my_key"
          style="width:260px;padding:6px 10px;border:1px solid var(--border);border-radius:6px;background:var(--bg-card);color:var(--text);font-family:var(--font-mono);font-size:14px">
      </div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:12px">
        <label style="font-weight:600;min-width:50px">Value:</label>
        <input type="number" id="ndb-new-value" value="0"
          style="width:200px;padding:6px 10px;border:1px solid var(--border);border-radius:6px;background:var(--bg-card);color:var(--text);font-family:var(--font-mono);font-size:14px">
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

window.ndbInsertNew = async function() {
  const key = ($('#ndb-new-key')?.value || '').trim();
  const value = parseInt($('#ndb-new-value')?.value);
  if (!key) {
    $('#ndb-new-result').innerHTML = '<div style="color:var(--red)">Key cannot be empty</div>';
    return;
  }
  if (isNaN(value)) {
    $('#ndb-new-result').innerHTML = '<div style="color:var(--red)">Value must be an integer</div>';
    return;
  }
  try {
    const res = await apiPost('/nucleusdb/edit', { key, value });
    if (res.error) {
      $('#ndb-new-result').innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      $('#ndb-detail-panel').innerHTML = `<div style="color:var(--green);padding:8px">Inserted ${esc(key)} = ${value} and committed.</div>`;
      setTimeout(() => ndbRenderBrowse(), 800);
    }
  } catch (e) {
    $('#ndb-new-result').innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

window.ndbDeleteKey = async function(key) {
  if (!confirm(`Delete key '${key}'? This queues a tombstone (value=0) and commits.`)) return;
  try {
    const res = await apiPost('/nucleusdb/edit', { key, value: 0 });
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
          <div class="ndb-verify-row"><span class="ndb-verify-label">Key</span><span class="ndb-mono">${esc(res.key)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Value</span><span class="ndb-mono">${res.value}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Index</span><span class="ndb-mono">${res.index}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Backend</span><span class="ndb-mono">${esc(res.backend)}</span></div>
          <div class="ndb-verify-row">
            <span class="ndb-verify-label">Verified</span>
            <span>${res.verified
              ? '<span class="badge badge-ok" style="font-size:14px">&#10003; VERIFIED</span>'
              : '<span class="badge badge-err" style="font-size:14px">&#10007; FAILED</span>'
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
    panel.innerHTML = `
      <div class="ndb-verify-panel">
        <div class="section-header">Key History: ${esc(key)}</div>
        <div class="ndb-verify-grid" style="margin-bottom:12px">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Current Value</span><span class="ndb-mono">${res.current_value}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Index</span><span class="ndb-mono">${res.index}</span></div>
        </div>
        ${res.commits && res.commits.length > 0 ? `
          <div style="font-size:13px;font-weight:600;margin-bottom:6px">Commits (${res.commits.length})</div>
          <div class="table-wrap"><table>
            <thead><tr><th>Height</th><th>State Root</th><th>Timestamp</th></tr></thead>
            <tbody>${res.commits.map(c => `
              <tr>
                <td>${c.height}</td>
                <td class="ndb-mono ndb-hash">${esc(c.state_root)}</td>
                <td style="font-size:12px">${c.timestamp_unix ? fmtTime(c.timestamp_unix) : 'n/a'}</td>
              </tr>
            `).join('')}</tbody>
          </table></div>
          ${res.note ? `<div style="color:var(--text-muted);font-size:12px;margin-top:4px">${esc(res.note)}</div>` : ''}
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
    <div style="margin:16px 0">
      <div style="display:flex;gap:8px;margin-bottom:8px">
        <input type="text" id="sql-input" placeholder="Enter SQL (e.g. SELECT * FROM data)"
          style="flex:1;padding:8px 12px;border:1px solid var(--border);border-radius:6px;background:var(--bg-card);color:var(--text);font-family:var(--font-mono);font-size:13px">
        <button class="btn btn-primary" onclick="runSQL()">Execute</button>
      </div>
      <div class="config-desc" style="margin-bottom:4px">
        <strong>Supported:</strong> SELECT, INSERT, UPDATE, DELETE, COMMIT, VERIFY 'key', SHOW STATUS/HISTORY/MODE, SET MODE APPEND_ONLY, EXPORT
      </div>
      <div class="ndb-sql-presets">
        <span style="color:var(--text-muted);font-size:12px">Quick:</span>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SELECT * FROM data')">All Data</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW STATUS')">Status</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW HISTORY')">History</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW MODE')">Mode</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('EXPORT')">Export</button>
      </div>
      <div id="sql-result" style="margin-top:12px"></div>
    </div>
  `;
  // Bind Enter key
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
            `<tr>${row.map(cell => `<td style="font-family:var(--font-mono);font-size:12px">${esc(cell)}</td>`).join('')}</tr>`
          ).join('')}</tbody>
        </table></div>
        <div style="color:var(--text-muted);font-size:12px;margin-top:4px">${data.rows.length} row(s)</div>`;
      }
    } else if (data.message) {
      el.innerHTML = `<div style="color:var(--green)">${esc(data.message)}</div>`;
    } else {
      el.innerHTML = `<pre style="background:var(--bg-card);border:1px solid var(--border);border-radius:6px;padding:12px;font-family:var(--font-mono);font-size:12px;overflow-x:auto">${esc(JSON.stringify(data, null, 2))}</pre>`;
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

// -- Commits Sub-Tab ----------------------------------------------------------
async function ndbRenderCommits() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading commits...</div>';
  try {
    const history = await api('/nucleusdb/history');
    el.innerHTML = `
      <div style="margin:16px 0">
        <div class="section-header">Commit Ledger</div>
        ${history.commits && history.commits.rows && history.commits.rows.length > 0 ? `
          <div class="table-wrap"><table>
            <thead><tr>${(history.commits.columns || []).map(c => `<th>${esc(c)}</th>`).join('')}</tr></thead>
            <tbody>
              ${history.commits.rows.map(row =>
                `<tr>${row.map((cell, i) => `<td class="ndb-mono" style="font-size:12px">${i === 1 ? `<span class="ndb-hash">${esc(cell)}</span>` : esc(cell)}</td>`).join('')}</tr>`
              ).join('')}
            </tbody>
          </table></div>
          <div style="color:var(--text-muted);font-size:12px;margin-top:4px">${history.commits.rows.length} commit(s)</div>
        ` : '<div style="color:var(--text-muted)">No commits yet. Insert data and COMMIT to create the first entry.</div>'}

        <div class="section-header" style="margin-top:24px">Recent Sessions</div>
        ${(history.sessions || []).length > 0 ? `
          <div class="table-wrap"><table>
            <thead><tr><th>Session ID</th><th>Agent</th><th>Model</th><th>Started</th><th>Status</th></tr></thead>
            <tbody>
              ${(history.sessions || []).map(h => `
                <tr class="clickable" onclick="location.hash='#/sessions/${encodeURIComponent(h.session_id)}'">
                  <td class="ndb-mono" style="font-size:12px">${esc(truncate(h.session_id, 28))}</td>
                  <td>${esc(h.agent)}</td>
                  <td>${esc(truncate(h.model || 'unknown', 20))}</td>
                  <td style="font-size:12px">${fmtTime(h.started_at)}</td>
                  <td>${statusBadge(h.status)}</td>
                </tr>
              `).join('')}
            </tbody>
          </table></div>
        ` : '<div style="color:var(--text-muted)">No sessions recorded.</div>'}
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

// -- Schema Sub-Tab -----------------------------------------------------------
async function ndbRenderSchema() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading schema...</div>';
  try {
    const stats = await api('/nucleusdb/stats');
    const prefixes = stats.top_prefixes || [];

    el.innerHTML = `
      <div style="margin:16px 0">
        <div class="section-header">Database Statistics</div>
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
            <div class="card-value" style="font-size:14px">${esc(stats.write_mode)}</div>
          </div>
          <div class="card">
            <div class="card-label">DB Size</div>
            <div class="card-value" style="font-size:16px">${formatBytes(stats.db_size_bytes)}</div>
          </div>
        </div>

        ${stats.key_count > 0 ? `
          <div class="section-header" style="margin-top:16px">Value Statistics</div>
          <div class="card-grid">
            <div class="card">
              <div class="card-label">Min</div>
              <div class="card-value">${stats.value_min != null ? stats.value_min : 'n/a'}</div>
            </div>
            <div class="card">
              <div class="card-label">Max</div>
              <div class="card-value">${stats.value_max != null ? stats.value_max : 'n/a'}</div>
            </div>
            <div class="card">
              <div class="card-label">Average</div>
              <div class="card-value" style="font-size:16px">${stats.value_avg != null ? stats.value_avg.toFixed(2) : 'n/a'}</div>
            </div>
            <div class="card">
              <div class="card-label">Sum</div>
              <div class="card-value" style="font-size:16px">${stats.value_sum != null ? stats.value_sum.toLocaleString() : 'n/a'}</div>
            </div>
          </div>
        ` : ''}

        ${prefixes.length > 0 ? `
          <div class="section-header" style="margin-top:16px">Key Prefix Distribution</div>
          <div class="ndb-prefix-list">
            ${prefixes.map(p => `
              <div class="ndb-prefix-item">
                <span class="ndb-prefix-name clickable" onclick="ndb.prefix='${esc(p.prefix)}';ndb.page=0;ndbSwitchTab('browse')">${esc(p.prefix)}</span>
                <div class="ndb-prefix-bar-wrap">
                  <div class="ndb-prefix-bar" style="width:${Math.max(4, (p.count / (prefixes[0]?.count || 1)) * 100)}%"></div>
                </div>
                <span class="ndb-prefix-count">${p.count}</span>
              </div>
            `).join('')}
          </div>
        ` : ''}

        ${stats.sth ? `
          <div class="section-header" style="margin-top:16px">Signed Tree Head</div>
          <div class="ndb-verify-grid">
            <div class="ndb-verify-row"><span class="ndb-verify-label">Tree Size</span><span class="ndb-mono">${stats.sth.tree_size}</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Root Hash</span><span class="ndb-mono ndb-hash">${esc(stats.sth.root_hash)}</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Timestamp</span><span>${fmtTime(stats.sth.timestamp_unix)}</span></div>
          </div>
        ` : ''}
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

function formatBytes(bytes) {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let val = bytes;
  while (val >= 1024 && i < units.length - 1) { val /= 1024; i++; }
  return val.toFixed(i === 0 ? 0 : 1) + ' ' + units[i];
}

// -- Settings Sub-Tab ---------------------------------------------------------
async function ndbRenderSettings() {
  const el = $('#ndb-content');
  el.innerHTML = '<div style="color:var(--text-muted)">Loading settings...</div>';
  try {
    const stats = await api('/nucleusdb/stats');
    el.innerHTML = `
      <div style="margin:16px 0">
        <div class="section-header">Write Mode</div>
        <div style="display:flex;align-items:center;gap:12px;margin-bottom:16px">
          <span class="badge ${stats.write_mode === 'AppendOnly' ? 'badge-warn' : 'badge-ok'}" style="font-size:14px">
            ${esc(stats.write_mode)}
          </span>
          ${stats.write_mode !== 'AppendOnly' ? `
            <button class="btn btn-sm" onclick="ndbSetAppendOnly()">Lock to Append-Only</button>
            <span style="color:var(--text-muted);font-size:12px">INSERT only. UPDATE/DELETE disabled. Irreversible.</span>
          ` : `
            <span style="color:var(--text-muted);font-size:12px">Database is locked. INSERT only. UPDATE/DELETE are disabled.</span>
          `}
        </div>

        <div class="section-header">Export</div>
        <div style="display:flex;gap:8px;margin-bottom:16px">
          <button class="btn btn-sm" onclick="ndbExport('json')">Export JSON</button>
          <button class="btn btn-sm" onclick="ndbExport('csv')">Export CSV</button>
        </div>

        <div class="section-header">Database Info</div>
        <div class="ndb-verify-grid">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Keys</span><span>${stats.key_count}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Commits</span><span>${stats.commit_count}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">DB Size</span><span>${formatBytes(stats.db_size_bytes)}</span></div>
        </div>
      </div>
    `;
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
      ndbRenderSettings();
    }
  } catch (e) {
    alert('Failed: ' + e.message);
  }
};
