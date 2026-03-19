'use strict';

/* ================================================================
   Skills Management Panel — AgentHALO Dashboard
   ================================================================ */

const __skillState = {
  skills: [],
  selected: null,
  editing: false,
  formData: {},
  error: '',
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

function __skillRenderCard(skill) {
  const id = skill.skill_id || '';
  const name = skill.name || id;
  const desc = skill.description || '';
  const category = skill.category || 'general';
  const triggers = Array.isArray(skill.triggers) ? skill.triggers : [];
  const isSelected = __skillState.selected?.skill_id === id;
  return `<div class="skill-card${isSelected ? ' is-selected' : ''}" data-skill-id="${__skillEsc(id)}">
    <div class="skill-card-header">
      <div class="skill-card-name">${__skillEsc(name)}</div>
      <span class="badge badge-info">${__skillEsc(category)}</span>
    </div>
    <div class="skill-card-desc">${__skillEsc(desc)}</div>
    ${triggers.length ? `<div class="skill-card-triggers">${triggers.slice(0, 3).map(t => `<span class="skill-trigger">${__skillEsc(t)}</span>`).join('')}</div>` : ''}
  </div>`;
}

function __skillRenderDetail(skill) {
  if (!skill) return '<div class="card"><div class="card-label">No skill selected</div><div class="card-sub">Select a skill or create a new one.</div></div>';
  const id = skill.skill_id || '';
  const name = skill.name || '';
  const desc = skill.description || '';
  const category = skill.category || 'general';
  const triggers = Array.isArray(skill.triggers) ? skill.triggers.join(', ') : '';
  const prompt = skill.prompt_template || '';
  const created = skill.created_at ? new Date(skill.created_at * 1000).toLocaleString() : 'N/A';
  const updated = skill.updated_at ? new Date(skill.updated_at * 1000).toLocaleString() : 'N/A';

  if (__skillState.editing) {
    const fd = __skillState.formData;
    return `<div class="card skill-detail-form">
      <div class="card-label">Edit Skill</div>
      <label class="mcp-form-field"><span class="mcp-form-label">Skill ID</span>
        <input class="input" data-skill-field="skill_id" value="${__skillEsc(fd.skill_id || '')}" ${id ? 'disabled' : ''} placeholder="my-skill-name"></label>
      <label class="mcp-form-field"><span class="mcp-form-label">Name</span>
        <input class="input" data-skill-field="name" value="${__skillEsc(fd.name || '')}" placeholder="Display Name"></label>
      <label class="mcp-form-field"><span class="mcp-form-label">Description</span>
        <input class="input" data-skill-field="description" value="${__skillEsc(fd.description || '')}" placeholder="What does this skill do?"></label>
      <label class="mcp-form-field"><span class="mcp-form-label">Category</span>
        <input class="input" data-skill-field="category" value="${__skillEsc(fd.category || '')}" placeholder="e.g., proof, translation, atp"></label>
      <label class="mcp-form-field"><span class="mcp-form-label">Triggers (comma-separated)</span>
        <input class="input" data-skill-field="triggers" value="${__skillEsc(fd.triggers || '')}" placeholder="formal proof, prove theorem"></label>
      <label class="mcp-form-field"><span class="mcp-form-label">Prompt Template</span>
        <textarea class="input" data-skill-field="prompt_template" rows="6" placeholder="The template injected when this skill is invoked">${__skillEsc(fd.prompt_template || '')}</textarea></label>
      <div class="network-form-actions">
        <button class="btn btn-primary" id="skill-save-btn">Save</button>
        <button class="btn" id="skill-cancel-btn">Cancel</button>
      </div>
      ${__skillState.error ? `<div class="networking-msg err">${__skillEsc(__skillState.error)}</div>` : ''}
    </div>`;
  }

  return `<div class="card skill-detail">
    <div class="skill-detail-header">
      <div class="card-label" style="font-family:var(--mono)">${__skillEsc(id)}</div>
      <div style="display:flex;gap:6px">
        <button class="btn btn-sm btn-primary" id="skill-edit-btn">Edit</button>
        <button class="btn btn-sm" id="skill-delete-btn">Delete</button>
      </div>
    </div>
    <div class="skill-detail-name">${__skillEsc(name)}</div>
    <div class="card-sub">${__skillEsc(desc)}</div>
    <div class="mcp-meta-grid" style="margin-top:12px">
      <div><span>Category</span><strong>${__skillEsc(category)}</strong></div>
      <div><span>Created</span><strong>${__skillEsc(created)}</strong></div>
      <div><span>Updated</span><strong>${__skillEsc(updated)}</strong></div>
      <div><span>Triggers</span><strong>${__skillEsc(triggers || 'None')}</strong></div>
    </div>
    ${prompt ? `<div style="margin-top:12px">
      <div class="card-label" style="font-size:11px">Prompt Template</div>
      <pre class="mcp-json" style="max-height:200px;overflow:auto">${__skillEsc(prompt)}</pre>
    </div>` : ''}
  </div>`;
}

window.renderSkillsPage = async function renderSkillsPage() {
  const content = document.querySelector('#content');
  const api = window.api;
  const apiPost = window.apiPost;
  if (!content || typeof api !== 'function') return;

  content.innerHTML = '<div class="loading">Loading skills...</div>';
  await __skillLoad();
  __skillRender();
};

function __skillRender() {
  const content = document.querySelector('#content');
  if (!content) return;

  const cardsHtml = __skillState.skills.length
    ? __skillState.skills.map(__skillRenderCard).join('')
    : '<div class="card-sub" style="padding:12px">No skills configured yet. Click "Add Skill" to create one.</div>';

  const detailHtml = __skillRenderDetail(__skillState.selected);

  content.innerHTML = `<div class="mcp-shell">
    <aside class="card mcp-sidebar">
      <div class="card-label">Skills Registry</div>
      <div class="card-sub">${__skillState.skills.length} skill${__skillState.skills.length === 1 ? '' : 's'} stored</div>
      <button class="btn btn-sm btn-primary" id="skill-add-btn" style="margin:8px 0;width:100%">+ Add Skill</button>
      <div class="skill-list">${cardsHtml}</div>
    </aside>
    <section class="mcp-main">
      <section class="card">
        <div class="mcp-header">
          <div><div class="page-title">Skills</div><div class="muted">Prompt templates that load into Orchestrator and Cockpit.</div></div>
          <div class="mcp-summary-card"><span>Total</span><strong>${__skillState.skills.length}</strong></div>
        </div>
      </section>
      <section class="mcp-layout">
        <div class="mcp-detail-panel">${detailHtml}</div>
      </section>
    </section>
  </div>`;

  __skillBindEvents();
}

function __skillBindEvents() {
  const apiPost = window.apiPost;

  // Select skill
  document.querySelectorAll('[data-skill-id]').forEach(card => {
    card.addEventListener('click', () => {
      const id = card.dataset.skillId;
      __skillState.selected = __skillState.skills.find(s => s.skill_id === id) || null;
      __skillState.editing = false;
      __skillState.error = '';
      __skillRender();
    });
  });

  // Add new skill
  document.querySelector('#skill-add-btn')?.addEventListener('click', () => {
    __skillState.selected = null;
    __skillState.editing = true;
    __skillState.formData = { skill_id: '', name: '', description: '', category: '', triggers: '', prompt_template: '' };
    __skillState.error = '';
    __skillRender();
  });

  // Edit existing
  document.querySelector('#skill-edit-btn')?.addEventListener('click', () => {
    const s = __skillState.selected;
    if (!s) return;
    __skillState.editing = true;
    __skillState.formData = {
      skill_id: s.skill_id || '',
      name: s.name || '',
      description: s.description || '',
      category: s.category || '',
      triggers: Array.isArray(s.triggers) ? s.triggers.join(', ') : '',
      prompt_template: s.prompt_template || '',
    };
    __skillState.error = '';
    __skillRender();
  });

  // Cancel edit
  document.querySelector('#skill-cancel-btn')?.addEventListener('click', () => {
    __skillState.editing = false;
    __skillState.error = '';
    __skillRender();
  });

  // Save
  document.querySelector('#skill-save-btn')?.addEventListener('click', async () => {
    // Collect form data
    document.querySelectorAll('[data-skill-field]').forEach(el => {
      __skillState.formData[el.dataset.skillField] = el.type === 'checkbox' ? el.checked : el.value;
    });
    const fd = __skillState.formData;
    if (!fd.skill_id) {
      __skillState.error = 'Skill ID is required';
      __skillRender();
      return;
    }
    const now = Math.floor(Date.now() / 1000);
    const existing = __skillState.skills.find(s => s.skill_id === fd.skill_id);
    const payload = {
      skill_id: fd.skill_id,
      name: fd.name || fd.skill_id,
      description: fd.description || '',
      category: fd.category || 'general',
      triggers: fd.triggers ? fd.triggers.split(',').map(t => t.trim()).filter(Boolean) : [],
      prompt_template: fd.prompt_template || '',
      created_at: existing?.created_at || now,
      updated_at: now,
    };
    try {
      await apiPost('/skills', payload);
      __skillState.editing = false;
      __skillState.error = '';
      await __skillLoad();
      __skillState.selected = __skillState.skills.find(s => s.skill_id === fd.skill_id) || null;
      __skillRender();
    } catch (e) {
      __skillState.error = String((e && e.message) || e || 'save failed');
      __skillRender();
    }
  });

  // Delete
  document.querySelector('#skill-delete-btn')?.addEventListener('click', async () => {
    const s = __skillState.selected;
    if (!s) return;
    if (!confirm(`Delete skill "${s.name || s.skill_id}"?`)) return;
    try {
      await fetch(`/api/skills/${encodeURIComponent(s.skill_id)}`, { method: 'DELETE' });
      __skillState.selected = null;
      __skillState.editing = false;
      await __skillLoad();
      __skillRender();
    } catch (e) {
      __skillState.error = String((e && e.message) || e || 'delete failed');
      __skillRender();
    }
  });

  // Form field bindings
  document.querySelectorAll('[data-skill-field]').forEach(el => {
    el.addEventListener('input', () => {
      __skillState.formData[el.dataset.skillField] = el.type === 'checkbox' ? el.checked : el.value;
    });
  });
}
