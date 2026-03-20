'use strict';

/* ================================================================
   Lean Project Browser — AgentHALO Dashboard
   Integrated with HeytingLean Observatory for health visualizations.
   ================================================================ */

const __leanState = {
  activeTab: 'project',
  tabs: [],
  tree: null,
  totalFiles: 0,
  root: '',
  loading: false,
  error: '',
  expandedDirs: new Set(),
  selectedFile: null,
  fileContent: null,
  fileLoading: false,
  searchQuery: '',
  modalContent: null,
  modalTitle: '',
  // Observatory data
  obsStatus: null,
  obsFileData: null,
  obsActiveViz: null, // 'treemap' | 'depgraph' | 'clusters' | null
};

function __leanEsc(value) {
  if (window.__escapeHtml) return window.__escapeHtml(value);
  if (value == null) return '';
  return String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

function __leanFmtSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function __leanFmtNum(n) { return (n || 0).toLocaleString(); }

// ---- API calls ----

async function __leanScan() {
  __leanState.loading = true;
  __leanState.error = '';
  __leanRender();
  try {
    const api = window.api;
    if (typeof api !== 'function') throw new Error('API not available');
    const res = await api('/lean/scan');
    if (!res.ok) {
      __leanState.error = res.message || 'Scan failed';
      __leanState.tree = null;
    } else {
      __leanState.tree = res.tree;
      __leanState.totalFiles = res.total_files || 0;
      __leanState.root = res.root || '';
      __leanState.tabs = [{ id: 'project', name: 'Project', root: res.root, totalFiles: res.total_files, tree: res.tree }];
      const libs = res.libraries || [];
      for (const lib of libs) {
        __leanState.tabs.push({ id: `lib:${lib.name}`, name: lib.name, root: lib.root, totalFiles: lib.total_files, tree: lib.tree });
      }
      const activeTree = __leanActiveTree();
      if (activeTree?.children) {
        for (const child of activeTree.children) {
          if (child.type === 'dir') __leanState.expandedDirs.add(child.name);
        }
      }
    }
  } catch (e) {
    __leanState.error = String((e && e.message) || e || 'scan failed');
    __leanState.tree = null;
  }
  __leanState.loading = false;
  // Fetch Observatory status in parallel
  __leanFetchObsStatus();
  __leanRender();
}

async function __leanFetchObsStatus() {
  try {
    const res = await fetch('/api/observatory/status');
    if (res.ok) __leanState.obsStatus = await res.json();
  } catch (_e) {}
}

async function __leanFetchObsFile(path) {
  try {
    const res = await fetch(`/api/observatory/file?path=${encodeURIComponent(path)}`);
    if (res.ok) {
      __leanState.obsFileData = await res.json();
      __leanRender();
    }
  } catch (_e) {}
}

function __leanActiveTree() {
  const tab = __leanState.tabs.find(t => t.id === __leanState.activeTab);
  return tab ? tab.tree : __leanState.tree;
}

function __leanActiveTab() {
  return __leanState.tabs.find(t => t.id === __leanState.activeTab) || __leanState.tabs[0] || null;
}

async function __leanLoadFile(path) {
  __leanState.fileLoading = true;
  __leanState.selectedFile = path;
  __leanState.obsFileData = null;
  __leanRender();
  try {
    const api = window.api;
    const res = await api(`/lean/file?path=${encodeURIComponent(path)}`);
    __leanState.fileContent = res.content || '';
  } catch (e) {
    __leanState.fileContent = `Error loading file: ${String((e && e.message) || e)}`;
  }
  __leanState.fileLoading = false;
  __leanRender();
  // Fetch Observatory data for this file
  __leanFetchObsFile(path);
}

// ---- Tree rendering ----

function __leanMatchesSearch(node, query) {
  if (!query) return true;
  const q = query.toLowerCase();
  if (node.type === 'file') return node.name.toLowerCase().includes(q) || (node.path || '').toLowerCase().includes(q);
  if (node.type === 'dir') {
    if (node.name.toLowerCase().includes(q)) return true;
    return (node.children || []).some(c => __leanMatchesSearch(c, q));
  }
  return false;
}

function __leanRenderTreeNode(node, depth, parentPath) {
  const q = __leanState.searchQuery;
  if (!__leanMatchesSearch(node, q)) return '';

  if (node.type === 'file') {
    const isSelected = __leanState.selectedFile === node.path;
    const indent = depth * 16;
    return `<div class="lean-tree-file${isSelected ? ' is-selected' : ''}" data-lean-file="${__leanEsc(node.path)}" style="padding-left:${indent + 8}px">
      <span class="lean-tree-file-icon">\uD83D\uDCC4</span>
      <span class="lean-tree-file-name">${__leanEsc(node.name.replace('.lean', ''))}</span>
      <span class="lean-tree-file-ext">.lean</span>
      <span class="lean-tree-file-size">${__leanFmtSize(node.size || 0)}</span>
    </div>`;
  }

  if (node.type === 'dir') {
    const dirKey = parentPath ? `${parentPath}/${node.name}` : node.name;
    const isExpanded = __leanState.expandedDirs.has(dirKey) || (q && __leanMatchesSearch(node, q));
    const indent = depth * 16;
    const childrenHtml = isExpanded
      ? (node.children || []).map(c => __leanRenderTreeNode(c, depth + 1, dirKey)).join('')
      : '';
    const count = node.lean_count || 0;
    return `<div class="lean-tree-dir">
      <div class="lean-tree-dir-header${isExpanded ? ' is-expanded' : ''}" data-lean-dir="${__leanEsc(dirKey)}" style="padding-left:${indent + 8}px">
        <span class="lean-tree-dir-arrow">${isExpanded ? '\u25BE' : '\u25B8'}</span>
        <span class="lean-tree-dir-icon">${isExpanded ? '\uD83D\uDCC2' : '\uD83D\uDCC1'}</span>
        <span class="lean-tree-dir-name">${__leanEsc(node.name)}</span>
        <span class="lean-tree-dir-count">${count}</span>
      </div>
      ${childrenHtml}
    </div>`;
  }
  return '';
}

// ---- Observatory health bar ----

function __leanRenderHealthBar() {
  const obs = __leanState.obsStatus;
  if (!obs) return '';
  const h = obs.health_score || 0;
  const hc = h >= 0.8 ? 'good' : h >= 0.4 ? 'warn' : 'bad';
  const hColor = hc === 'good' ? 'var(--green)' : hc === 'warn' ? 'var(--amber)' : 'var(--red)';

  const vizTools = [
    { id: 'treemap',    icon: '\u{1F5FA}', label: 'Health Treemap',     desc: 'Squarified file map sized by lines, colored by health status' },
    { id: 'depgraph',   icon: '\u{1F578}', label: 'Dependency Graph',   desc: 'Force-directed module dependency network with zoom and drag' },
    { id: 'clusters',   icon: '\u{1F52C}', label: 'Module Clusters',    desc: 'Packed bubble chart of module groups — click to explore in 3D' },
    { id: 'sorrys',     icon: '\u{26A0}',  label: 'Sorry Tracker',      desc: 'All incomplete proofs with file locations and declaration context' },
    { id: 'complexity',  icon: '\u{1F4CA}', label: 'Complexity Analysis', desc: 'Top declarations ranked by size — find the most complex proofs' },
  ];

  return `<div class="lean-obs-hero">
    <div class="lean-obs-hero-top">
      <div class="lean-obs-hero-title">
        <span class="lean-obs-hero-icon">\u{1F52D}</span>
        <div>
          <div class="lean-obs-hero-heading">Observatory</div>
          <div class="lean-obs-hero-sub">Lean project health &amp; visualization</div>
        </div>
      </div>
      <div class="lean-obs-stats-row">
        <div class="lean-obs-stat-pill" style="border-color:${hColor}">
          <span class="lean-obs-stat-val" style="color:${hColor}">${(h * 100).toFixed(0)}%</span>
          <span class="lean-obs-stat-lbl">Health</span>
        </div>
        <div class="lean-obs-stat-pill">
          <span class="lean-obs-stat-val">${__leanFmtNum(obs.total_files)}</span>
          <span class="lean-obs-stat-lbl">Files</span>
        </div>
        <div class="lean-obs-stat-pill">
          <span class="lean-obs-stat-val">${__leanFmtNum(obs.total_lines)}</span>
          <span class="lean-obs-stat-lbl">Lines</span>
        </div>
        <div class="lean-obs-stat-pill">
          <span class="lean-obs-stat-val">${__leanFmtNum(obs.total_decls)}</span>
          <span class="lean-obs-stat-lbl">Decls</span>
        </div>
        <div class="lean-obs-stat-pill" style="border-color:${obs.total_sorrys > 0 ? 'var(--red)' : 'var(--green)'}">
          <span class="lean-obs-stat-val" style="color:${obs.total_sorrys > 0 ? 'var(--red)' : 'var(--green)'}">${obs.total_sorrys}</span>
          <span class="lean-obs-stat-lbl">Sorrys</span>
        </div>
        <div class="lean-obs-stat-pill">
          <span class="lean-obs-stat-val">${__leanFmtNum(obs.clusters_count)}</span>
          <span class="lean-obs-stat-lbl">Modules</span>
        </div>
      </div>
    </div>
    <div class="lean-obs-progress">
      <div class="lean-obs-progress-fill ${hc}" style="width:${(h * 100).toFixed(1)}%"></div>
    </div>
    <div class="lean-obs-tool-grid">
      ${vizTools.map(v => `
        <button class="lean-obs-tool-card${__leanState.obsActiveViz === v.id ? ' active' : ''}" data-obs-page="${v.id}">
          <div class="lean-obs-tool-icon">${v.icon}</div>
          <div class="lean-obs-tool-body">
            <div class="lean-obs-tool-label">${v.label}</div>
            <div class="lean-obs-tool-desc">${v.desc}</div>
          </div>
        </button>
      `).join('')}
    </div>
  </div>`;
}

// ---- File detail panel ----

function __leanRenderDetail() {
  if (!__leanState.selectedFile) {
    return `<div class="lean-detail-empty">
      <div style="font-size:36px;opacity:0.3;margin-bottom:12px">\u2112</div>
      <div class="lean-detail-empty-title">Select a File</div>
      <div class="lean-detail-empty-sub">Click any .lean file in the tree to preview it here.</div>
    </div>`;
  }
  if (__leanState.fileLoading) {
    return '<div class="lean-detail-loading">Loading file...</div>';
  }
  const path = __leanState.selectedFile;
  const parts = path.split('/');
  const fileName = parts.pop();
  const modulePath = parts.join('.');
  const content = __leanState.fileContent || '';
  const lineCount = content.split('\n').length;
  const sizeBytes = new TextEncoder().encode(content).length;

  const imports = [];
  const declarations = [];
  let sorryCount = 0;
  for (const line of content.split('\n')) {
    const trimmed = line.trim();
    if (trimmed.startsWith('import ')) imports.push(trimmed.slice(7).trim());
    const declMatch = trimmed.match(/^(theorem|lemma|def|instance|structure|class|inductive|axiom|opaque|abbrev)\s+(\S+)/);
    if (declMatch) declarations.push({ kind: declMatch[1], name: declMatch[2] });
    if (/\bsorry\b/.test(trimmed)) sorryCount++;
  }

  // Observatory file health (if available)
  const obsFile = __leanState.obsFileData;
  const health = obsFile?.health;
  const healthPct = health ? (health.score * 100).toFixed(0) : null;
  const healthCls = health ? (health.score >= 0.8 ? 'good' : health.score >= 0.4 ? 'warn' : 'bad') : '';

  return `<div class="lean-detail">
    <div class="lean-detail-header">
      <div>
        <div class="lean-detail-filename">${__leanEsc(fileName)}</div>
        <div class="lean-detail-module">${__leanEsc(modulePath)}</div>
      </div>
      <div class="lean-detail-actions">
        ${healthPct !== null ? `<span class="lean-health-badge ${healthCls}" title="File health: ${healthPct}%">${healthPct}%</span>` : ''}
        <button class="btn btn-sm" id="lean-view-full">View Full</button>
      </div>
    </div>
    <div class="lean-detail-meta">
      <div><span>Lines</span><strong>${__leanFmtNum(lineCount)}</strong></div>
      <div><span>Size</span><strong>${__leanFmtSize(sizeBytes)}</strong></div>
      <div><span>Imports</span><strong>${imports.length}</strong></div>
      <div><span>Declarations</span><strong>${declarations.length}</strong></div>
      <div><span>Sorrys</span><strong style="color:${sorryCount > 0 ? 'var(--red)' : 'var(--green)'}">${sorryCount}</strong></div>
      ${health ? `<div><span>Health</span><strong style="color:${healthCls === 'good' ? 'var(--green)' : healthCls === 'warn' ? 'var(--amber)' : 'var(--red)'}">${healthPct}%</strong></div>` : ''}
    </div>
    ${declarations.length ? `<div class="lean-detail-section">
      <div class="lean-detail-section-label">Declarations <span class="lean-count-badge">${declarations.length}</span></div>
      <div class="lean-decl-grid">${declarations.slice(0, 60).map(d =>
        `<div class="lean-decl-row"><span class="lean-decl-kind lean-kind-${d.kind}">${__leanEsc(d.kind)}</span><span class="lean-decl-name">${__leanEsc(d.name)}</span></div>`
      ).join('')}${declarations.length > 60 ? `<div class="lean-decl-row" style="color:var(--text-dim)">... +${declarations.length - 60} more</div>` : ''}</div>
    </div>` : ''}
    ${imports.length ? `<div class="lean-detail-section">
      <div class="lean-detail-section-label">Imports <span class="lean-count-badge">${imports.length}</span></div>
      <div class="lean-import-list">${imports.slice(0, 20).map(i =>
        `<div class="lean-import">${__leanEsc(i)}</div>`
      ).join('')}${imports.length > 20 ? `<div class="lean-import" style="color:var(--text-dim)">+${imports.length - 20} more</div>` : ''}</div>
    </div>` : ''}
    <div class="lean-detail-section">
      <div class="lean-detail-section-label">Preview</div>
      <pre class="lean-preview">${__leanEsc(content.slice(0, 3000))}${content.length > 3000 ? '\n... (truncated)' : ''}</pre>
    </div>
  </div>`;
}

// ---- Observatory visualization panel ----

function __leanRenderVizPanel() {
  const viz = __leanState.obsActiveViz;
  if (!viz) return '';
  return `<div class="lean-viz-panel card" id="lean-viz-container">
    <div class="lean-viz-header">
      <div class="lean-viz-title">${__leanEsc(viz.charAt(0).toUpperCase() + viz.slice(1))}</div>
      <button class="btn btn-sm" id="lean-viz-close">\u2715</button>
    </div>
    <div class="lean-viz-body" id="lean-viz-body">
      <div class="loading">Loading visualization...</div>
    </div>
  </div>`;
}

async function __leanLoadViz(vizType) {
  const endpoints = {
    treemap: '/api/observatory/treemap',
    depgraph: '/api/observatory/depgraph',
    clusters: '/api/observatory/clusters',
    sorrys: '/api/observatory/sorrys',
    complexity: '/api/observatory/complexity',
  };
  const endpoint = endpoints[vizType];
  if (!endpoint) return;

  __leanState.obsActiveViz = vizType;
  __leanRender();

  try {
    const res = await fetch(endpoint);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    const body = document.getElementById('lean-viz-body');
    if (body && window.Observatory && typeof window.Observatory.renderViz === 'function') {
      body.innerHTML = '';
      window.Observatory.renderViz(vizType, data, body);
    }
  } catch (e) {
    const body = document.getElementById('lean-viz-body');
    if (body) body.innerHTML = `<div class="obs-error">Error: ${__leanEsc(e.message)}</div>`;
  }
}

// ---- Modal ----

function __leanRenderModal() {
  if (__leanState.modalContent === null) return '';
  return `<div class="skill-modal-overlay" id="lean-modal-overlay">
    <div class="skill-modal" style="max-width:900px">
      <div class="skill-modal-header">
        <div class="skill-modal-title">${__leanEsc(__leanState.modalTitle)}</div>
        <button class="btn btn-sm" id="lean-modal-close">\u2715 Close</button>
      </div>
      <pre class="skill-modal-content" style="font-size:12px;tab-size:2">${__leanEsc(__leanState.modalContent)}</pre>
    </div>
  </div>`;
}

// ---- Main render ----

window.renderLeanPage = async function renderLeanPage() {
  const content = document.querySelector('#content');
  if (!content) return;
  if (!__leanState.tree && !__leanState.loading) {
    content.innerHTML = '<div class="loading">Loading Lean project...</div>';
    await __leanScan();
    return;
  }
  __leanRender();
};

function __leanRender() {
  const content = document.querySelector('#content');
  if (!content) return;

  const treeEl = document.querySelector('.lean-tree');
  const savedTreeScroll = treeEl ? treeEl.scrollTop : 0;
  const savedMainScroll = content.scrollTop;

  if (__leanState.loading) {
    content.innerHTML = '<div class="loading">Scanning Lean files...</div>';
    return;
  }

  if (__leanState.error) {
    content.innerHTML = `<div class="card" style="padding:24px">
      <div class="page-title">Lean Project Browser</div>
      <div class="muted" style="margin:12px 0">${__leanEsc(__leanState.error)}</div>
      <button class="btn btn-primary" id="lean-scan-btn">Scan Now</button>
    </div>`;
    document.querySelector('#lean-scan-btn')?.addEventListener('click', () => __leanScan());
    return;
  }

  const activeTab = __leanActiveTab();
  const tree = activeTab ? activeTab.tree : __leanState.tree;
  const treeHtml = tree ? (tree.children || []).map(c => __leanRenderTreeNode(c, 0, '')).join('') : '';
  const detailHtml = __leanRenderDetail();
  const activeFiles = activeTab ? activeTab.totalFiles : __leanState.totalFiles;
  const activeRoot = activeTab ? activeTab.root : __leanState.root;

  const tabsHtml = __leanState.tabs.length > 1 ? `<div class="lean-tabs">${__leanState.tabs.map(t => {
    const isActive = t.id === __leanState.activeTab;
    const label = t.id === 'project' ? 'Project' : t.name;
    return `<button class="lean-tab${isActive ? ' is-active' : ''}" data-lean-tab="${__leanEsc(t.id)}">
      ${__leanEsc(label)} <span class="lean-tab-count">${(t.totalFiles || 0).toLocaleString()}</span>
    </button>`;
  }).join('')}</div>` : '';

  content.innerHTML = `<div class="lean-page">
    ${__leanRenderHealthBar()}
    ${__leanRenderVizPanel()}
    <div class="lean-shell">
      <aside class="card lean-sidebar">
        <div class="lean-sidebar-header">
          <div class="card-label">Lean Files</div>
          <div class="card-sub">${__leanFmtNum(activeFiles)} files</div>
          <button class="btn btn-sm" id="lean-rescan-btn" title="Rescan">\u21BB</button>
        </div>
        ${tabsHtml}
        <input id="lean-search" class="input" placeholder="Filter files... (/)" value="${__leanEsc(__leanState.searchQuery)}" style="margin:6px 0">
        <div class="lean-tree">${treeHtml || '<div class="card-sub" style="padding:8px">No files found.</div>'}</div>
      </aside>
      <section class="lean-main">${detailHtml}</section>
    </div>
    ${__leanRenderModal()}
  </div>`;

  const newTree = document.querySelector('.lean-tree');
  if (newTree) newTree.scrollTop = savedTreeScroll;
  content.scrollTop = savedMainScroll;

  __leanBindEvents();
}

function __leanBindEvents() {
  // Search
  document.querySelector('#lean-search')?.addEventListener('input', (e) => {
    __leanState.searchQuery = e.target.value || '';
    __leanRender();
  });

  // Tab switching
  document.querySelectorAll('[data-lean-tab]').forEach(btn => {
    btn.addEventListener('click', () => {
      __leanState.activeTab = btn.dataset.leanTab;
      __leanState.expandedDirs.clear();
      __leanState.selectedFile = null;
      __leanState.fileContent = null;
      __leanState.searchQuery = '';
      const activeTree = __leanActiveTree();
      if (activeTree?.children) {
        for (const child of activeTree.children) {
          if (child.type === 'dir') __leanState.expandedDirs.add(child.name);
        }
      }
      __leanRender();
    });
  });

  // Rescan
  document.querySelector('#lean-rescan-btn')?.addEventListener('click', () => {
    __leanState.tree = null;
    __leanState.expandedDirs.clear();
    __leanState.selectedFile = null;
    __leanState.fileContent = null;
    __leanState.obsStatus = null;
    __leanScan();
  });

  // Toggle directory
  document.querySelectorAll('[data-lean-dir]').forEach(el => {
    el.addEventListener('click', () => {
      const key = el.dataset.leanDir;
      if (__leanState.expandedDirs.has(key)) __leanState.expandedDirs.delete(key);
      else __leanState.expandedDirs.add(key);
      __leanRender();
    });
  });

  // Select file
  document.querySelectorAll('[data-lean-file]').forEach(el => {
    el.addEventListener('click', () => {
      let path = el.dataset.leanFile;
      const activeTab = __leanActiveTab();
      if (activeTab && activeTab.id.startsWith('lib:')) {
        const libName = activeTab.id.slice(4);
        path = `lib:${libName}/${path}`;
      }
      __leanLoadFile(path);
    });
  });

  // View full file
  document.querySelector('#lean-view-full')?.addEventListener('click', () => {
    __leanState.modalContent = __leanState.fileContent || '';
    __leanState.modalTitle = __leanState.selectedFile || 'Lean File';
    __leanRender();
  });

  // Observatory viz buttons
  document.querySelectorAll('[data-obs-page]').forEach(btn => {
    btn.addEventListener('click', () => {
      const viz = btn.dataset.obsPage;
      if (__leanState.obsActiveViz === viz) {
        __leanState.obsActiveViz = null;
        __leanRender();
      } else {
        __leanLoadViz(viz);
      }
    });
  });

  // Close viz panel
  document.querySelector('#lean-viz-close')?.addEventListener('click', () => {
    __leanState.obsActiveViz = null;
    __leanRender();
  });

  // Modal close
  document.querySelector('#lean-modal-close')?.addEventListener('click', () => {
    __leanState.modalContent = null;
    __leanRender();
  });
  document.querySelector('#lean-modal-overlay')?.addEventListener('click', (e) => {
    if (e.target.id === 'lean-modal-overlay') {
      __leanState.modalContent = null;
      __leanRender();
    }
  });

  // Keyboard
  if (!__leanState._keyBound) {
    __leanState._keyBound = true;
    document.addEventListener('keydown', (ev) => {
      const page = (location.hash.replace('#/', '') || 'setup').split('/')[0];
      if (page !== 'lean') return;
      if (ev.key === 'Escape') {
        if (__leanState.modalContent !== null) { __leanState.modalContent = null; __leanRender(); }
        else if (__leanState.obsActiveViz) { __leanState.obsActiveViz = null; __leanRender(); }
      }
      if (ev.key === '/' && !ev.ctrlKey && !ev.metaKey && __leanState.modalContent === null) {
        const input = document.querySelector('#lean-search');
        if (input) { ev.preventDefault(); input.focus(); input.select?.(); }
      }
    });
  }
}
