/* AgentHALO Deploy */
'use strict';

(function() {
  async function initDeployPage(hostEl) {
    hostEl.innerHTML = '<div class="loading">Loading deploy catalog...</div>';

    let catalog;
    try {
      const res = await fetch('/api/deploy/catalog');
      if (!res.ok) throw new Error(`catalog ${res.status}`);
      catalog = await res.json();
    } catch (e) {
      hostEl.innerHTML = `<div class="loading">Deploy catalog unavailable: ${escapeHtml(e.message)}</div>`;
      return;
    }

    const agents = catalog.agents || [];
    const preflights = await Promise.all(agents.map(async (agent) => {
      try {
        const res = await fetch('/api/deploy/preflight', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ agent_id: agent.id }),
        });
        if (!res.ok) throw new Error(await res.text());
        const data = await res.json();
        return [agent.id, data];
      } catch (e) {
        return [agent.id, { cli_installed: false, keys_configured: false, ready: false, error: String(e) }];
      }
    }));

    const preflightMap = new Map(preflights);

    hostEl.innerHTML = `
      <div class="page-header">
        <h1>Deploy</h1>
        <p class="subtitle">Launch and manage agents</p>
      </div>
      <div class="deploy-banner" style="display:flex;align-items:center;gap:8px;">
        <label style="display:inline-flex;align-items:center;gap:6px;cursor:pointer;">
          <input id="deploy-container-toggle" type="checkbox">
          Container isolation
        </label>
        <span style="opacity:0.8;">(requires Docker)</span>
      </div>
      <div class="deploy-grid">
        ${agents.map((agent) => renderAgentCard(agent, preflightMap.get(agent.id) || {})).join('')}
      </div>
      <div class="deploy-banner" id="deploy-banner">Select an agent to run in Cockpit.</div>
    `;

    const toggle = hostEl.querySelector('#deploy-container-toggle');
    if (toggle) {
      toggle.checked = localStorage.getItem('deploy_container_mode') === '1';
      toggle.addEventListener('change', () => {
        localStorage.setItem('deploy_container_mode', toggle.checked ? '1' : '0');
      });
    }

    hostEl.querySelectorAll('[data-launch]').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const agent = btn.dataset.agent;
        const mode = btn.dataset.mode;
        try {
          await launchAgent(agent, mode);
        } catch (e) {
          setBanner(`Launch failed: ${e.message || e}`, true);
        }
      });
    });
  }

  function renderAgentCard(agent, preflight) {
    const cliDot = preflight.cli_installed ? 'green' : 'red';
    const keyDot = preflight.keys_configured ? 'green' : 'red';
    const dockerDot = preflight.docker_available ? 'green' : 'grey';
    const missing = (preflight.missing_keys || []).join(', ');

    return `
      <div class="deploy-card" data-agent="${escapeHtml(agent.id)}">
        <div class="deploy-card-header">
          <span class="deploy-card-icon">${escapeHtml(agent.icon || '▣')}</span>
          <span class="deploy-card-name">${escapeHtml(agent.name || agent.id)}</span>
        </div>
        <p class="deploy-card-desc">${escapeHtml(agent.description || '')}</p>
        <div class="deploy-card-status">
          <div class="status-row"><span class="status-dot ${cliDot}"></span>CLI ${preflight.cli_installed ? 'installed' : 'missing'}</div>
          <div class="status-row"><span class="status-dot ${keyDot}"></span>Keys ${preflight.keys_configured ? 'configured' : `missing: ${escapeHtml(missing || 'none')}`}</div>
          <div class="status-row"><span class="status-dot ${dockerDot}"></span>Docker ${preflight.docker_available ? 'available' : 'not detected'}</div>
        </div>
        <div class="deploy-card-actions">
          <button class="btn btn-primary" data-launch="1" data-agent="${escapeHtml(agent.id)}" data-mode="terminal">Launch Terminal</button>
          <button class="btn" data-launch="1" data-agent="${escapeHtml(agent.id)}" data-mode="cockpit">Open in Cockpit</button>
        </div>
        ${preflight.install_hint ? `<div class="deploy-banner">${escapeHtml(preflight.install_hint)}</div>` : ''}
      </div>`;
  }

  async function launchAgent(agentId, mode) {
    setBanner(`Running preflight for ${agentId}...`);

    let pre;
    try {
      pre = await fetchJson('/api/deploy/preflight', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ agent_id: agentId }),
      });
    } catch (e) {
      if (typeof window.trySetupRedirect === 'function' && window.trySetupRedirect(e, agentId, 'deploy')) return;
      throw e;
    }

    if (!pre.ready) {
      if (!pre.cli_installed) {
        setBanner(`${agentId}: CLI missing. ${pre.install_hint || 'Install CLI then retry.'}`, true);
        return;
      }
      if (!pre.keys_configured) {
        const providers = (pre.missing_keys || []).map(p => String(p || '').toLowerCase());
        setBanner(`${agentId}: missing keys (${providers.join(', ')}). Opening guided setup...`, true);
        const redirected = typeof window.trySetupRedirect === 'function' && window.trySetupRedirect({
          status: 400,
          message: `missing API keys: ${providers.join(', ')}`,
          body: { missing_keys: providers },
        }, agentId, 'deploy');
        if (!redirected) {
          location.hash = '#/config';
        }
        return;
      }
    }

    if (localStorage.getItem('deploy_container_mode') === '1' && !pre.docker_available) {
      setBanner(`${agentId}: Docker is required for container isolation mode.`, true);
      return;
    }

    setBanner(`Launching ${agentId}...`);

    let launch;
    try {
      launch = await fetchJson('/api/deploy/launch', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          agent_id: agentId,
          mode,
          container: localStorage.getItem('deploy_container_mode') === '1',
          working_dir: null,
        }),
      });
    } catch (e) {
      if (typeof window.trySetupRedirect === 'function' && window.trySetupRedirect(e, agentId, 'deploy')) return;
      throw e;
    }

    setBanner(`${agentId} launched. Opening Cockpit.`);

    if (window.CockpitPage && typeof window.CockpitPage.queueLaunch === 'function') {
      window.CockpitPage.queueLaunch(launch);
    } else {
      localStorage.setItem('cockpit_pending_launch', JSON.stringify(launch));
    }
    location.hash = '#/cockpit';
  }

  async function fetchJson(url, init) {
    const res = await fetch(url, init);
    if (!res.ok) {
      throw await buildApiError(res, url);
    }
    return res.json();
  }

  async function buildApiError(res, url) {
    const raw = await res.text();
    let body = null;
    try { body = raw ? JSON.parse(raw) : null; } catch (_e) {}
    const message = (body && body.error) || raw || `${url} => ${res.status}`;
    const err = new Error(message);
    err.status = res.status;
    err.path = url;
    err.body = body;
    return err;
  }

  function setBanner(msg, isErr = false) {
    const el = document.getElementById('deploy-banner');
    if (!el) return;
    el.textContent = msg;
    el.style.color = isErr ? 'var(--red)' : 'var(--accent)';
    el.style.borderColor = isErr ? 'var(--red)' : 'var(--border)';
  }

  function escapeHtml(s) {
    if (typeof window.__escapeHtml === 'function') {
      return window.__escapeHtml(s);
    }
    return String(s || '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  window.DeployPage = { init: initDeployPage };
})();
