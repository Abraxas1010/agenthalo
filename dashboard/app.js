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
  return `<span class="badge ${cls}">${status}</span>`;
}
function eventTypeBadge(type) {
  const colors = { assistant: '#58a6ff', tool_call: '#bc8cff', tool_result: '#bc8cff',
    mcp_tool_call: '#d29922', file_change: '#3fb950', bash_command: '#db6d28',
    error: '#f85149', thinking: '#8b949e' };
  const c = colors[type] || '#8b949e';
  return `<span class="event-type" style="background:${c}22;color:${c}">${type}</span>`;
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
              return `<tr class="clickable" onclick="location.hash='#/sessions/${ss.session_id}'">
                <td style="font-family:var(--font-mono);font-size:12px">${truncate(ss.session_id, 24)}</td>
                <td>${ss.agent}</td>
                <td>${truncate(ss.model || 'unknown', 20)}</td>
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
    content.innerHTML = `<div class="loading">Error loading dashboard: ${e.message}</div>`;
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
    content.innerHTML = `<div class="loading">Error: ${e.message}</div>`;
  }
}

function sessionRow(item) {
  const ss = item.session, sm = item.summary || {};
  const tokens = (sm.total_input_tokens || 0) + (sm.total_output_tokens || 0);
  return `<tr class="clickable session-row" data-agent="${ss.agent}" data-model="${ss.model || ''}"
    onclick="location.hash='#/sessions/${ss.session_id}'">
    <td style="font-family:var(--font-mono);font-size:12px">${truncate(ss.session_id, 28)}</td>
    <td>${ss.agent}</td>
    <td>${truncate(ss.model || 'unknown', 22)}</td>
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
      <div class="page-title">${truncate(ss.session_id, 32)} ${statusBadge(ss.status)}</div>

      <div class="card-grid">
        <div class="card">
          <div class="card-label">Agent</div>
          <div class="card-value" style="font-size:18px">${ss.agent}</div>
          <div class="card-sub">${ss.model || 'unknown'}</div>
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
        <button class="btn" onclick="exportSession('${ss.session_id}')">Export JSON</button>
        <button class="btn btn-primary" onclick="attestSession('${ss.session_id}')">Attest</button>
      </div>

      <div class="section-header">Event Timeline (${events.length} events)</div>
      <div class="event-timeline">
        ${events.map(ev => `
          <div class="event-item">
            <span class="event-seq">#${ev.seq}</span>
            ${eventTypeBadge(ev.event_type)}
            <span class="event-content">${truncate(JSON.stringify(ev.content), 100)}</span>
            ${ev.input_tokens ? `<span style="color:var(--text-muted);font-size:11px;margin-left:8px">in:${ev.input_tokens} out:${ev.output_tokens || 0}</span>` : ''}
          </div>
        `).join('')}
      </div>
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${e.message}</div>`;
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

window.attestSession = async function(id) {
  if (!confirm('Create attestation for this session?')) return;
  try {
    const data = await apiPost('/sessions/' + encodeURIComponent(id) + '/attest', {});
    alert('Attestation created!\nDigest: ' + (data.attestation?.attestation_digest || 'unknown'));
  } catch (e) { alert('Attestation failed: ' + e.message); }
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
              <tr><td>${op.operation}</td><td>${op.count}</td><td>${fmtTokens(op.credits_spent)}</td><td>${fmtCost(op.usd_spent)}</td></tr>
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
    content.innerHTML = `<div class="loading">Error: ${e.message}</div>`;
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
          <div class="config-desc">Shell RC: ${cfg.wrapping.shell_rc}</div>
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
            <div class="config-desc">${cfg.agentpmt.budget_tag || '(none)'}</div>
          </div>
        </div>
      </div>

      <div class="section-header">On-Chain</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">Chain</div>
            <div class="config-desc">${cfg.onchain.chain_name || 'Not configured'} (ID: ${cfg.onchain.chain_id})</div>
          </div>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Contract</div>
            <div class="config-desc" style="font-family:var(--font-mono);font-size:11px">${cfg.onchain.contract_address || '(not deployed)'}</div>
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
        <div class="config-row"><div><div class="config-label">Home</div><div class="config-desc" style="font-family:var(--font-mono);font-size:11px">${cfg.paths.home}</div></div></div>
        <div class="config-row"><div><div class="config-label">Database</div><div class="config-desc" style="font-family:var(--font-mono);font-size:11px">${cfg.paths.db}</div></div></div>
        <div class="config-row"><div><div class="config-label">PQ Wallet</div><div class="config-desc">${cfg.pq_wallet ? 'Present (ML-DSA-65)' : 'Not created'}</div></div></div>
      </div>
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${e.message}</div>`;
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
                <td style="font-family:var(--font-mono);font-size:11px">${truncate(a.attestation_digest || '', 32)}</td>
                <td><span class="badge badge-info">${a.proof_type || 'merkle'}</span></td>
                <td style="font-family:var(--font-mono);font-size:11px">${truncate(a.session_id || '', 24)}</td>
                <td style="font-family:var(--font-mono);font-size:11px">${a.tx_hash ? truncate(a.tx_hash, 24) : '-'}</td>
              </tr>
            `).join('')}
          </tbody>
        </table></div>
      ` : '<div style="color:var(--text-muted)">No attestations created yet.</div>'}
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${e.message}</div>`;
  }
}

window.verifyDigest = async function() {
  const digest = ($('#verify-digest')?.value || '').trim();
  if (!digest) return;
  const el = $('#verify-result');
  el.innerHTML = '<div style="color:var(--text-muted)">Checking...</div>';
  try {
    const data = await apiPost('/attestations/verify', { digest });
    if (data.found) {
      el.innerHTML = '<div class="card" style="border-color:var(--green)"><div class="card-label" style="color:var(--green)">VERIFIED</div><div class="card-sub">Attestation found in local store</div></div>';
    } else {
      el.innerHTML = '<div class="card" style="border-color:var(--yellow)"><div class="card-label" style="color:var(--yellow)">NOT FOUND</div><div class="card-sub">No attestation with this digest in local store</div></div>';
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Verification failed: ${e.message}</div>`;
  }
};

// =============================================================================
// PAGE: NucleusDB
// =============================================================================
async function renderNucleusDB() {
  content.innerHTML = '<div class="loading">Loading NucleusDB status...</div>';
  try {
    const [status, history] = await Promise.all([
      api('/nucleusdb/status'), api('/nucleusdb/history')
    ]);

    content.innerHTML = `
      <div class="page-title">NucleusDB</div>

      <div class="card-grid">
        <div class="card">
          <div class="card-label">Backend</div>
          <div class="card-value" style="font-size:16px">${status.backend}</div>
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
          <div class="card-sub" style="font-family:var(--font-mono);font-size:10px">${status.db_path}</div>
        </div>
      </div>

      <div class="section-header">SQL Console</div>
      <div style="margin-bottom:24px">
        <div style="display:flex;gap:8px;margin-bottom:8px">
          <input type="text" id="sql-input" placeholder="Enter SQL query (e.g. SELECT * FROM kv)" style="flex:1;padding:8px 12px;border:1px solid var(--border);border-radius:6px;background:var(--bg-card);color:var(--text);font-family:var(--font-mono);font-size:13px">
          <button class="btn btn-primary" onclick="runSQL()">Execute</button>
        </div>
        <div class="config-desc">Full SQL execution available via <code>nucleusdb sql</code> CLI or <code>nucleusdb tui</code></div>
        <div id="sql-result" style="margin-top:8px"></div>
      </div>

      <div class="section-header">Recent History</div>
      ${(history.history || []).length > 0 ? `
        <div class="table-wrap"><table>
          <thead><tr><th>Session ID</th><th>Agent</th><th>Model</th><th>Started</th><th>Status</th></tr></thead>
          <tbody>
            ${(history.history || []).map(h => `
              <tr class="clickable" onclick="location.hash='#/sessions/${h.session_id}'">
                <td style="font-family:var(--font-mono);font-size:12px">${truncate(h.session_id, 28)}</td>
                <td>${h.agent}</td>
                <td>${truncate(h.model || 'unknown', 20)}</td>
                <td style="font-size:12px">${fmtTime(h.started_at)}</td>
                <td>${statusBadge(h.status)}</td>
              </tr>
            `).join('')}
          </tbody>
        </table></div>
      ` : '<div style="color:var(--text-muted)">No history entries.</div>'}
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${e.message}</div>`;
  }
}

window.runSQL = async function() {
  const query = ($('#sql-input')?.value || '').trim();
  if (!query) return;
  const el = $('#sql-result');
  el.innerHTML = '<div style="color:var(--text-muted)">Executing...</div>';
  try {
    const data = await apiPost('/nucleusdb/sql', { query });
    el.innerHTML = `<pre style="background:var(--bg-card);border:1px solid var(--border);border-radius:6px;padding:12px;font-family:var(--font-mono);font-size:12px;overflow-x:auto">${JSON.stringify(data, null, 2)}</pre>`;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${e.message}</div>`;
  }
};
