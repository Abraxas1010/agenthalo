'use strict';

/* ================================================================
   MCP Tools Page — Provider Tree + Usage Analytics Dashboard
   ================================================================ */

const __mcpState = {
  query: '',
  selectedTool: null,
  invokeJson: '{}',
  formValues: {},
  lastResult: null,
  error: '',
  treeData: null,
  usageData: null,
  usageExpanded: true,
  usageAgent: null,
  callTimelineVisible: false,
};

function __mcpEsc(value) {
  if (window.__escapeHtml) return window.__escapeHtml(value);
  if (value == null) return '';
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function __mcpJson(value) {
  return __mcpEsc(JSON.stringify(value == null ? {} : value, null, 2));
}

function __mcpSchemaDefaults(schema) {
  if (!schema || typeof schema !== 'object') return {};
  if (schema.type === 'object' && schema.properties && typeof schema.properties === 'object') {
    return Object.fromEntries(
      Object.entries(schema.properties).map(([key, def]) => {
        if (def && Object.prototype.hasOwnProperty.call(def, 'default'))
          return [key, def.default];
        if (def && def.type === 'boolean') return [key, false];
        return [key, ''];
      }),
    );
  }
  return {};
}

function __mcpToolParams(tool) {
  if (!tool || !tool.input_schema || tool.input_schema.type !== 'object') return null;
  return tool.input_schema.properties || null;
}

function __mcpBuildInvokePayload(tool) {
  const props = __mcpToolParams(tool);
  if (!props) {
    return JSON.parse(String(__mcpState.invokeJson || '{}') || '{}');
  }
  const payload = {};
  Object.entries(props).forEach(([key, schema]) => {
    const raw = __mcpState.formValues[key];
    if (raw === '' || raw == null) return;
    if (schema.type === 'integer') payload[key] = Number.parseInt(raw, 10);
    else if (schema.type === 'number') payload[key] = Number(raw);
    else if (schema.type === 'boolean') payload[key] = !!raw;
    else payload[key] = raw;
  });
  return payload;
}

function __mcpRenderField(tool, key, schema) {
  const value = __mcpState.formValues[key] ?? '';
  const label = schema.title || key;
  const hint = schema.description || '';
  if (schema.type === 'boolean') {
    return `<label class="mcp-form-field">
      <span class="mcp-form-label">${__mcpEsc(label)}</span>
      <span class="mcp-form-check">
        <input type="checkbox" data-mcp-input="${__mcpEsc(key)}" ${value ? 'checked' : ''}>
        <span>${__mcpEsc(hint || 'Toggle')}</span>
      </span>
    </label>`;
  }
  const type = schema.type === 'integer' || schema.type === 'number' ? 'number' : 'text';
  return `<label class="mcp-form-field">
    <span class="mcp-form-label">${__mcpEsc(label)}</span>
    <input class="input" type="${type}" data-mcp-input="${__mcpEsc(key)}"
      value="${__mcpEsc(value)}"
      placeholder="${__mcpEsc(schema.examples?.[0] || hint || key)}">
    ${hint ? `<span class="mcp-form-help">${__mcpEsc(hint)}</span>` : ''}
  </label>`;
}

function __mcpFmtTime(unix) {
  if (!unix) return 'Never';
  return new Date(unix * 1000).toLocaleString();
}

function __mcpFmtTimeShort(unix) {
  if (!unix) return '--';
  const d = new Date(unix * 1000);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const ss = String(d.getSeconds()).padStart(2, '0');
  const mon = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${mon}-${day} ${hh}:${mm}:${ss}`;
}

// ---- Tree builder ----
function __mcpBuildTree(haloTools, haloCategories, pmtTools) {
  const tree = [];

  // AgentHALO provider
  const haloCats = {};
  for (const cat of haloCategories) {
    haloCats[cat.category] = { name: cat.category, domain: cat.domain, tools: [] };
  }
  for (const tool of haloTools) {
    const catKey = tool.category || 'uncategorized';
    if (!haloCats[catKey]) haloCats[catKey] = { name: catKey, domain: catKey, tools: [] };
    haloCats[catKey].tools.push(tool);
  }
  const haloCatList = Object.values(haloCats).filter(c => c.tools.length > 0);
  haloCatList.sort((a, b) => a.name.localeCompare(b.name));
  tree.push({ provider: 'AgentHALO', count: haloTools.length, categories: haloCatList });

  // AgentPMT provider
  if (pmtTools.length > 0) {
    const pmtCats = {};
    for (const tool of pmtTools) {
      const catKey = tool.category || 'uncategorized';
      if (!pmtCats[catKey]) pmtCats[catKey] = { name: catKey, domain: catKey, tools: [] };
      pmtCats[catKey].tools.push(tool);
    }
    const pmtCatList = Object.values(pmtCats).filter(c => c.tools.length > 0);
    pmtCatList.sort((a, b) => a.name.localeCompare(b.name));
    tree.push({ provider: 'AgentPMT', count: pmtTools.length, categories: pmtCatList });
  }
  return tree;
}

function __mcpMatchesQuery(tool, query) {
  const needle = String(query || '').trim().toLowerCase();
  if (!needle) return true;
  return [tool.name, tool.description, tool.category, tool.domain]
    .filter(Boolean).join(' ').toLowerCase().includes(needle);
}

function __mcpRenderTree(tree, query) {
  return tree.map(provider => {
    const cats = provider.categories.map(cat => {
      const tools = cat.tools.filter(t => __mcpMatchesQuery(t, query));
      if (tools.length === 0) return '';
      const toolItems = tools.map(t => {
        const isSelected = __mcpState.selectedTool && __mcpState.selectedTool.name === t.name;
        return `<button class="mcp-tree-tool${isSelected ? ' is-active' : ''}" data-mcp-select="${__mcpEsc(t.name)}">${__mcpEsc(t.name.replace(/^agentpmt\//, ''))}</button>`;
      }).join('');
      return `<details class="mcp-tree-cat" open>
        <summary><span>${__mcpEsc(cat.domain || cat.name)}</span><strong>${tools.length}</strong></summary>
        <div class="mcp-tree-tools">${toolItems}</div>
      </details>`;
    }).filter(Boolean).join('');
    if (!cats) return '';
    const matchCount = provider.categories.reduce((sum, c) =>
      sum + c.tools.filter(t => __mcpMatchesQuery(t, query)).length, 0);
    return `<details class="mcp-tree-provider" open>
      <summary><span>${__mcpEsc(provider.provider)}</span><strong>${matchCount}</strong></summary>
      ${cats}
    </details>`;
  }).filter(Boolean).join('');
}

// ---- Usage analytics ----
function __mcpRenderUsage(data) {
  if (!data || !data.ok) {
    return `<div class="card mcp-usage-section"><div class="card-label">Usage Analytics</div>
      <div class="card-sub" style="color:var(--text-dim)">No usage data yet. Launch agents to generate trace data.</div></div>`;
  }
  const agents = Array.isArray(data.agents) ? data.agents : [];
  const topTools = Array.isArray(data.top_tools) ? data.top_tools : [];
  const recentCalls = Array.isArray(data.recent_calls) ? data.recent_calls : [];
  const totalCalls = Number(data.total_tool_calls || 0) + Number(data.total_mcp_calls || 0);
  const successCount = recentCalls.filter(c => c.success).length;
  const successRate = recentCalls.length > 0 ? Math.round((successCount / recentCalls.length) * 100) : 100;

  const summaryHtml = `<div class="mcp-usage-summary">
    <div class="mcp-usage-stat"><span class="mcp-usage-stat-value">${totalCalls}</span><span class="mcp-usage-stat-label">Total Calls</span></div>
    <div class="mcp-usage-stat"><span class="mcp-usage-stat-value">${Number(data.total_sessions || 0)}</span><span class="mcp-usage-stat-label">Sessions</span></div>
    <div class="mcp-usage-stat"><span class="mcp-usage-stat-value">${successRate}%</span><span class="mcp-usage-stat-label">Success Rate</span></div>
    <div class="mcp-usage-stat"><span class="mcp-usage-stat-value">${agents.length}</span><span class="mcp-usage-stat-label">Active Agents</span></div>
  </div>`;

  if (!__mcpState.usageExpanded) {
    return `<div class="card mcp-usage-section">
      <div class="mcp-usage-header" id="mcp-usage-toggle">
        <div class="card-label">Usage Analytics</div>
        <button class="btn btn-sm">Expand</button>
      </div>${summaryHtml}</div>`;
  }

  const agentCardsHtml = agents.map(a => {
    const calls = Number(a.tool_calls || 0) + Number(a.mcp_tool_calls || 0);
    const cost = Number(a.estimated_cost_usd || 0);
    const isSelected = __mcpState.usageAgent === a.agent_id;
    const pct = totalCalls > 0 ? Math.round((calls / totalCalls) * 100) : 0;
    return `<div class="mcp-usage-agent-card${isSelected ? ' is-selected' : ''}" data-usage-agent="${__mcpEsc(a.agent_id)}">
      <div class="mcp-usage-agent-name">${__mcpEsc(a.agent_id)}</div>
      <div class="mcp-usage-agent-row"><span>${calls} calls</span><span>$${cost.toFixed(2)}</span></div>
      <div class="mcp-usage-agent-bar"><div class="mcp-usage-agent-bar-fill" style="width:${pct}%"></div></div>
      <div class="mcp-usage-agent-meta">Last: ${__mcpFmtTime(a.last_seen)}</div>
    </div>`;
  }).join('');

  // Agent detail panel
  let detailHtml = '';
  if (__mcpState.usageAgent) {
    const ag = agents.find(a => a.agent_id === __mcpState.usageAgent);
    if (ag) {
      const tools = Array.isArray(ag.top_tools) ? ag.top_tools : [];
      detailHtml = `<div class="mcp-usage-detail">
        <div class="mcp-usage-detail-header"><strong>${__mcpEsc(ag.agent_id)}</strong><button class="btn btn-sm" data-usage-close>Close</button></div>
        <div class="mcp-usage-detail-stats">
          <div><span>Sessions</span><strong>${Number(ag.sessions || 0)}</strong></div>
          <div><span>Tool Calls</span><strong>${Number(ag.tool_calls || 0)}</strong></div>
          <div><span>MCP Calls</span><strong>${Number(ag.mcp_tool_calls || 0)}</strong></div>
          <div><span>Input Tokens</span><strong>${Number(ag.total_input_tokens || 0).toLocaleString()}</strong></div>
          <div><span>Output Tokens</span><strong>${Number(ag.total_output_tokens || 0).toLocaleString()}</strong></div>
          <div><span>Est. Cost</span><strong>$${Number(ag.estimated_cost_usd || 0).toFixed(2)}</strong></div>
        </div>
        ${tools.length ? `<div class="card-label" style="margin-top:12px;font-size:11px">Top Tools</div>
          <div class="mcp-usage-tool-list">${tools.map(t =>
            `<div class="mcp-usage-tool-row"><span class="mcp-usage-tool-name">${__mcpEsc(t.name)}</span><span class="mcp-usage-tool-count">${Number(t.count)}</span></div>`
          ).join('')}</div>` : ''}
      </div>`;
    }
  }

  // Global top tools
  const globalToolsHtml = topTools.length ? `<div class="mcp-usage-global-tools">
    <div class="card-label" style="font-size:11px">Most Used Tools</div>
    <div class="mcp-usage-tool-list">${topTools.slice(0, 15).map(t =>
      `<div class="mcp-usage-tool-row"><span class="mcp-usage-tool-name">${__mcpEsc(t.name)}</span><span class="mcp-usage-tool-count">${Number(t.count)}</span></div>`
    ).join('')}</div>
  </div>` : '';

  // Call timeline
  const timelineToggle = recentCalls.length > 0 ? `<div style="margin-top:12px">
    <button class="btn btn-sm" id="mcp-timeline-toggle">${__mcpState.callTimelineVisible ? 'Hide' : 'Show'} Call Timeline (${recentCalls.length})</button>
  </div>` : '';
  const timelineHtml = __mcpState.callTimelineVisible && recentCalls.length > 0 ? `
    <div class="mcp-timeline">
      <table class="mcp-timeline-table">
        <thead><tr><th>Time</th><th>Agent</th><th>Tool</th><th>Status</th></tr></thead>
        <tbody>${recentCalls.slice(0, 100).map(c => `<tr>
          <td>${__mcpFmtTimeShort(c.timestamp)}</td>
          <td>${__mcpEsc(c.agent)}</td>
          <td class="mcp-timeline-tool">${__mcpEsc(c.tool)}</td>
          <td><span class="badge ${c.success ? 'badge-ok' : 'badge-err'}">${c.success ? 'OK' : 'FAIL'}</span></td>
        </tr>`).join('')}</tbody>
      </table>
    </div>` : '';

  return `<div class="card mcp-usage-section">
    <div class="mcp-usage-header" id="mcp-usage-toggle">
      <div class="card-label">Usage Analytics</div>
      <button class="btn btn-sm">Collapse</button>
    </div>
    ${summaryHtml}
    <div class="mcp-usage-body">
      <div class="mcp-usage-agents">${agentCardsHtml || '<div class="card-sub">No agent data.</div>'}</div>
      <div class="mcp-usage-right">${detailHtml || globalToolsHtml || '<div class="card-sub">Click an agent for details.</div>'}</div>
    </div>
    ${timelineToggle}${timelineHtml}
  </div>`;
}

// ---- Detail panel ----
function __mcpRenderDetail(tool) {
  if (!tool) {
    return `<div class="card"><div class="card-label">No tool selected</div><div class="card-sub">Choose a tool from the tree.</div></div>`;
  }
  const params = __mcpToolParams(tool);
  const formHtml = params
    ? `<div class="mcp-form-grid">${Object.entries(params).map(([key, schema]) => __mcpRenderField(tool, key, schema || {})).join('')}</div>`
    : `<textarea id="mcp-invoke-json" class="input mcp-invoke-json" spellcheck="false">${__mcpEsc(__mcpState.invokeJson || '{}')}</textarea>`;

  return `<div class="card mcp-invoke-shell">
    <div class="card-label" style="font-family:var(--mono)">${__mcpEsc(tool.name)}</div>
    <div class="card-sub">${__mcpEsc(tool.description || tool.title || 'No description')}</div>
    <div class="mcp-meta-grid">
      <div><span>Category</span><strong>${__mcpEsc(tool.category || 'uncategorized')}</strong></div>
      <div><span>Domain</span><strong>${__mcpEsc(tool.domain || 'n/a')}</strong></div>
    </div>
    ${formHtml}
    <div class="network-form-actions">
      <button class="btn btn-primary" id="mcp-invoke-btn">Execute</button>
      <button class="btn" id="mcp-reset-btn">Reset</button>
    </div>
    <pre class="mcp-json mcp-result">${__mcpJson(__mcpState.lastResult || { status: 'idle' })}</pre>
    ${__mcpState.error ? `<div class="networking-msg err">${__mcpEsc(__mcpState.error)}</div>` : ''}
  </div>`;
}

// ---- Main render ----
window.renderMcpToolsPage = async function renderMcpToolsPage() {
  const content = document.querySelector('#content');
  const api = window.api;
  const apiPost = window.apiPost;
  if (!content || typeof api !== 'function' || typeof apiPost !== 'function') return;

  content.innerHTML = '<div class="loading">Loading MCP tool catalog...</div>';

  try {
    // Load all data in parallel
    const [catalogRes, categoriesRes, pmtRes, usageRes] = await Promise.all([
      api('/mcp/tools?limit=500&offset=0'),
      api('/mcp/categories'),
      api('/agentpmt/tools').catch(() => ({ tools: [] })),
      api('/mcp/usage-stats?include_calls=true&limit=200').catch(() => null),
    ]);

    const haloTools = Array.isArray(catalogRes?.tools) ? catalogRes.tools : [];
    const categories = Array.isArray(categoriesRes?.categories) ? categoriesRes.categories : [];
    const pmtTools = Array.isArray(pmtRes?.tools) ? pmtRes.tools : [];
    __mcpState.treeData = __mcpBuildTree(haloTools, categories, pmtTools);
    __mcpState.usageData = usageRes;

    // Build a flat lookup for tool selection
    const allTools = [...haloTools, ...pmtTools];
    if (!__mcpState.selectedTool && allTools.length > 0) {
      __mcpState.selectedTool = allTools[0];
      __mcpState.formValues = __mcpSchemaDefaults(allTools[0].input_schema);
    }

    __mcpRender(allTools);
  } catch (err) {
    content.innerHTML = `<div class="card"><div class="card-label">MCP Catalog Unavailable</div>
      <div class="card-sub">${__mcpEsc(String((err && err.message) || err || 'unknown error'))}</div></div>`;
  }
};

function __mcpRender(allTools) {
  const content = document.querySelector('#content');
  if (!content) return;

  const treeHtml = __mcpRenderTree(__mcpState.treeData || [], __mcpState.query);
  const usageHtml = __mcpRenderUsage(__mcpState.usageData);
  const detailHtml = __mcpRenderDetail(__mcpState.selectedTool);
  const totalCount = (allTools || []).length;

  content.innerHTML = `<div class="mcp-shell">
    <aside class="card mcp-sidebar">
      <div class="card-label">Tool Registry</div>
      <div class="card-sub">${totalCount} tools across ${(__mcpState.treeData || []).length} providers</div>
      <input id="mcp-search" class="input" style="margin:8px 0" placeholder="Filter tools..." value="${__mcpEsc(__mcpState.query)}">
      <div class="mcp-tree">${treeHtml || '<div class="card-sub">No matching tools.</div>'}</div>
    </aside>
    <section class="mcp-main">
      <section class="card">
        <div class="mcp-header">
          <div><div class="page-title">MCP Tools</div><div class="muted">Provider tree, tool invocation, and usage analytics.</div></div>
          <div class="mcp-summary-card"><span>Total</span><strong>${totalCount}</strong></div>
        </div>
      </section>
      ${usageHtml}
      <section class="mcp-layout">
        <div class="mcp-detail-panel">${detailHtml}</div>
      </section>
    </section>
  </div>`;

  // Bind events
  __mcpBindEvents(allTools);
}

function __mcpBindEvents(allTools) {
  const api = window.api;
  const apiPost = window.apiPost;

  // Search
  const searchInput = document.querySelector('#mcp-search');
  if (searchInput) {
    searchInput.addEventListener('input', () => {
      __mcpState.query = searchInput.value || '';
      __mcpRender(allTools);
    });
  }

  // Tree tool selection
  document.querySelectorAll('[data-mcp-select]').forEach(btn => {
    btn.addEventListener('click', () => {
      const name = btn.dataset.mcpSelect;
      const tool = allTools.find(t => t.name === name);
      if (tool) {
        __mcpState.selectedTool = tool;
        __mcpState.formValues = __mcpSchemaDefaults(tool.input_schema);
        __mcpState.invokeJson = JSON.stringify(__mcpState.formValues, null, 2);
        __mcpState.lastResult = null;
        __mcpState.error = '';
        __mcpRender(allTools);
      }
    });
  });

  // Usage toggle
  const usageToggle = document.querySelector('#mcp-usage-toggle');
  if (usageToggle) {
    usageToggle.addEventListener('click', () => {
      __mcpState.usageExpanded = !__mcpState.usageExpanded;
      __mcpRender(allTools);
    });
  }

  // Usage agent selection
  document.querySelectorAll('[data-usage-agent]').forEach(card => {
    card.addEventListener('click', () => {
      __mcpState.usageAgent = card.dataset.usageAgent || null;
      __mcpRender(allTools);
    });
  });
  document.querySelectorAll('[data-usage-close]').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      __mcpState.usageAgent = null;
      __mcpRender(allTools);
    });
  });

  // Timeline toggle
  const timelineToggle = document.querySelector('#mcp-timeline-toggle');
  if (timelineToggle) {
    timelineToggle.addEventListener('click', () => {
      __mcpState.callTimelineVisible = !__mcpState.callTimelineVisible;
      __mcpRender(allTools);
    });
  }

  // Invoke
  document.querySelector('#mcp-invoke-btn')?.addEventListener('click', async () => {
    const tool = __mcpState.selectedTool;
    if (!tool) return;
    const rawInput = document.querySelector('#mcp-invoke-json');
    if (rawInput) __mcpState.invokeJson = String(rawInput.value || '{}');
    __mcpState.error = '';
    try {
      const params = rawInput
        ? JSON.parse(__mcpState.invokeJson || '{}')
        : __mcpBuildInvokePayload(tool);
      const res = await apiPost('/mcp/invoke', { tool: tool.name, params });
      __mcpState.lastResult = res.result || res;
      __mcpRender(allTools);
    } catch (err) {
      __mcpState.error = String((err && err.message) || err || 'invoke failed');
      __mcpRender(allTools);
    }
  });

  // Reset
  document.querySelector('#mcp-reset-btn')?.addEventListener('click', () => {
    const tool = __mcpState.selectedTool;
    __mcpState.formValues = __mcpSchemaDefaults(tool?.input_schema);
    __mcpState.invokeJson = JSON.stringify(__mcpState.formValues, null, 2);
    __mcpState.error = '';
    __mcpRender(allTools);
  });

  // Form fields
  document.querySelectorAll('[data-mcp-input]').forEach(input => {
    const key = input.dataset.mcpInput || '';
    const handler = () => {
      __mcpState.formValues[key] = input.type === 'checkbox' ? !!input.checked : String(input.value || '');
    };
    input.addEventListener(input.type === 'checkbox' ? 'change' : 'input', handler);
  });

  // Keyboard shortcut: / or Ctrl+K to focus search
  if (!__mcpState._shortcutBound) {
    __mcpState._shortcutBound = true;
    document.addEventListener('keydown', (ev) => {
      const page = (location.hash.replace('#/', '') || 'setup').split('/')[0];
      if (page !== 'mcp-tools') return;
      if ((ev.key === '/' && !ev.ctrlKey && !ev.metaKey) || (ev.ctrlKey && ev.key.toLowerCase() === 'k')) {
        const input = document.querySelector('#mcp-search');
        if (input) { ev.preventDefault(); input.focus(); input.select?.(); }
      }
    });
  }
}
