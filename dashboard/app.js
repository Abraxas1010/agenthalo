const state = { page: 'overview' };
const pageEl = document.getElementById('page');

document.querySelectorAll('[data-page]').forEach(btn => btn.addEventListener('click', () => { state.page = btn.dataset.page; render(); }));

async function api(path, options = {}) {
  const res = await fetch(`/api${path}`, { headers: { 'Content-Type': 'application/json' }, ...options });
  return res.json();
}

function section(title, body) { return `<section class="panel"><h2>${title}</h2>${body}</section>`; }

async function renderOverview() {
  const [status, db, discord] = await Promise.all([api('/status'), api('/nucleusdb/status'), api('/discord/status')]);
  pageEl.innerHTML = section('Overview', `
    <div class="grid two">
      <div><strong>Home</strong><div>${status.home}</div></div>
      <div><strong>DB</strong><div>${status.db_path}</div></div>
      <div><strong>Discord connected</strong><div>${discord.connected}</div></div>
      <div><strong>Seal count</strong><div>${db.rows ? db.rows.find(r => r[0] === 'entries')?.[1] : 'n/a'}</div></div>
    </div>
  `);
}

async function renderGenesis() {
  const status = await api('/genesis/status');
  pageEl.innerHTML = section('Genesis', `
    <div class="stack">
      <div>Seed exists: <strong>${status.seed_exists}</strong></div>
      <div>DID: <code>${status.did || 'not initialized'}</code></div>
      <button id="harvest-btn">Harvest Entropy + Initialize</button>
      <button id="reset-btn">Reset Genesis</button>
      <pre id="genesis-output"></pre>
    </div>
  `);
  document.getElementById('harvest-btn').onclick = async () => {
    const out = await api('/genesis/harvest', { method: 'POST' });
    document.getElementById('genesis-output').textContent = JSON.stringify(out, null, 2);
  };
  document.getElementById('reset-btn').onclick = async () => {
    const out = await api('/genesis/reset', { method: 'POST' });
    document.getElementById('genesis-output').textContent = JSON.stringify(out, null, 2);
  };
}

async function renderIdentity() {
  const status = await api('/identity/status');
  pageEl.innerHTML = section('Identity', `<pre>${JSON.stringify(status, null, 2)}</pre>`);
}

async function renderSecurity() {
  const status = await api('/crypto/status');
  pageEl.innerHTML = section('Security', `
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
    const out = await api('/crypto/create-password', { method: 'POST', body: JSON.stringify(Object.fromEntries(fd.entries())) });
    document.getElementById('security-output').textContent = JSON.stringify(out, null, 2);
  };
  document.getElementById('unlock-form').onsubmit = async (e) => {
    e.preventDefault();
    const fd = new FormData(e.target);
    const out = await api('/crypto/unlock', { method: 'POST', body: JSON.stringify(Object.fromEntries(fd.entries())) });
    document.getElementById('security-output').textContent = JSON.stringify(out, null, 2);
  };
  document.getElementById('lock-btn').onclick = async () => {
    const out = await api('/crypto/lock', { method: 'POST' });
    document.getElementById('security-output').textContent = JSON.stringify(out, null, 2);
  };
}

async function renderNucleusdb() {
  const [status, history] = await Promise.all([api('/nucleusdb/status'), api('/nucleusdb/history')]);
  pageEl.innerHTML = section('NucleusDB', `
    <div class="stack">
      <pre>${JSON.stringify(status, null, 2)}</pre>
      <h3>SQL</h3>
      <textarea id="sql-text" rows="8">SHOW STATUS;</textarea>
      <button id="sql-run">Run SQL</button>
      <pre id="sql-output"></pre>
      <h3>History</h3>
      <pre>${JSON.stringify(history, null, 2)}</pre>
    </div>
  `);
  document.getElementById('sql-run').onclick = async () => {
    const query = document.getElementById('sql-text').value;
    const out = await api('/nucleusdb/sql', { method: 'POST', body: JSON.stringify({ query }) });
    document.getElementById('sql-output').textContent = JSON.stringify(out, null, 2);
  };
}

async function renderFormalProofs() {
  const status = await api('/formal-proofs');
  const toolRows = (status.tools || []).map(tool => `
    <tr>
      <td><code>${tool.tool}</code></td>
      <td>${tool.requirements_met}/${tool.requirements_checked}</td>
      <td>${tool.trust_tier || 'none'}</td>
      <td>${tool.passed}</td>
    </tr>
  `).join('');
  pageEl.innerHTML = section('Formal Proofs', `
    <div class="stack">
      <div class="grid two">
        <div><strong>Gate enabled</strong><div>${status.gate_enabled}</div></div>
        <div><strong>Evaluation mode</strong><div>${status.evaluation_mode || 'unknown'}</div></div>
        <div><strong>Certificates</strong><div>${status.certificate_count}</div></div>
      </div>
      <div><strong>Advisory note</strong><div>${status.advisory_note || 'n/a'}</div></div>
      <table>
        <thead>
          <tr><th>Tool</th><th>Met</th><th>Trust Tier</th><th>Pass</th></tr>
        </thead>
        <tbody>${toolRows}</tbody>
      </table>
      <h3>Formal Provenance</h3>
      <pre>${JSON.stringify(status.provenance, null, 2)}</pre>
    </div>
  `);
}

async function renderDiscord() {
  const [status, recent] = await Promise.all([api('/discord/status'), api('/discord/recent')]);
  pageEl.innerHTML = section('Discord', `
    <div class="stack">
      <pre>${JSON.stringify(status, null, 2)}</pre>
      <form id="search-form" class="stack compact">
        <input type="text" name="q" placeholder="Search messages" />
        <button>Search</button>
      </form>
      <pre id="discord-search"></pre>
      <h3>Recent</h3>
      <pre>${JSON.stringify(recent, null, 2)}</pre>
    </div>
  `);
  document.getElementById('search-form').onsubmit = async (e) => {
    e.preventDefault();
    const q = new FormData(e.target).get('q');
    const out = await api(`/discord/search?q=${encodeURIComponent(q)}`);
    document.getElementById('discord-search').textContent = JSON.stringify(out, null, 2);
  };
}

async function render() {
  if (state.page === 'overview') return renderOverview();
  if (state.page === 'genesis') return renderGenesis();
  if (state.page === 'identity') return renderIdentity();
  if (state.page === 'security') return renderSecurity();
  if (state.page === 'nucleusdb') return renderNucleusdb();
  if (state.page === 'formal') return renderFormalProofs();
  if (state.page === 'discord') return renderDiscord();
}

render();
