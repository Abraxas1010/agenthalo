'use strict';

const __mcpState = {
  query: '',
  category: 'all',
  selectedTool: '',
  invokeJson: '{}',
  formValues: {},
  lastResult: null,
  error: '',
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

function __mcpMatches(tool, query) {
  const needle = String(query || '').trim().toLowerCase();
  if (!needle) return true;
  const haystack = [
    tool.name,
    tool.title,
    tool.description,
    tool.category,
    tool.domain,
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
  return haystack.includes(needle);
}

function __mcpSchemaDefaults(schema) {
  if (!schema || typeof schema !== 'object') return {};
  if (schema.type === 'object' && schema.properties && typeof schema.properties === 'object') {
    return Object.fromEntries(
      Object.entries(schema.properties).map(([key, def]) => {
        if (def && Object.prototype.hasOwnProperty.call(def, 'default')) {
          return [key, def.default];
        }
        if (def && def.type === 'boolean') return [key, false];
        return [key, ''];
      }),
    );
  }
  return {};
}

function __mcpSelectedTool(tools) {
  return tools.find((tool) => tool.name === __mcpState.selectedTool) || tools[0] || null;
}

function __mcpToolParams(tool) {
  if (!tool || !tool.input_schema || tool.input_schema.type !== 'object') {
    return null;
  }
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
    if (schema.type === 'integer') {
      payload[key] = Number.parseInt(raw, 10);
    } else if (schema.type === 'number') {
      payload[key] = Number(raw);
    } else if (schema.type === 'boolean') {
      payload[key] = !!raw;
    } else {
      payload[key] = raw;
    }
  });
  return payload;
}

function __mcpRenderField(tool, key, schema) {
  const value = __mcpState.formValues[key] ?? '';
  const label = schema.title || key;
  const hint = schema.description || '';
  if (schema.type === 'boolean') {
    return `
      <label class="mcp-form-field">
        <span class="mcp-form-label">${__mcpEsc(label)}</span>
        <span class="mcp-form-check">
          <input type="checkbox" data-mcp-input="${__mcpEsc(key)}" ${value ? 'checked' : ''}>
          <span>${__mcpEsc(hint || 'Toggle')}</span>
        </span>
      </label>
    `;
  }
  const type = schema.type === 'integer' || schema.type === 'number' ? 'number' : 'text';
  return `
    <label class="mcp-form-field">
      <span class="mcp-form-label">${__mcpEsc(label)}</span>
      <input
        class="input"
        type="${type}"
        data-mcp-input="${__mcpEsc(key)}"
        value="${__mcpEsc(value)}"
        placeholder="${__mcpEsc(schema.examples?.[0] || hint || key)}"
      >
      ${hint ? `<span class="mcp-form-help">${__mcpEsc(hint)}</span>` : ''}
    </label>
  `;
}

function __mcpBindSearchShortcut() {
  if (__mcpState._shortcutBound) return;
  __mcpState._shortcutBound = true;
  document.addEventListener('keydown', (ev) => {
    const page = (location.hash.replace('#/', '') || 'setup').split('/')[0];
    if (page !== 'mcp-tools') return;
    if ((ev.key === '/' && !ev.ctrlKey && !ev.metaKey) || (ev.ctrlKey && ev.key.toLowerCase() === 'k')) {
      const input = document.querySelector('#mcp-search');
      if (input) {
        ev.preventDefault();
        input.focus();
        input.select?.();
      }
    }
  });
}

window.renderMcpToolsPage = async function renderMcpToolsPage(arg) {
  const content = document.querySelector('#content');
  const api = window.api;
  const apiPost = window.apiPost;
  if (!content || typeof api !== 'function' || typeof apiPost !== 'function') return;

  const requestedCategory = decodeURIComponent(String(arg || '')).trim();
  if (requestedCategory) __mcpState.category = requestedCategory;

  content.innerHTML = '<div class="loading">Loading MCP tool catalog...</div>';
  __mcpBindSearchShortcut();

  try {
    const [catalogRes, categoriesRes] = await Promise.all([
      api('/mcp/tools?limit=500&offset=0'),
      api('/mcp/categories'),
    ]);
    const tools = Array.isArray(catalogRes?.tools) ? catalogRes.tools : [];
    const categories = Array.isArray(categoriesRes?.categories) ? categoriesRes.categories : [];
    const activeCategory = __mcpState.category || 'all';
    const filtered = tools.filter((tool) => {
      const categoryOk = activeCategory === 'all' || tool.category === activeCategory;
      return categoryOk && __mcpMatches(tool, __mcpState.query);
    });
    const selectedTool = __mcpSelectedTool(filtered);
    if (selectedTool && selectedTool.name !== __mcpState.selectedTool) {
      __mcpState.selectedTool = selectedTool.name;
      __mcpState.formValues = __mcpSchemaDefaults(selectedTool.input_schema);
      __mcpState.invokeJson = JSON.stringify(__mcpState.formValues, null, 2);
    }

    const sidebar = [
      { category: 'all', domain: 'All Categories', tool_count: tools.length },
      ...categories,
    ]
      .map((item) => `
        <button class="mcp-sidebar-item${item.category === activeCategory ? ' is-active' : ''}" data-mcp-category="${__mcpEsc(item.category)}">
          <span>${__mcpEsc(item.domain || item.category)}</span>
          <strong>${Number(item.tool_count || 0)}</strong>
        </button>
      `)
      .join('');

    const cards = filtered
      .map((tool) => `
        <article class="card mcp-tool-card${selectedTool && selectedTool.name === tool.name ? ' is-selected' : ''}">
          <div class="mcp-tool-card-head">
            <div>
              <div class="card-label" style="font-family:var(--mono)">${__mcpEsc(tool.name)}</div>
              <div class="card-sub">${__mcpEsc(tool.description || tool.title || 'No description')}</div>
            </div>
            <div style="display:flex;gap:6px;flex-wrap:wrap;justify-content:flex-end">
              <span class="badge badge-info">${__mcpEsc(tool.category || 'uncategorized')}</span>
              ${tool.domain ? `<span class="badge">${__mcpEsc(tool.domain)}</span>` : ''}
            </div>
          </div>
          <div class="mcp-tool-card-actions">
            <button class="btn btn-sm" data-mcp-tool="${__mcpEsc(tool.name)}">Inspect</button>
            <button class="btn btn-sm btn-primary" data-mcp-quick="${__mcpEsc(tool.name)}">Quick Invoke</button>
          </div>
        </article>
      `)
      .join('');

    const params = __mcpToolParams(selectedTool);
    const invokePanel = !selectedTool
      ? '<div class="card"><div class="card-label">No tool selected</div><div class="card-sub">Choose a tool from the catalog.</div></div>'
      : `
        <div class="card mcp-invoke-shell">
          <div class="card-label" style="font-family:var(--mono)">${__mcpEsc(selectedTool.name)}</div>
          <div class="card-sub">${__mcpEsc(selectedTool.description || selectedTool.title || 'No description')}</div>
          <div class="mcp-meta-grid">
            <div><span>Category</span><strong>${__mcpEsc(selectedTool.category || 'uncategorized')}</strong></div>
            <div><span>Domain</span><strong>${__mcpEsc(selectedTool.domain || 'n/a')}</strong></div>
            <div><span>Result Count</span><strong>${filtered.length}</strong></div>
            <div><span>Shortcut</span><strong>/ or Ctrl+K</strong></div>
          </div>
          ${
            params
              ? `
              <div class="mcp-form-grid">
                ${Object.entries(params).map(([key, schema]) => __mcpRenderField(selectedTool, key, schema || {})).join('')}
              </div>
            `
              : `
              <textarea id="mcp-invoke-json" class="input mcp-invoke-json" spellcheck="false">${__mcpEsc(__mcpState.invokeJson || '{}')}</textarea>
            `
          }
          <div class="network-form-actions">
            <button class="btn btn-primary" id="mcp-invoke-btn">Execute</button>
            <button class="btn" id="mcp-reset-btn">Reset</button>
          </div>
          <div class="mcp-result-count">${filtered.length} tool${filtered.length === 1 ? '' : 's'} visible</div>
          <pre class="mcp-json mcp-result">${__mcpJson(__mcpState.lastResult || { status: 'idle' })}</pre>
          ${__mcpState.error ? `<div class="networking-msg err">${__mcpEsc(__mcpState.error)}</div>` : ''}
        </div>
      `;

    content.innerHTML = `
      <div class="mcp-shell">
        <aside class="card mcp-sidebar">
          <div class="card-label">Categories</div>
          <div class="card-sub">Live MCP registry groups with tool counts.</div>
          <div class="mcp-sidebar-list">${sidebar}</div>
        </aside>
        <section class="mcp-main">
          <section class="card">
            <div class="mcp-header">
              <div>
                <div class="page-title">MCP Tools</div>
                <div class="muted">Schema-aware invocation over the live AgentHALO MCP surface.</div>
              </div>
              <div class="mcp-summary-card">
                <span>Total</span>
                <strong>${Number(catalogRes?.total || tools.length)}</strong>
              </div>
            </div>
            <div class="mcp-toolbar">
              <input id="mcp-search" class="input" placeholder="Search tools, categories, or domains" value="${__mcpEsc(__mcpState.query)}">
              <a class="btn" href="#/p2pclaw">Open P2PCLAW</a>
            </div>
          </section>
          <section class="mcp-layout">
            <div class="mcp-card-grid">${cards || '<div class="card"><div class="card-label">No matching tools</div></div>'}</div>
            <div class="mcp-detail-panel">${invokePanel}</div>
          </section>
        </section>
      </div>
    `;

    document.querySelectorAll('[data-mcp-category]').forEach((button) => {
      button.addEventListener('click', () => {
        __mcpState.category = button.dataset.mcpCategory || 'all';
        window.renderMcpToolsPage(__mcpState.category === 'all' ? '' : __mcpState.category);
      });
    });

    const searchInput = document.querySelector('#mcp-search');
    if (searchInput) {
      searchInput.addEventListener('input', () => {
        __mcpState.query = searchInput.value || '';
        window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
      });
    }

    document.querySelectorAll('[data-mcp-tool],[data-mcp-quick]').forEach((button) => {
      button.addEventListener('click', () => {
        __mcpState.selectedTool = button.dataset.mcpTool || button.dataset.mcpQuick || '';
        __mcpState.error = '';
        window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
      });
    });

    document.querySelectorAll('[data-mcp-input]').forEach((input) => {
      const key = input.dataset.mcpInput || '';
      const handler = () => {
        __mcpState.formValues[key] =
          input.type === 'checkbox' ? !!input.checked : String(input.value || '');
      };
      input.addEventListener(input.type === 'checkbox' ? 'change' : 'input', handler);
    });

    document.querySelector('#mcp-reset-btn')?.addEventListener('click', () => {
      __mcpState.formValues = __mcpSchemaDefaults(selectedTool?.input_schema);
      __mcpState.invokeJson = JSON.stringify(__mcpState.formValues, null, 2);
      __mcpState.error = '';
      window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
    });

    document.querySelector('#mcp-invoke-btn')?.addEventListener('click', async () => {
      if (!selectedTool) return;
      const rawInput = document.querySelector('#mcp-invoke-json');
      if (rawInput) __mcpState.invokeJson = String(rawInput.value || '{}');
      __mcpState.error = '';
      try {
        const params = rawInput
          ? JSON.parse(__mcpState.invokeJson || '{}')
          : __mcpBuildInvokePayload(selectedTool);
        const res = await apiPost('/mcp/invoke', { tool: selectedTool.name, params });
        __mcpState.lastResult = res.result || res;
        window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
      } catch (err) {
        __mcpState.error = String((err && err.message) || err || 'invoke failed');
        window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
      }
    });
  } catch (err) {
    content.innerHTML = `
      <div class="card">
        <div class="card-label">MCP Catalog Unavailable</div>
        <div class="card-sub">${__mcpEsc(String((err && err.message) || err || 'unknown error'))}</div>
      </div>
    `;
  }
};
