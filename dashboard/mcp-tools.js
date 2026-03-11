'use strict';

const __mcpState = {
  query: '',
  category: 'all',
  selectedTool: '',
  invokeJson: '{}',
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

window.renderMcpToolsPage = async function renderMcpToolsPage(arg) {
  const content = document.querySelector('#content');
  const api = window.api;
  const apiPost = window.apiPost;
  if (!content || typeof api !== 'function' || typeof apiPost !== 'function') return;

  const requestedCategory = decodeURIComponent(String(arg || '')).trim();
  if (requestedCategory) __mcpState.category = requestedCategory;

  content.innerHTML = '<div class="loading">Loading MCP tool catalog...</div>';

  try {
    const [catalogRes, categoriesRes] = await Promise.all([
      api('/mcp/tools?limit=500&offset=0'),
      api('/mcp/categories'),
    ]);
    const tools = Array.isArray(catalogRes && catalogRes.tools) ? catalogRes.tools : [];
    const categories = Array.isArray(categoriesRes && categoriesRes.categories)
      ? categoriesRes.categories
      : [];
    const activeCategory = __mcpState.category || 'all';
    const filtered = tools.filter((tool) => {
      const categoryOk = activeCategory === 'all' || tool.category === activeCategory;
      return categoryOk && __mcpMatches(tool, __mcpState.query);
    });

    if (!filtered.some((tool) => tool.name === __mcpState.selectedTool)) {
      __mcpState.selectedTool = filtered[0] ? filtered[0].name : '';
      __mcpState.invokeJson = '{}';
    }
    const selectedTool = filtered.find((tool) => tool.name === __mcpState.selectedTool) || filtered[0] || null;

    const grouped = filtered.reduce((acc, tool) => {
      acc[tool.category] = acc[tool.category] || [];
      acc[tool.category].push(tool);
      return acc;
    }, {});

    const categoryButtons = [
      { category: 'all', domain: 'All Domains', tool_count: tools.length },
      ...categories,
    ]
      .map((item) => `
        <button class="mcp-category-chip${item.category === activeCategory ? ' is-active' : ''}" data-mcp-category="${__mcpEsc(item.category)}">
          <span>${__mcpEsc(item.domain || item.category)}</span>
          <strong>${Number(item.tool_count || 0)}</strong>
        </button>
      `)
      .join('');

    const groupsHtml = Object.keys(grouped)
      .sort()
      .map((category) => `
        <section class="mcp-tool-group">
          <div class="mcp-tool-group-head">
            <h3>${__mcpEsc(category)}</h3>
            <span>${grouped[category].length} tools</span>
          </div>
          <div class="mcp-tool-list">
            ${grouped[category]
              .map((tool) => `
                <button class="mcp-tool-row${selectedTool && tool.name === selectedTool.name ? ' is-selected' : ''}" data-mcp-tool="${__mcpEsc(tool.name)}">
                  <div class="mcp-tool-row-name">${__mcpEsc(tool.name)}</div>
                  <div class="mcp-tool-row-desc">${__mcpEsc(tool.description || tool.title || 'No description')}</div>
                </button>
              `)
              .join('')}
          </div>
        </section>
      `)
      .join('');

    const detailHtml = !selectedTool
      ? '<div class="card"><div class="card-label">No tool selected</div><div class="card-sub">Adjust the search or category filter.</div></div>'
      : `
        <div class="card mcp-detail-card">
          <div class="card-label">${__mcpEsc(selectedTool.name)}</div>
          <div class="card-sub">${__mcpEsc(selectedTool.description || selectedTool.title || 'No description')}</div>
          <div class="mcp-meta-grid">
            <div><span>Category</span><strong>${__mcpEsc(selectedTool.category)}</strong></div>
            <div><span>Domain</span><strong>${__mcpEsc(selectedTool.domain)}</strong></div>
            <div><span>Read-only</span><strong>${selectedTool.read_only_hint === true ? 'Yes' : 'Unspecified'}</strong></div>
            <div><span>Open-world</span><strong>${selectedTool.open_world_hint === false ? 'Closed' : 'Open / Unspecified'}</strong></div>
          </div>
          <div class="mcp-schema-grid">
            <div>
              <div class="card-label">Input Schema</div>
              <pre class="mcp-json">${__mcpJson(selectedTool.input_schema)}</pre>
            </div>
            <div>
              <div class="card-label">Output Schema</div>
              <pre class="mcp-json">${__mcpJson(selectedTool.output_schema || {})}</pre>
            </div>
          </div>
        </div>
        <div class="card mcp-invoke-card">
          <div class="card-label">Invoke</div>
          <div class="card-sub">Calls <code>/api/mcp/invoke</code> against the live <code>agenthalo-mcp-server</code> child process.</div>
          <textarea id="mcp-invoke-json" class="input mcp-invoke-json" spellcheck="false">${__mcpEsc(__mcpState.invokeJson || '{}')}</textarea>
          <div class="network-form-actions">
            <button class="btn btn-primary" id="mcp-invoke-btn">Invoke Tool</button>
            <button class="btn" id="mcp-reset-btn">Reset Params</button>
          </div>
          <pre class="mcp-json mcp-result">${__mcpJson(__mcpState.lastResult || { status: 'idle' })}</pre>
          ${__mcpState.error ? `<div class="networking-msg err">${__mcpEsc(__mcpState.error)}</div>` : ''}
        </div>
      `;

    content.innerHTML = `
      <div class="mcp-page">
        <div class="mcp-header">
          <div>
            <h1>MCP Tools</h1>
            <p class="muted">Catalog, inspect, and invoke the live AgentHALO MCP surface from the dashboard.</p>
          </div>
          <div class="mcp-summary-card">
            <span>Total tools</span>
            <strong>${Number(catalogRes && catalogRes.total || tools.length)}</strong>
            <small>Visible: ${filtered.length}</small>
          </div>
        </div>
        <section class="card">
          <div class="mcp-toolbar">
            <input id="mcp-search" class="input" placeholder="Search by tool, category, or description" value="${__mcpEsc(__mcpState.query)}">
            <a class="btn" href="#/networking">Open P2PCLAW Hub</a>
          </div>
          <div class="mcp-category-bar">${categoryButtons}</div>
        </section>
        <section class="mcp-layout">
          <div class="mcp-left">${groupsHtml || '<div class="card"><div class="card-label">No matching tools</div></div>'}</div>
          <div class="mcp-right">${detailHtml}</div>
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

    document.querySelectorAll('[data-mcp-tool]').forEach((button) => {
      button.addEventListener('click', () => {
        __mcpState.selectedTool = button.dataset.mcpTool || '';
        __mcpState.invokeJson = '{}';
        __mcpState.error = '';
        window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
      });
    });

    const resetBtn = document.querySelector('#mcp-reset-btn');
    if (resetBtn) {
      resetBtn.addEventListener('click', () => {
        __mcpState.invokeJson = '{}';
        __mcpState.error = '';
        window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
      });
    }

    const invokeBtn = document.querySelector('#mcp-invoke-btn');
    if (invokeBtn && selectedTool) {
      invokeBtn.addEventListener('click', async () => {
        const input = document.querySelector('#mcp-invoke-json');
        const raw = String(input && input.value || '{}').trim() || '{}';
        __mcpState.invokeJson = raw;
        __mcpState.error = '';
        try {
          const params = JSON.parse(raw);
          const res = await apiPost('/mcp/invoke', { tool: selectedTool.name, params });
          __mcpState.lastResult = res.result || res;
          window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
        } catch (err) {
          __mcpState.error = String(err && err.message || err || 'invoke failed');
          window.renderMcpToolsPage(activeCategory === 'all' ? '' : activeCategory);
        }
      });
    }
  } catch (err) {
    content.innerHTML = `
      <div class="card">
        <div class="card-label">MCP Catalog Unavailable</div>
        <div class="card-sub">${__mcpEsc(String(err && err.message || err || 'unknown error'))}</div>
      </div>
    `;
  }
};
