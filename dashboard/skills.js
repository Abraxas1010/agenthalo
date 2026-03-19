'use strict';

/* ================================================================
   Skills Registry — AgentHALO Dashboard
   Read-only categorized skill catalog with detail panel and
   full SKILL.md popup viewer.
   ================================================================ */

const __skillState = {
  skills: [],
  selected: null,
  query: '',
  activeCategory: null,
  modalContent: null,
  modalTitle: '',
};

function __skillEsc(value) {
  if (window.__escapeHtml) return window.__escapeHtml(value);
  if (value == null) return '';
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

async function __skillLoad() {
  try {
    const api = window.api;
    if (typeof api !== 'function') return;
    const res = await api('/skills');
    __skillState.skills = Array.isArray(res?.skills) ? res.skills : [];
  } catch (_) {
    __skillState.skills = [];
  }
}

function __skillCategories() {
  const cats = {};
  for (const s of __skillState.skills) {
    const c = s.category || 'uncategorized';
    if (!cats[c]) cats[c] = [];
    cats[c].push(s);
  }
  return Object.entries(cats)
    .sort((a, b) => b[1].length - a[1].length)
    .map(([name, skills]) => ({ name, skills, count: skills.length }));
}

function __skillMatchesQuery(skill, query) {
  if (!query) return true;
  const q = query.toLowerCase();
  return [skill.skill_id, skill.name, skill.description, skill.category]
    .concat(skill.triggers || [])
    .filter(Boolean)
    .join(' ')
    .toLowerCase()
    .includes(q);
}

const __SKILL_CAT_ICONS = {
  'meta-atp': '\u2699',
  'meta-atp-retrieval': '\uD83D\uDD0D',
  'meta-atp-sheaf': '\uD83E\uDDE9',
  'meta-atp-experimental': '\uD83E\uDDEA',
  'meta-proof': '\uD83D\uDCCB',
  'meta-algebra-category': '\u2202',
  'meta-combinatorics-topology': '\uD83D\uDD37',
  'meta-analysis-optimization': '\uD83D\uDCC8',
  'meta-physics-quantum': '\u269B',
  'meta-translation': '\uD83C\uDF10',
  'meta-export-synthesis': '\uD83D\uDCE6',
  'meta-overlay': '\uD83C\uDFA8',
  'meta-paper-pipeline': '\uD83D\uDCDD',
  'meta-llm-infra': '\uD83E\uDD16',
  'meta-system': '\u2692',
  'meta-skill-router': '\u21C4',
};

function __skillCatIcon(cat) {
  return __SKILL_CAT_ICONS[cat] || '\uD83D\uDCC2';
}

function __skillCatLabel(cat) {
  return cat
    .replace(/^meta-/, '')
    .replace(/-/g, ' ')
    .replace(/\b\w/g, c => c.toUpperCase());
}

// ---- Render ----

window.renderSkillsPage = async function renderSkillsPage() {
  const content = document.querySelector('#content');
  if (!content) return;
  content.innerHTML = '<div class="loading">Loading skills...</div>';
  await __skillLoad();
  __skillRender();
};

function __skillRender() {
  const content = document.querySelector('#content');
  if (!content) return;

  const categories = __skillCategories();
  const q = __skillState.query;
  const activeCat = __skillState.activeCategory;
  const totalCount = __skillState.skills.length;
  const extCount = __skillState.skills.filter(s => s.source === 'external').length;

  // Category filter pills
  const pillsHtml = categories.map(cat => {
    const filtered = cat.skills.filter(s => __skillMatchesQuery(s, q));
    if (filtered.length === 0) return '';
    const isActive = activeCat === cat.name;
    return `<button class="skill-cat-pill${isActive ? ' is-active' : ''}" data-skill-cat="${__skillEsc(cat.name)}">
      <span class="skill-cat-pill-icon">${__skillCatIcon(cat.name)}</span>
      <span>${__skillEsc(__skillCatLabel(cat.name))}</span>
      <span class="skill-cat-pill-count">${filtered.length}</span>
    </button>`;
  }).filter(Boolean).join('');

  // Skill cards (filtered + category-filtered)
  const visibleSkills = __skillState.skills
    .filter(s => __skillMatchesQuery(s, q))
    .filter(s => !activeCat || s.category === activeCat);

  // Group visible skills by category
  const grouped = {};
  for (const s of visibleSkills) {
    const c = s.category || 'uncategorized';
    if (!grouped[c]) grouped[c] = [];
    grouped[c].push(s);
  }

  let gridHtml = '';
  const sortedGroups = Object.entries(grouped).sort((a, b) => b[1].length - a[1].length);
  for (const [cat, skills] of sortedGroups) {
    const cardsHtml = skills.map(s => {
      const isSelected = __skillState.selected?.skill_id === s.skill_id;
      const triggers = Array.isArray(s.triggers) ? s.triggers : [];
      return `<div class="skill-grid-card${isSelected ? ' is-selected' : ''}" data-skill-id="${__skillEsc(s.skill_id)}">
        <div class="skill-grid-card-name">${__skillEsc(s.name || s.skill_id)}</div>
        <div class="skill-grid-card-desc">${__skillEsc((s.description || '').slice(0, 120))}${(s.description || '').length > 120 ? '...' : ''}</div>
        ${triggers.length ? `<div class="skill-grid-card-triggers">${triggers.slice(0, 3).map(t => `<span class="skill-trigger">${__skillEsc(t)}</span>`).join('')}${triggers.length > 3 ? `<span class="skill-trigger">+${triggers.length - 3}</span>` : ''}</div>` : ''}
      </div>`;
    }).join('');

    gridHtml += `<div class="skill-group">
      <div class="skill-group-header">
        <span class="skill-group-icon">${__skillCatIcon(cat)}</span>
        <span class="skill-group-label">${__skillEsc(__skillCatLabel(cat))}</span>
        <span class="skill-group-count">${skills.length}</span>
      </div>
      <div class="skill-grid">${cardsHtml}</div>
    </div>`;
  }

  if (!gridHtml) {
    gridHtml = '<div class="card-sub" style="padding:20px;text-align:center">No skills match your filter.</div>';
  }

  // Detail panel (right side, smaller)
  const detailHtml = __skillRenderDetail(__skillState.selected);

  content.innerHTML = `<div class="skill-shell">
    <section class="skill-main">
      <section class="card">
        <div class="mcp-header">
          <div>
            <div class="page-title">Skills Registry</div>
            <div class="muted">Read-only catalog of available agent skills. ${extCount} external, ${totalCount - extCount} local.</div>
          </div>
          <div style="display:flex;gap:10px">
            <div class="mcp-summary-card"><span>Total</span><strong>${totalCount}</strong></div>
            <div class="mcp-summary-card"><span>Categories</span><strong>${categories.length}</strong></div>
          </div>
        </div>
      </section>
      <div class="skill-filter-bar">
        <input id="skill-search" class="input" placeholder="Filter skills by name, trigger, or description..." value="${__skillEsc(q)}">
        ${activeCat ? `<button class="btn btn-sm" id="skill-clear-cat">Clear filter: ${__skillEsc(__skillCatLabel(activeCat))}</button>` : ''}
      </div>
      <div class="skill-cat-pills">${pillsHtml}</div>
      <div class="skill-grid-area">${gridHtml}</div>
    </section>
    <aside class="skill-detail-aside">
      ${detailHtml}
    </aside>
    ${__skillState.modalContent !== null ? __skillRenderModal() : ''}
  </div>`;

  __skillBindEvents();
}

function __skillRenderDetail(skill) {
  if (!skill) {
    return `<div class="card skill-detail-empty">
      <div class="skill-detail-empty-icon">\uD83D\uDCDA</div>
      <div class="card-label">Select a Skill</div>
      <div class="card-sub">Click any skill card to see details, triggers, and source info.</div>
    </div>`;
  }
  const id = skill.skill_id || '';
  const name = skill.name || id;
  const desc = skill.description || '';
  const category = skill.category || 'uncategorized';
  const triggers = Array.isArray(skill.triggers) ? skill.triggers : [];
  const isExternal = skill.source === 'external';
  const sourcePath = skill.source_path || '';

  return `<div class="card skill-detail">
    <div class="skill-detail-top">
      <span class="skill-detail-cat-icon">${__skillCatIcon(category)}</span>
      <span class="badge ${isExternal ? 'badge-info' : 'badge-ok'}">${isExternal ? 'External' : 'Local'}</span>
    </div>
    <div class="skill-detail-name">${__skillEsc(name)}</div>
    <div class="skill-detail-id">${__skillEsc(id)}</div>
    <div class="skill-detail-desc">${__skillEsc(desc)}</div>
    <div class="skill-detail-section">
      <div class="skill-detail-section-label">Category</div>
      <div class="skill-detail-section-value">${__skillEsc(__skillCatLabel(category))}</div>
    </div>
    ${triggers.length ? `<div class="skill-detail-section">
      <div class="skill-detail-section-label">Triggers</div>
      <div class="skill-detail-triggers">${triggers.map(t => `<span class="skill-trigger">${__skillEsc(t)}</span>`).join('')}</div>
    </div>` : ''}
    ${sourcePath ? `<div class="skill-detail-section">
      <div class="skill-detail-section-label">Source</div>
      <div class="skill-detail-section-value" style="font-size:10px;word-break:break-all">${__skillEsc(sourcePath)}</div>
    </div>` : ''}
    <div class="skill-detail-actions">
      <button class="btn btn-sm btn-primary" id="skill-view-full">View Full Skill File</button>
    </div>
  </div>`;
}

function __skillRenderModal() {
  return `<div class="skill-modal-overlay" id="skill-modal-overlay">
    <div class="skill-modal">
      <div class="skill-modal-header">
        <div class="skill-modal-title">${__skillEsc(__skillState.modalTitle)}</div>
        <button class="btn btn-sm" id="skill-modal-close">\u2715 Close</button>
      </div>
      <pre class="skill-modal-content">${__skillEsc(__skillState.modalContent)}</pre>
    </div>
  </div>`;
}

function __skillBindEvents() {
  // Search
  const searchInput = document.querySelector('#skill-search');
  if (searchInput) {
    searchInput.addEventListener('input', () => {
      __skillState.query = searchInput.value || '';
      __skillRender();
    });
  }

  // Category pills
  document.querySelectorAll('[data-skill-cat]').forEach(pill => {
    pill.addEventListener('click', () => {
      const cat = pill.dataset.skillCat;
      __skillState.activeCategory = __skillState.activeCategory === cat ? null : cat;
      __skillRender();
    });
  });

  // Clear category filter
  document.querySelector('#skill-clear-cat')?.addEventListener('click', () => {
    __skillState.activeCategory = null;
    __skillRender();
  });

  // Select skill card
  document.querySelectorAll('[data-skill-id]').forEach(card => {
    card.addEventListener('click', () => {
      const id = card.dataset.skillId;
      __skillState.selected = __skillState.skills.find(s => s.skill_id === id) || null;
      __skillRender();
    });
  });

  // View full skill file
  document.querySelector('#skill-view-full')?.addEventListener('click', async () => {
    const skill = __skillState.selected;
    if (!skill) return;
    try {
      const api = window.api;
      const res = await api(`/skills/${encodeURIComponent(skill.skill_id)}/content`);
      __skillState.modalContent = res?.content || 'No content available.';
      __skillState.modalTitle = `${skill.name || skill.skill_id} — SKILL.md`;
      __skillRender();
    } catch (e) {
      __skillState.modalContent = `Failed to load skill content: ${String((e && e.message) || e)}`;
      __skillState.modalTitle = skill.name || skill.skill_id;
      __skillRender();
    }
  });

  // Close modal
  document.querySelector('#skill-modal-close')?.addEventListener('click', () => {
    __skillState.modalContent = null;
    __skillRender();
  });
  document.querySelector('#skill-modal-overlay')?.addEventListener('click', (e) => {
    if (e.target.id === 'skill-modal-overlay') {
      __skillState.modalContent = null;
      __skillRender();
    }
  });

  // Keyboard: Escape closes modal, / focuses search
  if (!__skillState._keyBound) {
    __skillState._keyBound = true;
    document.addEventListener('keydown', (ev) => {
      const page = (location.hash.replace('#/', '') || 'setup').split('/')[0];
      if (page !== 'skills') return;
      if (ev.key === 'Escape' && __skillState.modalContent !== null) {
        __skillState.modalContent = null;
        __skillRender();
      }
      if (ev.key === '/' && !ev.ctrlKey && !ev.metaKey && __skillState.modalContent === null) {
        const input = document.querySelector('#skill-search');
        if (input) { ev.preventDefault(); input.focus(); input.select?.(); }
      }
    });
  }
}
