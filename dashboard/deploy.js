/* NucleusDB Deploy */
'use strict';

(function() {
  function currentAdmissionMode() {
    return localStorage.getItem('deploy_admission_mode') || 'warn';
  }

  function currentReusePolicy() {
    return localStorage.getItem('deploy_reuse_policy') || 'reusable';
  }

  function currentContainerImage() {
    return localStorage.getItem('deploy_container_image') || 'nucleusdb:latest';
  }

  function containerHookupForAgent(agentId) {
    return {
      kind: 'cli',
      cli_name: String(agentId || 'codex').trim().toLowerCase(),
    };
  }

  function admissionIssues(preflight) {
    const issues = Array.isArray(preflight?.admission?.issues) ? preflight.admission.issues : [];
    return issues.map(issue => String(issue?.message || '').trim()).filter(Boolean);
  }

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
          body: JSON.stringify({ agent_id: agent.id, admission_mode: currentAdmissionMode() }),
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
        <label style="display:inline-flex;align-items:center;gap:6px;margin-left:16px;">
          Reuse policy
          <select id="deploy-reuse-policy">
            <option value="reusable">reusable</option>
            <option value="single_use">single_use</option>
          </select>
        </label>
        <label style="display:inline-flex;align-items:center;gap:6px;margin-left:16px;min-width:260px;">
          Container image
          <input id="deploy-container-image" class="input" style="min-width:220px" value="${escapeHtml(currentContainerImage())}">
        </label>
        <label style="display:inline-flex;align-items:center;gap:6px;margin-left:16px;">
          Admission mode
          <select id="deploy-admission-mode">
            <option value="warn">warn</option>
            <option value="block">block</option>
            <option value="force">force</option>
          </select>
        </label>
      </div>
      <div class="deploy-grid">
        ${agents.map((agent) => renderAgentCard(agent, preflightMap.get(agent.id) || {})).join('')}
      </div>
      <div class="deploy-banner" id="deploy-banner">Select an agent to launch through the orchestrator.</div>
    `;

    const toggle = hostEl.querySelector('#deploy-container-toggle');
    if (toggle) {
      toggle.checked = localStorage.getItem('deploy_container_mode') === '1';
      toggle.addEventListener('change', () => {
        localStorage.setItem('deploy_container_mode', toggle.checked ? '1' : '0');
      });
    }
    const reusePolicy = hostEl.querySelector('#deploy-reuse-policy');
    if (reusePolicy) {
      reusePolicy.value = currentReusePolicy();
      reusePolicy.addEventListener('change', () => {
        localStorage.setItem('deploy_reuse_policy', reusePolicy.value || 'reusable');
      });
    }
    const imageInput = hostEl.querySelector('#deploy-container-image');
    if (imageInput) {
      imageInput.addEventListener('change', () => {
        localStorage.setItem('deploy_container_image', imageInput.value || 'nucleusdb:latest');
      });
    }
    const admissionSelect = hostEl.querySelector('#deploy-admission-mode');
    if (admissionSelect) {
      admissionSelect.value = currentAdmissionMode();
      admissionSelect.addEventListener('change', () => {
        localStorage.setItem('deploy_admission_mode', admissionSelect.value || 'warn');
        initDeployPage(hostEl);
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
    const topo = preflight.binary_topology || null;
    const admission = preflight.admission || null;
    const admissionMsgs = admissionIssues(preflight);
    const topoBanner = topo
      ? `<div class="deploy-banner" style="margin-top:8px">
          SHA-256: ${escapeHtml(String(topo.binary_sha256 || '').slice(0, 16))}...
          | Betti β̂₁=${Number(topo.signature?.betti1_heuristic || 0)}
          ${topo.structural_change_flagged ? '| structure changed beyond formal bound' : topo.hash_changed ? '| hash changed within loose bound' : '| topology stable'}
        </div>`
      : '';

    const admissionBanner = admission
      ? `<div class="deploy-banner" style="margin-top:8px;color:${admission.allowed ? 'var(--text-dim)' : 'var(--red)'}">
          Admission ${escapeHtml(String(admission.mode || 'warn'))}: ${admission.allowed ? 'allowed' : 'blocked'}
          ${admission.forced ? '| forced override active' : ''}
          ${admissionMsgs.length ? `<br>${admissionMsgs.map(msg => escapeHtml(msg)).join('<br>')}` : ''}
        </div>`
      : '';

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
        ${topoBanner}
        ${admissionBanner}
        ${topo?.warning ? `<div class="deploy-banner" style="color:var(--amber)">${escapeHtml(topo.warning)}</div>` : ''}
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
        body: JSON.stringify({ agent_id: agentId, admission_mode: currentAdmissionMode() }),
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
      if (pre.admission && pre.admission.allowed === false) {
        setBanner(`${agentId}: AETHER admission blocked launch. ${(admissionIssues(pre).join(' | ') || 'Review governor and topology state.')}`, true);
        return;
      }
    }

    if (localStorage.getItem('deploy_container_mode') === '1' && !pre.docker_available) {
      setBanner(`${agentId}: Docker is required for container isolation mode.`, true);
      return;
    }

    if (localStorage.getItem('deploy_container_mode') === '1') {
      setBanner(`Provisioning container for ${agentId}...`);
      const provision = await fetchJson('/api/containers/provision', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          image: currentContainerImage(),
          agent_id: `deploy-${agentId}-${Date.now()}`,
          admission_mode: currentAdmissionMode(),
        }),
      });
      setBanner(`Initializing ${agentId} in container ${provision.session_id}...`);
      await fetchJson('/api/containers/initialize', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          session_id: provision.session_id,
          hookup: containerHookupForAgent(agentId),
          reuse_policy: currentReusePolicy(),
        }),
      });
      localStorage.setItem('containers_selected_session', JSON.stringify(provision.session_id));
      setBanner(`${agentId} container ready. Opening Containers.`);
      location.hash = '#/containers';
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
          admission_mode: currentAdmissionMode(),
        }),
      });
    } catch (e) {
      if (typeof window.trySetupRedirect === 'function' && window.trySetupRedirect(e, agentId, 'deploy')) return;
      throw e;
    }

    const launchIssues = admissionIssues(launch);
    setBanner(`${agentId} launched. ${launchIssues.length ? launchIssues.join(' | ') + ' ' : ''}Opening Orchestrator.`);
    localStorage.setItem('orchestrator_pending_launch', JSON.stringify(launch));
    location.hash = '#/orchestrator';
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
