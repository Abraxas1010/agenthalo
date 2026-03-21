// CodeGuard Dashboard — vanilla JavaScript, no framework.
// Uses global d3 (vendor/d3.v7.min.js) and global monaco (vendor/monaco/).
// Does NOT import from cockpit.js, observatory.js, or orchestration.js.

'use strict';

// -- State ------------------------------------------------------------------

const state = {
  manifest: null,
  manifestPath: null,
  graph: null,
  config: null,
  selectedNode: null,
  auditSessions: [],
  gateLog: [],
  ws: null,
  simulation: null,
  studioEditor: null,
  studioTemplate: null,
};

// -- Initialization ---------------------------------------------------------

document.addEventListener('DOMContentLoaded', initCodeGuardPage);

function initCodeGuardPage() {
  initTabs();
  initResizeHandle();
  loadConfig();
  loadRepositories();
  initWebSocket();
  renderConfigPanel();
}

// -- Tabs -------------------------------------------------------------------

function initTabs() {
  document.querySelectorAll('.codeguard-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.codeguard-tab').forEach(t => t.classList.remove('active'));
      document.querySelectorAll('.codeguard-tab-content').forEach(c => c.classList.add('hidden'));
      tab.classList.add('active');
      const target = document.getElementById('tab-' + tab.dataset.tab);
      if (target) target.classList.remove('hidden');
    });
  });
}

// -- Resize Handle ----------------------------------------------------------

function initResizeHandle() {
  const handle = document.getElementById('resize-handle');
  const dag = document.querySelector('.codeguard-dag');
  const panel = document.querySelector('.codeguard-panel');
  let dragging = false;

  handle.addEventListener('mousedown', (e) => {
    dragging = true;
    e.preventDefault();
  });

  document.addEventListener('mousemove', (e) => {
    if (!dragging) return;
    const container = document.querySelector('.codeguard-main');
    const rect = container.getBoundingClientRect();
    const pct = ((e.clientX - rect.left) / rect.width) * 100;
    const clamped = Math.max(30, Math.min(80, pct));
    dag.style.flex = `0 0 ${clamped}%`;
    panel.style.flex = `0 0 ${100 - clamped}%`;
  });

  document.addEventListener('mouseup', () => { dragging = false; });
}

// -- Repository Loading -----------------------------------------------------

async function loadRepositories() {
  const sel = document.getElementById('repo-selector');
  // Use the file API to discover connected repos via workspace profile
  try {
    const resp = await fetch('/api/codeguard/config');
    const data = await resp.json();
    if (data.ok) {
      sel.innerHTML = '<option value="default">Active Workspace</option>';
      selectRepository('default');
    }
  } catch (e) {
    sel.innerHTML = '<option value="">Error loading</option>';
  }

  sel.addEventListener('change', () => selectRepository(sel.value));
}

async function selectRepository(repoId) {
  // Load manifest
  try {
    const resp = await fetch('/api/codeguard/manifest');
    const data = await resp.json();
    if (data.ok) {
      state.manifest = data.manifest;
      state.manifestPath = data.path;
      updateManifestInfo();
    } else {
      state.manifest = null;
      state.manifestPath = null;
      updateManifestInfo();
    }
  } catch (e) {
    state.manifest = null;
  }

  // Load graph
  try {
    const resp = await fetch('/api/codeguard/graph');
    const data = await resp.json();
    if (data.ok && data.graph) {
      state.graph = data.graph;
      renderGraph(data.graph);
    } else {
      showDagEmpty();
    }
  } catch (e) {
    showDagEmpty();
  }

  // Update gate indicators
  updateGateIndicators();
}

function updateManifestInfo() {
  const el = document.getElementById('manifest-info');
  if (state.manifest) {
    const bindings = state.manifest.bindings || [];
    const ver = state.manifest.version || '?';
    el.textContent = `v${ver} | ${bindings.length} binding(s) | ${state.manifestPath || 'unknown path'}`;
  } else {
    el.textContent = 'No manifest loaded — click "Run Full Scan" to generate';
  }
}

function showDagEmpty() {
  document.getElementById('dag-empty').style.display = 'flex';
  const svg = document.querySelector('#dag-container svg');
  if (svg) svg.remove();
}

// -- DAG Viewer (D3 force-directed graph) -----------------------------------

function renderGraph(graphData) {
  const container = document.getElementById('dag-container');
  document.getElementById('dag-empty').style.display = 'none';

  // Remove existing SVG
  const existing = container.querySelector('svg');
  if (existing) existing.remove();

  const width = container.clientWidth;
  const height = container.clientHeight;

  const svg = d3.select(container)
    .append('svg')
    .attr('width', width)
    .attr('height', height);

  // Zoom
  const g = svg.append('g');
  svg.call(d3.zoom().scaleExtent([0.2, 5]).on('zoom', (event) => {
    g.attr('transform', event.transform);
  }));

  const nodes = graphData.nodes || [];
  const links = graphData.links || [];

  // Force simulation
  state.simulation = d3.forceSimulation(nodes)
    .force('link', d3.forceLink(links).id(d => d.id).distance(80))
    .force('charge', d3.forceManyBody().strength(-200))
    .force('center', d3.forceCenter(width / 2, height / 2))
    .force('collision', d3.forceCollide().radius(30));

  // Links
  const link = g.append('g')
    .selectAll('line')
    .data(links)
    .join('line')
    .attr('class', d => 'dag-link ' + (d.type || 'weak'));

  // Nodes
  const node = g.append('g')
    .selectAll('g')
    .data(nodes)
    .join('g')
    .attr('class', d => 'dag-node ' + (d.status || 'unlocked'))
    .call(d3.drag()
      .on('start', dragStarted)
      .on('drag', dragged)
      .on('end', dragEnded));

  node.append('circle')
    .attr('r', 10)
    .on('click', (event, d) => onNodeClick(d))
    .on('contextmenu', (event, d) => {
      event.preventDefault();
      showNodeContextMenu(event, d);
    })
    .on('mouseenter', (event, d) => showTooltip(event, d))
    .on('mouseleave', hideTooltip);

  node.append('text')
    .attr('dy', -16)
    .attr('text-anchor', 'middle')
    .text(d => {
      const path = d.codePath || d.id || '';
      const parts = path.split('/');
      return parts[parts.length - 1] || path;
    });

  state.simulation.on('tick', () => {
    link
      .attr('x1', d => d.source.x)
      .attr('y1', d => d.source.y)
      .attr('x2', d => d.target.x)
      .attr('y2', d => d.target.y);
    node.attr('transform', d => `translate(${d.x},${d.y})`);
  });

  function dragStarted(event, d) {
    if (!event.active) state.simulation.alphaTarget(0.3).restart();
    d.fx = d.x;
    d.fy = d.y;
  }
  function dragged(event, d) {
    d.fx = event.x;
    d.fy = event.y;
  }
  function dragEnded(event, d) {
    if (!event.active) state.simulation.alphaTarget(0);
    d.fx = null;
    d.fy = null;
  }
}

function showTooltip(event, d) {
  const tip = document.getElementById('tooltip');
  tip.style.display = 'block';
  tip.style.left = (event.pageX + 12) + 'px';
  tip.style.top = (event.pageY - 8) + 'px';
  tip.innerHTML = `<strong>${d.id || 'unnamed'}</strong><br>
    Path: ${d.codePath || 'N/A'}<br>
    Status: ${d.status || 'unknown'}<br>
    Hash verified: ${d.hashVerified ? 'yes' : 'no'}`;
}

function hideTooltip() {
  document.getElementById('tooltip').style.display = 'none';
}

function showNodeContextMenu(event, node) {
  // Simple context menu via prompt for v1
  const action = prompt(
    `Node: ${node.id}\nStatus: ${node.status}\n\nActions:\n1 = Lock\n2 = Unlock\n3 = Mark Review Required\n\nEnter number:`,
    ''
  );
  if (action === '1') toggleNodeState(node.id, 'locked');
  else if (action === '2') toggleNodeState(node.id, 'unlocked');
  else if (action === '3') toggleNodeState(node.id, 'reviewRequired');
}

// -- Node Detail Panel ------------------------------------------------------

function onNodeClick(node) {
  state.selectedNode = node;

  // Switch to detail tab
  document.querySelectorAll('.codeguard-tab').forEach(t => t.classList.remove('active'));
  document.querySelectorAll('.codeguard-tab-content').forEach(c => c.classList.add('hidden'));
  document.querySelector('[data-tab="detail"]').classList.add('active');
  document.getElementById('tab-detail').classList.remove('hidden');

  const panel = document.getElementById('tab-detail');
  panel.innerHTML = `
    <div class="node-detail">
      <h3>${node.id || 'Unnamed Node'}</h3>
      <div class="field">
        <div class="field-label">Code Path</div>
        <div class="field-value">${node.codePath || 'N/A'}</div>
      </div>
      <div class="field">
        <div class="field-label">Status</div>
        <div class="field-value">${node.status || 'unknown'}</div>
      </div>
      <div class="field">
        <div class="field-label">Locked</div>
        <div class="field-value">${node.locked ? 'Yes' : 'No'}</div>
      </div>
      <div class="field">
        <div class="field-label">Review Required</div>
        <div class="field-value">${node.reviewRequired ? 'Yes' : 'No'}</div>
      </div>
      <div class="field">
        <div class="field-label">Hash Verified</div>
        <div class="field-value">${node.hashVerified ? 'Yes' : 'No'}</div>
      </div>
      <div class="field">
        <div class="field-label">File Exists</div>
        <div class="field-value">${node.fileExists ? 'Yes' : 'No'}</div>
      </div>
      <div class="node-actions">
        <button class="codeguard-btn${node.locked ? ' danger' : ''}"
                onclick="toggleNodeState('${node.id}', '${node.locked ? 'unlocked' : 'locked'}')">
          ${node.locked ? 'Unlock' : 'Lock'}
        </button>
        <button class="codeguard-btn"
                onclick="toggleNodeState('${node.id}', 'reviewRequired')">
          Mark Review
        </button>
        <button class="codeguard-btn"
                onclick="openInCockpit('${node.codePath || ''}')">
          Open in Cockpit
        </button>
      </div>
      <div style="margin-top:12px;font-size:10px;color:var(--cg-text-dim)">
        Note: Locking a node does not automatically lock its dependents.
        Dependent modifications that change locked node semantics require manual review. (v1 limitation)
      </div>
    </div>
  `;
}

function toggleNodeState(nodeId, newState) {
  if (!state.manifest || !state.manifest.bindings) return;

  const binding = state.manifest.bindings.find(b => b.bindingId === nodeId);
  if (binding) {
    binding.locked = (newState === 'locked');
    binding.reviewRequired = (newState === 'reviewRequired');
  }

  // Re-render graph
  if (state.graph) {
    const node = state.graph.nodes.find(n => n.id === nodeId);
    if (node) {
      node.status = newState;
      node.locked = (newState === 'locked');
      node.reviewRequired = (newState === 'reviewRequired');
    }
    renderGraph(state.graph);
  }

  addGateLogEntry('node_state', `Node ${nodeId} set to ${newState}`);
}

function openInCockpit(codePath) {
  if (codePath) {
    window.open(`/#/cockpit?file=${encodeURIComponent(codePath)}`, '_blank');
  }
}

// -- Gate Indicators --------------------------------------------------------

async function updateGateIndicators() {
  if (!state.config) return;

  const gates = [
    { id: 'gate-1', enabled: state.config.gates.worktree_required, name: 'Worktree' },
    { id: 'gate-2', enabled: state.config.gates.schema_required, name: 'Schema' },
    { id: 'gate-3', enabled: state.config.gates.pre_push_human_approval || state.config.gates.pre_push_hostile_audit, name: 'Pre-Push' },
  ];

  for (const gate of gates) {
    const el = document.getElementById(gate.id);
    el.className = 'gate-indicator ' + (gate.enabled ? 'pass' : 'disabled');
    el.title = `${gate.name}: ${gate.enabled ? 'Enabled' : 'Disabled'}`;
    if (gate.id === 'gate-1' && gate.enabled) {
      el.title += ' (API-level enforcement only)';
    }
  }
}

// -- Config Panel -----------------------------------------------------------

async function loadConfig() {
  try {
    const resp = await fetch('/api/codeguard/config');
    const data = await resp.json();
    if (data.ok) {
      state.config = data.config;
      updateGateIndicators();
      renderConfigPanel();
    }
  } catch (e) {
    state.config = { gates: {}, audit: {} };
  }
}

function renderConfigPanel() {
  const el = document.getElementById('config-content');
  if (!el || !state.config) return;

  const g = state.config.gates || {};
  const a = state.config.audit || {};

  el.innerHTML = `
    <h3 style="color:var(--cg-accent);font-size:13px;margin-bottom:12px">Gate Configuration</h3>
    <div class="config-toggle">
      <label>Gate 1: Worktree Isolation</label>
      <input type="checkbox" id="cfg-worktree" ${g.worktree_required ? 'checked' : ''}
             onchange="updateConfig('worktree_required', this.checked)">
    </div>
    <div style="font-size:10px;color:var(--cg-text-dim);margin-bottom:8px;padding-left:4px">
      API-level enforcement only. Agents with shell access can bypass this gate.
    </div>
    <div class="config-toggle">
      <label>Gate 2: Schema Mandate</label>
      <input type="checkbox" id="cfg-schema" ${g.schema_required ? 'checked' : ''}
             onchange="updateConfig('schema_required', this.checked)">
    </div>
    <div class="config-toggle">
      <label>Gate 3: Pre-Push Human Approval</label>
      <input type="checkbox" id="cfg-human" ${g.pre_push_human_approval ? 'checked' : ''}
             onchange="updateConfig('pre_push_human_approval', this.checked)">
    </div>
    <div class="config-toggle">
      <label>Gate 3: Pre-Push Hostile Audit</label>
      <input type="checkbox" id="cfg-audit" ${g.pre_push_hostile_audit ? 'checked' : ''}
             onchange="updateConfig('pre_push_hostile_audit', this.checked)">
    </div>
    <h3 style="color:var(--cg-accent);font-size:13px;margin:16px 0 8px">Audit Configuration</h3>
    <div class="field" style="margin-bottom:8px">
      <label class="field-label">Audit Model (null = system default)</label>
      <input type="text" value="${a.model || ''}" placeholder="null"
             style="background:rgba(10,20,8,0.9);color:var(--cg-text);border:1px solid var(--cg-panel-border);
                    border-radius:4px;padding:4px 8px;font-family:var(--cg-font);font-size:11px;width:100%;margin-top:4px"
             onchange="updateAuditModel(this.value)">
    </div>
    <div class="config-toggle">
      <label>Prefer Different Model for Audit</label>
      <input type="checkbox" ${a.prefer_different_model ? 'checked' : ''}
             onchange="updateAuditConfig('prefer_different_model', this.checked)">
    </div>
    <div style="margin-top:16px">
      <button class="codeguard-btn primary" onclick="saveConfig()">Save Config</button>
    </div>
  `;
}

function updateConfig(key, value) {
  if (!state.config) return;
  if (!state.config.gates) state.config.gates = {};
  state.config.gates[key] = value;
  updateGateIndicators();
}

function updateAuditModel(value) {
  if (!state.config) return;
  if (!state.config.audit) state.config.audit = {};
  state.config.audit.model = value || null;
}

function updateAuditConfig(key, value) {
  if (!state.config) return;
  if (!state.config.audit) state.config.audit = {};
  state.config.audit[key] = value;
}

async function saveConfig() {
  try {
    const resp = await fetch('/api/codeguard/config', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ config: state.config }),
    });
    const data = await resp.json();
    if (data.ok) {
      addGateLogEntry('config', 'Configuration saved');
    } else {
      alert('Failed to save config: ' + (data.error || 'unknown'));
    }
  } catch (e) {
    alert('Failed to save config: ' + e.message);
  }
}

// -- Save / Scan / Verify / Export ------------------------------------------

async function saveManifest() {
  if (!state.manifest) {
    alert('No manifest to save. Run a scan first.');
    return;
  }
  try {
    const resp = await fetch('/api/codeguard/manifest', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ manifest: state.manifest }),
    });
    const data = await resp.json();
    if (data.ok) {
      state.manifestPath = data.path;
      updateManifestInfo();
      addGateLogEntry('manifest', 'Manifest saved to ' + data.path);
    } else {
      alert('Save failed: ' + (data.error || 'unknown'));
    }
  } catch (e) {
    alert('Save failed: ' + e.message);
  }
}

async function runFullScan() {
  try {
    const resp = await fetch('/api/codeguard/scan', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    const data = await resp.json();
    if (data.ok && data.proposed) {
      state.manifest = data.proposed;
      updateManifestInfo();
      addGateLogEntry('scan', `Scan complete: ${data.proposed.scan_summary?.files_scanned || 0} files`);
      // Reload graph
      selectRepository('default');
    } else {
      alert('Scan failed: ' + (data.error || 'unknown'));
    }
  } catch (e) {
    alert('Scan failed: ' + e.message);
  }
}

async function verifyBindings() {
  try {
    const resp = await fetch('/api/codeguard/verify', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    const data = await resp.json();
    if (data.ok) {
      const msg = data.verified
        ? 'All bindings verified: ' + data.output
        : 'Verification failed: ' + (data.error || data.output);
      addGateLogEntry('verify', msg);
      alert(msg);
    } else {
      alert('Verify error: ' + (data.error || 'unknown'));
    }
  } catch (e) {
    alert('Verify error: ' + e.message);
  }
}

function exportGraph() {
  if (!state.graph) {
    alert('No graph to export.');
    return;
  }
  const blob = new Blob([JSON.stringify(state.graph, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'codeguard_graph.json';
  a.click();
  URL.revokeObjectURL(url);
}

// -- Audit ------------------------------------------------------------------

async function spawnAudit() {
  try {
    const resp = await fetch('/api/codeguard/audit/spawn', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    const data = await resp.json();
    if (data.ok) {
      state.auditSessions.push({
        id: data.audit_session_id,
        status: 'in_progress',
      });
      addGateLogEntry('audit', 'Audit agent spawned: ' + data.audit_session_id);
      renderAuditFeed();
    } else {
      alert('Audit spawn failed: ' + (data.error || 'unknown'));
    }
  } catch (e) {
    alert('Audit spawn failed: ' + e.message);
  }
}

function renderAuditFeed() {
  const el = document.getElementById('tab-audit');
  if (state.auditSessions.length === 0) {
    el.innerHTML = `
      <div class="empty-state">
        <p>No active audit. Click "Spawn Audit" to start a hostile audit agent.</p>
        <button class="codeguard-btn primary" onclick="spawnAudit()">Spawn Audit</button>
      </div>`;
    return;
  }

  let html = '<button class="codeguard-btn primary" onclick="spawnAudit()" style="margin-bottom:12px">Spawn New Audit</button>';
  for (const session of state.auditSessions) {
    html += `
      <div class="audit-finding">
        <strong>Session:</strong> ${session.id}<br>
        <strong>Status:</strong> ${session.status}<br>
        ${session.status === 'in_progress' ? '<em>Audit in progress...</em>' : ''}
      </div>`;
  }
  el.innerHTML = html;
}

// -- Gate Log ---------------------------------------------------------------

function addGateLogEntry(gate, message) {
  const now = new Date().toLocaleTimeString();
  state.gateLog.unshift({ gate, message, timestamp: now });
  if (state.gateLog.length > 100) state.gateLog.length = 100;
  renderGateLog();
}

function renderGateLog() {
  const el = document.getElementById('gate-log-list');
  if (!el) return;
  el.innerHTML = state.gateLog.map(e => `
    <div class="gate-event">
      <span class="timestamp">${e.timestamp}</span>
      <span class="gate-name">[${e.gate}]</span>
      ${e.message}
    </div>
  `).join('');
}

// -- Document Studio --------------------------------------------------------

function initDocumentStudio() {
  const el = document.getElementById('studio-content');
  if (!el) return;

  // Detect repo type
  const templates = getTemplatesForRepo();

  el.innerHTML = `
    <h3 style="color:var(--cg-accent);font-size:13px;margin-bottom:8px">Templates</h3>
    <div class="studio-template-list">
      ${templates.map(t => `
        <div class="studio-template-card" onclick="selectTemplate('${t.id}')">
          <div class="studio-template-icon">${t.icon}</div>
          <div>${t.name}</div>
        </div>
      `).join('')}
    </div>
    <div class="studio-editor-container" id="studio-editor-el"></div>
    <div style="display:flex;gap:8px;margin-bottom:8px">
      <button class="codeguard-btn" onclick="previewStudioDoc()">Preview</button>
      <button class="codeguard-btn primary" onclick="exportStudioDoc()">Export</button>
    </div>
    <div class="studio-preview" id="studio-preview"></div>
  `;
}

function getTemplatesForRepo() {
  // Base templates available for all repos
  const templates = [
    { id: 'blueprint', name: 'Blueprint Graph', icon: '&#9670;', language: 'json',
      content: '{\n  "nodes": [],\n  "edges": []\n}' },
    { id: 'codeguard-node', name: 'CodeGuard Node', icon: '&#128274;', language: 'json',
      content: '{\n  "bindingId": "",\n  "codePath": "",\n  "witnessPath": "",\n  "artifactPath": "",\n  "locked": false,\n  "reviewRequired": false\n}' },
    { id: 'markdown', name: 'Freeform Markdown', icon: '&#9998;', language: 'markdown',
      content: '# Title\n\n## Section\n\nContent here.\n' },
    { id: 'lean-spec', name: 'Lean Specification', icon: '&#8466;', language: 'lean4',
      content: '/-- TODO: specify the theorem statement --/\ntheorem example (n : Nat) : n + 0 = n := by\n  -- Proof outline: use induction or omega\n  omega\n' },
    { id: 'rust-doc', name: 'Rust Module Doc', icon: '&#9881;', language: 'rust',
      content: '//! Module documentation.\n//!\n//! ## Overview\n//!\n//! Describe the module purpose.\n\n/// Primary function.\npub fn example() {\n    todo!()\n}\n' },
    { id: 'api-spec', name: 'API Endpoint Spec', icon: '&#9889;', language: 'markdown',
      content: '# Endpoint: `GET /api/example`\n\n## Description\n\nDescribe what this endpoint does.\n\n## Request\n\n| Parameter | Type | Required | Description |\n|-----------|------|----------|-------------|\n| id | string | yes | Resource ID |\n\n## Response\n\n```json\n{\n  "ok": true,\n  "data": {}\n}\n```\n' },
    { id: 'binding-receipt', name: 'Binding Receipt', icon: '&#128196;', language: 'json',
      content: '{\n  "bindingId": "",\n  "bindingVersion": 1,\n  "bindingType": "code-witness-artifact",\n  "codePath": "",\n  "witnessPath": "",\n  "artifactPath": "",\n  "codeHash": "",\n  "witnessHash": "",\n  "artifactHash": "",\n  "bindingHash": "",\n  "irHash": "",\n  "createdAt": ""\n}' },
  ];

  return templates;
}

function selectTemplate(templateId) {
  const templates = getTemplatesForRepo();
  const template = templates.find(t => t.id === templateId);
  if (!template) return;

  state.studioTemplate = template;

  // Highlight selected card
  document.querySelectorAll('.studio-template-card').forEach(c => c.classList.remove('selected'));
  document.querySelector(`[onclick="selectTemplate('${templateId}')"]`)?.classList.add('selected');

  // Create Monaco editor
  const container = document.getElementById('studio-editor-el');
  if (!container) return;

  if (state.studioEditor) {
    state.studioEditor.dispose();
    state.studioEditor = null;
  }

  const waitForMonaco = () => {
    if (window.__monacoReady && typeof monaco !== 'undefined') {
      state.studioEditor = monaco.editor.create(container, {
        value: template.content,
        language: template.language,
        theme: 'halo-terminal',
        minimap: { enabled: false },
        fontSize: 12,
        lineNumbers: 'on',
        scrollBeyondLastLine: false,
        automaticLayout: true,
      });
    } else {
      setTimeout(waitForMonaco, 100);
    }
  };
  waitForMonaco();
}

function previewStudioDoc() {
  if (!state.studioEditor) return;
  const content = state.studioEditor.getValue();
  const preview = document.getElementById('studio-preview');
  if (!preview) return;

  if (state.studioTemplate?.language === 'markdown') {
    preview.innerHTML = renderMarkdown(content);
  } else if (state.studioTemplate?.language === 'json') {
    try {
      const parsed = JSON.parse(content);
      preview.innerHTML = '<pre>' + escapeHtml(JSON.stringify(parsed, null, 2)) + '</pre>';
    } catch (e) {
      preview.innerHTML = '<span style="color:var(--cg-red)">Invalid JSON: ' + escapeHtml(e.message) + '</span>';
    }
  } else {
    preview.innerHTML = '<pre>' + escapeHtml(content) + '</pre>';
  }
}

function exportStudioDoc() {
  if (!state.studioEditor) {
    alert('Select a template first.');
    return;
  }
  const content = state.studioEditor.getValue();

  // H8: Do NOT execute arbitrary code. Only write the file.
  const filename = prompt('Enter file path (relative to repo root):', '');
  if (!filename) return;

  const blob = new Blob([content], { type: 'text/plain' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename.split('/').pop();
  a.click();
  URL.revokeObjectURL(url);

  addGateLogEntry('studio', 'Document exported: ' + filename);
}

// Simple Markdown renderer (no external dependency — H8 compliant)
function renderMarkdown(md) {
  let html = escapeHtml(md);
  // Headers
  html = html.replace(/^### (.+)$/gm, '<h3>$1</h3>');
  html = html.replace(/^## (.+)$/gm, '<h2>$1</h2>');
  html = html.replace(/^# (.+)$/gm, '<h1>$1</h1>');
  // Bold / italic
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');
  // Code blocks
  html = html.replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code>$2</code></pre>');
  // Inline code
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');
  // Lists
  html = html.replace(/^- (.+)$/gm, '<li>$1</li>');
  html = html.replace(/(<li>.*<\/li>\n?)+/g, '<ul>$&</ul>');
  // Line breaks
  html = html.replace(/\n\n/g, '<br><br>');
  return html;
}

function escapeHtml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// -- WebSocket (codeguard:* prefix per H7) ----------------------------------

function initWebSocket() {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${proto}//${location.host}/api/system/stream`;

  try {
    state.ws = new WebSocket(wsUrl);
    state.ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.type && msg.type.startsWith('codeguard:')) {
          handleWSMessage(msg.type, msg.payload || msg);
        }
      } catch (e) { /* ignore non-JSON */ }
    };
    state.ws.onclose = () => {
      // Reconnect after delay
      setTimeout(initWebSocket, 5000);
    };
  } catch (e) {
    // WebSocket not available — page still works via polling
  }
}

function handleWSMessage(type, payload) {
  switch (type) {
    case 'codeguard:graph_update':
      selectRepository('default'); // Reload
      break;
    case 'codeguard:gate_event':
      addGateLogEntry(payload.gate || 'gate', payload.detail || 'Gate event');
      break;
    case 'codeguard:audit_finding':
      addGateLogEntry('audit', `Finding: [${payload.finding?.severity}] ${payload.finding?.description}`);
      break;
    case 'codeguard:audit_complete':
      addGateLogEntry('audit', `Audit complete: ${payload.findings_count} finding(s), max severity: ${payload.max_severity}`);
      break;
    case 'codeguard:manifest_proposed':
      addGateLogEntry('manifest', 'Manifest proposed by agent ' + (payload.agent_id || 'unknown'));
      break;
  }
}

// -- Init Document Studio on tab click --------------------------------------

document.addEventListener('DOMContentLoaded', () => {
  const studioTab = document.querySelector('[data-tab="studio"]');
  if (studioTab) {
    studioTab.addEventListener('click', () => {
      if (!document.getElementById('studio-editor-el')) {
        initDocumentStudio();
      }
    });
  }
});
