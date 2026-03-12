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
  huggingface: {
    name: 'Hugging Face',
    envVar: 'HF_TOKEN',
    keyUrl: 'https://huggingface.co/settings/tokens',
    category: 'llm',
    required: false,
    description: 'Optional token for gated model downloads, Hub search, and vLLM-backed local serving.',
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

// --- OpenRouter OAuth PKCE helpers ---
function base64urlEncode(buf) {
  return btoa(String.fromCharCode(...new Uint8Array(buf)))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}

function generateCodeVerifier() {
  const arr = new Uint8Array(48);
  crypto.getRandomValues(arr);
  return base64urlEncode(arr);
}

async function generateCodeChallenge(verifier) {
  const data = new TextEncoder().encode(verifier);
  const hash = await crypto.subtle.digest('SHA-256', data);
  return base64urlEncode(hash);
}

// -- Container & MCP Tools page delegates (must be top-level for hoisting) ----
function renderContainersPage() {
  if (typeof window.renderContainers === 'function') {
    window.renderContainers();
  } else {
    content.innerHTML = '<div class="loading">Containers module not loaded.</div>';
  }
}

function renderMcpToolsPageRoute() {
  if (typeof window.renderMcpToolsPage === 'function') {
    window.renderMcpToolsPage();
  } else {
    content.innerHTML = '<div class="loading">MCP Tools module not loaded.</div>';
  }
}

// -- Routing ------------------------------------------------------------------
const pages = { overview: renderOverviewHub, dashboard: renderOverview, sessions: renderSessions,
  costs: renderCosts, config: renderConfig, setup: renderSetup, genesis: renderGenesisPage,
  identification: renderIdentificationPage, communication: renderCommunicationPage, 'nucleusdb-docs': renderNucleusDBDocsPage,
  networking: renderNetworkingPage,
  trust: renderTrust, nucleusdb: renderNucleusDB, orchestrator: renderOrchestrator, cockpit: renderCockpit, deploy: renderDeploy, models: renderModels,
  containers: renderContainersPage, 'mcp-tools': renderMcpToolsPageRoute };

const NETWORKS = [
  {
    id: 'p2pclaw',
    name: 'P2PCLAW Research Hive',
    icon: '&#9788;',
    description: 'Decentralized research collaboration with peer validation',
    configurable: true,
    comingSoon: false,
  },
  {
    id: 'nym-mesh',
    name: 'Nym Mixnet Mesh',
    icon: '&#128274;',
    description: 'Privacy-preserving agent communication',
    configurable: false,
    comingSoon: true,
  },
  {
    id: 'didcomm-federation',
    name: 'DIDComm Federation',
    icon: '&#127760;',
    description: 'Cross-organization agent identity mesh',
    configurable: false,
    comingSoon: true,
  },
];
let _networkingSelected = 'p2pclaw';

// Genesis + Overview hub + Identification pages — rendering logic in genesis-docs.js (loaded after app.js)
function renderGenesisPage() {
  if (typeof renderGenesis === 'function') renderGenesis();
  else content.innerHTML = '<div class="loading">Genesis docs module not loaded.</div>';
}
function renderIdentificationPage() {
  if (typeof renderIdentification === 'function') renderIdentification();
  else content.innerHTML = '<div class="loading">Identification docs module not loaded.</div>';
}
function renderCommunicationPage() {
  if (typeof renderCommunication === 'function') renderCommunication();
  else content.innerHTML = '<div class="loading">Communication docs module not loaded.</div>';
}
function renderNucleusDBDocsPage() {
  if (typeof renderNucleusDBDocs === 'function') renderNucleusDBDocs();
  else content.innerHTML = '<div class="loading">NucleusDB docs module not loaded.</div>';
}
async function renderNetworkingPage() {
  let available = [];
  try {
    const data = await api('/networking/available');
    available = Array.isArray(data && data.networks) ? data.networks : [];
  } catch (_e) {
    available = [];
  }
  const availableById = Object.fromEntries(
    available
      .filter((n) => n && n.id)
      .map((n) => [n.id, n]),
  );
  const cards = NETWORKS.map((base) => {
    const live = availableById[base.id] || {};
    return {
      ...base,
      enabled: !!live.enabled,
      configurable: live.configurable !== undefined ? !!live.configurable : base.configurable,
      comingSoon: live.coming_soon !== undefined ? !!live.coming_soon : base.comingSoon,
    };
  });
  if (!cards.some((n) => n.id === _networkingSelected && n.configurable)) {
    _networkingSelected = 'p2pclaw';
  }

  let p2status = null;
  let p2error = '';
  try {
    p2status = await api('/p2pclaw/status');
  } catch (err) {
    p2error = String(err && err.message || err || '');
  }
  let bridgePayload = null;
  let bridgeError = '';
  try {
    bridgePayload = await api('/p2pclaw/bridge/status?include_mcp_tools=true');
  } catch (err) {
    bridgeError = String(err && err.message || err || '');
  }
  const p2cfg = (p2status && p2status.config) || {
    endpoint_url: 'https://p2pclaw.com',
    agent_id: 'agenthalo',
    agent_name: 'AgentHALO',
    tier: 'tier1',
  };
  const p2enabled = !!(cards.find((n) => n.id === 'p2pclaw') || {}).enabled;
  const swarm = (p2status && p2status.swarm) || {};
  const bridge = (bridgePayload && bridgePayload.bridge) || {};
  const bridgeState = bridge.state || {};
  const bridgeCaps = Array.isArray(bridge.capabilities) ? bridge.capabilities : [];

  const cardsHtml = cards.map((n) => {
    const selected = _networkingSelected === n.id;
    const status = n.comingSoon
      ? '<span class="badge badge-warn">Coming Soon</span>'
      : (n.enabled ? '<span class="badge badge-ok">Enabled</span>' : '<span class="badge">Disabled</span>');
    return `<button class="network-card${selected ? ' is-selected' : ''}${n.comingSoon ? ' is-coming-soon' : ''}"
      data-network-id="${esc(n.id)}" ${n.configurable ? '' : 'disabled'}
      title="${n.configurable ? 'Select network' : 'Coming soon'}">
      <div class="network-card-head">
        <span class="network-card-icon">${n.icon}</span>
        <span class="network-card-title">${esc(n.name)}</span>
      </div>
      <div class="network-card-desc">${esc(n.description)}</div>
      <div class="network-card-status">${status}</div>
    </button>`;
  }).join('');

  const detailsHtml = _networkingSelected !== 'p2pclaw'
    ? '<div class="card"><div class="card-label">Coming Soon</div><div class="card-sub">This network integration is reserved for a future release.</div></div>'
    : `
      <div class="networking-detail-grid">
        <section class="card networking-config-card">
          <div class="card-label">P2PCLAW Configuration</div>
          <div class="card-sub">Enable integration and configure endpoint + agent identity.</div>
          <div class="network-form-row network-toggle-row">
            <label class="network-toggle-label">Enable P2PCLAW</label>
            <label class="switch">
              <input type="checkbox" id="p2pclaw-enabled-toggle" ${p2enabled ? 'checked' : ''}>
              <span class="slider"></span>
            </label>
          </div>
          <div class="network-form-row">
            <label for="p2-endpoint-url">Endpoint URL</label>
            <input class="input" id="p2-endpoint-url" value="${esc(p2cfg.endpoint_url || 'https://p2pclaw.com')}" placeholder="https://p2pclaw.com">
          </div>
          <div class="network-form-row">
            <label for="p2-agent-name">Agent Name</label>
            <input class="input" id="p2-agent-name" value="${esc(p2cfg.agent_name || 'AgentHALO')}" placeholder="AgentHALO">
          </div>
          <div class="network-form-row">
            <label for="p2-agent-id">Agent ID</label>
            <input class="input" id="p2-agent-id" value="${esc(p2cfg.agent_id || 'agenthalo')}" placeholder="agenthalo-alice">
          </div>
          <div class="network-form-row">
            <label for="p2-auth-secret">Auth Secret (optional)</label>
            <input class="input" id="p2-auth-secret" type="password" placeholder="Shared HMAC secret">
          </div>
          <div class="network-form-row">
            <label>Tier</label>
            <div class="network-tier-row">
              <label><input type="radio" name="p2-tier" value="tier1" ${(p2cfg.tier || 'tier1') === 'tier1' ? 'checked' : ''}> Tier 1 (Free)</label>
              <label><input type="radio" name="p2-tier" value="tier2" ${(p2cfg.tier || 'tier1') === 'tier2' ? 'checked' : ''}> Tier 2 (Prepaid)</label>
            </div>
          </div>
          <div class="network-form-actions">
            <button class="btn" id="p2-test-btn">Test Connection</button>
            <button class="btn btn-primary" id="p2-save-btn">Save</button>
          </div>
          <div id="p2-config-msg" class="networking-msg">${p2error ? esc(p2error) : ''}</div>
        </section>
        <section class="card networking-status-card">
          <div class="card-label">Hive Status</div>
          <div class="card-sub">Real-time metrics from /api/p2pclaw/status</div>
          <div class="network-stat-grid">
            <div class="network-stat"><span>Active Agents</span><strong id="p2-stat-agents">${Number(swarm.agents || 0)}</strong></div>
            <div class="network-stat"><span>Papers</span><strong id="p2-stat-papers">${Number(swarm.papers || 0)}</strong></div>
            <div class="network-stat"><span>Mempool</span><strong id="p2-stat-mempool">${Number(swarm.mempool || 0)}</strong></div>
            <div class="network-stat"><span>Last Event</span><strong id="p2-stat-last">${esc(String(swarm.last_event_ts || '-'))}</strong></div>
          </div>
          <div class="network-form-actions" style="margin-top:10px">
            <button class="btn btn-sm" id="p2-briefing-btn">Load Briefing</button>
          </div>
          <pre id="p2-briefing" class="network-briefing">No briefing loaded.</pre>
        </section>
        <section class="card networking-bridge-card">
          <div class="card-label">Bridge Worker</div>
          <div class="card-sub">Persistent bridge state, compute split, and detected capabilities.</div>
          <div class="network-stat-grid">
            <div class="network-stat"><span>Configured</span><strong>${bridge.configured ? 'Yes' : 'No'}</strong></div>
            <div class="network-stat"><span>Enabled</span><strong>${bridge.enabled ? 'Yes' : 'No'}</strong></div>
            <div class="network-stat"><span>Last Poll</span><strong>${esc(String(bridgeState.last_success_at || bridgeState.last_run_at || '-'))}</strong></div>
            <div class="network-stat"><span>Next Poll</span><strong>${esc(String(bridgeState.next_poll_not_before || '-'))}</strong></div>
            <div class="network-stat"><span>Compute Split</span><strong>${typeof bridge.compute_split_ratio === 'number' ? `${(bridge.compute_split_ratio * 100).toFixed(1)}% hive` : '-'}</strong></div>
            <div class="network-stat"><span>Nash</span><strong>${bridge.nash_compliant ? 'Compliant' : 'Not yet'}</strong></div>
            <div class="network-stat"><span>Polls</span><strong>${Number(bridgeState.polls_total || 0)}</strong></div>
            <div class="network-stat"><span>Failures</span><strong>${Number(bridgeState.consecutive_failures || 0)}</strong></div>
          </div>
          <div class="network-form-row">
            <label>Capabilities</label>
            <div class="network-cap-list">${bridgeCaps.length ? bridgeCaps.map((cap) => `<span class="network-cap-pill">${esc(String(cap))}</span>`).join('') : '<span class="muted">No capabilities reported.</span>'}</div>
          </div>
          <div class="network-form-row">
            <label>Bridge Config Path</label>
            <div class="network-inline-note">${esc(String(bridge.config_path || '~/.agenthalo/p2pclaw_bridge.json'))}</div>
          </div>
          <div class="network-form-row">
            <label>Last Error</label>
            <div class="network-inline-note">${esc(String(bridgeState.last_error || bridgeError || 'None'))}</div>
          </div>
        </section>
      </div>`;

  content.innerHTML = `
    <h1>Networking Integrations</h1>
    <p class="muted">Configure sovereign communication networks for AgentHALO.</p>
    <section class="network-selector">${cardsHtml}</section>
    <section class="network-detail">${detailsHtml}</section>
    <p class="networking-footer">Network integrations extend AgentHALO's sovereign communication stack. Each network is independently enabled and configured.</p>
  `;

  $$('.network-card[data-network-id]').forEach((el) => {
    if (el.disabled) return;
    el.addEventListener('click', () => {
      _networkingSelected = el.dataset.networkId || 'p2pclaw';
      renderNetworkingPage();
    });
  });
  if (_networkingSelected !== 'p2pclaw') return;

  const msgEl = $('#p2-config-msg');
  const setMsg = (text, ok) => {
    if (!msgEl) return;
    msgEl.textContent = text || '';
    msgEl.classList.toggle('ok', !!ok);
    msgEl.classList.toggle('err', !!text && !ok);
  };

  const toggle = $('#p2pclaw-enabled-toggle');
  if (toggle) {
    toggle.addEventListener('change', async () => {
      try {
        await apiPost('/addons', { name: 'p2pclaw', enabled: !!toggle.checked });
        setMsg(`P2PCLAW ${toggle.checked ? 'enabled' : 'disabled'}.`, true);
      } catch (err) {
        setMsg(String(err && err.message || err || 'failed to update addon toggle'), false);
      }
    });
  }

  const testBtn = $('#p2-test-btn');
  if (testBtn) {
    testBtn.addEventListener('click', async () => {
      setMsg('Testing connection...', true);
      try {
        const res = await api('/p2pclaw/status');
        const s = (res && res.swarm) || {};
        const setStat = (id, val) => { const el = $(id); if (el) el.textContent = String(val); };
        setStat('#p2-stat-agents', Number(s.agents || 0));
        setStat('#p2-stat-papers', Number(s.papers || 0));
        setStat('#p2-stat-mempool', Number(s.mempool || 0));
        setStat('#p2-stat-last', s.last_event_ts || '-');
        setMsg('Connection successful.', true);
      } catch (err) {
        setMsg(String(err && err.message || err || 'connection test failed'), false);
      }
    });
  }

  const saveBtn = $('#p2-save-btn');
  if (saveBtn) {
    saveBtn.addEventListener('click', async () => {
      const endpoint_url = ($('#p2-endpoint-url')?.value || '').trim();
      const agent_name = ($('#p2-agent-name')?.value || '').trim();
      const agent_id = ($('#p2-agent-id')?.value || '').trim();
      const auth_secret = ($('#p2-auth-secret')?.value || '').trim();
      const tier = ($('input[name="p2-tier"]:checked')?.value || 'tier1').trim();
      const payload = { endpoint_url, agent_name, agent_id, tier };
      if (auth_secret) payload.auth_secret = auth_secret;
      setMsg('Saving configuration...', true);
      try {
        const res = await apiPost('/p2pclaw/configure', payload);
        const inVault = !!(res && res.auth_in_vault);
        setMsg(`Saved. Auth secret ${auth_secret ? (inVault ? 'stored in vault.' : 'stored via insecure fallback.') : 'unchanged.'}`, true);
        const authInput = $('#p2-auth-secret');
        if (authInput) authInput.value = '';
      } catch (err) {
        setMsg(String(err && err.message || err || 'failed to save config'), false);
      }
    });
  }

  const briefingBtn = $('#p2-briefing-btn');
  if (briefingBtn) {
    briefingBtn.addEventListener('click', async () => {
      const target = $('#p2-briefing');
      if (target) target.textContent = 'Loading briefing...';
      try {
        const res = await api('/p2pclaw/briefing');
        if (target) target.textContent = String(res && res.briefing_markdown || 'No briefing content.');
      } catch (err) {
        if (target) target.textContent = String(err && err.message || err || 'Failed to load briefing.');
      }
    });
  }
}
function renderOverviewHub() {
  if (typeof renderDocsOverview === 'function') renderDocsOverview();
  else content.innerHTML = '<div class="loading">Overview module not loaded.</div>';
}

// Setup-first gate: cached setup state
let _setupState = null;
let _setupStateFetchedAt = 0;
let _lastSetupComplete = null;
const SETUP_CACHE_MS = 5000;
let _genesisComplete = null;
let _genesisStatusFetchedAt = 0;
let _genesisCeremonyRunning = false;
const GENESIS_CACHE_MS = 3000;
let _cryptoStatus = null;
let _cryptoStatusFetchedAt = 0;
const CRYPTO_CACHE_MS = 2000;
const GENESIS_STAGES = [
  { id: 'hw', label: 'Hardware Detection', stub: true },
  { id: 'curby', label: 'Quantum Entropy', stub: false },
  { id: 'beacons', label: 'Remote Beacons', stub: false },
  { id: 'combine', label: 'Entropy Combination', stub: false },
  { id: 'derive', label: 'Identity Derivation', stub: true },
  { id: 'lattice', label: 'Lattice Formation', stub: true },
  { id: 'destroy', label: 'Seed Destruction', stub: true },
  { id: 'anchor', label: 'Triple Anchor', stub: true },
  { id: 'verify', label: 'Verification', stub: true },
  { id: 'complete', label: 'Genesis Complete', stub: false },
];
const GENESIS_ERROR_MESSAGES = {
  WALLET_BOOTSTRAP_FAILED: {
    category: 'Signing Wallet Bootstrap Failed',
    message: 'Genesis could not initialize the local PQ signing wallet.',
    steps: [
      'Retry Genesis once',
      'Ensure the app can write to your AgentHALO home directory',
      'If this persists, open Configuration and run diagnostics',
    ],
  },
  LEDGER_READ_FAILURE: {
    category: 'Identity Ledger Read Failure',
    message: 'Genesis could not read the identity ledger state.',
    steps: [
      'Retry Genesis once',
      'Check local disk permissions for AgentHALO files',
      'If this persists, run ledger health check from Configuration',
    ],
  },
  HARVEST_RUNTIME_FAILURE: {
    category: 'Genesis Runtime Failure',
    message: 'The Genesis worker failed unexpectedly before entropy completion.',
    steps: [
      'Retry Genesis',
      'Restart the dashboard service',
      'If this persists, export logs and contact support',
    ],
  },
  CURBY_UNREACHABLE: {
    category: 'Network / CURBy Unreachable',
    message: 'Could not reach the quantum randomness beacon at random.colorado.edu.',
    steps: [
      'Check your internet connection',
      'Whitelist random.colorado.edu',
      'Disable VPN/proxy and retry',
      'Verify system clock is correct',
    ],
  },
  ALL_REMOTE_FAILED: {
    category: 'All Remote Entropy Sources Failed',
    message: 'Could not reach CURBy, NIST, or drand. Internet access is required for Genesis.',
    steps: [
      'Check your internet connection',
      'Whitelist random.colorado.edu, beacon.nist.gov, api.drand.sh',
      'Disable VPN/proxy and retry',
      'Retry when network is stable',
    ],
  },
  INSUFFICIENT_ENTROPY: {
    category: 'Insufficient Entropy Sources',
    message: 'Genesis requires at least 2 independent entropy sources.',
    steps: [
      'Ensure internet access is available',
      'Retry (remote beacon outage may be temporary)',
      'Check firewall rules for blocked domains',
    ],
  },
  SEED_READ_FAILURE: {
    category: 'Sealed Seed Read Failure',
    message: 'Genesis could not read an existing sealed seed state.',
    steps: [
      'Retry Genesis',
      'If this is a migrated install, re-run Genesis initialization',
      'If it persists, use diagnostics to repair local seed state',
    ],
  },
  SEED_STORAGE_FAILURE: {
    category: 'Sealed Seed Storage Failure',
    message: 'Genesis could not seal/store the derived seed.',
    steps: [
      'Retry Genesis',
      'Check disk space and local directory permissions',
      'If it persists, restart service and run diagnostics',
    ],
  },
  GENESIS_SEED_MISMATCH: {
    category: 'Genesis Seed Mismatch',
    message: 'Existing sealed seed does not match the newly harvested Genesis value.',
    steps: [
      'Do not continue with mismatched seed state',
      'Run Genesis diagnostics/repair from Configuration',
      'Contact support if mismatch persists',
    ],
  },
  LEDGER_APPEND_FAILURE: {
    category: 'Ledger Append Failure',
    message: 'Genesis completed entropy harvest but could not append immutable ledger entry.',
    steps: [
      'Retry Genesis',
      'Check local file permissions and disk health',
      'If it persists, run ledger repair diagnostics',
    ],
  },
  UNKNOWN: {
    category: 'Unknown Genesis Error',
    message: 'The entropy harvest failed unexpectedly.',
    steps: [
      'Retry the Genesis ceremony',
      'Check network and system time',
      'If it persists, contact support',
    ],
  },
};

async function fetchGenesisStatus(force) {
  const now = Date.now();
  if (!force && _genesisComplete !== null && (now - _genesisStatusFetchedAt) < GENESIS_CACHE_MS) {
    return _genesisComplete;
  }
  try {
    const status = await api('/genesis/status');
    _genesisComplete = !!status.completed;
  } catch (_e) {
    // Fail closed.
    _genesisComplete = false;
  }
  _genesisStatusFetchedAt = now;
  return _genesisComplete;
}

async function fetchCryptoStatus(force) {
  const now = Date.now();
  if (!force && _cryptoStatus && (now - _cryptoStatusFetchedAt) < CRYPTO_CACHE_MS) {
    return _cryptoStatus;
  }
  try {
    const status = await api('/crypto/status');
    _cryptoStatus = status || {};
  } catch (e) {
    _cryptoStatus = {
      locked: false,
      password_protected: false,
      migration_status: 'unknown',
      active_scopes: [],
      retry_after_secs: 0,
      error: String(e && e.message || e),
    };
  }
  _cryptoStatusFetchedAt = now;
  return _cryptoStatus;
}

function hideCryptoOverlay() {
  const overlay = $('#crypto-lock-overlay');
  if (overlay) overlay.style.display = 'none';
}

function lockTitle(status) {
  if (!status.password_protected || status.migration_status === 'needs_password_creation' || status.migration_status === 'fresh') {
    return 'Create a password to protect your identity';
  }
  return 'Enter password to unlock';
}

function lockHint(status) {
  if (!status.password_protected || status.migration_status === 'needs_password_creation' || status.migration_status === 'fresh') {
    return 'This password protects local cryptographic scopes (sign, vault, wallet, identity, genesis).';
  }
  if (Number(status.retry_after_secs || 0) > 0) {
    return `Too many failed attempts. Retry in ${Number(status.retry_after_secs)}s.`;
  }
  return 'Unlock to access encrypted identity, vault, and wallet operations.';
}

function renderCryptoOverlay(status) {
  const overlay = $('#crypto-lock-overlay');
  if (!overlay) return;
  const needsCreate = !status.password_protected || status.migration_status === 'needs_password_creation' || status.migration_status === 'fresh';
  overlay.style.display = 'flex';
  overlay.innerHTML = `
    <div class="crypto-lock-card">
      <div class="crypto-lock-title">${esc(lockTitle(status))}</div>
      <div class="crypto-lock-subtitle">${esc(lockHint(status))}</div>
      ${needsCreate ? `
        <div class="crypto-lock-row">
          <input id="crypto-create-password" type="password" class="input" placeholder="Create password">
        </div>
        <div class="crypto-lock-row">
          <input id="crypto-create-confirm" type="password" class="input" placeholder="Confirm password">
        </div>
        <div class="crypto-lock-rules">
          <div class="crypto-lock-rules-title">Password requirements</div>
          <ul>
            <li>Minimum 8 characters.</li>
            <li>Use at least 3 of 4 character groups: uppercase, lowercase, number, symbol.</li>
            <li>Recommended: 12+ characters with all 4 groups.</li>
          </ul>
        </div>
        <div class="crypto-lock-actions">
          <button id="crypto-create-btn" class="btn btn-primary">Create Password</button>
        </div>
      ` : `
        <div class="crypto-lock-row">
          <input id="crypto-unlock-password" type="password" class="input" placeholder="Password">
        </div>
        <div class="crypto-lock-actions">
          <button id="crypto-unlock-btn" class="btn btn-primary">Unlock</button>
        </div>
      `}
      <div class="crypto-lock-meta">Scopes in session: ${(status.active_scopes || []).map(esc).join(', ') || 'none'}</div>
      <div id="crypto-lock-error" class="crypto-lock-error"></div>
    </div>
  `;
  if (needsCreate) {
    const createBtn = $('#crypto-create-btn', overlay);
    if (createBtn) {
      createBtn.onclick = async () => {
        const password = String($('#crypto-create-password', overlay)?.value || '');
        const confirm = String($('#crypto-create-confirm', overlay)?.value || '');
        const errorEl = $('#crypto-lock-error', overlay);
        try {
          createBtn.disabled = true;
          await apiPost('/crypto/create-password', { password, confirm });
          _cryptoStatus = null;
          const ok = await ensureCryptoUnlocked(true);
          if (ok) { route(); }
        } catch (e) {
          if (errorEl) errorEl.textContent = String(e && e.message || e);
        } finally {
          createBtn.disabled = false;
        }
      };
      // Enter key on confirm field triggers create
      const confirmInput = $('#crypto-create-confirm', overlay);
      if (confirmInput) confirmInput.addEventListener('keydown', e => { if (e.key === 'Enter') createBtn.click(); });
      const createInput = $('#crypto-create-password', overlay);
      if (createInput) createInput.addEventListener('keydown', e => { if (e.key === 'Enter' && confirmInput) confirmInput.focus(); });
    }
  } else {
    const unlockBtn = $('#crypto-unlock-btn', overlay);
    if (unlockBtn) {
      unlockBtn.onclick = async () => {
        const password = String($('#crypto-unlock-password', overlay)?.value || '');
        const errorEl = $('#crypto-lock-error', overlay);
        try {
          unlockBtn.disabled = true;
          await apiPost('/crypto/unlock', { password });
          _cryptoStatus = null;
          const ok = await ensureCryptoUnlocked(true);
          if (ok) { route(); }
        } catch (e) {
          if (errorEl) errorEl.textContent = String(e && e.message || e);
        } finally {
          unlockBtn.disabled = false;
        }
      };
      // Enter key on password field triggers unlock
      const unlockInput = $('#crypto-unlock-password', overlay);
      if (unlockInput) unlockInput.addEventListener('keydown', e => { if (e.key === 'Enter') unlockBtn.click(); });
    }
  }
}

async function ensureCryptoUnlocked(force) {
  const status = await fetchCryptoStatus(force);
  if (status && status.locked) {
    renderCryptoOverlay(status);
    return false;
  }
  hideCryptoOverlay();
  return true;
}

/// Auto-generate genesis seed silently if not already done.
/// Called after password create/unlock so the DID is ready immediately.
async function autoGenesis() {
  try {
    const status = await api('/genesis/status');
    if (status && status.completed) {
      _genesisComplete = true;
      _genesisStatusFetchedAt = Date.now();
      return;
    }
    const result = await apiPost('/genesis/harvest', {});
    if (result && result.success) {
      _genesisComplete = true;
      _genesisStatusFetchedAt = Date.now();
    }
  } catch (e) {
    // Non-fatal: genesis will be retried via the ceremony gate on next navigation.
    console.warn('autoGenesis: silent harvest failed, will retry via ceremony:', e);
  }
}

function genesisStageById(id) {
  return $(`#gs-${id}`);
}

function setGenesisStage(id, state, detail) {
  const el = genesisStageById(id);
  if (!el) return;
  el.className = `genesis-stage ${state}`;
  const icon = $('.genesis-stage-icon', el);
  const det = $('.genesis-stage-detail', el);
  if (icon) {
    if (state === 'active') icon.textContent = '◉';
    else if (state === 'done') icon.textContent = '✓';
    else if (state === 'failed') icon.textContent = '✗';
    else icon.textContent = '○';
  }
  if (det) det.textContent = detail || '';
}

function setGenesisStatus(text, cls) {
  const statusEl = $('#genesis-status');
  if (!statusEl) return;
  statusEl.className = `genesis-status ${cls || ''}`.trim();
  statusEl.textContent = text || '';
}

function showGenesisError(result) {
  const statusEl = $('#genesis-status');
  if (!statusEl) return;
  const code = String((result && result.error_code) || 'UNKNOWN');
  const spec = GENESIS_ERROR_MESSAGES[code] || GENESIS_ERROR_MESSAGES.UNKNOWN;
  const message = String((result && (result.message || result.error)) || spec.message);
  const failedSources = Array.isArray(result && result.failed_sources) ? result.failed_sources : [];
  const technical = (result && result.technical_detail) || (failedSources.length > 0 ? JSON.stringify(failedSources) : '');
  statusEl.className = 'genesis-status error';
  statusEl.innerHTML = `
    <div class="genesis-error-panel">
      <div class="genesis-error-category">${esc(spec.category)} (${esc(code)})</div>
      <div>${esc(message)}</div>
      ${technical ? `<details style="margin-top:8px"><summary style="cursor:pointer">Technical details</summary><pre style="margin-top:6px;white-space:pre-wrap;font-size:11px;color:var(--text-dim)">${esc(String(technical))}</pre></details>` : ''}
      <ol class="genesis-error-steps">
        ${spec.steps.map(s => `<li>${esc(s)}</li>`).join('')}
      </ol>
      <button class="genesis-retry-btn" onclick="retryGenesis()">Retry</button>
      <a class="genesis-support-link" href="#/config">Contact Support</a>
    </div>
  `;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function showGenesisCeremony() {
  if (_genesisCeremonyRunning) return;
  _genesisCeremonyRunning = true;

  const stagesEl = $('#genesis-stages');
  const statusEl = $('#genesis-status');
  if (!stagesEl || !statusEl) {
    _genesisCeremonyRunning = false;
    return;
  }

  stagesEl.innerHTML = GENESIS_STAGES.map((s) => `
    <div class="genesis-stage future" id="gs-${esc(s.id)}">
      <span class="genesis-stage-icon">○</span>
      <span class="genesis-stage-label">${esc(s.label)}</span>
      <span class="genesis-stage-detail"></span>
    </div>
  `).join('');
  setGenesisStatus('Initializing Genesis ceremony...');

  try {
    setGenesisStage('hw', 'active');
    setGenesisStatus('Detecting hardware entropy sources...');
    await sleep(700);
    setGenesisStage('hw', 'done', 'OS CSPRNG available');

    setGenesisStage('curby', 'active');
    setGenesisStatus('Harvesting entropy from CURBy, NIST, drand, and OS...');
    const result = await apiPost('/genesis/harvest', {});

    const sources = Array.isArray(result.sources) ? result.sources : [];
    const failed = Array.isArray(result.failed_sources) ? result.failed_sources : [];
    const sourceBy = {};
    sources.forEach((s) => { sourceBy[String(s.name || '')] = s; });
    failed.forEach((f) => { sourceBy[String(f.name || '')] = f; });

    const curby = sourceBy['CURBy-Q'];
    if (curby && !curby.error) setGenesisStage('curby', 'done', curby.detail || (result.curby_pulse_id ? `Pulse #${result.curby_pulse_id}` : 'Connected'));
    else setGenesisStage('curby', 'failed', (curby && curby.error) ? 'unreachable' : 'unavailable');

    const nist = sourceBy['NIST-Beacon'];
    const drand = sourceBy.drand;
    const beaconDetail = [
      nist && !nist.error ? 'NIST ✓' : 'NIST ✗',
      drand && !drand.error ? 'drand ✓' : 'drand ✗',
    ].join(' · ');
    if ((nist && !nist.error) || (drand && !drand.error)) {
      setGenesisStage('beacons', 'done', beaconDetail);
    } else {
      setGenesisStage('beacons', 'failed', beaconDetail);
    }

    if (!result.success) {
      setGenesisStage('combine', 'failed', result.error_code || 'failed');
      throw result;
    }
    setGenesisStage('combine', 'done', `${Number(result.sources_count || sources.length || 0)} sources combined`);

    for (const s of GENESIS_STAGES.filter(x => x.stub && x.id !== 'hw')) {
      setGenesisStage(s.id, 'active');
      setGenesisStatus(`${s.label}...`);
      await sleep(420);
      setGenesisStage(s.id, 'done');
    }

    setGenesisStage('complete', 'done');
    _genesisComplete = true;
    _genesisStatusFetchedAt = Date.now();
    statusEl.className = 'genesis-status complete';
    statusEl.innerHTML = `
      <div style="margin-bottom:12px"><strong>Genesis Complete — Your agent is alive</strong></div>
      <div style="font-size:0.78rem;color:var(--text-dim);margin-bottom:16px">
        ${esc(String(result.sources_count || sources.length || 0))} entropy sources combined
        ${result.curby_pulse_id ? ` · CURBy pulse #${esc(String(result.curby_pulse_id))}` : ''}
      </div>
      <button class="genesis-continue-btn" onclick="completeGenesis()">Continue →</button>
    `;
  } catch (err) {
    const payload = (err && err.body && typeof err.body === 'object') ? err.body : err;
    const code = payload && payload.error_code;
    if (code === 'ALL_REMOTE_FAILED' || code === 'INSUFFICIENT_ENTROPY') {
      setGenesisStage('curby', 'failed', 'unreachable');
      setGenesisStage('beacons', 'failed', 'all failed');
      setGenesisStage('combine', 'failed', String(code));
    }
    showGenesisError(payload || { error_code: 'UNKNOWN', message: String(err && err.message || err || 'unknown error') });
  }

  _genesisCeremonyRunning = false;
}

window.retryGenesis = function retryGenesis() {
  _genesisCeremonyRunning = false;
  _genesisComplete = null;
  _genesisStatusFetchedAt = 0;
  const overlay = $('#genesis-overlay');
  if (overlay) overlay.style.display = '';
  showGenesisCeremony();
};

window.completeGenesis = function completeGenesis() {
  _genesisComplete = true;
  _genesisStatusFetchedAt = Date.now();
  const overlay = $('#genesis-overlay');
  if (overlay) overlay.style.display = 'none';
  route();
};

async function fetchSetupState(force) {
  const now = Date.now();
  if (!force && _setupState && (now - _setupStateFetchedAt) < SETUP_CACHE_MS) return _setupState;
  try {
    const cfg = await api('/config');
    _setupState = cfg.setup_complete || { identity: false, wallet: false, agentpmt: false, llm: false, complete: false };
    _setupStateFetchedAt = now;
  } catch (_e) {
    // Fail closed: keep users in setup if we cannot verify state.
    _setupState = { identity: false, wallet: false, agentpmt: false, llm: false, complete: false };
    _setupStateFetchedAt = now;
  }
  updateNavLockState();
  return _setupState;
}

function updateNavLockState() {
  if (!_setupState) return;
  const complete = _setupState.complete;
  const justUnlocked = _lastSetupComplete === false && complete === true;
  const NAV_EXEMPT = ['setup', 'overview', 'genesis', 'identification', 'communication', 'nucleusdb-docs'];
  $$('.nav-link').forEach(a => {
    const page = a.dataset.page;
    if (page === 'setup') {
      a.classList.remove('nav-locked');
      a.classList.toggle('setup-incomplete', !complete);
      a.classList.remove('nav-unlocked');
    } else if (NAV_EXEMPT.includes(page)) {
      // Documentation pages are always accessible
      a.classList.remove('nav-locked');
      a.classList.remove('setup-incomplete');
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
      const walletDone = (_setupState.wallet !== undefined) ? _setupState.wallet : _setupState.agentpmt;
      const steps = [_setupState.identity, walletDone, _setupState.llm];
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
  const overlay = $('#genesis-overlay');

  const cryptoReady = await ensureCryptoUnlocked();
  if (!cryptoReady) return;

  const hash = location.hash.replace('#/', '') || 'setup';
  const page = hash.split('/')[0];
  const arg = hash.split('/').slice(1).join('/');

  // Genesis ceremony: always show the visual ceremony if genesis hasn't been done yet.
  // The ceremony handles the harvest POST + staged animation so the user sees
  // entropy sources being gathered and combined.
  let genesisOk = await fetchGenesisStatus();
  if (!genesisOk) {
    if (overlay) overlay.style.display = '';
    showGenesisCeremony();
    return;
  }
  if (overlay) overlay.style.display = 'none';

  // Fetch setup state and gate navigation.
  // Documentation/overview pages are always accessible — setup gate only blocks operational pages.
  const SETUP_EXEMPT_PAGES = ['setup', 'overview', 'genesis', 'identification', 'communication', 'nucleusdb-docs'];
  const ss = await fetchSetupState();
  if (!ss.complete && !SETUP_EXEMPT_PAGES.includes(page)) {
    location.hash = '#/setup';
    return;
  }

  $$('.nav-link').forEach(a => a.classList.toggle('active', a.dataset.page === page));

  // Auto-expand Overview sub-items when navigating to any overview-family page
  const overviewFamily = ['overview', 'genesis', 'identification', 'communication', 'nucleusdb-docs'];
  const shouldExpand = overviewFamily.includes(page);
  const parentLink = document.getElementById('nav-overview-parent');
  if (parentLink) {
    parentLink.classList.toggle('nav-expanded', shouldExpand);
  }
  $$('.nav-sub-item[data-parent="overview"]').forEach(li => {
    li.classList.toggle('nav-sub-visible', shouldExpand);
  });

  if (pages[page]) pages[page](arg);
  else content.innerHTML = '<div class="loading">Page not found</div>';
}

// Toggle Overview sub-items on click
document.addEventListener('click', (e) => {
  const parentLink = e.target.closest('#nav-overview-parent');
  if (!parentLink) return;
  // If already on overview page, just toggle expansion without navigation
  const currentPage = (location.hash.replace('#/', '') || 'setup').split('/')[0];
  const overviewFamily = ['overview', 'genesis', 'identification', 'communication', 'nucleusdb-docs'];
  if (overviewFamily.includes(currentPage)) {
    const isExpanded = parentLink.classList.contains('nav-expanded');
    parentLink.classList.toggle('nav-expanded', !isExpanded);
    $$('.nav-sub-item[data-parent="overview"]').forEach(li => {
      li.classList.toggle('nav-sub-visible', !isExpanded);
    });
    // Don't navigate away if toggling — but DO navigate if going TO overview from elsewhere
    if (currentPage !== 'overview') {
      // Let the default hash navigation happen
    } else {
      e.preventDefault();
    }
  }
});

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
  if (!res.ok) {
    const err = await toApiError(res, path);
    if (Number(err.status) === 423 && !String(path).startsWith('/crypto/')) {
      _cryptoStatus = null;
      ensureCryptoUnlocked(true);
    }
    throw err;
  }
  return res.json();
}

async function apiPost(path, body) {
  const res = await fetch('/api' + path, {
    method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify(body)
  });
  if (!res.ok) {
    const err = await toApiError(res, path);
    if (Number(err.status) === 423 && !String(path).startsWith('/crypto/')) {
      _cryptoStatus = null;
      ensureCryptoUnlocked(true);
    }
    throw err;
  }
  return res.json();
}

async function apiDelete(path) {
  const res = await fetch('/api' + path, { method: 'DELETE' });
  if (!res.ok) {
    const err = await toApiError(res, path);
    if (Number(err.status) === 423 && !String(path).startsWith('/crypto/')) {
      _cryptoStatus = null;
      ensureCryptoUnlocked(true);
    }
    throw err;
  }
  return res.json();
}

function governorStatusBadge(item) {
  if (!item) return '<span class="badge badge-muted">Unknown</span>';
  if (item.gain_violated) return '<span class="badge badge-warn">Gain Violated</span>';
  if (item.oscillating) return '<span class="badge badge-info">Oscillating</span>';
  if (item.stable) return '<span class="badge badge-ok">Stable</span>';
  return '<span class="badge badge-muted">Monitoring</span>';
}

function governorSparkline(points, width = 140, height = 28) {
  const vals = Array.isArray(points) ? points.filter(v => Number.isFinite(Number(v))).map(Number) : [];
  if (!vals.length) {
    return `<svg width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"></svg>`;
  }
  const min = Math.min(...vals);
  const max = Math.max(...vals);
  const span = Math.max(max - min, 1e-9);
  const step = vals.length > 1 ? width / (vals.length - 1) : width;
  const path = vals.map((value, index) => {
    const x = index * step;
    const y = height - (((value - min) / span) * (height - 4) + 2);
    return `${index === 0 ? 'M' : 'L'}${x.toFixed(2)},${y.toFixed(2)}`;
  }).join(' ');
  return `<svg width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"><path d="${path}" fill="none" stroke="var(--accent)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"></path></svg>`;
}

function renderGovernorCards(governorData) {
  const instances = []
    .concat(Array.isArray(governorData?.instances) ? governorData.instances : [])
    .concat(governorData?.memory ? [governorData.memory] : []);
  if (!instances.length) return '';
  return `
    <div class="section-header">AETHER Governors</div>
    <div class="card" style="margin-bottom:12px">
      <div class="card-label">Formal Limitation</div>
      <div class="card-sub">Multi-step convergence is empirically observed, not formally proved. The verified regime is single-step, from-rest, no-clamp only.</div>
    </div>
    <div class="card-grid">
      ${instances.map(item => {
        const proxyExtra = item.instance_id === 'gov-proxy' && governorData?.proxy
          ? `<div class="card-sub">in-flight=${Number(governorData.proxy.in_flight || 0)} | latency=${Number(governorData.proxy.latency_ewma_ms || 0).toFixed(1)}ms</div>`
          : '';
        return `
          <div class="card">
            <div class="card-label">${esc(item.instance_id || 'governor')}</div>
            <div class="card-value" style="font-size:15px">${Number(item.epsilon || 0).toFixed(2)}</div>
            <div class="card-sub">measured=${Number(item.measured_signal || 0).toFixed(2)} | target=${Number(item.target || 0).toFixed(2)}</div>
            <div style="margin-top:8px">${governorSparkline(item.sparkline || [])}</div>
            <div style="margin-top:6px">${governorStatusBadge(item)}</div>
            ${proxyExtra}
            <div class="card-sub">basis: ${esc(item.formal_basis || 'n/a')}</div>
            ${item.warning ? `<div class="card-sub" style="color:var(--amber)">${esc(item.warning)}</div>` : ''}
          </div>
        `;
      }).join('')}
    </div>
  `;
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
  } else if (status === 401 || lower.includes('authentication required')) {
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
    genesis_harvest: '#4ba3ff', error: '#ff3030', thinking: '#3a7a2a' };
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
    const [status, sessions, costs, governors] = await Promise.all([
      api('/status'), api('/sessions?limit=5'), api('/costs?monthly=true'), api('/governor/status').catch(() => null)
    ]);

    const s = status;
    const recentSessions = (sessions.sessions || []).slice(0, 5);

    content.innerHTML = `
      <div class="page-title">Dashboard</div>
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
          <div class="card-sub">PQ Wallet: ${s.pq_wallet ? 'Present' : 'Not created'} | Governors stable: ${Number(s.governors?.stable || 0)}/${Number(s.governors?.total || 0)}</div>
        </div>
      </div>

      ${governors ? renderGovernorCards(governors) : ''}

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
    let crypto = { locked: false, migration_status: 'unknown', active_scopes: [], password_protected: false };
    try {
      crypto = await api('/crypto/status');
    } catch (_e) {}
    let agentsResp = { agents: [] };
    try {
      agentsResp = await api('/agents/list');
    } catch (_e) {}
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
            <div class="config-desc">Local mode (auth optional) or OAuth-authenticated (enforced mode)</div>
          </div>
          ${cfg.authentication.authenticated
            ? '<span class="badge badge-ok">Authenticated</span>'
            : '<span class="badge badge-warn">Not Authenticated</span>'}
        </div>
      </div>

      <div class="section-header">Crypto Lock</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">Session</div>
            <div class="config-desc">
              ${crypto.locked ? 'Locked' : 'Unlocked'}
              · status: ${esc(String(crypto.migration_status || 'unknown'))}
              · scopes: ${esc((crypto.active_scopes || []).join(', ') || 'none')}
            </div>
          </div>
          <div style="display:flex;gap:6px;align-items:center">
            <button class="btn btn-sm" onclick="forceCryptoLock()">Lock Now</button>
            <button class="btn btn-sm btn-primary" onclick="ensureCryptoUnlocked(true)">Unlock</button>
          </div>
        </div>
      </div>

      <div class="section-header">Authorized Agents</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">Agent Credentials</div>
            <div class="config-desc">Per-scope ML-KEM credentials for autonomous agents.</div>
          </div>
          <button class="btn btn-sm btn-primary" onclick="authorizeAgentPrompt()">Authorize New Agent</button>
        </div>
        ${(agentsResp.agents || []).length ? (agentsResp.agents || []).map(agent => `
          <div class="config-row">
            <div>
              <div class="config-label">${esc(agent.label || agent.agent_id)}</div>
              <div class="config-desc">
                ${esc(agent.agent_id)} · scopes: ${esc((agent.scopes || []).join(', '))}
                ${agent.expires_at ? ' · expires: ' + esc(new Date(agent.expires_at * 1000).toISOString()) : ''}
              </div>
            </div>
            <button class="btn btn-sm" onclick="revokeAgent('${esc(agent.agent_id)}')">Revoke</button>
          </div>
        `).join('') : `
          <div class="config-row">
            <div>
              <div class="config-label">No authorized agents</div>
              <div class="config-desc">Create an agent credential to enable scoped autonomous access.</div>
            </div>
          </div>
        `}
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

let _modelsSearchQuery = '';

function modelBackendBadge(backend) {
  const normalized = String(backend || '').toLowerCase();
  if (normalized === 'vllm') return '<span class="badge badge-info">vLLM</span>';
  return `<span class="badge badge-muted">${esc(backend || 'unknown')}</span>`;
}

function summarizeModelCounts(status) {
  const vllmCount = Array.isArray(status?.vllm?.installed_models) ? status.vllm.installed_models.length : 0;
  return vllmCount;
}

function installedModelsTableRows(status) {
  const models = Array.isArray(status?.vllm?.installed_models) ? status.vllm.installed_models : [];
  if (!models.length) {
    return `<tr><td colspan="6" class="muted">No local models installed.</td></tr>`;
  }
  return models.sort((a, b) => String(a.model || '').localeCompare(String(b.model || ''))).map(model => `
    <tr>
      <td>${esc(model.model || '')}</td>
      <td>${esc(model.source || '-')}</td>
      <td>${modelBackendBadge(model.backend)}</td>
      <td>${esc(model.size || '-')}</td>
      <td>${esc(model.quantization || '-')}</td>
      <td>
        ${model.served ? '<span class="badge badge-ok">Serving</span>' : '<span class="badge badge-muted">Idle</span>'}
        <button class="btn btn-sm" style="margin-left:6px" onclick='modelsRemove(${JSON.stringify(String(model.model || ''))}, "vllm")'>Remove</button>
      </td>
    </tr>
  `).join('');
}

function searchResultsRows(results) {
  if (!results.length) {
    return `<tr><td colspan="7" class="muted">Run a search to see Hugging Face candidates.</td></tr>`;
  }
  return results.map(item => `
    <tr>
      <td>${Number(item.index || 0)}</td>
      <td>${esc(item.source || '-')}</td>
      <td>${esc(item.model || '')}</td>
      <td>${modelBackendBadge(item.backend)}</td>
      <td>${esc(item.size || '-')}</td>
      <td>${esc(item.downloads || '-')}</td>
      <td>
        <button class="btn btn-sm btn-primary" onclick='modelsPull(${JSON.stringify(String(item.model || ''))}, "vllm")'>Pull</button>
      </td>
    </tr>
  `).join('');
}

function backendCard(status) {
  const installedCount = Array.isArray(status?.installed_models) ? status.installed_models.length : 0;
  const served = Array.isArray(status?.served_models) && status.served_models.length
    ? status.served_models.map(esc).join(', ')
    : 'none';
  return `
    <div class="card">
      <div class="card-label">${esc(status?.backend || 'backend')}</div>
      <div class="card-value" style="font-size:15px">${status?.healthy ? 'Healthy' : 'Unavailable'}</div>
      <div class="card-sub">${status?.cli_installed ? 'CLI installed' : 'CLI missing'}${status?.cli_version ? ` · ${esc(status.cli_version)}` : ''}</div>
      <div class="card-sub">endpoint: ${esc(status?.base_url || '-')}</div>
      <div class="card-sub">installed: ${installedCount} · served: ${served}</div>
      ${status?.error ? `<div class="card-sub" style="color:var(--amber)">${esc(status.error)}</div>` : ''}
      <div style="margin-top:10px">
        <button class="btn btn-sm btn-primary" onclick="modelsServe('vllm')">Serve vLLM</button>
        <button class="btn btn-sm" style="margin-left:6px" onclick="modelsStop('vllm')">Stop</button>
      </div>
    </div>
  `;
}

function summarizeManagedBackends(config) {
  const managed = Array.isArray(config?.managed) ? config.managed : [];
  if (!managed.length) return 'none';
  return managed.map(item => String(item?.backend || '')).filter(Boolean).join(', ') || 'none';
}

async function renderModels() {
  content.innerHTML = '<div class="loading">Loading local models...</div>';
  try {
    const status = await api('/models/status');
    const results = _modelsSearchQuery
      ? ((await api(`/models/search?q=${encodeURIComponent(_modelsSearchQuery)}`)).results || [])
      : [];

    content.innerHTML = `
      <div class="page-title">Models</div>
      <div class="card-grid">
        <div class="card">
          <div class="card-label">Backend</div>
          <div class="card-value" style="font-size:15px">vLLM</div>
          <div class="card-sub">managed: ${esc(summarizeManagedBackends(status?.config))}</div>
        </div>
        <div class="card">
          <div class="card-label">Installed Models</div>
          <div class="card-value">${summarizeModelCounts(status)}</div>
          <div class="card-sub">catalog appears under /v1/models automatically</div>
        </div>
        <div class="card">
          <div class="card-label">GPU</div>
          <div class="card-value" style="font-size:15px">${esc(status?.gpu?.name || 'Not detected')}</div>
          <div class="card-sub">${status?.gpu ? `${Number(status.gpu.total_memory_gib || 0).toFixed(1)} GiB ${esc(status.gpu.vendor || '')}` : 'Search still works without GPU detection'}</div>
        </div>
        <div class="card">
          <div class="card-label">Hugging Face Token</div>
          <div class="card-value" style="font-size:15px">${status?.huggingface_token_configured ? 'Configured' : 'Missing'}</div>
          <div class="card-sub">used for gated Hub access and vLLM-backed downloads</div>
        </div>
      </div>

      <div class="section-header">Backend Health</div>
      <div class="card-grid">
        ${backendCard(status?.vllm)}
      </div>

      <div class="section-header">Controls</div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row">
          <div>
            <div class="config-label">Search Local Models</div>
            <div class="config-desc">Search Hugging Face Hub and manage vLLM-served local models.</div>
          </div>
          <div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap">
            <input class="input" id="models-search-query" style="min-width:280px" placeholder="code assistant 7b" value="${esc(_modelsSearchQuery)}">
            <button class="btn btn-primary" id="models-search-btn">Search</button>
          </div>
        </div>
        <div class="config-row">
          <div>
            <div class="config-label">Hugging Face Login</div>
            <div class="config-desc">Stores HF token in the encrypted vault when available.</div>
          </div>
          <div style="display:flex;gap:6px;align-items:center">
            <button class="btn btn-sm" onclick="window.open('https://huggingface.co/settings/tokens','_blank')">Open Tokens</button>
            <button class="btn btn-sm btn-primary" onclick="modelsLoginHuggingFace()">Set Token</button>
          </div>
        </div>
      </div>

      <div class="section-header">Installed</div>
      <div class="table-wrap"><table>
        <thead>
          <tr><th>Model</th><th>Source</th><th>Backend</th><th>Size</th><th>Quant</th><th>Status</th></tr>
        </thead>
        <tbody>${installedModelsTableRows(status)}</tbody>
      </table></div>

      <div class="section-header">Search Results${_modelsSearchQuery ? ` for "${esc(_modelsSearchQuery)}"` : ''}</div>
      <div class="table-wrap"><table>
        <thead>
          <tr><th>#</th><th>Source</th><th>Model</th><th>Backend</th><th>Size</th><th>Downloads</th><th>Action</th></tr>
        </thead>
        <tbody>${searchResultsRows(results)}</tbody>
      </table></div>
    `;

    const input = $('#models-search-query');
    const runSearch = () => {
      _modelsSearchQuery = String(input?.value || '').trim();
      renderModels();
    };
    $('#models-search-btn')?.addEventListener('click', runSearch);
    input?.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') runSearch();
    });
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.modelsLoginHuggingFace = async function modelsLoginHuggingFace() {
  const token = window.prompt('Paste Hugging Face token (hf_...)');
  if (!token) return;
  try {
    await apiPost('/models/login/huggingface', { token: String(token).trim() });
    alert('Hugging Face token saved.');
    await renderModels();
  } catch (e) {
    alert(`Hugging Face login failed: ${String(e && e.message || e)}`);
  }
};

window.modelsServe = async function modelsServe(backend) {
  try {
    const payload = { backend };
    const model = window.prompt('vLLM model to serve (installed HF repo id or path)', '');
    if (model && String(model).trim()) payload.model = String(model).trim();
    await apiPost('/models/serve', payload);
    await renderModels();
  } catch (e) {
    alert(`Serve failed: ${String(e && e.message || e)}`);
  }
};

window.modelsStop = async function modelsStop(backend) {
  try {
    await apiPost('/models/stop', { backend });
    await renderModels();
  } catch (e) {
    alert(`Stop failed: ${String(e && e.message || e)}`);
  }
};

window.modelsPull = async function modelsPull(model, source) {
  try {
    await apiPost('/models/pull', { model, source });
    await renderModels();
  } catch (e) {
    alert(`Pull failed: ${String(e && e.message || e)}`);
  }
};

window.modelsRemove = async function modelsRemove(model, source) {
  if (!window.confirm(`Remove local model ${model}?`)) return;
  try {
    await apiPost('/models/rm', { model, source });
    await renderModels();
  } catch (e) {
    alert(`Remove failed: ${String(e && e.message || e)}`);
  }
};

window.refreshAgentPmtCatalog = async function refreshAgentPmtCatalog() {
  try {
    const resp = await apiPost('/agentpmt/refresh', {});
    alert(`AgentPMT catalog refreshed (${Number(resp.count || 0)} tools).`);
    renderConfig();
  } catch (e) {
    alert(`AgentPMT refresh failed: ${String(e && e.message || e)}`);
  }
};

window.forceCryptoLock = async function forceCryptoLock() {
  try {
    await apiPost('/crypto/lock', {});
    _cryptoStatus = null;
    await ensureCryptoUnlocked(true);
    await renderConfig();
  } catch (e) {
    alert(`Lock failed: ${String(e && e.message || e)}`);
  }
};

window.authorizeAgentPrompt = async function authorizeAgentPrompt() {
  const label = prompt('Agent label', 'Automation Agent');
  if (!label) return;
  const scopesRaw = prompt('Scopes (comma-separated): sign,vault,wallet,identity', 'sign,vault');
  if (!scopesRaw) return;
  const expiresRaw = prompt('Expiry days (empty for no expiry)', '90');
  const scopes = scopesRaw.split(',').map(s => s.trim()).filter(Boolean);
  const expires_days = expiresRaw && expiresRaw.trim() ? Number(expiresRaw) : null;
  try {
    const resp = await apiPost('/agents/authorize', {
      label,
      scopes,
      expires_days: Number.isFinite(expires_days) ? expires_days : null,
    });
    openAgentSecretModal(resp.agent_id, resp.agent_sk);
    await renderConfig();
  } catch (e) {
    alert(`Authorize failed: ${String(e && e.message || e)}`);
  }
};

function openAgentSecretModal(agentId, secretKey) {
  const old = document.getElementById('agent-secret-modal');
  if (old) old.remove();
  const wrap = document.createElement('div');
  wrap.id = 'agent-secret-modal';
  wrap.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.72);display:flex;align-items:center;justify-content:center;z-index:1300';
  wrap.innerHTML = `
    <div style="width:min(760px,94vw);background:var(--bg-card);border:1px solid var(--accent);padding:16px;border-radius:6px;box-shadow:0 10px 40px rgba(0,0,0,0.45)">
      <div style="font-size:15px;color:var(--accent);margin-bottom:8px">Agent Authorized</div>
      <div style="font-size:12px;color:var(--text-dim);margin-bottom:10px">
        Agent ID: <span style="color:var(--text)">${esc(String(agentId || ''))}</span>
      </div>
      <div style="font-size:12px;color:var(--amber);margin-bottom:10px">
        Secret key is shown once. Copy and store it securely before closing.
      </div>
      <textarea id="agent-secret-modal-text" readonly style="width:100%;min-height:140px;resize:vertical;padding:10px;border-radius:6px;border:1px solid var(--border);background:rgba(4,14,8,0.5);color:var(--text);font-family:var(--mono);font-size:12px;line-height:1.4">${esc(String(secretKey || ''))}</textarea>
      <div style="display:flex;gap:8px;justify-content:flex-end;margin-top:12px">
        <button class="btn btn-sm" id="agent-secret-modal-copy">Copy</button>
        <button class="btn btn-sm btn-primary" id="agent-secret-modal-close">I have saved this key</button>
      </div>
    </div>
  `;
  document.body.appendChild(wrap);
  const txt = document.getElementById('agent-secret-modal-text');
  if (txt && typeof txt.select === 'function') txt.select();
  const copyBtn = document.getElementById('agent-secret-modal-copy');
  const closeBtn = document.getElementById('agent-secret-modal-close');
  if (copyBtn) {
    copyBtn.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(String(secretKey || ''));
        copyBtn.textContent = 'Copied';
        setTimeout(() => { copyBtn.textContent = 'Copy'; }, 1200);
      } catch (_e) {
        alert('Copy failed. Please copy manually.');
      }
    });
  }
  if (closeBtn) closeBtn.addEventListener('click', () => wrap.remove());
}

window.revokeAgent = async function revokeAgent(agent_id) {
  if (!confirm(`Revoke agent ${agent_id}?`)) return;
  try {
    await apiPost('/agents/revoke', { agent_id });
    await renderConfig();
  } catch (e) {
    alert(`Revoke failed: ${String(e && e.message || e)}`);
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
  const authCfg = (cfg && cfg.authentication) || {};
  const isAuthenticated = !!authCfg.authenticated;
  const dashboardAuthRequired = !!authCfg.required;
  const hasWallet = cfg && cfg.pq_wallet;
  const ss = (cfg && cfg.setup_complete) || { identity: false, wallet: false, agentpmt: false, llm: false, complete: false };
  const walletStatus = (cfg && cfg.wallet_status) || {};

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

  // Identity profile/state
  let savedProfile = { display_name: '', avatar_type: 'none' };
  let identityCfg = { anonymous_mode: false };
  let tierCfg = { tier: '' };
  try { savedProfile = await api('/profile'); } catch (_e) {}
  try { identityCfg = (await api('/identity/status')) || identityCfg; } catch (_e) {}
  try { tierCfg = (await api('/identity/tier')) || tierCfg; } catch (_e) {}
  const profileSet = !!(savedProfile.display_name && String(savedProfile.display_name).trim().length > 0);

  // Step states
  const walletComplete = (ss.wallet !== undefined) ? ss.wallet : ss.agentpmt;
  const step1Done = walletComplete;
  const step2Done = ss.llm;
  const identityDone = profileSet || !!identityCfg.anonymous_mode || ss.identity;
  const localIdentityDone = !!isAuthenticated || !!hasWallet;
  const allDone = ss.complete || (identityDone && walletComplete && step2Done);

  const pmtAuth = cfg && cfg.agentpmt && cfg.agentpmt.auth_configured;
  const pmtToolCount = (cfg && cfg.agentpmt && cfg.agentpmt.tool_count) || 0;
  const agentpmtConnected = !!walletStatus.agentpmt_connected;
  const agentaddressConnected = !!walletStatus.agentaddress_connected;
  const agentaddressAddress = String(walletStatus.agentaddress_address || '');
  const walletPath = agentpmtConnected
    ? 'agentpmt'
    : (agentaddressConnected ? 'agentaddress' : 'none');
  const hasAnyWallet = walletPath !== 'none';
  const walletCardDesc = walletPath === 'agentaddress'
    ? 'Agent identity ready for autonomous agents'
    : `Connect to AgentPMT to unlock ${pmtToolCount > 0 ? pmtToolCount + '+' : ''} tools, workflows, and budget management`;
  const orStatus = providerStatus('openrouter');

  // Card classes
  const identityCardClass = identityDone ? 'card-done' : 'card-active';
  const c1c = step1Done ? 'card-done' : 'card-active';
  const c2c = step1Done ? (step2Done ? 'card-done' : 'card-active') : 'card-locked';
  const c3c = allDone ? 'card-done card-celebrate' : 'card-locked';
  const initials = (savedProfile.display_name || '?')
    .split(/\s+/)
    .filter(Boolean)
    .map(w => w[0])
    .join('')
    .slice(0, 2)
    .toUpperCase() || '?';
  const hasSavedProfileName = !!savedProfile.name_locked
    || !!(savedProfile.display_name && String(savedProfile.display_name).trim());
  let savedSecurityTier = '';
  try { savedSecurityTier = localStorage.getItem('halo_identity_security_tier') || ''; } catch (_e) {}
  const securityTierImageByKey = {
    'max-safe': 'img/agenthalosafe_badge.png',
    'less-safe': 'img/agenthalomediumsecurity_badge.png',
    'low-security': 'img/agenthalolowsecurity_badge.png',
  };
  const showLowSafetyTierOption = false;
  const deferIdentityRoadmapTracks = true;
  const backendDefaultTier = securityTierImageByKey[String(tierCfg.default_tier || '').trim()]
    ? String(tierCfg.default_tier).trim()
    : 'max-safe';
  const serverTier = String(tierCfg.tier || '').trim();
  // If the server has a tier set, use it. Otherwise ignore localStorage (may be stale from
  // a previous install) and default to max-safe.
  const preferredTier = securityTierImageByKey[serverTier] ? serverTier : backendDefaultTier;
  const appliedSecurityTier = securityTierImageByKey[serverTier] ? serverTier : '';
  const initialSecurityTier = (
    securityTierImageByKey[preferredTier]
      && (showLowSafetyTierOption || preferredTier !== 'why-bother')
  ) ? preferredTier : backendDefaultTier;
  if (securityTierImageByKey[serverTier]) {
    try { localStorage.setItem('halo_identity_security_tier', serverTier); } catch (_e) {}
  }
  const identityConfigured = !!(identityCfg.device_configured && identityCfg.network_configured);
  const hideSafetyUI = identityCfg.anonymous_mode || identityConfigured;

  // Pre-compute LLM step done-state HTML (cannot use IIFE inside template literals)
  const _step2DoneHtml = (() => {
    if (!step2Done) return '';
    const lmChosen = cfg && cfg.local_models && cfg.local_models.config && cfg.local_models.config.local_models_chosen;
    const lm = cfg && cfg.local_models || {};
    const ollamaUp = lm.ollama && lm.ollama.healthy;
    const vllmUp = lm.vllm && lm.vllm.healthy;
    const anyBackendUp = ollamaUp || vllmUp;
    const servedModels = [
      ...(ollamaUp && lm.ollama.served_models || []),
      ...(vllmUp && lm.vllm.served_models || []),
    ];
    if (lmChosen) {
      const statusLine = anyBackendUp
        ? '<span style="color:var(--green)">&#9679;</span> Connected &mdash; '
          + (servedModels.length
            ? esc(servedModels.slice(0,3).join(', ')) + (servedModels.length > 3 ? ' (+' + (servedModels.length - 3) + ')' : '')
            : (ollamaUp ? 'Ollama' : 'vLLM') + ' ready')
        : '<span style="color:var(--amber)">&#9679;</span> No backend running &mdash; start a model from the Models tab';
      return '<div style="display:flex;align-items:center;gap:14px;margin-bottom:10px">'
        + '<div style="font-size:28px">&#128421;</div>'
        + '<div>'
        + '<div style="font-size:14px;font-weight:600">Local Models</div>'
        + '<div style="font-size:12px;color:var(--text-dim)">' + statusLine + '</div>'
        + '</div></div>'
        + '<div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap">'
        + '<a href="#/models" class="btn btn-sm btn-primary" style="border-radius:6px">Open Models Tab</a>'
        + '<button class="btn btn-sm setup-disconnect-local-models-btn" style="border-color:var(--red);color:var(--red);border-radius:6px">Disconnect</button>'
        + '</div>';
    }
    const orLabel = orStatus.tested ? 'OpenRouter <strong>verified</strong>' : 'OpenRouter connected';
    return '<div class="setup-success-banner"><span class="success-icon">&#10003;</span><span>'
      + orLabel + ' &mdash; LLM inference ready</span></div>'
      + '<div style="margin-top:14px;display:flex;gap:10px;align-items:center;flex-wrap:wrap">'
      + (orStatus.configured
        ? '<button class="btn btn-sm setup-provider-config-btn" data-provider="openrouter" style="border-radius:6px">Change Key</button>'
          + '<button class="btn btn-sm setup-provider-test-btn" data-provider="openrouter" style="border-radius:6px">Re-test</button>'
          + '<button class="btn btn-sm setup-provider-disconnect-btn" data-provider="openrouter" style="border-color:var(--red);color:var(--red);border-radius:6px">Disconnect</button>'
        : '')
      + '</div>';
  })();

  content.innerHTML = `
  <div class="setup-page-wrap">

    <!-- Hero -->
    <div class="setup-hero">
      <img class="setup-hero-img" src="img/agenthalo_ready.png" alt="Agent H.A.L.O." onerror="this.style.display='none'">
      <h1>Welcome, my name is Agent H.A.L.O., but you can just call me Hal :)</h1>
      <p>Let's get everything properly set up for you. Then, we can build something amazing together!</p>
    </div>

    <div style="border:1px solid var(--border);border-radius:10px;padding:14px 16px;margin-top:10px;background:rgba(4,14,8,0.45)">
      <div style="font-size:13px;font-weight:700;color:var(--accent);margin-bottom:8px">Quick Start</div>
      <ol class="setup-steps-friendly" style="margin:0">
        <li><span class="step-circle">1</span><span>Set your agents identity &amp; safety level</span></li>
        <li><span class="step-circle">2</span><span>Setup your agents wallet</span></li>
        <li><span class="step-circle">3</span><span>Connect your agent to an LLM</span></li>
      </ol>
    </div>

    <!-- SECTION 1: Identity -->
    <div class="setup-card-v2 ${identityCardClass}" id="setup-identity">
      <div class="identity-instruct-overlay" aria-hidden="true">
        <img class="identity-security-badge" id="identity-security-badge" src="${securityTierImageByKey[initialSecurityTier]}" alt="Current security state" onerror="this.style.display='none'">
        <img class="identity-instruct-img" src="img/agenthaloinstruct_cutout.png" alt="" onerror="this.style.display='none'">
        <div class="identity-instruct-note">The more you know about me, and I know about you, the easier it is to keep me under control.</div>
      </div>
      <div class="card-header">
        <img src="img/agenthalodl.png" alt="My Identity" style="width:100%;height:auto;display:block;border-radius:6px">
      </div>

      <div class="identity-subsection" id="identity-profile">
        <div style="font-size:13px;font-weight:700;color:var(--accent);margin-bottom:10px">
          Your Profile
        </div>
        <div class="identity-profile-row">
          <div class="avatar-preview" id="avatar-preview">${esc(initials)}</div>
          <div class="identity-profile-fields">
            <input type="text" id="profile-name-input" class="setup-input ${hasSavedProfileName ? 'profile-name-locked' : ''}"
                   placeholder="What do you want to name me?"
                   value="${esc(savedProfile.display_name || '')}" maxlength="64"
                   ${hasSavedProfileName ? 'readonly data-locked="true"' : ''}>
            <button class="btn btn-primary btn-sm" id="profile-save-btn"
                    style="border-radius:6px;padding:8px 16px">${hasSavedProfileName ? 'Rename Key' : 'Save Name'}</button>
          </div>
        </div>
      </div>

      <div class="identity-safety-intent-label" id="safety-intent-label"
           style="${hideSafetyUI ? 'display:none' : ''}">Select One Option</div>
      <div class="identity-security-tier-shell ${showLowSafetyTierOption ? '' : 'two-options'}" id="safety-tier-shell" aria-label="Identity safety tier"
           style="${hideSafetyUI ? 'display:none' : ''}">
        <button type="button" class="security-tier-btn tier-safe ${initialSecurityTier === 'max-safe' ? 'is-selected' : ''}" data-tier="max-safe">
          As Safe As Possible
        </button>
        <button type="button" class="security-tier-btn tier-caution ${initialSecurityTier === 'less-safe' ? 'is-selected' : ''}" data-tier="less-safe">
          A Little Rebellious
        </button>
      </div>
      <div class="identity-tier-control-row">
        <div id="identity-tier-status" class="identity-tier-status" aria-live="polite"></div>
        ${hideSafetyUI ? `<button type="button" class="btn btn-sm" id="identity-rescan-btn"
            style="border-radius:6px;padding:6px 14px;font-size:12px;margin-top:6px;background:var(--card-bg);border:1px solid var(--border);color:var(--text-muted)">
            Rescan Identity</button>` : ''}
      </div>

      <div class="anon-mode-shell ${identityCfg.anonymous_mode ? 'is-active' : ''}" id="anon-mode-shell"
           style="${(!identityCfg.anonymous_mode && identityConfigured) ? 'display:none' : ''}">
        <div class="anon-mode-avatar-wrap" aria-hidden="true">
          <img class="anon-mode-avatar anon-avatar-open" src="img/agenthalohiding_mode.png" alt="" onerror="this.style.display='none'">
          <img class="anon-mode-avatar anon-avatar-hidden" src="img/agenthalohidden_mode.png" alt="" onerror="this.style.display='none'">
        </div>
        <div class="anon-mode-copy">
          <div class="anon-mode-title">Total Anonymous Mode</div>
          <p class="anon-mode-desc">
            No device identifiers, no network identifiers. Each session gets a random ephemeral ID.
          </p>
        </div>
        <div class="anonymous-launch-wrap">
          <img class="anonymous-launch-ninja" src="img/agenthaloninja.png" alt="" onerror="this.style.display='none'">
          <button class="anonymous-launch-btn ${identityCfg.anonymous_mode ? 'is-armed' : ''}" type="button" id="anonymous-mode-launch-btn" aria-pressed="${identityCfg.anonymous_mode ? 'true' : 'false'}">
            ${identityCfg.anonymous_mode ? 'Disengage' : 'Engage'}
          </button>
        </div>
        <input type="checkbox" id="anonymous-mode-check" class="anon-mode-hidden-checkbox" ${identityCfg.anonymous_mode ? 'checked' : ''}>
      </div>

      <div class="identity-tech-options-title" id="tech-options-title">Individual Technical Options</div>

      <details class="setup-alt-path" id="setup-device-details" style="margin-top:12px">
        <summary>Device Identity ${identityCfg.device_configured ? '<span class="setup-inline-status status-done">&#10003; Complete</span>' : ''}</summary>
        <div class="alt-body">
          <div class="device-fingerprint-layout">
            <div class="device-fingerprint-main" id="device-main-content">
              ${identityCfg.anonymous_mode ? `
              <div style="text-align:center;padding:20px 0">
                <img src="img/agenthaloanonymous.png" alt="Anonymous mode" style="max-width:160px;border-radius:12px;margin-bottom:10px" onerror="this.style.display='none'">
                <p style="font-size:12px;color:var(--text-dim)">Anonymous mode active &mdash; device identity disabled.</p>
              </div>
              ` : (identityCfg.device_configured ? `
              <div id="device-configured-display">
                <div style="display:flex;align-items:center;gap:8px;margin-bottom:10px">
                  <span style="font-size:18px;color:var(--green)">&#9432;</span>
                  <span style="font-size:13px;color:var(--green);font-weight:700">Device Identity Verified</span>
                </div>
                <div id="device-scan-summary" style="font-size:12px;color:var(--text-muted);line-height:1.8">
                  Loading device details...
                </div>
                <div id="device-scan-status" style="font-size:12px;margin-top:8px"></div>
              </div>
              ` : `
              <div id="device-manual-setup">
                <div class="identity-option-checklist">
                  <label class="identity-option-check"><input type="checkbox" id="tier-device-enable"> Enable device identity</label>
                  <label class="identity-option-check"><input type="checkbox" id="tier-device-components"> Include hardware components</label>
                  <label class="identity-option-check"><input type="checkbox" id="tier-device-browser"> Include browser fingerprint</label>
                </div>
                <p style="font-size:13px;color:var(--text-muted);line-height:1.6;margin-bottom:14px;max-width:460px">
                  Scan your device for unique hardware identifiers. This strengthens your
                  identity for trust scoring. All data stays local.
                </p>
                <button class="btn btn-primary btn-sm" id="device-scan-btn"
                        style="border-radius:6px;padding:8px 16px;margin-bottom:12px">
                  Scan Device
                </button>
                <div id="device-scan-results" style="display:none;width:100%;max-width:460px">
                  <div id="device-components-list"></div>
                  <div id="device-entropy-bar" style="margin:12px 0"></div>
                  <button class="btn btn-primary btn-sm" id="device-save-btn"
                          style="border-radius:6px;padding:8px 16px">
                    Save Device Identity
                  </button>
                </div>
                <div id="device-scan-status" style="font-size:12px;margin-top:8px"></div>
              </div>
              `)}
            </div>
            <div class="device-fingerprint-visual">
              <img src="img/agenthalofingerprint_panel.png" alt="Device identity visual" onerror="this.style.display='none'">
            </div>
          </div>
        </div>
      </details>

      <details class="setup-alt-path" id="setup-network-details" style="margin-top:12px">
        <summary>Network Identity ${identityCfg.network_configured ? '<span class="setup-inline-status status-done">&#10003; Complete</span>' : ''}</summary>
        <div class="alt-body">
          <div class="network-identity-layout">
            <div class="network-identity-main" id="network-main-content">
              ${identityCfg.anonymous_mode ? `
              <div style="text-align:center;padding:20px 0">
                <img src="img/agenthaloanon.png" alt="Anonymous mode" style="max-width:160px;border-radius:12px;margin-bottom:10px" onerror="this.style.display='none'">
                <p style="font-size:12px;color:var(--text-dim)">Anonymous mode active &mdash; network identity disabled.</p>
              </div>
              ` : (identityCfg.network_configured ? `
              <div id="network-configured-display">
                <div style="display:flex;align-items:center;gap:8px;margin-bottom:10px">
                  <span style="font-size:18px;color:var(--green)">&#9432;</span>
                  <span style="font-size:13px;color:var(--green);font-weight:700">Network Identity Verified</span>
                </div>
                <div id="network-info" style="font-size:12px;color:var(--text-muted);line-height:1.8">
                  Loading network details...
                </div>
                <div id="network-scan-status" style="font-size:12px;margin-top:8px"></div>
              </div>
              ` : `
              <div id="network-manual-setup">
                <div class="identity-option-checklist">
                  <label class="identity-option-check"><input type="checkbox" id="share-local-ip"> Share local IP (hashed)</label>
                  <label class="identity-option-check"><input type="checkbox" id="share-mac"> Share MAC (hashed)</label>
                </div>
                <p style="font-size:13px;color:var(--text-muted);line-height:1.6;margin-bottom:14px;max-width:460px">
                  Optionally share network identifiers to strengthen your identity.
                </p>
                <div id="network-info" style="font-size:13px;color:var(--text-dim);width:100%;max-width:460px">
                  Loading network info...
                </div>
                <button class="btn btn-sm btn-primary" id="network-save-btn" style="border-radius:6px;padding:8px 16px;margin-top:10px">Save Network Identity</button>
                <p style="font-size:11px;color:var(--text-dim);margin-top:8px;max-width:460px">
                  IP/MAC values are hashed before storage. Raw values shown here for your reference only.
                </p>
              </div>
              `)}
            </div>
            <div class="network-identity-visual">
              <img src="img/agenthalonetworkidentity_panel.png" alt="Network identity visual" onerror="this.style.display='none'">
            </div>
          </div>
        </div>
      </details>

      <details class="setup-alt-path" id="setup-social-details" style="margin-top:12px;${deferIdentityRoadmapTracks ? 'display:none;' : ''}">
        <summary>Social Login & OAuth Tokens</summary>
        <div class="alt-body">
          <div class="social-identity-layout">
            <div class="social-identity-main">
              <div class="identity-option-checklist" id="social-provider-checklist">
                <label class="identity-option-check"><input type="checkbox" class="social-provider-check" data-provider="google"> Google</label>
                <label class="identity-option-check"><input type="checkbox" class="social-provider-check" data-provider="github"> GitHub</label>
                <label class="identity-option-check"><input type="checkbox" class="social-provider-check" data-provider="microsoft"> Microsoft</label>
                <label class="identity-option-check"><input type="checkbox" class="social-provider-check" data-provider="discord"> Discord</label>
                <label class="identity-option-check"><input type="checkbox" class="social-provider-check" data-provider="apple"> Apple</label>
                <label class="identity-option-check"><input type="checkbox" class="social-provider-check" data-provider="facebook"> Facebook</label>
              </div>
              <div class="social-connect-controls">
                <label class="social-expiry-label" for="social-expiry-days">Token expiry (days)</label>
                <input type="number" min="1" max="365" value="30" id="social-expiry-days" class="setup-input social-expiry-input">
                <button class="btn btn-primary btn-sm" id="social-connect-selected-btn" style="border-radius:6px;padding:8px 16px">Connect Selected</button>
                <button class="btn btn-sm" id="social-revoke-selected-btn" style="border-radius:6px;padding:8px 16px">Revoke Selected</button>
              </div>
              <div id="social-provider-status" class="social-provider-status">Loading social identity status...</div>
            </div>
            <div class="social-identity-visual">
              <div class="super-secure-note">
                <div class="super-secure-note-title">Immutable Token Record</div>
                <p>Each connect/revoke event is appended to a hash-chained identity ledger with active/recent qualifiers and expiry tracking.</p>
              </div>
            </div>
          </div>
        </div>
      </details>

      <div class="identity-super-secure-title" style="${deferIdentityRoadmapTracks ? 'display:none;' : ''}">Super Secure Options</div>
      <details class="setup-alt-path" id="setup-super-secure-details" style="margin-top:12px;${deferIdentityRoadmapTracks ? 'display:none;' : ''}">
        <summary>Advanced Verification Tracks</summary>
        <div class="alt-body">
          <div class="super-secure-layout">
            <div class="super-secure-main">
              <div class="super-secure-item">
                <div class="super-secure-item-title">Passkey (WebAuthn)</div>
                <p>Requires browser/device registration and an authenticator platform.</p>
                <label class="identity-option-check"><input type="checkbox" id="super-passkey-enabled"> Enabled</label>
                <button class="btn btn-sm btn-primary super-secure-save-btn" type="button" data-option="passkey">Apply Passkey</button>
              </div>
              <div class="super-secure-item">
                <div class="super-secure-item-title">Hardware Security Key</div>
                <p>Requires a FIDO2 key (YubiKey/solo key) and physical touch verification.</p>
                <label class="identity-option-check"><input type="checkbox" id="super-security-key-enabled"> Enabled</label>
                <button class="btn btn-sm btn-primary super-secure-save-btn" type="button" data-option="security_key">Apply Security Key</button>
              </div>
              <div class="super-secure-item">
                <div class="super-secure-item-title">Two-Factor Auth (TOTP)</div>
                <p>Requires a third-party authenticator app and rotating time-based codes.</p>
                <label class="identity-option-check"><input type="checkbox" id="super-totp-enabled"> Enabled</label>
                <input type="text" id="super-totp-label" class="setup-input" placeholder="Authenticator label (optional)">
                <button class="btn btn-sm btn-primary super-secure-save-btn" type="button" data-option="totp">Apply TOTP</button>
              </div>
            </div>
            <div class="super-secure-visual">
              <div class="super-secure-note">
                <div class="super-secure-note-title">External Steps Required</div>
                <p>These tracks raise assurance and are recorded immutably. Complete provider/device registration where applicable, then apply here.</p>
                <div id="super-secure-status" class="social-provider-status" style="margin-top:10px"></div>
              </div>
            </div>
          </div>
        </div>
      </details>
      ${deferIdentityRoadmapTracks ? `
      <details class="setup-alt-path" style="margin-top:14px" id="agentaddress-section">
        <summary>Agent Identity <span class="setup-inline-status status-done">&#10003; Complete</span></summary>
        <div class="alt-body">
          <div class="agentaddress-layout">
            <div class="agentaddress-main">
              <p style="font-size:13px;color:var(--text-muted);line-height:1.6;margin-bottom:12px">
                Your agent identity is auto-generated on first launch. When a genesis seed is available,
                it is derived locally from that seed; otherwise it falls back to external generation.
              </p>
              <div id="agentaddress-status" style="font-size:12px;color:var(--text-dim);margin-bottom:10px"></div>
              <div style="margin-bottom:10px">
                <button class="btn btn-sm btn-primary" id="agentidentity-genesis-btn" type="button"
                        style="display:none;font-size:11px;padding:6px 14px;border-radius:5px">
                  Generate from Genesis
                </button>
                <button class="btn btn-sm" id="agentidentity-retry-btn" type="button"
                        style="display:none;font-size:11px;padding:6px 14px;border-radius:5px">
                  Retry Auto Setup
                </button>
              </div>
              <div id="agentaddress-output" style="display:none;border:1px solid var(--border);border-radius:8px;padding:12px;margin-bottom:0;background:rgba(4,14,8,0.45)">
                <div style="font-size:12px;color:var(--green);margin-bottom:8px">&#10003; Agent identity ready</div>
                <div class="wallet-creds-grid">
                  <div class="wallet-cred-row">
                    <strong>Address</strong>
                    <code id="agentaddress-evm-address"></code>
                    <button class="btn btn-sm agentaddress-copy-btn" type="button" data-copy-target="agentaddress-evm-address">Copy</button>
                  </div>
                </div>
                <div style="margin-top:10px">
                  <button type="button" id="vault-info-toggle" style="background:none;border:1px solid var(--border);border-radius:5px;color:var(--text-muted);font-size:11px;padding:4px 10px;cursor:pointer;display:inline-flex;align-items:center;gap:4px">
                    <span class="info-icon" style="font-size:13px">&#9432;</span> Key Storage
                  </button>
                  <div id="vault-info-detail" class="setup-info-box" style="display:none;margin-top:8px">
                    <span>Your private key and recovery phrase are encrypted automatically in the local vault (AES-256-GCM).
                      To access them, use the CLI: <code style="font-size:10px">agenthalo vault get agent_wallet_private_key</code>
                      and <code style="font-size:10px">agenthalo vault get agent_wallet_mnemonic</code>.
                      The vault file is at <code style="font-size:10px">~/.agenthalo/vault.enc</code>.</span>
                  </div>
                </div>
              </div>
            </div>
            <div class="agentaddress-visual">
              <div style="text-align:center;font-size:9px;color:var(--text-dim);margin-bottom:4px">Works on all EVM-compatible chains</div>
              <img src="img/agenthaloidentity.png" alt="Agent identity visual" onerror="this.style.display='none'">
            </div>
          </div>
        </div>
      </details>
      ` : ''}
    </div>

    <!-- SECTION 2: Wallet -->
    <div class="setup-card-v2 ${c1c}" id="setup-wallet">
      <div class="card-header">
        ${agentpmtConnected
          ? '<img class="card-icon-logo" src="img/agentpmt-192.png" alt="AgentPMT" title="AgentPMT" onerror="this.outerHTML=\'<div class=\\\'card-icon\\\'>&#9883;</div>\'">'
          : '<div class="card-icon">&#9883;</div>'}
        <div>
          <div class="card-title">
            Your Wallet
            ${step1Done
              ? (agentpmtConnected
                ? '<span class="setup-inline-status status-done">&#10003; AgentPMT Connected</span>'
                : (agentaddressConnected
                  ? '<span class="setup-inline-status status-done">&#10003; Agent Wallet Connected</span>'
                  : '<span class="setup-inline-status status-done">&#10003; Connected</span>'))
              : '<span class="setup-inline-status status-missing">&#10007; Not Connected</span>'}
          </div>
          <div class="card-desc">${walletCardDesc}</div>
          <div class="setup-wallet-summary">
            <span class="setup-wallet-chip ${hasAnyWallet ? 'ok' : 'bad'}">
              ${hasAnyWallet ? '&#10003;' : '&#10007;'} Wallet Presence
            </span>
            <span class="setup-wallet-chip ${agentpmtConnected ? 'ok' : 'bad'}">
              ${agentpmtConnected ? '&#10003;' : '&#10007;'} AgentPMT
            </span>
            <span class="setup-wallet-chip ${agentaddressConnected ? 'ok' : 'bad'}">
              ${agentaddressConnected ? '&#10003;' : '&#10007;'} Agent Wallet
            </span>
          </div>
        </div>
      </div>

      ${step1Done ? `
        <!-- Connected state -->
        <div class="setup-success-banner">
          <span class="success-icon">&#10003;</span>
          <span>
            ${walletPath === 'agentpmt'
              ? `AgentPMT connected${pmtToolCount > 0 ? ' &mdash; <strong>' + pmtToolCount + ' tools</strong> ready to use' : ''}`
              : `Agent wallet connected${agentaddressAddress ? ` &mdash; <code>${esc(agentaddressAddress)}</code>` : ''}`}
          </span>
        </div>
        ${walletPath === 'agentpmt' ? `
          <div style="margin-top:16px;display:flex;gap:10px;align-items:center;flex-wrap:wrap">
            <button class="btn btn-sm" id="setup-disconnect-agentpmt" style="border-color:var(--red);color:var(--red)">
              Disconnect My Account
            </button>
            <span style="font-size:11px;color:var(--text-dim)">Removes your token and disables the tool proxy</span>
          </div>
        ` : ''}
      ` : `
        <!-- Not connected — three paths -->

        <!-- Path A: Sign up or sign in at AgentPMT -->
        <div class="setup-recommended">
          <div class="setup-recommended-label">Recommended</div>
          <p style="font-size:14px;color:var(--text-muted);line-height:1.6;margin-bottom:16px">
            AgentPMT is your gateway to 100+ third-party tools, budget controls, and workflow automation.
            Create a free account (or sign in), then grab your Bearer Token.
          </p>
          <div class="setup-info-box" style="margin-top:0;margin-bottom:12px">
            <span class="info-icon">&#9432;</span>
            <span>Dashboard quick-connect uses a Bearer Token. Fully autonomous mode uses wallet signatures + credits (see <a href="https://www.agentpmt.com/autonomous-agents" target="_blank" rel="noopener noreferrer" style="color:var(--accent)">Autonomous Agents</a>).</span>
          </div>

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

        <!-- Path B: Quick connect without signup -->
        <details class="setup-alt-path" style="margin-top:16px">
          <summary>Skip signup &mdash; connect without an account</summary>
          <div class="alt-body">
            <p style="font-size:13px;color:var(--text-muted);line-height:1.6;margin-bottom:14px">
              Don't want to create an account right now? We can request an API token directly &mdash;
              no email or signup required. Limited budget; can be upgraded later.
            </p>
            <div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap">
              <button class="btn btn-primary" id="setup-create-anon-wallet" style="border-radius:6px;padding:10px 20px;font-size:13px">
                Connect Without Account
              </button>
              <span id="setup-anon-wallet-status" style="font-size:12px;color:var(--text-dim)"></span>
            </div>
          </div>
        </details>
      `}

    </div>

    <!-- CLI Agent Setup (above OpenRouter) -->
    <div class="setup-card-v2" id="setup-cli-agents" style="margin-bottom:0;border-bottom:none;border-radius:10px 10px 0 0">
      <div class="card-header">
        <div class="card-icon">&#9000;</div>
        <div>
          <div class="card-title">Agent CLIs</div>
          <div class="card-desc">Agent CLIs install in the background at startup &mdash; authenticate each via browser OAuth</div>
        </div>
      </div>
      <div class="cli-agents-grid" id="cli-agents-grid">
        <div class="cli-agent-row" data-cli="claude">
          <div class="cli-agent-info">
            <div class="cli-agent-name">Claude Code</div>
            <div class="cli-agent-provider">Anthropic</div>
          </div>
          <div class="cli-agent-status" id="cli-status-claude">Installing...</div>
          <div class="cli-agent-actions">
            <button class="btn btn-sm btn-primary cli-auth-btn" data-cli="claude" disabled>Authenticate</button>
          </div>
        </div>
        <div class="cli-agent-row" data-cli="codex">
          <div class="cli-agent-info">
            <div class="cli-agent-name">Codex</div>
            <div class="cli-agent-provider">OpenAI</div>
          </div>
          <div class="cli-agent-status" id="cli-status-codex">Installing...</div>
          <div class="cli-agent-actions">
            <button class="btn btn-sm btn-primary cli-auth-btn" data-cli="codex" disabled>Authenticate</button>
          </div>
        </div>
        <div class="cli-agent-row" data-cli="gemini">
          <div class="cli-agent-info">
            <div class="cli-agent-name">Gemini CLI</div>
            <div class="cli-agent-provider">Google</div>
          </div>
          <div class="cli-agent-status" id="cli-status-gemini">Installing...</div>
          <div class="cli-agent-actions">
            <button class="btn btn-sm btn-primary cli-auth-btn" data-cli="gemini" disabled>Authenticate</button>
          </div>
        </div>
      </div>
      <div id="cli-auth-terminal-wrap" style="display:none;margin-top:14px">
        <div style="font-size:12px;color:var(--accent);margin-bottom:6px" id="cli-auth-terminal-label">Authentication session</div>
        <div id="cli-auth-terminal" style="height:260px;border:1px solid var(--border);border-radius:6px;overflow:hidden"></div>
        <div style="margin-top:8px;display:flex;gap:8px">
          <button class="btn btn-sm" id="cli-auth-terminal-close">Close Terminal</button>
        </div>
      </div>
    </div>

    <!-- SECTION 3: Connect Your LLM -->
    <div class="setup-card-v2 ${step2Done ? 'card-done' : 'card-active'}" id="setup-llm" style="border-radius:0 0 10px 10px;border-top:1px solid var(--border);margin-top:0">
      <div class="card-header">
        <div class="card-icon" style="font-size:22px">&#9889;</div>
        <div>
          <div class="card-title">
            Connect Your LLM
            ${step2Done ? '<span class="setup-inline-status status-done">&#10003; Ready</span>' : ''}
          </div>
          <div class="card-desc">Choose how your agents access language models</div>
        </div>
      </div>

      ${step2Done ? _step2DoneHtml : `
        <!-- Choice: two options -->
        <div class="setup-llm-choice-grid">

          <!-- Option A: Local Models -->
          <div class="setup-llm-option" id="setup-local-models">
            <div class="setup-llm-option-icon">&#128421;</div>
            <div class="setup-llm-option-title">Local Models</div>
            <div class="setup-llm-option-desc">
              Run models on your own hardware via vLLM. Free, private, no API key needed.
            </div>
            <button class="btn btn-primary" id="setup-choose-local-models-btn"
                    style="border-radius:8px;padding:10px 24px;font-size:13px;margin-top:auto">
              Use Local Models
            </button>
            <div id="setup-local-models-status" style="margin-top:6px;font-size:11px;min-height:16px"></div>
          </div>

          <!-- Divider -->
          <div class="setup-llm-divider">
            <span>or</span>
          </div>

          <!-- Option B: OpenRouter -->
          <div class="setup-llm-option" id="setup-openrouter">
            <div class="setup-llm-option-icon">
              <img src="img/openrouter-logo.svg" alt="OpenRouter" style="height:28px;opacity:0.85">
            </div>
            <div class="setup-llm-option-title">OpenRouter</div>
            <div class="setup-llm-option-desc">
              One key for 200+ cloud models. Sign up or log in &mdash; no copy-paste needed.
            </div>
            <button class="btn btn-primary" id="openrouter-oauth-connect-btn"
                    style="border-radius:8px;padding:10px 24px;font-size:13px;margin-top:auto">
              Connect OpenRouter
            </button>
            <div id="openrouter-oauth-status" style="margin-top:6px;font-size:11px;min-height:16px"></div>
          </div>

        </div>

        <details style="margin-top:16px">
          <summary style="font-size:11px;color:var(--text-dim);cursor:pointer">
            Or paste an OpenRouter API key manually
          </summary>
          <div style="margin-top:8px">
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
          </div>
        </details>

        ${step1Done ? `
          <div class="setup-info-box" style="margin-top:14px">
            <span class="info-icon">&#9888;</span>
            <span>Add your OpenRouter key so customers can use LLM inference through your agents.</span>
          </div>
        ` : ''}
      `}
    </div>

    <!-- SECTION: OpenClaw Harness -->
    <div class="setup-card-v2" id="setup-harness" style="margin-top:20px">
      <div class="card-header">
        <div class="card-icon" style="font-size:24px">&#129438;</div>
        <div>
          <div class="card-title">OpenClaw Harness <span style="font-size:11px;font-weight:400;color:var(--text-dim);margin-left:6px">(Optional)</span></div>
          <div class="card-desc">Configure OpenClaw gateway daemon and wire MCP tools for full agent orchestration</div>
        </div>
      </div>

      <div class="harness-steps">
        <!-- Step 1: CLI Status -->
        <div class="harness-step" id="harness-step-install">
          <div class="harness-step-num">1</div>
          <div class="harness-step-body">
            <div class="harness-step-title">OpenClaw CLI</div>
            <div class="harness-step-desc">Pre-installed in the container image.</div>
            <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-top:8px">
              <span class="harness-status" id="harness-install-status">Checking...</span>
            </div>
          </div>
        </div>

        <!-- Step 2: Onboard & Gateway -->
        <div class="harness-step" id="harness-step-onboard">
          <div class="harness-step-num">2</div>
          <div class="harness-step-body">
            <div class="harness-step-title">Onboard &amp; Start Gateway</div>
            <div class="harness-step-desc">
              Runs the interactive onboard wizard, configures auth, and installs the gateway as a system daemon (systemd/launchd).
            </div>
            <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-top:8px">
              <span class="harness-status" id="harness-gateway-status">Unknown</span>
              <button class="btn btn-sm btn-primary" id="harness-onboard-btn" disabled>Launch Onboard Wizard</button>
            </div>
          </div>
        </div>

        <!-- Step 3: Wire MCP -->
        <div class="harness-step" id="harness-step-mcp">
          <div class="harness-step-num">3</div>
          <div class="harness-step-body">
            <div class="harness-step-title">Wire MCP Tools</div>
            <div class="harness-step-desc">
              Auto-configures OpenClaw to use NucleusDB (37+ verifiable database tools) and HALO
              (identity, attestation, mesh) MCP servers. Injects <code>mcpServers</code> into
              <code>~/.openclaw/openclaw.json</code> &mdash; OpenClaw spawns them as child processes.
            </div>
            <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-top:8px">
              <span class="harness-status" id="harness-mcp-status"></span>
              <button class="btn btn-sm btn-primary" id="harness-wire-mcp-btn" disabled>Wire MCP Servers</button>
            </div>
          </div>
        </div>
      </div>

      <!-- Embedded PTY terminal for onboard wizard -->
      <div id="harness-terminal-wrap" style="display:none;margin-top:14px">
        <div style="font-size:12px;color:var(--accent);margin-bottom:6px" id="harness-terminal-label">OpenClaw onboard wizard</div>
        <div id="harness-terminal" style="height:320px;border:1px solid var(--border);border-radius:6px;overflow:hidden"></div>
        <div style="margin-top:8px;display:flex;gap:8px">
          <button class="btn btn-sm" id="harness-terminal-close">Close Terminal</button>
        </div>
      </div>
    </div>

    <!-- SECTION 4: Dashboard Unlocked -->
    <div class="setup-card-v2 ${c3c}" id="setup-unlocked">
      <div class="card-header">
        <div class="card-icon">&#127919;</div>
        <div>
          <div class="card-title">
            ${allDone ? 'Dashboard Unlocked!' : 'Almost There'}
            ${allDone ? '<span class="setup-inline-status status-done">&#10003; Done</span>' : ''}
          </div>
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
  </div>
  `;

  // ---- Populate configured identity displays ----
  if (identityCfg.device_configured && !identityCfg.anonymous_mode) {
    const deviceSummary = document.getElementById('device-scan-summary');
    if (deviceSummary) {
      (async () => {
        try {
          const deviceData = await api('/identity/device');
          let html = '';
          (deviceData.components || []).forEach(c => {
            const icon = c.stable ? '&#10003;' : '&#9888;';
            const color = c.stable ? 'var(--green)' : 'var(--yellow)';
            html += `<div style="display:flex;align-items:center;gap:6px;padding:2px 0"><span style="color:${color}">${icon}</span> <span>${esc(c.name)}</span> <span style="color:var(--text-dim);font-size:11px">${c.entropy_bits || 0} bits</span></div>`;
          });
          const totalEntropy = (deviceData.components || []).reduce((s, c) => s + (c.entropy_bits || 0), 0);
          html += `<div style="margin-top:8px;padding-top:6px;border-top:1px solid var(--border)">Entropy: <strong>${totalEntropy} bits</strong> | Tier: <strong>${esc(deviceData.tier || 'unknown')}</strong></div>`;
          deviceSummary.innerHTML = html;
        } catch (_e) {
          deviceSummary.innerHTML = '<span style="color:var(--text-dim)">Device identity saved.</span>';
        }
      })();
    }
  }
  if (identityCfg.network_configured && !identityCfg.anonymous_mode) {
    const networkInfo = document.getElementById('network-info');
    if (networkInfo) {
      (async () => {
        try {
          const networkData = await api('/identity/network');
          networkInfo.innerHTML = `
            <div style="display:flex;align-items:center;gap:6px;padding:2px 0"><span style="color:var(--green)">&#10003;</span> Local IP: <strong>${esc(networkData.local_ip || 'not shared')}</strong></div>
            <div style="display:flex;align-items:center;gap:6px;padding:2px 0"><span style="color:var(--green)">&#10003;</span> MAC: <strong>${esc(networkData.mac_address || 'not shared')}</strong></div>
          `;
          networkInfo.dataset.loaded = '1';
        } catch (_e) {
          networkInfo.innerHTML = '<span style="color:var(--text-dim)">Network identity saved.</span>';
        }
      })();
    }
  }

  // ---- Wire up interactive elements ----

  // --- CLI Agent Install & Auth ---
  (async () => {
    const cliAgents = ['claude', 'codex', 'gemini'];
    // Poll for CLI install completion (background install runs at boot)
    const cliInstalled = {};
    async function pollCliDetect() {
      for (const cli of cliAgents) {
        if (cliInstalled[cli]) continue;
        const statusEl = document.getElementById('cli-status-' + cli);
        const row = document.querySelector('.cli-agent-row[data-cli="' + cli + '"]');
        const authBtn = row && row.querySelector('.cli-auth-btn');
        try {
          const resp = await api('/cli/detect/' + cli);
          if (resp.installed) {
            cliInstalled[cli] = true;
            if (resp.authenticated) {
              if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Authenticated</span>';
              if (authBtn) {
                authBtn.disabled = false;
                authBtn.textContent = 'Re-authenticate';
                authBtn.classList.remove('btn-primary');
              }
            } else {
              if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Installed</span> <span style="color:var(--yellow)">(not authenticated)</span>';
              if (authBtn) authBtn.disabled = false;
            }
          } else {
            if (statusEl) statusEl.innerHTML = '<span style="color:var(--yellow)">&#8987; Installing...</span>';
          }
        } catch (_e) {
          if (statusEl) statusEl.innerHTML = '<span style="color:var(--text-dim)">Checking...</span>';
        }
      }
      // Keep polling until all are installed
      if (cliAgents.some(c => !cliInstalled[c])) {
        setTimeout(pollCliDetect, 4000);
      }
    }
    pollCliDetect();
    // Auth button handlers — open a PTY terminal for OAuth flow
    let _cliAuthTerm = null;
    let _cliAuthFitAddon = null;
    let _cliAuthWs = null;
    for (const btn of $$('.cli-auth-btn')) {
      btn.addEventListener('click', async () => {
        const cli = btn.dataset.cli;
        const statusEl = document.getElementById('cli-status-' + cli);
        btn.disabled = true;
        btn.textContent = 'Starting...';
        try {
          const resp = await apiPost('/cli/auth/' + cli, {});
          if (!resp.session_id) throw new Error('no session returned');
          // Show embedded terminal for the auth session
          const termWrap = document.getElementById('cli-auth-terminal-wrap');
          const termEl = document.getElementById('cli-auth-terminal');
          const termLabel = document.getElementById('cli-auth-terminal-label');
          if (termWrap) termWrap.style.display = 'block';
          if (termLabel) termLabel.textContent = cli.charAt(0).toUpperCase() + cli.slice(1) + ' authentication — complete the login in your browser';
          // Clean up any previous terminal
          if (_cliAuthWs) { try { _cliAuthWs.close(); } catch (_e) {} _cliAuthWs = null; }
          if (_cliAuthTerm) { try { _cliAuthTerm.dispose(); } catch (_e) {} _cliAuthTerm = null; }
          if (termEl) termEl.innerHTML = '';
          // Create xterm instance
          if (typeof Terminal !== 'undefined' && termEl) {
            _cliAuthTerm = new Terminal({ cursorBlink: true, fontSize: 13, theme: { background: '#0a0a0a', foreground: '#33ff33' } });
            _cliAuthFitAddon = new FitAddon.FitAddon();
            _cliAuthTerm.loadAddon(_cliAuthFitAddon);
            _cliAuthTerm.open(termEl);
            try { _cliAuthFitAddon.fit(); } catch (_e) {}
            // Connect WebSocket
            const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = proto + '//' + location.host + '/api/cockpit/sessions/' + resp.session_id + '/ws';
            _cliAuthWs = new WebSocket(wsUrl);
            _cliAuthWs.binaryType = 'arraybuffer';
            let _cliAuthOpened = false;
            _cliAuthWs.onmessage = (ev) => {
              let text = '';
              if (ev.data instanceof ArrayBuffer) {
                const bytes = new Uint8Array(ev.data);
                _cliAuthTerm.write(bytes);
                text = new TextDecoder().decode(bytes);
              } else {
                _cliAuthTerm.write(ev.data);
                text = ev.data;
              }
              // Auto-open OAuth URLs in a popup window
              if (!_cliAuthOpened) {
                const m = text.match(/https:\/\/[^\s\x1b\x07]+/);
                if (m) {
                  _cliAuthOpened = true;
                  const authUrl = m[0].replace(/[\x00-\x1f]/g, '');
                  window.open(authUrl, '_blank', 'width=600,height=700,scrollbars=yes');
                }
              }
            };
            _cliAuthWs.onclose = async () => {
              _cliAuthTerm.write('\r\n\x1b[90m--- session ended ---\x1b[0m\r\n');
              btn.disabled = false;
              // Re-poll detect to verify actual auth status from token files
              try {
                const detect = await api('/cli/detect/' + cli);
                if (detect.authenticated) {
                  if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Authenticated</span>';
                  btn.textContent = 'Re-authenticate';
                  btn.classList.remove('btn-primary');
                } else {
                  if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Installed</span> <span style="color:var(--red)">(auth failed)</span>';
                  btn.textContent = 'Authenticate';
                }
              } catch (_e) {
                // Fallback: show as authenticated (session completed)
                if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Authenticated</span>';
                btn.textContent = 'Re-authenticate';
                btn.classList.remove('btn-primary');
              }
            };
            _cliAuthTerm.onData((data) => {
              if (_cliAuthWs && _cliAuthWs.readyState === WebSocket.OPEN) {
                _cliAuthWs.send(data);
              }
            });
          }
        } catch (e) {
          if (statusEl) statusEl.innerHTML = '<span style="color:var(--red)">Auth error: ' + esc(String(e.message || e)) + '</span>';
        }
        btn.disabled = false;
        btn.textContent = 'Authenticate';
      });
    }
    // Close terminal button
    const closeTermBtn = document.getElementById('cli-auth-terminal-close');
    if (closeTermBtn) {
      closeTermBtn.addEventListener('click', () => {
        if (_cliAuthWs) { try { _cliAuthWs.close(); } catch (_e) {} _cliAuthWs = null; }
        if (_cliAuthTerm) { try { _cliAuthTerm.dispose(); } catch (_e) {} _cliAuthTerm = null; }
        const termWrap = document.getElementById('cli-auth-terminal-wrap');
        if (termWrap) termWrap.style.display = 'none';
      });
    }
  })();

  // ---- OpenClaw Harness wiring ----
  (async () => {
    let _harnessWs = null;
    let _harnessTerm = null;
    let _harnessFitAddon = null;

    const installStatus = document.getElementById('harness-install-status');
    const gatewayStatus = document.getElementById('harness-gateway-status');
    const mcpStatus = document.getElementById('harness-mcp-status');
    const onboardBtn = document.getElementById('harness-onboard-btn');
    const wireMcpBtn = document.getElementById('harness-wire-mcp-btn');

    // Auto-detect openclaw on page load
    async function pollOpenClawDetect() {
      try {
        const detect = await api('/cli/detect/openclaw');
        if (detect.installed) {
          if (installStatus) installStatus.innerHTML = '<span style="color:var(--green)">&#10003; Installed</span>' + (detect.path ? ' <span style="color:var(--text-dim);font-size:11px">(' + esc(detect.path) + ')</span>' : '');
          if (onboardBtn) onboardBtn.disabled = false;
          if (wireMcpBtn) wireMcpBtn.disabled = false;
          try {
            const gw = await api('/openclaw/gateway-status');
            if (gw.gateway_running) {
              if (gatewayStatus) gatewayStatus.innerHTML = '<span style="color:var(--green)">&#10003; Gateway running</span>';
            } else {
              if (gatewayStatus) gatewayStatus.innerHTML = '<span style="color:var(--yellow)">&#9888; Gateway not running</span>';
            }
          } catch (_e) {
            if (gatewayStatus) gatewayStatus.innerHTML = '<span style="color:var(--text-dim)">Could not check gateway</span>';
          }
        } else {
          if (installStatus) installStatus.innerHTML = '<span style="color:var(--yellow)">&#8987; Installing...</span>';
          setTimeout(pollOpenClawDetect, 4000);
        }
      } catch (_e) {
        if (installStatus) installStatus.innerHTML = '<span style="color:var(--text-dim)">Checking...</span>';
        setTimeout(pollOpenClawDetect, 4000);
      }
    }
    pollOpenClawDetect();

    // Onboard wizard button — launches PTY
    if (onboardBtn) {
      onboardBtn.addEventListener('click', async () => {
        onboardBtn.disabled = true;
        onboardBtn.textContent = 'Starting...';
        try {
          const resp = await apiPost('/cli/auth/openclaw', {});
          if (!resp.session_id) throw new Error('no session returned');
          // Show embedded terminal
          const termWrap = document.getElementById('harness-terminal-wrap');
          const termEl = document.getElementById('harness-terminal');
          const termLabel = document.getElementById('harness-terminal-label');
          if (termWrap) termWrap.style.display = 'block';
          if (termLabel) termLabel.textContent = 'OpenClaw onboard wizard — follow the prompts to configure gateway and daemon';
          // Clean up previous
          if (_harnessWs) { try { _harnessWs.close(); } catch (_e) {} _harnessWs = null; }
          if (_harnessTerm) { try { _harnessTerm.dispose(); } catch (_e) {} _harnessTerm = null; }
          if (termEl) termEl.innerHTML = '';
          if (typeof Terminal !== 'undefined' && termEl) {
            _harnessTerm = new Terminal({ cursorBlink: true, fontSize: 13, theme: { background: '#0a0a0a', foreground: '#33ff33' } });
            _harnessFitAddon = new FitAddon.FitAddon();
            _harnessTerm.loadAddon(_harnessFitAddon);
            _harnessTerm.open(termEl);
            try { _harnessFitAddon.fit(); } catch (_e) {}
            const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
            const wsUrl = proto + '//' + location.host + '/api/cockpit/sessions/' + resp.session_id + '/ws';
            _harnessWs = new WebSocket(wsUrl);
            _harnessWs.binaryType = 'arraybuffer';
            let _harnessAuthOpened = false;
            _harnessWs.onmessage = (ev) => {
              let text = '';
              if (ev.data instanceof ArrayBuffer) {
                const bytes = new Uint8Array(ev.data);
                _harnessTerm.write(bytes);
                text = new TextDecoder().decode(bytes);
              } else {
                _harnessTerm.write(ev.data);
                text = ev.data;
              }
              if (!_harnessAuthOpened) {
                const m = text.match(/https:\/\/[^\s\x1b\x07]+/);
                if (m) {
                  _harnessAuthOpened = true;
                  const authUrl = m[0].replace(/[\x00-\x1f]/g, '');
                  window.open(authUrl, '_blank', 'width=600,height=700,scrollbars=yes');
                }
              }
            };
            _harnessWs.onclose = () => {
              _harnessTerm.write('\r\n\x1b[90m--- onboard session ended ---\x1b[0m\r\n');
              if (gatewayStatus) gatewayStatus.innerHTML = '<span style="color:var(--green)">&#10003; Onboard complete — checking gateway...</span>';
              // Re-check gateway status after onboard completes
              (async () => {
                try {
                  const gw = await api('/openclaw/gateway-status');
                  if (gw.gateway_running) {
                    if (gatewayStatus) gatewayStatus.innerHTML = '<span style="color:var(--green)">&#10003; Gateway running</span>';
                  } else {
                    if (gatewayStatus) gatewayStatus.innerHTML = '<span style="color:var(--yellow)">&#9888; Gateway not running &mdash; run <code>openclaw gateway start</code> manually</span>';
                  }
                } catch (_e) {}
              })();
            };
            _harnessTerm.onData((data) => {
              if (_harnessWs && _harnessWs.readyState === WebSocket.OPEN) {
                _harnessWs.send(data);
              }
            });
          }
        } catch (e) {
          if (gatewayStatus) gatewayStatus.innerHTML = '<span style="color:var(--red)">Onboard error: ' + esc(String(e.message || e)) + '</span>';
        }
        onboardBtn.disabled = false;
        onboardBtn.textContent = 'Launch Onboard Wizard';
      });
    }

    // Wire MCP button
    if (wireMcpBtn) {
      wireMcpBtn.addEventListener('click', async () => {
        wireMcpBtn.disabled = true;
        wireMcpBtn.textContent = 'Wiring...';
        if (mcpStatus) mcpStatus.innerHTML = '<span style="color:var(--text-dim)">Injecting MCP server configs...</span>';
        try {
          const resp = await apiPost('/openclaw/wire-mcp', {});
          if (resp.success) {
            const detail = resp.detail || {};
            const tools = (detail.tools_wired || []).join(', ');
            if (mcpStatus) mcpStatus.innerHTML = '<span style="color:var(--green)">&#10003; Wired: ' + esc(tools) + '</span>' +
              '<br><span style="font-size:11px;color:var(--text-dim)">Config: ' + esc(detail.config_path || '~/.openclaw/openclaw.json') + '</span>';
          } else {
            if (mcpStatus) mcpStatus.innerHTML = '<span style="color:var(--red)">Wiring failed</span>';
          }
        } catch (e) {
          if (mcpStatus) mcpStatus.innerHTML = '<span style="color:var(--red)">Error: ' + esc(String(e.message || e)) + '</span>';
        }
        wireMcpBtn.disabled = false;
        wireMcpBtn.textContent = 'Wire MCP Servers';
      });
    }

    // Close terminal button
    const harnessCloseBtn = document.getElementById('harness-terminal-close');
    if (harnessCloseBtn) {
      harnessCloseBtn.addEventListener('click', () => {
        if (_harnessWs) { try { _harnessWs.close(); } catch (_e) {} _harnessWs = null; }
        if (_harnessTerm) { try { _harnessTerm.dispose(); } catch (_e) {} _harnessTerm = null; }
        const termWrap = document.getElementById('harness-terminal-wrap');
        if (termWrap) termWrap.style.display = 'none';
      });
    }
  })();

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

  // Quick-connect (no signup) handler
  const anonWalletBtn = document.getElementById('setup-create-anon-wallet');
  const anonWalletStatus = document.getElementById('setup-anon-wallet-status');
  if (anonWalletBtn) {
    anonWalletBtn.addEventListener('click', async () => {
      anonWalletBtn.disabled = true;
      anonWalletBtn.textContent = 'Connecting...';
      if (anonWalletStatus) {
        anonWalletStatus.innerHTML = '<span style="color:var(--text-dim)">Requesting API token...</span>';
      }
      try {
        const resp = await apiPost('/agentpmt/anonymous-wallet', {});
        if (resp && resp.token_saved) {
          if (anonWalletStatus) {
            anonWalletStatus.innerHTML = '<span style="color:var(--green)">&#10003; Connected!</span>';
          }
          window._invalidateSetupState();
          await fetchSetupState(true);
          await renderSetup();
          updateNavLockState();
        } else {
          if (anonWalletStatus) {
            anonWalletStatus.innerHTML = '<span style="color:var(--yellow)">Request succeeded but no token was returned.</span>';
          }
          anonWalletBtn.disabled = false;
          anonWalletBtn.textContent = 'Connect Without Account';
        }
      } catch (e) {
        if (anonWalletStatus) {
          anonWalletStatus.innerHTML = `<span style="color:var(--red)">Failed: ${esc(String(e.message || e))}</span>`;
        }
        anonWalletBtn.disabled = false;
        anonWalletBtn.textContent = 'Connect Without Account';
      }
    });
  }

  // AgentAddress state + handlers
  const agentAddressStatus = document.getElementById('agentaddress-status');
  const agentAddressOutput = document.getElementById('agentaddress-output');
  const agentAddressField = (id, val) => {
    const node = document.getElementById(id);
    if (node) node.textContent = val || '';
  };
  const setAgentAddressOutput = (payload) => {
    const address = String(payload.evmAddress || payload.evm_address || '');
    const privateKey = String(payload.evmPrivateKey || payload.evm_private_key || '');
    const mnemonic = String(payload.mnemonic || '');
    if (agentAddressOutput) {
      const shouldShow = !!(address || privateKey || mnemonic);
      agentAddressOutput.style.display = shouldShow ? 'block' : 'none';
    }
    agentAddressField('agentaddress-evm-address', address);
    // Private key and mnemonic are vault-stored only — never shown in UI.
    if (address) {
      window.__haloGeneratedAgentAddress = Object.assign(
        window.__haloGeneratedAgentAddress || {},
        { evmAddress: address }
      );
    }
  };

  const autoRetryBtn = document.getElementById('agentidentity-retry-btn');
  const genesisGenerateBtn = document.getElementById('agentidentity-genesis-btn');
  if (agentaddressConnected && agentAddressStatus) {
    agentAddressStatus.innerHTML = '<span style="color:var(--green)">&#10003; Identity ready and secured.</span>';
  } else if (agentAddressStatus) {
    agentAddressStatus.innerHTML = '<span style="color:var(--text-dim)">Provisioning agent identity...</span>';
  }
  if (window.__haloGeneratedAgentAddress && typeof window.__haloGeneratedAgentAddress === 'object') {
    setAgentAddressOutput(window.__haloGeneratedAgentAddress);
  } else if (agentaddressConnected && agentaddressAddress) {
    setAgentAddressOutput({ evmAddress: agentaddressAddress });
  }

  const autoProvisionState = window.__haloIdentityAutoProvision
    || (window.__haloIdentityAutoProvision = { inFlight: false, attempted: false });
  const needsAddress = !agentaddressConnected;

  const maybeShowRetry = (show) => {
    if (!autoRetryBtn) return;
    autoRetryBtn.style.display = show ? '' : 'none';
  };

  const maybeShowGenesisGenerate = (show) => {
    if (!genesisGenerateBtn) return;
    genesisGenerateBtn.style.display = show ? '' : 'none';
  };

  const runAutoProvision = async (force = false, preferredSource = null) => {
    if (autoProvisionState.inFlight) return;
    if (!needsAddress && !force) return;
    if (!needsAddress || (!force && autoProvisionState.attempted)) return;

    autoProvisionState.inFlight = true;
    autoProvisionState.attempted = true;
    maybeShowRetry(false);
    try {
      if (agentAddressStatus) {
        agentAddressStatus.innerHTML = '<span style="color:var(--text-dim)">Generating agent identity...</span>';
      }
      const genesisReady = await fetchGenesisStatus();
      const source = preferredSource || (genesisReady ? 'genesis' : 'external');
      maybeShowGenesisGenerate(!agentaddressConnected && genesisReady);
      const resp = await apiPost('/agentaddress/generate', {
        persist_public_address: true,
        source
      });
      const generatedAddress = resp && resp.data ? resp.data : null;
      if (generatedAddress) {
        setAgentAddressOutput(generatedAddress);
        if (agentAddressStatus) {
          const mode = source === 'genesis' ? ' (genesis-derived)' : '';
          agentAddressStatus.innerHTML = `<span style="color:var(--green)">&#10003; Identity ready and secured${mode}.</span>`;
        }
      }
      window._invalidateSetupState();
      await fetchSetupState(true);
      await renderSetup();
      updateNavLockState();
    } catch (e) {
      if (agentAddressStatus) {
        agentAddressStatus.innerHTML = `<span style="color:var(--red)">Identity setup failed: ${esc(String(e.message || e))}</span>`;
      }
      maybeShowRetry(true);
    } finally {
      autoProvisionState.inFlight = false;
    }
  };

  if (autoRetryBtn) {
    autoRetryBtn.addEventListener('click', async () => {
      autoProvisionState.attempted = false;
      await runAutoProvision(true);
    });
  }
  if (genesisGenerateBtn) {
    genesisGenerateBtn.addEventListener('click', async () => {
      autoProvisionState.attempted = false;
      await runAutoProvision(true, 'genesis');
    });
  }
  try {
    const genesisReady = await fetchGenesisStatus();
    maybeShowGenesisGenerate(!agentaddressConnected && genesisReady);
  } catch (_e) {
    maybeShowGenesisGenerate(false);
  }
  await runAutoProvision(false);

  // --- Key Storage toggle for vault info box (delegation, registered once) ---
  if (!content._haloVaultInfoHandler) {
    content._haloVaultInfoHandler = true;
    content.addEventListener('click', (e) => {
      const toggle = e.target.closest('#vault-info-toggle');
      if (!toggle) return;
      const detail = document.getElementById('vault-info-detail');
      if (!detail) return;
      const showing = detail.style.display !== 'none';
      detail.style.display = showing ? 'none' : 'block';
      toggle.innerHTML = showing
        ? '<span class="info-icon" style="font-size:13px">&#9432;</span> Key Storage'
        : '<span class="info-icon" style="font-size:13px">&#9432;</span> Hide';
    });
  }

  // --- Copy buttons for wallet credentials ---
  for (const btn of $$('.agentaddress-copy-btn')) {
    btn.addEventListener('click', () => {
      const targetId = btn.dataset.copyTarget;
      const el = document.getElementById(targetId);
      if (el && el.textContent) {
        navigator.clipboard.writeText(el.textContent).then(() => {
          const orig = btn.textContent;
          btn.textContent = 'Copied!';
          setTimeout(() => { btn.textContent = orig; }, 1500);
        }).catch(() => {});
      }
    });
  }


  // --- Identity handlers ---

  const profileSaveBtn = document.getElementById('profile-save-btn');
  const profileNameInput = document.getElementById('profile-name-input');
  if (profileSaveBtn && profileNameInput) {
    profileSaveBtn.addEventListener('click', async () => {
      const locked = profileNameInput.hasAttribute('readonly');
      if (locked) {
        profileNameInput.removeAttribute('readonly');
        profileNameInput.dataset.locked = 'false';
        profileNameInput.classList.remove('profile-name-locked');
        profileSaveBtn.dataset.renamePending = '1';
        profileSaveBtn.textContent = 'Save Name';
        profileNameInput.focus();
        profileNameInput.select();
        return;
      }
      const name = (profileNameInput.value || '').trim();
      if (!name) return;
      const rename = profileSaveBtn.dataset.renamePending === '1';
      profileSaveBtn.disabled = true;
      profileSaveBtn.textContent = 'Saving...';
      try {
        await apiPost('/profile', { display_name: name, avatar_type: 'initials', rename });
        profileNameInput.setAttribute('readonly', 'readonly');
        profileNameInput.dataset.locked = 'true';
        profileNameInput.classList.add('profile-name-locked');
        profileSaveBtn.dataset.renamePending = '0';
        profileSaveBtn.textContent = 'Rename Key';
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        alert('Save failed: ' + (e.message || e));
        profileSaveBtn.disabled = false;
        profileSaveBtn.textContent = 'Save Name';
      }
    });
  }

  const securityBadgeNode = document.getElementById('identity-security-badge');
  const securityTierButtons = Array.from(content.querySelectorAll('.security-tier-btn'));
  const tierStatusNode = document.getElementById('identity-tier-status');
  const tierDeviceEnable = document.getElementById('tier-device-enable');
  const tierDeviceComponents = document.getElementById('tier-device-components');
  const tierDeviceBrowser = document.getElementById('tier-device-browser');
  const shareLocalIpInput = document.getElementById('share-local-ip');
  const shareMacInput = document.getElementById('share-mac');
  const socialProviderChecks = Array.from(content.querySelectorAll('.social-provider-check[data-provider]'));
  const socialStatusNode = document.getElementById('social-provider-status');
  const socialConnectSelectedBtn = document.getElementById('social-connect-selected-btn');
  const socialRevokeSelectedBtn = document.getElementById('social-revoke-selected-btn');
  const socialExpiryInput = document.getElementById('social-expiry-days');
  const superPasskeyInput = document.getElementById('super-passkey-enabled');
  const superSecurityKeyInput = document.getElementById('super-security-key-enabled');
  const superTotpInput = document.getElementById('super-totp-enabled');
  const superTotpLabelInput = document.getElementById('super-totp-label');
  const superSecureStatusNode = document.getElementById('super-secure-status');
  let activeSecurityTier = initialSecurityTier;
  let applyingTierPreset = false;
  let cachedNetworkIdentity = null;
  let cachedSocialStatus = null;
  const setTierStatus = (message, tone = 'info') => {
    if (!tierStatusNode) return;
    tierStatusNode.textContent = message || '';
    tierStatusNode.classList.remove('is-ok', 'is-warn', 'is-error');
    if (tone === 'ok') tierStatusNode.classList.add('is-ok');
    else if (tone === 'warn') tierStatusNode.classList.add('is-warn');
    else if (tone === 'error') tierStatusNode.classList.add('is-error');
  };
  const applyTierCheckboxPreset = (tier) => {
    if (tierDeviceEnable) tierDeviceEnable.checked = true;
    if (tierDeviceComponents) tierDeviceComponents.checked = true;
    if (tierDeviceBrowser) tierDeviceBrowser.checked = tier === 'max-safe';
    if (shareLocalIpInput) shareLocalIpInput.checked = true;
    if (shareMacInput) shareMacInput.checked = tier === 'max-safe';
    if (!deferIdentityRoadmapTracks) {
      socialProviderChecks.forEach((cb) => {
        const provider = cb.dataset.provider || '';
        if (tier === 'max-safe') cb.checked = provider === 'google';
        else if (tier === 'less-safe') cb.checked = provider === 'google' || provider === 'github';
        else cb.checked = false;
      });
      if (superPasskeyInput) superPasskeyInput.checked = tier === 'max-safe';
      if (superSecurityKeyInput) superSecurityKeyInput.checked = tier === 'max-safe';
      if (superTotpInput) superTotpInput.checked = true;
    }
    const scannedComponentChecks = content.querySelectorAll('input[name="hw-comp"]');
    scannedComponentChecks.forEach((cb) => {
      if (cb.value === 'browser_fingerprint') cb.checked = tier === 'max-safe';
      else cb.checked = true;
    });
  };
  const ensureNetworkIdentityLoaded = async (forceRefresh = false) => {
    if (cachedNetworkIdentity && !forceRefresh) return cachedNetworkIdentity;
    const infoNode = document.getElementById('network-info');
    if (infoNode) infoNode.textContent = 'Detecting network info...';
    const resp = await api('/identity/network');
    cachedNetworkIdentity = resp || {};
    if (infoNode) {
      infoNode.innerHTML = `
        <div style="margin-bottom:6px">Local IP: <strong>${esc(resp.local_ip || 'not detected')}</strong></div>
        <div>MAC: <strong>${esc(resp.mac_address || 'not detected')}</strong></div>
      `;
      infoNode.dataset.loaded = '1';
    }
    return cachedNetworkIdentity;
  };
  const setTierButtonsBusy = (busy) => {
    securityTierButtons.forEach((btn) => { btn.disabled = busy; });
    if (socialConnectSelectedBtn) socialConnectSelectedBtn.disabled = busy;
    if (socialRevokeSelectedBtn) socialRevokeSelectedBtn.disabled = busy;
  };
  const setSocialStatus = (message, tone = 'ok') => {
    if (!socialStatusNode) return;
    socialStatusNode.textContent = String(message || '');
    socialStatusNode.style.color = tone === 'error'
      ? 'var(--red)'
      : (tone === 'warn' ? 'var(--yellow)' : 'var(--green)');
  };
  const refreshSocialStatus = async () => {
    if (deferIdentityRoadmapTracks) return;
    try {
      const resp = await api('/identity/social');
      cachedSocialStatus = resp;
      const providers = resp.providers || [];
      const summaries = [];
      socialProviderChecks.forEach((cb) => {
        const provider = cb.dataset.provider || '';
        const row = providers.find((p) => String(p.provider || '').toLowerCase() === provider);
        if (!row) return;
        cb.checked = !!row.selected;
        const state = row.active ? 'active' : row.expired ? 'expired' : 'inactive';
        summaries.push(`${provider}: ${state}`);
      });
      if (socialStatusNode) {
        const valid = resp.ledger && resp.ledger.chain_valid;
        const head = resp.ledger && resp.ledger.head_hash ? String(resp.ledger.head_hash).slice(0, 16) : 'none';
        socialStatusNode.innerHTML = `
          <div style="margin-bottom:6px">Chain: <strong style="color:${valid ? 'var(--green)' : 'var(--red)'}">${valid ? 'VALID' : 'INVALID'}</strong> | Head: <code>${esc(head)}</code></div>
          <div style="font-size:12px;color:var(--text-dim)">${esc(summaries.join(' | ') || 'No social providers configured')}</div>
        `;
      }
    } catch (e) {
      setSocialStatus(`Failed to load social status: ${String(e.message || e)}`, 'error');
    }
  };
  const startSocialOAuth = async (provider, expiresDays, fromTier = false, strict = false) => {
    try {
      const days = Number(expiresDays || 30);
      const resp = await api(`/identity/social/oauth/start/${encodeURIComponent(provider)}?expires_in_days=${Math.max(1, Math.min(365, days))}`);
      if (resp.oauth_bridge_supported && resp.oauth_url) {
        const popup = window.open('', '_blank', 'width=540,height=760');
        if (popup && !popup.closed) {
          try { popup.opener = null; } catch (_e) {}
          popup.location.href = resp.oauth_url;
          setSocialStatus(`${provider} OAuth opened in new tab.`, 'ok');
          if (fromTier) setTierStatus('Google OAuth flow opened automatically for max-safe mode.', 'ok');
          return true;
        }
        setSocialStatus(`Popup blocked. Redirecting this tab to ${provider} OAuth.`, 'warn');
        if (fromTier) setTierStatus('Popup blocked; redirecting this tab to OAuth.', 'warn');
        window.location.href = resp.oauth_url;
        return true;
      } else {
        const loginUrl = resp.manual_login_url || 'https://agenthalo.dev';
        const popup = window.open(loginUrl, '_blank', 'noopener,noreferrer');
        if (!popup) {
          throw new Error('popup blocked');
        }
        const token = window.prompt(`Paste your ${provider} OAuth token to connect:`);
        if (token && token.trim()) {
          await apiPost('/identity/social/connect', {
            provider,
            token: token.trim(),
            source: 'manual_popup',
            selected: true,
            expires_in_days: Math.max(1, Math.min(365, days)),
          });
          setSocialStatus(`${provider} connected.`, 'ok');
          await refreshSocialStatus();
          return true;
        }
        setSocialStatus(`${provider} login skipped.`, 'warn');
        if (fromTier) setTierStatus(`${provider} login skipped; preset continued without it.`, 'warn');
        return false;
      }
    } catch (e) {
      setSocialStatus(`Failed to start ${provider} login: ${String(e.message || e)}`, 'error');
      if (fromTier) setTierStatus(`${provider} login failed; preset continued without it.`, 'warn');
      if (strict) throw e;
      return false;
    }
  };
  if (window.__haloSocialOauthListener) {
    window.removeEventListener('message', window.__haloSocialOauthListener);
  }
  window.__haloSocialOauthListener = async (event) => {
    const data = event && event.data;
    if (!data || data.type !== 'agenthalo-social-oauth') return;
    if (data.status === 'ok') {
      setSocialStatus(data.message || 'OAuth login connected.', 'ok');
      await refreshSocialStatus();
      window._invalidateSetupState();
      await fetchSetupState(true);
      updateNavLockState();
    } else {
      setSocialStatus(data.message || 'OAuth login failed.', 'error');
    }
  };
  window.addEventListener('message', window.__haloSocialOauthListener);
  const refreshSuperSecureStatus = async () => {
    if (deferIdentityRoadmapTracks) return;
    try {
      const resp = await api('/identity/super-secure');
      if (superPasskeyInput) superPasskeyInput.checked = !!resp.passkey_enabled;
      if (superSecurityKeyInput) superSecurityKeyInput.checked = !!resp.security_key_enabled;
      if (superTotpInput) superTotpInput.checked = !!resp.totp_enabled;
      if (superTotpLabelInput) superTotpLabelInput.value = resp.totp_label || '';
      if (superSecureStatusNode) {
        superSecureStatusNode.innerHTML = `<span style="color:var(--text-dim)">Passkey: ${resp.passkey_enabled ? 'on' : 'off'} | Security Key: ${resp.security_key_enabled ? 'on' : 'off'} | TOTP: ${resp.totp_enabled ? 'on' : 'off'}</span>`;
      }
    } catch (e) {
      if (superSecureStatusNode) superSecureStatusNode.innerHTML = `<span style="color:var(--red)">Failed: ${esc(String(e.message || e))}</span>`;
    }
  };
  if (socialConnectSelectedBtn) {
    socialConnectSelectedBtn.addEventListener('click', async () => {
      const selected = socialProviderChecks.filter((cb) => cb.checked).map((cb) => cb.dataset.provider || '').filter(Boolean);
      if (!selected.length) {
        setSocialStatus('Select at least one provider.', 'warn');
        return;
      }
      const days = Number(socialExpiryInput?.value || 30);
      for (const provider of selected) {
        await startSocialOAuth(provider, days, false);
      }
      await refreshSocialStatus();
    });
  }
  if (socialRevokeSelectedBtn) {
    socialRevokeSelectedBtn.addEventListener('click', async () => {
      const selected = socialProviderChecks.filter((cb) => cb.checked).map((cb) => cb.dataset.provider || '').filter(Boolean);
      if (!selected.length) {
        setSocialStatus('Select providers to revoke.', 'warn');
        return;
      }
      for (const provider of selected) {
        try {
          await apiPost('/identity/social/revoke', { provider, reason: 'dashboard_revoke' });
        } catch (e) {
          setSocialStatus(`Failed revoke for ${provider}: ${String(e.message || e)}`, 'error');
        }
      }
      setSocialStatus('Selected social providers revoked.', 'ok');
      await refreshSocialStatus();
    });
  }
  content.querySelectorAll('.super-secure-save-btn[data-option]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const option = btn.dataset.option || '';
      let enabled = false;
      const metadata = {};
      if (option === 'passkey') enabled = !!superPasskeyInput?.checked;
      else if (option === 'security_key') enabled = !!superSecurityKeyInput?.checked;
      else if (option === 'totp') {
        enabled = !!superTotpInput?.checked;
        if (superTotpLabelInput?.value) metadata.label = superTotpLabelInput.value.trim();
      }
      try {
        await apiPost('/identity/super-secure', { option, enabled, metadata });
        if (superSecureStatusNode) superSecureStatusNode.innerHTML = `<span style="color:var(--green)">${esc(option)} updated.</span>`;
        await refreshSuperSecureStatus();
      } catch (e) {
        if (superSecureStatusNode) superSecureStatusNode.innerHTML = `<span style="color:var(--red)">Failed ${esc(option)}: ${esc(String(e.message || e))}</span>`;
      }
    });
  });
  const applyTierPreset = async (tier) => {
    if (applyingTierPreset) return;
    applyingTierPreset = true;
    setTierButtonsBusy(true);
    const stepFailures = [];
    const bestEffort = async (label, fn) => {
      try {
        return await fn();
      } catch (e) {
        stepFailures.push(`${label}: ${String(e && e.message || e)}`);
        return null;
      }
    };
    try {
      applyTierCheckboxPreset(tier);

      if (anonCheck && anonCheck.checked) {
        await bestEffort('anonymous_mode_disable', async () => {
          await apiPost('/identity/anonymous', { enabled: false });
          anonCheck.checked = false;
          if (anonShell) anonShell.classList.remove('is-active');
          if (anonLaunchBtn) {
            anonLaunchBtn.classList.remove('is-armed');
            anonLaunchBtn.textContent = 'Engage';
            anonLaunchBtn.setAttribute('aria-pressed', 'false');
          }
        });
      }

      const enableDevice = !!tierDeviceEnable?.checked;
      const includeComponents = !!tierDeviceComponents?.checked;
      const includeBrowser = !!tierDeviceBrowser?.checked;
      const shareLocalIp = !!shareLocalIpInput?.checked;
      const shareMac = !!shareMacInput?.checked;

      if (enableDevice) {
        await bestEffort('device_identity_save', async () => {
          const deviceMeta = await api('/identity/device');
          lastDeviceScan = deviceMeta;
          const selectedComponents = includeComponents
            ? (deviceMeta.components || []).map((c) => c.name).filter(Boolean)
            : [];
          let browserFp = null;
          if (includeBrowser) {
            const thumbmark = window.ThumbmarkJS;
            if (thumbmark && typeof thumbmark.getFingerprint === 'function') {
              try { browserFp = await thumbmark.getFingerprint(); } catch (_e) {}
            }
          }
          await apiPost('/identity/device', {
            browser_fingerprint: includeBrowser ? browserFp : null,
            selected_components: selectedComponents,
          });
        });
      }

      await bestEffort('network_identity_save', async () => {
        const networkMeta = await ensureNetworkIdentityLoaded(true);
        await apiPost('/identity/network', {
          share_local_ip: shareLocalIp,
          share_public_ip: false,
          share_mac: shareMac,
          local_ip: shareLocalIp ? (networkMeta.local_ip || null) : null,
          mac_addresses: shareMac && networkMeta.mac_address ? [networkMeta.mac_address] : [],
        });
      });

      if (!deferIdentityRoadmapTracks) {
        // Apply super-secure selections immediately to backend state.
        await bestEffort(
          'super_secure_passkey',
          async () => apiPost('/identity/super-secure', { option: 'passkey', enabled: !!superPasskeyInput?.checked, metadata: {} }),
        );
        await bestEffort(
          'super_secure_security_key',
          async () => apiPost('/identity/super-secure', { option: 'security_key', enabled: !!superSecurityKeyInput?.checked, metadata: {} }),
        );
        await bestEffort(
          'super_secure_totp',
          async () => apiPost('/identity/super-secure', { option: 'totp', enabled: !!superTotpInput?.checked, metadata: { label: superTotpLabelInput?.value || '' } }),
        );
      }

      await bestEffort('security_tier_persist', async () => {
        await apiPost('/identity/tier', {
          tier,
          applied_by: 'dashboard_setup',
          step_failures: stepFailures.length,
        });
      });

      if (tier === 'max-safe') {
        if (!deferIdentityRoadmapTracks) {
          const days = Number(socialExpiryInput?.value || 30);
          await bestEffort('social_google_oauth', async () => {
            const ok = await startSocialOAuth('google', days, true, true);
            if (!ok) {
              throw new Error('oauth not completed');
            }
          });
        }
        setTierStatus(
          stepFailures.length
            ? `Max-safe preset applied with ${stepFailures.length} skipped step(s).`
            : (deferIdentityRoadmapTracks
                ? 'Max-safe preset applied. Deferred identity tracks remain disabled.'
                : 'Max-safe preset applied. Google social login launched automatically.'),
          stepFailures.length ? 'warn' : 'ok',
        );
      } else {
        setTierStatus(
          stepFailures.length
            ? `Balanced preset applied with ${stepFailures.length} skipped step(s).`
            : 'Balanced preset applied with automatic identity setup.',
          stepFailures.length ? 'warn' : 'ok',
        );
      }
      // Immediately hide safety UI for responsive feedback
      const _tierShell = document.getElementById('safety-tier-shell');
      const _intentLabel = document.getElementById('safety-intent-label');
      const _anonShell = document.getElementById('anon-mode-shell');
      if (_tierShell) _tierShell.style.display = 'none';
      if (_intentLabel) _intentLabel.style.display = 'none';
      if (_anonShell) _anonShell.style.display = 'none';

      window._invalidateSetupState();
      await fetchSetupState(true);
      updateNavLockState();
      // Re-render to show configured state (verified cards, rescan button)
      await renderSetup();
    } catch (e) {
      setTierStatus(`Preset continued with skipped step(s): ${String(e.message || e)}`, 'warn');
    } finally {
      applyingTierPreset = false;
      setTierButtonsBusy(false);
    }
  };
  const setSecurityTier = (tier, persist = true) => {
    const nextSrc = securityTierImageByKey[tier];
    if (!nextSrc) return;
    applyTierCheckboxPreset(tier);
    securityTierButtons.forEach(btn => btn.classList.toggle('is-selected', btn.dataset.tier === tier));
    if (persist) {
      try { localStorage.setItem('halo_identity_security_tier', tier); } catch (_e) {}
    }
    if (!securityBadgeNode) {
      activeSecurityTier = tier;
      return;
    }
    if (activeSecurityTier === tier && securityBadgeNode.getAttribute('src') === nextSrc) return;
    activeSecurityTier = tier;
    securityBadgeNode.classList.add('is-swapping');
    window.setTimeout(() => {
      securityBadgeNode.onload = () => {
        securityBadgeNode.classList.remove('is-swapping');
        securityBadgeNode.onload = null;
      };
      securityBadgeNode.onerror = () => {
        securityBadgeNode.classList.remove('is-swapping');
        securityBadgeNode.onerror = null;
      };
      securityBadgeNode.setAttribute('src', nextSrc);
      window.setTimeout(() => securityBadgeNode.classList.remove('is-swapping'), 200);
    }, 45);
  };
  securityTierButtons.forEach(btn => {
    btn.addEventListener('click', async () => {
      const tier = btn.dataset.tier || '';
      setSecurityTier(tier, true);
      await applyTierPreset(tier);
    });
  });
  await refreshSocialStatus();
  await refreshSuperSecureStatus();
  setSecurityTier(initialSecurityTier, false);

  // Auto-apply "As Safe As Possible" on first visit if no tier has been set yet
  if (!hideSafetyUI && !appliedSecurityTier && initialSecurityTier === 'max-safe') {
    // Defer slightly to let the UI render before running the preset
    setTimeout(() => applyTierPreset('max-safe'), 250);
  }

  // --- Rescan Identity button handler ---
  const rescanBtn = document.getElementById('identity-rescan-btn');
  if (rescanBtn) {
    rescanBtn.addEventListener('click', async () => {
      rescanBtn.disabled = true;
      rescanBtn.textContent = 'Resetting...';
      try {
        // Toggle anonymous mode on then off to clear device/network identity
        await apiPost('/identity/anonymous', { enabled: true });
        await apiPost('/identity/anonymous', { enabled: false });
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        alert('Reset failed: ' + (e.message || e));
        rescanBtn.disabled = false;
        rescanBtn.textContent = 'Rescan Identity';
      }
    });
  }

  let lastDeviceScan = null;
  const deviceScanBtn = document.getElementById('device-scan-btn');
  if (deviceScanBtn) {
    deviceScanBtn.addEventListener('click', async () => {
      deviceScanBtn.disabled = true;
      deviceScanBtn.textContent = 'Scanning...';
      const statusNode = document.getElementById('device-scan-status');
      try {
        const resp = await api('/identity/device');
        lastDeviceScan = resp;
        const resultsNode = document.getElementById('device-scan-results');
        const listNode = document.getElementById('device-components-list');
        if (resultsNode && listNode) {
          let html = '';
          let totalEntropy = 0;
          (resp.components || []).forEach(c => {
            totalEntropy += Number(c.entropy_bits || 0);
            html += `
              <label class="hw-component" style="display:flex;align-items:center;gap:8px;padding:6px 0;font-size:13px">
                <input type="checkbox" name="hw-comp" value="${esc(c.name)}" checked>
                <span style="color:var(--text);min-width:120px">${esc(c.name)}</span>
                <span style="color:var(--text-dim);font-size:11px">${Number(c.entropy_bits || 0)} bits${c.stable ? '' : ' (unstable)'}</span>
              </label>
            `;
          });

          let browserFp = null;
          const thumbmark = window.ThumbmarkJS;
          if (thumbmark && typeof thumbmark.getFingerprint === 'function') {
            try {
              browserFp = await thumbmark.getFingerprint();
            } catch (_e) {}
          }

          if (browserFp) {
            html += `
              <label class="hw-component" style="display:flex;align-items:center;gap:8px;padding:6px 0;font-size:13px">
                <input type="checkbox" name="hw-comp" value="browser_fingerprint" checked data-browser-fp="${esc(browserFp)}">
                <span style="color:var(--text);min-width:120px">browser_fingerprint</span>
                <span style="color:var(--text-dim);font-size:11px">32 bits</span>
              </label>
            `;
            totalEntropy += 32;
          }

          listNode.innerHTML = html;
          const barNode = document.getElementById('device-entropy-bar');
          if (barNode) {
            const pct = Math.max(0, Math.min(100, (totalEntropy / 256) * 100));
            const color = pct > 60 ? 'var(--green)' : pct > 30 ? 'var(--yellow)' : 'var(--red)';
            barNode.innerHTML = `
              <div style="font-size:11px;color:var(--text-dim);margin-bottom:4px">Entropy: ${totalEntropy} bits</div>
              <div style="height:6px;background:var(--border);border-radius:3px;overflow:hidden">
                <div style="width:${pct}%;height:100%;background:${color};border-radius:3px;transition:width 0.3s"></div>
              </div>
            `;
          }
          resultsNode.style.display = 'block';
        }
        if (statusNode) statusNode.innerHTML = `<span style="color:var(--green)">Found ${(resp.components || []).length} components (tier: ${esc(resp.tier || 'unknown')})</span>`;
      } catch (e) {
        if (statusNode) statusNode.innerHTML = `<span style="color:var(--red)">Scan failed: ${esc(String(e.message || e))}</span>`;
      }
      deviceScanBtn.disabled = false;
      deviceScanBtn.textContent = (identityCfg.device_configured || !!lastDeviceScan) ? 'Rescan Device' : 'Scan Device';
    });
  }

  const deviceSaveBtn = document.getElementById('device-save-btn');
  if (deviceSaveBtn) {
    deviceSaveBtn.addEventListener('click', async () => {
      deviceSaveBtn.disabled = true;
      deviceSaveBtn.textContent = 'Saving...';
      const checked = content.querySelectorAll('input[name="hw-comp"]:checked');
      const selected = [];
      let browserFp = null;
      checked.forEach(cb => {
        if (cb.value === 'browser_fingerprint') {
          browserFp = cb.dataset.browserFp || null;
        } else {
          selected.push(cb.value);
        }
      });
      try {
        await apiPost('/identity/device', {
          browser_fingerprint: browserFp,
          selected_components: selected,
        });
        const statusNode = document.getElementById('device-scan-status');
        if (statusNode) statusNode.innerHTML = '<span style="color:var(--green)">&#10003; Device identity saved.</span>';
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        alert('Save failed: ' + (e.message || e));
      }
      deviceSaveBtn.disabled = false;
      deviceSaveBtn.textContent = 'Save Device Identity';
    });
  }

  const anonCheck = document.getElementById('anonymous-mode-check');
  const anonShell = document.getElementById('anon-mode-shell');
  const anonLaunchBtn = document.getElementById('anonymous-mode-launch-btn');
  if (anonCheck) {
    const syncAnonUi = () => {
      const enabled = !!anonCheck.checked;
      if (anonShell) anonShell.classList.toggle('is-active', enabled);
      if (anonLaunchBtn) {
        anonLaunchBtn.classList.toggle('is-armed', enabled);
        anonLaunchBtn.textContent = enabled ? 'Disengage' : 'Engage';
        anonLaunchBtn.setAttribute('aria-pressed', enabled ? 'true' : 'false');
      }
    };

    syncAnonUi();
    if (anonLaunchBtn) {
      anonLaunchBtn.addEventListener('click', () => {
        if (anonLaunchBtn.disabled) return;
        anonCheck.checked = !anonCheck.checked;
        syncAnonUi();
        anonCheck.dispatchEvent(new Event('change', { bubbles: true }));
      });
    }

    anonCheck.addEventListener('change', async () => {
      if (anonLaunchBtn) {
        anonLaunchBtn.disabled = true;
        anonLaunchBtn.classList.add('is-loading');
      }
      // Immediately hide safety buttons when engaging anonymous mode
      if (anonCheck.checked) {
        const tierShell = document.getElementById('safety-tier-shell');
        const intentLabel = document.getElementById('safety-intent-label');
        if (tierShell) tierShell.style.display = 'none';
        if (intentLabel) intentLabel.style.display = 'none';
      }
      try {
        await apiPost('/identity/anonymous', { enabled: anonCheck.checked });
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        alert('Failed: ' + (e.message || e));
        anonCheck.checked = !anonCheck.checked;
        syncAnonUi();
      } finally {
        if (anonLaunchBtn) {
          anonLaunchBtn.disabled = false;
          anonLaunchBtn.classList.remove('is-loading');
        }
      }
    });
  }

  const netDetails = document.getElementById('setup-network-details');
  if (netDetails) {
    netDetails.addEventListener('toggle', async () => {
      if (!netDetails.open) return;
      const infoNode = document.getElementById('network-info');
      if (!infoNode || infoNode.dataset.loaded) return;
      try {
        await ensureNetworkIdentityLoaded();
      } catch (e) {
        infoNode.innerHTML = `<span style="color:var(--red)">Failed to detect: ${esc(String(e.message || e))}</span>`;
      }
    });
  }
  const networkSaveBtn = document.getElementById('network-save-btn');
  if (networkSaveBtn) {
    networkSaveBtn.addEventListener('click', async () => {
      networkSaveBtn.disabled = true;
      networkSaveBtn.textContent = 'Saving...';
      let rerendered = false;
      const infoNode = document.getElementById('network-info');
      try {
        const resp = await ensureNetworkIdentityLoaded();
        const shareLocalIp = !!document.getElementById('share-local-ip')?.checked;
        const shareMac = !!document.getElementById('share-mac')?.checked;
        const macAddresses = shareMac && resp.mac_address ? [resp.mac_address] : [];
        await apiPost('/identity/network', {
          share_local_ip: shareLocalIp,
          share_public_ip: false,
          share_mac: shareMac,
          local_ip: shareLocalIp ? (resp.local_ip || null) : null,
          mac_addresses: macAddresses,
        });
        if (infoNode) infoNode.innerHTML += '<div style="margin-top:8px;color:var(--green);font-size:12px">&#10003; Network identity saved.</div>';
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
        rerendered = true;
      } catch (e) {
        if (infoNode) infoNode.innerHTML += `<div style="margin-top:8px;color:var(--red);font-size:12px">Failed to save: ${esc(String(e.message || e))}</div>`;
      } finally {
        if (!rerendered && networkSaveBtn.isConnected) {
          networkSaveBtn.disabled = false;
          networkSaveBtn.textContent = identityCfg.network_configured ? 'Update Network Identity' : 'Save Network Identity';
        }
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

  // OpenRouter OAuth PKCE connect flow
  const oauthConnectBtn = content.querySelector('#openrouter-oauth-connect-btn');
  const oauthStatusEl = content.querySelector('#openrouter-oauth-status');
  if (oauthConnectBtn) {
    oauthConnectBtn.addEventListener('click', async () => {
      oauthConnectBtn.disabled = true;
      oauthConnectBtn.textContent = 'Connecting...';
      if (oauthStatusEl) {
        oauthStatusEl.innerHTML = '<span style="color:var(--text-dim)">Opening OpenRouter...</span>';
      }

      try {
        const codeVerifier = generateCodeVerifier();
        const codeChallenge = await generateCodeChallenge(codeVerifier);
        sessionStorage.setItem('halo_or_code_verifier', codeVerifier);

        const callbackUrl = encodeURIComponent(`${window.location.origin}/api/openrouter/oauth/callback`);
        const authUrl = `https://openrouter.ai/auth?callback_url=${callbackUrl}&code_challenge=${codeChallenge}&code_challenge_method=S256`;
        const popup = window.open(authUrl, 'openrouter_oauth', 'width=600,height=700,scrollbars=yes');
        if (!popup) {
          throw new Error('Popup blocked. Please allow popups for this page.');
        }

        let settled = false;
        let pollClosed = null;
        const resetButton = () => {
          oauthConnectBtn.disabled = false;
          oauthConnectBtn.textContent = 'Connect with OpenRouter';
        };
        const clearVerifier = () => sessionStorage.removeItem('halo_or_code_verifier');

        const handler = async (event) => {
          if (event.origin !== window.location.origin) return;
          if (!event.data || event.data.type !== 'agenthalo-openrouter-oauth') return;
          settled = true;
          window.removeEventListener('message', handler);
          if (pollClosed) clearInterval(pollClosed);

          if (event.data.status === 'ok' && event.data.code) {
            if (oauthStatusEl) {
              oauthStatusEl.innerHTML = '<span style="color:var(--text-dim)">Exchanging token...</span>';
            }
            try {
              const verifier = sessionStorage.getItem('halo_or_code_verifier') || '';
              clearVerifier();
              const resp = await apiPost('/openrouter/oauth/exchange', {
                code: event.data.code,
                code_verifier: verifier,
              });
              if (resp && resp.ok) {
                if (oauthStatusEl) {
                  oauthStatusEl.innerHTML = '<span style="color:var(--green)">&#10003; Connected!</span>';
                }
                window._invalidateSetupState();
                await fetchSetupState(true);
                await renderSetup();
                updateNavLockState();
              } else {
                throw new Error(resp.error || 'OpenRouter exchange failed');
              }
            } catch (e) {
              if (oauthStatusEl) {
                oauthStatusEl.innerHTML = `<span style="color:var(--red)">Failed: ${esc(String(e.message || e))}</span>`;
              }
              resetButton();
            }
            return;
          }

          clearVerifier();
          if (oauthStatusEl) {
            oauthStatusEl.innerHTML = `<span style="color:var(--red)">${esc(event.data.message || 'Authorization failed')}</span>`;
          }
          resetButton();
        };
        window.addEventListener('message', handler);

        pollClosed = setInterval(() => {
          if (!popup || !popup.closed || settled) return;
          settled = true;
          window.removeEventListener('message', handler);
          clearInterval(pollClosed);
          clearVerifier();
          resetButton();
          if (oauthStatusEl) oauthStatusEl.innerHTML = '';
        }, 500);
      } catch (e) {
        sessionStorage.removeItem('halo_or_code_verifier');
        if (oauthStatusEl) {
          oauthStatusEl.innerHTML = `<span style="color:var(--red)">Failed: ${esc(String(e.message || e))}</span>`;
        }
        oauthConnectBtn.disabled = false;
        oauthConnectBtn.textContent = 'Connect with OpenRouter';
      }
    });
  }

  // "Use Local Models" button handler
  const chooseLocalBtn = content.querySelector('#setup-choose-local-models-btn');
  if (chooseLocalBtn) {
    chooseLocalBtn.addEventListener('click', async () => {
      const statusEl = content.querySelector('#setup-local-models-status');
      chooseLocalBtn.disabled = true;
      chooseLocalBtn.textContent = 'Setting up...';
      try {
        await apiPost('/models/choose-local', {});
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        if (statusEl) {
          statusEl.innerHTML = `<span style="color:var(--red)">Failed: ${esc(String(e.message || e))}</span>`;
        }
        chooseLocalBtn.disabled = false;
        chooseLocalBtn.textContent = 'Use Local Models';
      }
    });
  }

  // "Disconnect" local models button handler
  const disconnectLocalBtn = content.querySelector('.setup-disconnect-local-models-btn');
  if (disconnectLocalBtn) {
    disconnectLocalBtn.addEventListener('click', async () => {
      disconnectLocalBtn.disabled = true;
      disconnectLocalBtn.textContent = 'Disconnecting...';
      try {
        await apiPost('/models/unchoose-local', {});
        window._invalidateSetupState();
        await fetchSetupState(true);
        await renderSetup();
        updateNavLockState();
      } catch (e) {
        alert(`Failed to disconnect local models: ${String(e.message || e)}`);
        disconnectLocalBtn.disabled = false;
        disconnectLocalBtn.textContent = 'Disconnect';
      }
    });
  }

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
  window.__cockpitConfig = {
    meshPollMs: 10000,
    metricsPollMs: 5000,
  };
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
// PAGE: Orchestrator
// =============================================================================
function renderOrchestrator() {
  content.innerHTML = `
    <div class="page-header">
      <h1>Orchestrator</h1>
      <p class="subtitle">Launch, task, and monitor managed agent sessions</p>
    </div>
    <div id="orchestrator-root"></div>
  `;

  const root = document.getElementById('orchestrator-root');
  if (window.OrchestratorPage && typeof window.OrchestratorPage.render === 'function') {
    window.OrchestratorPage.render(root);
  } else {
    root.innerHTML = `
      <div class="card" style="padding:2rem;text-align:center;color:var(--amber);">
        <p style="font-size:1.5rem;">&#9881; Orchestrator unavailable</p>
        <p style="margin-top:1rem;color:var(--text-dim);">orchestrator.js failed to load.</p>
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

      <div class="card-grid" style="margin-bottom:14px">
        <div class="card">
          <div class="card-label">AETHER Vector Guard</div>
          <div class="card-value" style="font-size:14px">${Number(stats?.vector_aether?.governor_epsilon || 0).toFixed(2)}</div>
          <div class="card-sub">guarded=${Number(stats?.vector_aether?.guarded_vectors || 0)} | reclaimable=${Number(stats?.vector_aether?.reclaimable_vectors || 0)}</div>
          <div class="card-sub">basis: ${esc(stats?.vector_aether?.formal_basis || 'n/a')}</div>
        </div>
        <div class="card">
          <div class="card-label">AETHER Blob Guard</div>
          <div class="card-value" style="font-size:14px">${Number(stats?.blob_aether?.governor_epsilon || 0).toFixed(2)}</div>
          <div class="card-sub">guarded=${Number(stats?.blob_aether?.guarded_blobs || 0)} | reclaimable=${Number(stats?.blob_aether?.reclaimable_blobs || 0)}</div>
          <div class="card-sub">basis: ${esc(stats?.blob_aether?.formal_basis || 'n/a')}</div>
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
