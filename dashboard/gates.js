/**
 * Unified Gates Dashboard — vanilla JS, no framework, no external imports.
 *
 * Fetches /api/gates/status and renders all three categories.
 * Standalone page — no imports from cockpit.js, observatory.js, or codeguard.js.
 */

'use strict';

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

let refreshTimer = null;
let lastData = null;

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

document.addEventListener('DOMContentLoaded', () => {
  setupCardToggles();
  setupModal();
  setupRefreshControls();
  refresh();
});

// ---------------------------------------------------------------------------
// Data fetching
// ---------------------------------------------------------------------------

async function refresh() {
  try {
    const resp = await fetch('/api/gates/status');
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    const data = await resp.json();
    lastData = data;
    renderAll(data);
    document.getElementById('last-refreshed').textContent =
      'Last refreshed: ' + new Date().toLocaleTimeString();
  } catch (err) {
    console.error('Gates refresh failed:', err);
    document.getElementById('last-refreshed').textContent =
      'Refresh failed: ' + err.message;
  }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function renderAll(data) {
  renderGitGates(data.git_gates || {});
  renderCommGates(data.communication_gates || {});
  renderInternalGates(data.internal_gates || {});
  renderHealth(data);
}

function renderGitGates(git) {
  const wt = git.worktree_enforcement || {};
  const cg = git.codeguard || {};
  const ws = git.workspace_profile || {};

  // Worktree enforcement
  const enabled = wt.enabled;
  const el = document.getElementById('wt-enforcement');
  el.textContent = enabled ? 'ENABLED' : 'DISABLED';
  el.className = 'gate-value ' + (enabled ? 'ok' : 'off');

  // Worktree status
  const worktrees = wt.active_worktrees || [];
  const statusEl = document.getElementById('wt-status');
  if (worktrees.length > 0) {
    statusEl.textContent = worktrees.length + ' active worktree' + (worktrees.length > 1 ? 's' : '');
    statusEl.className = 'gate-value ok';
  } else {
    statusEl.textContent = 'No active worktrees';
    statusEl.className = 'gate-value off';
  }

  // Worktree table
  const tbody = document.getElementById('wt-tbody');
  tbody.innerHTML = '';
  if (worktrees.length === 0) {
    const tr = document.createElement('tr');
    tr.innerHTML = '<td colspan="3" style="color:var(--g-text-dim)">No managed worktrees</td>';
    tbody.appendChild(tr);
  } else {
    for (const wt of worktrees) {
      const tr = document.createElement('tr');
      const pathStr = wt.path || '—';
      const shortPath = pathStr.split('/').slice(-2).join('/');
      const created = wt.created_at ? timeAgo(wt.created_at) : '—';
      tr.innerHTML = `<td title="${esc(pathStr)}">${esc(shortPath)}</td>` +
        `<td>${esc(wt.session_id || '—')}</td>` +
        `<td>${esc(created)}</td>`;
      tbody.appendChild(tr);
    }
  }

  // CodeGuard summary
  const cgSummary = document.getElementById('cg-summary');
  if (cg.manifest_exists) {
    cgSummary.textContent = cg.bindings_count + ' bindings';
    cgSummary.className = 'gate-value ok';
  } else {
    cgSummary.textContent = 'No manifest';
    cgSummary.className = 'gate-value off';
  }

  // Gate indicators
  setIndicator('cg-gate1', cg.gate1_pass);
  setIndicator('cg-gate2', cg.gate2_pass);
  setIndicator('cg-gate3', cg.gate3_pass);

  // Git status summary
  const gitOk = (!enabled || worktrees.length > 0);
  document.getElementById('git-status').textContent = gitOk ? 'OK' : 'Blocked';
  document.getElementById('git-status').style.color = gitOk ? 'var(--g-ok)' : 'var(--g-warn)';

  // Workspace
  const wsEl = document.getElementById('ws-profile');
  wsEl.textContent = ws.name ? (ws.name + (ws.lean_project_path ? ' (' + ws.lean_project_path + ')' : '')) : 'default';
}

function renderCommGates(comms) {
  // Proxy Governor
  const proxy = comms.proxy_governor || {};
  const proxyEl = document.getElementById('proxy-status');
  if (proxy.error) {
    proxyEl.textContent = 'error: ' + proxy.error;
    proxyEl.className = 'gate-value error';
  } else if (proxy.stable !== undefined) {
    const stable = proxy.stable;
    proxyEl.textContent = (stable ? 'stable' : 'unstable') +
      (proxy.epsilon !== undefined ? ' (ε=' + proxy.epsilon.toFixed(3) + ')' : '');
    proxyEl.className = 'gate-value ' + (stable ? 'ok' : 'warn');

    // Sparkline
    if (proxy.sparkline && proxy.sparkline.length > 1) {
      renderSparkline('proxy-sparkline', proxy.sparkline);
    }
  } else {
    proxyEl.textContent = 'idle';
    proxyEl.className = 'gate-value off';
  }

  // Privacy
  const privacy = comms.privacy_controller || {};
  document.getElementById('privacy-status').textContent =
    'default=' + (privacy.default_level || 'Maximum');

  // Mesh
  const mesh = comms.mesh || {};
  const meshEl = document.getElementById('mesh-status');
  meshEl.textContent = mesh.enabled ? 'enabled' : 'disabled';
  meshEl.className = 'gate-value ' + (mesh.enabled ? 'ok' : 'off');

  // OpenClaw
  const oc = comms.openclaw || {};
  const ocEl = document.getElementById('openclaw-status');
  ocEl.textContent = oc.installed ? 'installed' : 'not installed';
  ocEl.className = 'gate-value ' + (oc.installed ? 'ok' : 'off');

  // P2PCLAW
  const p2p = comms.p2pclaw || {};
  const p2pEl = document.getElementById('p2pclaw-status');
  p2pEl.textContent = p2p.configured ? 'configured' : 'not configured';
  p2pEl.className = 'gate-value ' + (p2p.configured ? 'ok' : 'off');

  // DIDComm
  const did = comms.didcomm || {};
  const didEl = document.getElementById('didcomm-status');
  didEl.textContent = did.identity_present ? 'identity present' : 'no identity';
  didEl.className = 'gate-value ' + (did.identity_present ? 'ok' : 'off');

  // Nym
  const nym = comms.nym || {};
  const nymEl = document.getElementById('nym-status');
  nymEl.textContent = nym.available ? 'available' : 'not available';
  nymEl.className = 'gate-value ' + (nym.available ? 'ok' : 'off');

  // Status summary
  document.getElementById('comms-status').textContent = 'OK';
  document.getElementById('comms-status').style.color = 'var(--g-ok)';
}

function renderInternalGates(internal) {
  // Proof Gate
  const pg = internal.proof_gate || {};
  const pgEl = document.getElementById('proof-gate-status');
  pgEl.textContent = pg.enabled
    ? 'enabled (' + (pg.requirements_count || 0) + ' requirements)'
    : 'disabled';
  pgEl.className = 'gate-value ' + (pg.enabled ? 'ok' : 'off');

  document.getElementById('proof-gate-certs').textContent =
    (pg.certificates_count || 0) + ' certificates';

  // Admission
  const adm = internal.admission || {};
  const admEl = document.getElementById('admission-status');
  admEl.textContent = 'mode=' + (adm.mode || 'warn');
  admEl.className = 'gate-value ' + (adm.mode === 'block' ? 'error' : adm.mode === 'force' ? 'warn' : 'ok');

  // EVM Gate
  const evm = internal.evm_gate || {};
  document.getElementById('evm-status').textContent =
    evm.formal_basis ? 'formal basis verified' : 'not configured';

  // Crypto
  const crypto = internal.crypto || {};
  const cryptoEl = document.getElementById('crypto-status');
  cryptoEl.textContent = (crypto.locked ? 'locked' : 'unlocked') +
    ' (' + (crypto.scoped_keys || 0) + ' scoped keys)';
  cryptoEl.className = 'gate-value ' + (crypto.locked ? 'warn' : 'ok');

  // Governors
  const governors = internal.governors || [];
  document.getElementById('gov-count').textContent = governors.length + ' instances';
  const govList = document.getElementById('gov-list');
  govList.innerHTML = '';
  for (const gov of governors) {
    const item = document.createElement('div');
    item.className = 'gov-item';

    const badge = gov.stable ? 'stable' :
      (gov.last_updated_unix === 0 || gov.epsilon === 0) ? 'idle' : 'unstable';

    item.innerHTML =
      `<span class="gov-name">${esc(gov.instance_id)}</span>` +
      `<span class="gov-badge ${badge}">${badge}</span>` +
      `<span class="gov-epsilon">ε=${(gov.epsilon || 0).toFixed(3)}</span>` +
      `<svg class="gov-sparkline" viewBox="0 0 80 20" data-gov="${esc(gov.instance_id)}"></svg>`;
    govList.appendChild(item);

    // Render governor sparkline
    if (gov.sparkline && gov.sparkline.length > 1) {
      const svg = item.querySelector('.gov-sparkline');
      renderSparklineSVG(svg, gov.sparkline, 80, 20);
    }
  }

  // Policy Registry
  const policy = internal.policy_registry || {};
  document.getElementById('policy-status').textContent =
    'v' + (policy.schema_version || '?') + ', digest=' + (policy.digest || '?').substring(0, 8) + '...';
  const violEl = document.getElementById('policy-violations');
  const vCount = policy.invariant_violations || 0;
  violEl.textContent = vCount + ' violations';
  violEl.className = 'gate-value ' + (vCount > 0 ? 'error' : 'ok');

  // Internal status summary
  const hasIssues = vCount > 0 || (internal.admission && internal.admission.mode === 'block');
  document.getElementById('internal-status').textContent = hasIssues ? 'Issues' : 'OK';
  document.getElementById('internal-status').style.color = hasIssues ? 'var(--g-warn)' : 'var(--g-ok)';
}

function renderHealth(data) {
  const el = document.getElementById('health-indicator');
  const git = data.git_gates || {};
  const internal = data.internal_gates || {};
  const policy = internal.policy_registry || {};
  const violations = policy.invariant_violations || 0;
  const wtEnforced = (git.worktree_enforcement || {}).enabled;
  const wtActive = ((git.worktree_enforcement || {}).active_worktrees || []).length;

  if (violations > 0) {
    el.className = 'health-indicator error';
    el.title = violations + ' policy invariant violation(s)';
  } else if (wtEnforced && wtActive === 0) {
    el.className = 'health-indicator warn';
    el.title = 'Worktree enforcement on, but no active worktrees';
  } else {
    el.className = 'health-indicator ok';
    el.title = 'All gates healthy';
  }
}

// ---------------------------------------------------------------------------
// Sparkline rendering (pure SVG, no D3)
// ---------------------------------------------------------------------------

function renderSparkline(svgId, values) {
  const svg = document.getElementById(svgId);
  if (!svg) return;
  renderSparklineSVG(svg, values, 200, 30);
}

function renderSparklineSVG(svg, values, width, height) {
  svg.innerHTML = '';
  if (!values || values.length < 2) return;

  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const padding = 2;
  const effectiveHeight = height - padding * 2;
  const stepX = width / (values.length - 1);

  const points = values.map((v, i) => {
    const x = i * stepX;
    const y = padding + effectiveHeight - ((v - min) / range) * effectiveHeight;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });

  // Area
  const areaPath = `M0,${height} L${points.join(' L')} L${width},${height} Z`;
  const area = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  area.setAttribute('d', areaPath);
  area.setAttribute('class', 'sparkline-area');
  svg.appendChild(area);

  // Line
  const linePath = `M${points.join(' L')}`;
  const line = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  line.setAttribute('d', linePath);
  line.setAttribute('class', 'sparkline-line');
  svg.appendChild(line);
}

// ---------------------------------------------------------------------------
// Card collapse/expand
// ---------------------------------------------------------------------------

function setupCardToggles() {
  document.querySelectorAll('.card-header').forEach(header => {
    header.addEventListener('click', () => {
      const targetId = header.getAttribute('data-toggle');
      const body = document.getElementById(targetId);
      if (body) {
        body.classList.toggle('collapsed');
        const toggle = header.querySelector('.card-toggle');
        if (toggle) {
          toggle.textContent = body.classList.contains('collapsed') ? '▸' : '▾';
        }
      }
    });
  });
}

// ---------------------------------------------------------------------------
// Worktree creation modal
// ---------------------------------------------------------------------------

function setupModal() {
  const overlay = document.getElementById('modal-overlay');
  const btnOpen = document.getElementById('btn-create-worktree');
  const btnCancel = document.getElementById('btn-modal-cancel');
  const btnCreate = document.getElementById('btn-modal-create');
  const resultEl = document.getElementById('modal-result');

  btnOpen.addEventListener('click', () => {
    overlay.style.display = 'flex';
    resultEl.textContent = '';
    resultEl.className = 'modal-result';
    document.getElementById('wt-purpose').value = '';
    document.getElementById('wt-purpose').focus();
  });

  btnCancel.addEventListener('click', () => {
    overlay.style.display = 'none';
  });

  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) overlay.style.display = 'none';
  });

  btnCreate.addEventListener('click', async () => {
    const purpose = document.getElementById('wt-purpose').value.trim();
    const agentId = document.getElementById('wt-agent').value.trim() || 'dashboard';
    if (!purpose) {
      resultEl.textContent = 'Purpose is required';
      resultEl.className = 'modal-result error';
      return;
    }

    btnCreate.disabled = true;
    btnCreate.textContent = 'Creating...';
    resultEl.textContent = '';

    try {
      const resp = await fetch('/api/gates/worktree/create', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ purpose, agent_id: agentId }),
      });
      const data = await resp.json();
      if (data.ok) {
        resultEl.textContent = 'Created: ' + data.worktree_path;
        resultEl.className = 'modal-result ok';
        setTimeout(() => {
          overlay.style.display = 'none';
          refresh();
        }, 1500);
      } else {
        resultEl.textContent = data.error || 'Creation failed';
        resultEl.className = 'modal-result error';
      }
    } catch (err) {
      resultEl.textContent = err.message;
      resultEl.className = 'modal-result error';
    } finally {
      btnCreate.disabled = false;
      btnCreate.textContent = 'Create';
    }
  });
}

// ---------------------------------------------------------------------------
// Auto-refresh controls
// ---------------------------------------------------------------------------

function setupRefreshControls() {
  document.getElementById('btn-refresh').addEventListener('click', refresh);

  const select = document.getElementById('refresh-interval');
  select.addEventListener('change', () => {
    if (refreshTimer) clearInterval(refreshTimer);
    const secs = parseInt(select.value, 10);
    if (secs > 0) {
      refreshTimer = setInterval(refresh, secs * 1000);
    }
  });

  // Start default auto-refresh (5s)
  const defaultInterval = parseInt(select.value, 10);
  if (defaultInterval > 0) {
    refreshTimer = setInterval(refresh, defaultInterval * 1000);
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function setIndicator(id, pass) {
  const el = document.getElementById(id);
  if (!el) return;
  el.className = 'gate-indicator ' + (pass ? 'pass' : 'fail');
}

function timeAgo(unixSecs) {
  const now = Math.floor(Date.now() / 1000);
  const diff = now - unixSecs;
  if (diff < 60) return diff + 's ago';
  if (diff < 3600) return Math.floor(diff / 60) + 'm ago';
  if (diff < 86400) return Math.floor(diff / 3600) + 'h ago';
  return Math.floor(diff / 86400) + 'd ago';
}

function esc(str) {
  const el = document.createElement('span');
  el.textContent = str;
  return el.innerHTML;
}
