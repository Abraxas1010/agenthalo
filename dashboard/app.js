'use strict';

(function() {
  const state = {
    page: resolvePage(),
  };

  const contentEl = document.getElementById('content');

  function escapeHtml(value) {
    const s = String(value == null ? '' : value);
    return s
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }

  window.__escapeHtml = escapeHtml;

  function resolvePage() {
    const hash = window.location.hash.replace(/^#\/?/, '').trim();
    return hash || 'overview';
  }

  async function api(path, options = {}) {
    const res = await fetch(`/api${path}`, {
      headers: { 'Content-Type': 'application/json' },
      ...options,
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) {
      throw new Error(body.error || `HTTP ${res.status}`);
    }
    return body;
  }

  async function apiPost(path, payload) {
    return api(path, {
      method: 'POST',
      body: JSON.stringify(payload || {}),
    });
  }

  window.api = api;
  window.apiPost = apiPost;

  function section(title, body) {
    return `<section class="panel"><h2>${title}</h2>${body}</section>`;
  }

  function navSync() {
    document.querySelectorAll('[data-page]').forEach((btn) => {
      const active = btn.dataset.page === state.page;
      btn.classList.toggle('active', active);
      btn.onclick = () => {
        window.location.hash = `#/${btn.dataset.page}`;
      };
    });
  }

  async function renderOverview() {
    const [status, db, discord, orchestrator, models] = await Promise.all([
      api('/status'),
      api('/nucleusdb/status'),
      api('/discord/status'),
      api('/orchestrator/status'),
      api('/models/status'),
    ]);
    const entries = db.rows ? db.rows.find((r) => r[0] === 'entries')?.[1] : 'n/a';
    const servedModels = Array.isArray(models?.backend?.served_models)
      ? models.backend.served_models.join(', ')
      : '';
    contentEl.innerHTML = section('Overview', `
      <div class="grid two">
        <div><strong>Home</strong><div>${escapeHtml(status.home)}</div></div>
        <div><strong>DB</strong><div>${escapeHtml(status.db_path)}</div></div>
        <div><strong>Seal count</strong><div>${escapeHtml(entries)}</div></div>
        <div><strong>Discord connected</strong><div>${escapeHtml(discord.connected)}</div></div>
        <div><strong>Agents</strong><div>${Number(orchestrator.agents_total || 0)} total / ${Number(orchestrator.agents_busy || 0)} busy</div></div>
        <div><strong>Local models</strong><div>${servedModels ? escapeHtml(servedModels) : 'none served'}</div></div>
      </div>
    `);
  }

  async function renderGenesis() {
    const status = await api('/genesis/status');
    contentEl.innerHTML = section('Genesis', `
      <div class="stack">
        <div>Seed exists: <strong>${status.seed_exists}</strong></div>
        <div>DID: <code>${escapeHtml(status.did || 'not initialized')}</code></div>
        <button id="harvest-btn">Harvest Entropy + Initialize</button>
        <button id="reset-btn">Reset Genesis</button>
        <pre id="genesis-output"></pre>
      </div>
    `);
    document.getElementById('harvest-btn').onclick = async () => {
      const out = await apiPost('/genesis/harvest');
      document.getElementById('genesis-output').textContent = JSON.stringify(out, null, 2);
    };
    document.getElementById('reset-btn').onclick = async () => {
      const out = await apiPost('/genesis/reset');
      document.getElementById('genesis-output').textContent = JSON.stringify(out, null, 2);
    };
  }

  async function renderIdentity() {
    const status = await api('/identity/status');
    contentEl.innerHTML = section('Identity', `<pre>${escapeHtml(JSON.stringify(status, null, 2))}</pre>`);
  }

  async function renderSecurity() {
    const status = await api('/crypto/status');
    contentEl.innerHTML = section('Security', `
      <div class="stack">
        <div>Password unlocked: <strong>${status.password_unlocked}</strong></div>
        <form id="pw-form" class="stack compact">
          <input type="password" name="password" placeholder="Password" />
          <input type="password" name="confirm" placeholder="Confirm" />
          <button>Create Password</button>
        </form>
        <form id="unlock-form" class="stack compact">
          <input type="password" name="password" placeholder="Unlock password" />
          <button>Unlock</button>
        </form>
        <button id="lock-btn">Lock</button>
        <pre id="security-output"></pre>
      </div>
    `);
    document.getElementById('pw-form').onsubmit = async (e) => {
      e.preventDefault();
      const fd = new FormData(e.target);
      const out = await apiPost('/crypto/create-password', Object.fromEntries(fd.entries()));
      document.getElementById('security-output').textContent = JSON.stringify(out, null, 2);
    };
    document.getElementById('unlock-form').onsubmit = async (e) => {
      e.preventDefault();
      const fd = new FormData(e.target);
      const out = await apiPost('/crypto/unlock', Object.fromEntries(fd.entries()));
      document.getElementById('security-output').textContent = JSON.stringify(out, null, 2);
    };
    document.getElementById('lock-btn').onclick = async () => {
      const out = await apiPost('/crypto/lock');
      document.getElementById('security-output').textContent = JSON.stringify(out, null, 2);
    };
  }

  async function renderNucleusdb() {
    const [status, history] = await Promise.all([api('/nucleusdb/status'), api('/nucleusdb/history')]);
    contentEl.innerHTML = section('NucleusDB', `
      <div class="stack">
        <pre>${escapeHtml(JSON.stringify(status, null, 2))}</pre>
        <h3>SQL</h3>
        <textarea id="sql-text" rows="8">SHOW STATUS;</textarea>
        <button id="sql-run">Run SQL</button>
        <pre id="sql-output"></pre>
        <h3>History</h3>
        <pre>${escapeHtml(JSON.stringify(history, null, 2))}</pre>
      </div>
    `);
    document.getElementById('sql-run').onclick = async () => {
      const query = document.getElementById('sql-text').value;
      const out = await apiPost('/nucleusdb/sql', { query });
      document.getElementById('sql-output').textContent = JSON.stringify(out, null, 2);
    };
  }

  async function renderFormalProofs() {
    const status = await api('/formal-proofs');
    const toolRows = (status.tools || []).map((tool) => `
      <tr>
        <td><code>${escapeHtml(tool.tool)}</code></td>
        <td>${escapeHtml(tool.requirements_met)}/${escapeHtml(tool.requirements_checked)}</td>
        <td>${escapeHtml(tool.trust_tier || 'none')}</td>
        <td>${escapeHtml(tool.passed)}</td>
      </tr>
    `).join('');
    contentEl.innerHTML = section('Formal Proofs', `
      <div class="stack">
        <div class="grid two">
          <div><strong>Gate enabled</strong><div>${escapeHtml(status.gate_enabled)}</div></div>
          <div><strong>Certificates</strong><div>${escapeHtml(status.certificate_count)}</div></div>
        </div>
        <table>
          <thead><tr><th>Tool</th><th>Met</th><th>Trust Tier</th><th>Pass</th></tr></thead>
          <tbody>${toolRows}</tbody>
        </table>
        <h3>Formal Provenance</h3>
        <pre>${escapeHtml(JSON.stringify(status.provenance, null, 2))}</pre>
      </div>
    `);
  }

  async function renderDiscord() {
    const [status, recent] = await Promise.all([api('/discord/status'), api('/discord/recent')]);
    contentEl.innerHTML = section('Discord', `
      <div class="stack">
        <pre>${escapeHtml(JSON.stringify(status, null, 2))}</pre>
        <form id="search-form" class="stack compact">
          <input type="text" name="q" placeholder="Search messages" />
          <button>Search</button>
        </form>
        <pre id="discord-search"></pre>
        <h3>Recent</h3>
        <pre>${escapeHtml(JSON.stringify(recent, null, 2))}</pre>
      </div>
    `);
    document.getElementById('search-form').onsubmit = async (e) => {
      e.preventDefault();
      const q = new FormData(e.target).get('q');
      const out = await api(`/discord/search?q=${encodeURIComponent(q)}`);
      document.getElementById('discord-search').textContent = JSON.stringify(out, null, 2);
    };
  }

  async function renderModels() {
    const status = await api('/models/status');
    const backend = status.backend || {};
    const installed = Array.isArray(backend.installed_models) ? backend.installed_models : [];
    const managed = backend.managed || null;
    const installedRows = installed.map((model) => `
      <tr>
        <td><code>${escapeHtml(model.model)}</code></td>
        <td>${escapeHtml(model.size || '-')}</td>
        <td>${escapeHtml(model.quantization || '-')}</td>
        <td>${model.served ? 'yes' : 'no'}</td>
        <td><button class="btn btn-sm" data-remove="${escapeHtml(model.model)}">Remove</button></td>
      </tr>
    `).join('');
    contentEl.innerHTML = `
      ${section('Local Models', `
        <div class="stack">
          <div class="grid two">
            <div><strong>Backend</strong><div>${escapeHtml(backend.base_url || 'vLLM')}</div></div>
            <div><strong>Healthy</strong><div>${escapeHtml(backend.healthy)}</div></div>
            <div><strong>Managed PID</strong><div>${escapeHtml(managed?.pid || '-')}</div></div>
            <div><strong>Served Model</strong><div>${escapeHtml(managed?.model || backend.served_models?.join(', ') || '-')}</div></div>
          </div>
          <form id="model-serve-form" class="stack compact">
            <input name="model" placeholder="HF model or installed local model id" value="${escapeHtml(managed?.model || status.config?.vllm_default_model || '')}" />
            <button>Serve with vLLM</button>
          </form>
          <button id="model-stop-btn">Stop vLLM</button>
          <pre id="model-serve-output"></pre>
        </div>
      `)}
      ${section('Search + Pull', `
        <div class="stack">
          <form id="model-search-form" class="stack compact">
            <input name="q" placeholder="Search Hugging Face models" value="Qwen coder 7b" />
            <button>Search</button>
          </form>
          <div id="model-search-results" class="table-wrap"></div>
          <pre id="model-search-output"></pre>
        </div>
      `)}
      ${section('Installed', `
        <div class="table-wrap">
          <table>
            <thead><tr><th>Model</th><th>Size</th><th>Quant</th><th>Served</th><th>Action</th></tr></thead>
            <tbody>${installedRows || '<tr><td colspan="5" class="muted">No local models installed.</td></tr>'}</tbody>
          </table>
        </div>
      `)}
    `;

    document.getElementById('model-serve-form').onsubmit = async (e) => {
      e.preventDefault();
      const model = new FormData(e.target).get('model');
      const out = await apiPost('/models/serve', { model });
      document.getElementById('model-serve-output').textContent = JSON.stringify(out, null, 2);
    };
    document.getElementById('model-stop-btn').onclick = async () => {
      const out = await apiPost('/models/stop', {});
      document.getElementById('model-serve-output').textContent = JSON.stringify(out, null, 2);
    };
    document.getElementById('model-search-form').onsubmit = async (e) => {
      e.preventDefault();
      const q = new FormData(e.target).get('q');
      const out = await api(`/models/search?q=${encodeURIComponent(q)}&limit=8`);
      const rows = (out.results || []).map((item) => `
        <tr>
          <td><code>${escapeHtml(item.model)}</code></td>
          <td>${escapeHtml(item.size || '-')}</td>
          <td>${escapeHtml(item.downloads || '-')}</td>
          <td>${item.fits_gpu == null ? '-' : item.fits_gpu ? 'yes' : 'no'}</td>
          <td>${item.installed ? 'installed' : `<button class="btn btn-sm" data-pull="${escapeHtml(item.model)}">Pull</button>`}</td>
        </tr>
      `).join('');
      document.getElementById('model-search-results').innerHTML = `
        <table>
          <thead><tr><th>Model</th><th>Size</th><th>Downloads</th><th>Fits GPU</th><th>Action</th></tr></thead>
          <tbody>${rows}</tbody>
        </table>
      `;
      document.querySelectorAll('[data-pull]').forEach((btn) => {
        btn.onclick = async () => {
          const model = btn.dataset.pull;
          const pulled = await apiPost('/models/pull', { model, source: 'vllm' });
          document.getElementById('model-search-output').textContent = JSON.stringify(pulled, null, 2);
          await renderModels();
        };
      });
    };
    document.querySelectorAll('[data-remove]').forEach((btn) => {
      btn.onclick = async () => {
        const model = btn.dataset.remove;
        const out = await apiPost('/models/remove', { model, source: 'vllm' });
        document.getElementById('model-search-output').textContent = JSON.stringify(out, null, 2);
        await renderModels();
      };
    });
  }

  async function renderContainersPage() {
    if (typeof window.renderContainers === 'function') {
      await window.renderContainers();
      return;
    }
    contentEl.innerHTML = section('Containers', '<div class="loading">Containers page unavailable.</div>');
  }

  async function renderDeployPage() {
    if (window.DeployPage && typeof window.DeployPage.init === 'function') {
      await window.DeployPage.init(contentEl);
      return;
    }
    contentEl.innerHTML = section('Deploy', '<div class="loading">Deploy page unavailable.</div>');
  }

  async function renderOrchestratorPage() {
    if (window.OrchestratorPage && typeof window.OrchestratorPage.render === 'function') {
      window.OrchestratorPage.render(contentEl);
      return;
    }
    contentEl.innerHTML = section('Orchestrator', '<div class="loading">Orchestrator page unavailable.</div>');
  }

  async function render() {
    navSync();
    if (window.OrchestratorPage && typeof window.OrchestratorPage.cleanup === 'function') {
      window.OrchestratorPage.cleanup();
    }
    state.page = resolvePage();
    navSync();
    if (state.page === 'overview') return renderOverview();
    if (state.page === 'deploy') return renderDeployPage();
    if (state.page === 'models') return renderModels();
    if (state.page === 'containers') return renderContainersPage();
    if (state.page === 'orchestrator') return renderOrchestratorPage();
    if (state.page === 'genesis') return renderGenesis();
    if (state.page === 'identity') return renderIdentity();
    if (state.page === 'security') return renderSecurity();
    if (state.page === 'nucleusdb') return renderNucleusdb();
    if (state.page === 'formal') return renderFormalProofs();
    if (state.page === 'discord') return renderDiscord();
    window.location.hash = '#/overview';
  }

  window.addEventListener('hashchange', () => {
    state.page = resolvePage();
    render().catch((err) => {
      contentEl.innerHTML = `<section class="panel"><h2>Error</h2><pre>${escapeHtml(String(err?.stack || err?.message || err))}</pre></section>`;
    });
  });

  render().catch((err) => {
    contentEl.innerHTML = `<section class="panel"><h2>Error</h2><pre>${escapeHtml(String(err?.stack || err?.message || err))}</pre></section>`;
  });
})();
