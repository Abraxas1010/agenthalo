/**
 * Proof Forge — frontend logic.
 * Vanilla JS, no framework. Matches cockpit/codeguard/gates patterns.
 */

'use strict';

const API = '/api/forge';
let currentMode = 'latex';
let currentPipelineId = null;
let pipelineWs = null;
let outputEditor = null;      // Monaco editor for output
let inputEditor = null;        // Monaco editor for LaTeX/Lean input
let monacoReady = false;
let fixIteration = 0;
let lastVerification = null;   // last verification result
let currentLeanCode = '';      // current output code

// ─── Mode System ──────────────────────────────────────────────────────────

const MODES = {
  image:   { label: 'Image',   phases: ['ocr','latex_parse','formalize','explain'], inputType: 'drop' },
  latex:   { label: 'LaTeX',   phases: ['latex_parse','formalize','explain'], inputType: 'editor-latex' },
  nl:      { label: 'Text',    phases: ['formalize','explain'], inputType: 'textarea' },
  diagram: { label: 'Diagram', phases: ['ocr','formalize','explain'], inputType: 'drop' },
  lean:    { label: 'Lean',    phases: [], inputType: 'editor-lean' },
};

// ─── Init ─────────────────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
  setupModeTabs();
  setupButtons();
  setupModals();
  loadTemplates();
  initMonaco();
  switchMode('latex');
});

function initMonaco() {
  require.config({ paths: { vs: 'vendor/monaco' } });
  require(['vs/editor/editor.main'], function() {
    monaco.editor.defineTheme('forge-dark', {
      base: 'vs-dark',
      inherit: true,
      rules: [
        { token: 'comment', foreground: '4ca43a', fontStyle: 'italic' },
        { token: 'keyword', foreground: 'ff6a00' },
        { token: 'string',  foreground: '00ff41' },
        { token: 'number',  foreground: 'ff9f2a' },
      ],
      colors: {
        'editor.background': '#080c05',
        'editor.foreground': '#35ff3e',
        'editor.lineHighlightBackground': '#142210',
        'editorCursor.foreground': '#ff6a00',
        'editorLineNumber.foreground': '#4ca43a',
      }
    });
    monacoReady = true;
    // Initialize output editor
    const outputEl = document.getElementById('output-editor');
    outputEl.innerHTML = '';
    outputEditor = monaco.editor.create(outputEl, {
      value: '',
      language: 'lean4',
      theme: 'forge-dark',
      readOnly: false,
      minimap: { enabled: false },
      fontSize: 13,
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      scrollBeyondLastLine: false,
      automaticLayout: true,
      wordWrap: 'on',
    });
    outputEditor.onDidChangeModelContent(() => {
      currentLeanCode = outputEditor.getValue();
      updateOutputButtons();
    });
    // Re-render input if mode needs monaco
    switchMode(currentMode);
  });
}

// ─── Mode Tabs ────────────────────────────────────────────────────────────

function setupModeTabs() {
  document.querySelectorAll('.mode-tab').forEach(tab => {
    tab.addEventListener('click', () => switchMode(tab.dataset.mode));
  });
}

function switchMode(mode) {
  currentMode = mode;
  document.querySelectorAll('.mode-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.mode === mode);
  });
  renderInput(mode);
}

function renderInput(mode) {
  const area = document.getElementById('input-area');
  const cfg = MODES[mode];

  // Destroy existing input editor
  if (inputEditor) {
    inputEditor.dispose();
    inputEditor = null;
  }

  if (cfg.inputType === 'drop') {
    area.innerHTML =
      '<div class="drop-zone" id="drop-zone">' +
      '<input type="file" id="file-input" accept="image/*"/>' +
      '<div>Drop an image here or click to browse</div>' +
      '<div style="font-size:10px;color:var(--f-off)">PNG, JPG, WEBP</div>' +
      '</div>';
    setupDropZone();
  } else if (cfg.inputType === 'textarea') {
    area.innerHTML = '<textarea class="forge-textarea" id="nl-input" placeholder="Describe the mathematical statement in natural language..."></textarea>';
  } else if (cfg.inputType === 'editor-latex') {
    area.innerHTML = '<div class="input-editor-container" id="input-editor-el"></div>' +
      '<div class="latex-preview" id="latex-preview"></div>';
    if (monacoReady) {
      inputEditor = monaco.editor.create(document.getElementById('input-editor-el'), {
        value: '',
        language: 'latex',
        theme: 'forge-dark',
        minimap: { enabled: false },
        fontSize: 13,
        fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
        scrollBeyondLastLine: false,
        automaticLayout: true,
        wordWrap: 'on',
        lineNumbers: 'off',
      });
      inputEditor.onDidChangeModelContent(() => updateLatexPreview());
    }
  } else if (cfg.inputType === 'editor-lean') {
    area.innerHTML = '<div class="input-editor-container" id="input-editor-el"></div>';
    if (monacoReady) {
      inputEditor = monaco.editor.create(document.getElementById('input-editor-el'), {
        value: '',
        language: 'lean4',
        theme: 'forge-dark',
        minimap: { enabled: false },
        fontSize: 13,
        fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
        scrollBeyondLastLine: false,
        automaticLayout: true,
        wordWrap: 'on',
      });
    }
  }
}

function updateLatexPreview() {
  if (!inputEditor) return;
  const preview = document.getElementById('latex-preview');
  if (!preview) return;
  const text = inputEditor.getValue().trim();
  if (!text) { preview.innerHTML = ''; return; }
  try {
    preview.innerHTML = katex.renderToString(text, {
      throwOnError: false,
      displayMode: true,
    });
  } catch (e) {
    preview.innerHTML = '<span class="katex-error">' + escHtml(e.message) + '</span>';
  }
}

// ─── Drop Zone ────────────────────────────────────────────────────────────

function setupDropZone() {
  const zone = document.getElementById('drop-zone');
  const input = document.getElementById('file-input');
  if (!zone || !input) return;

  zone.addEventListener('click', () => input.click());
  zone.addEventListener('dragover', (e) => { e.preventDefault(); zone.classList.add('drag-over'); });
  zone.addEventListener('dragleave', () => zone.classList.remove('drag-over'));
  zone.addEventListener('drop', (e) => {
    e.preventDefault();
    zone.classList.remove('drag-over');
    if (e.dataTransfer.files.length) handleFile(e.dataTransfer.files[0]);
  });
  input.addEventListener('change', () => {
    if (input.files.length) handleFile(input.files[0]);
  });
}

function handleFile(file) {
  const zone = document.getElementById('drop-zone');
  if (!zone) return;
  const reader = new FileReader();
  reader.onload = () => {
    zone.innerHTML = '<img class="preview-img" src="' + reader.result + '"/>';
    zone._base64 = reader.result;
  };
  reader.readAsDataURL(file);
}

// ─── Buttons ──────────────────────────────────────────────────────────────

function setupButtons() {
  document.getElementById('btn-submit').addEventListener('click', handleSubmit);
  document.getElementById('btn-verify').addEventListener('click', handleVerify);
  document.getElementById('btn-fix').addEventListener('click', handleFix);
  document.getElementById('btn-prove').addEventListener('click', handleProve);
  document.getElementById('btn-save').addEventListener('click', () => {
    document.getElementById('save-modal').style.display = 'flex';
  });
  document.getElementById('btn-export').addEventListener('click', handleExport);
}

function updateOutputButtons() {
  const hasCode = currentLeanCode.trim().length > 0;
  document.getElementById('btn-verify').disabled = !hasCode;
  document.getElementById('btn-save').disabled = !hasCode;
  document.getElementById('btn-export').disabled = !hasCode;
}

// ─── Submit ───────────────────────────────────────────────────────────────

async function handleSubmit() {
  const content = getInputContent();
  if (!content) return;

  setStatus('Submitting...', 'running');
  fixIteration = 0;

  try {
    const res = await fetch(`${API}/submit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ mode: currentMode, content }),
    });
    const data = await res.json();
    if (data.error) {
      setStatus('Error: ' + data.error, 'failed');
      return;
    }

    currentPipelineId = data.pipeline_id;

    if (data.status === 'completed') {
      // Lean mode: direct output
      setOutput(data.lean_code || content, data.explanation || '');
      setStatus('Ready for verification', 'completed');
      renderPipelineGraph([]);
    } else {
      // Start pipeline: connect WS, render graph
      const phases = MODES[currentMode].phases.map(p => ({ name: p, status: 'waiting' }));
      renderPipelineGraph(phases);
      renderTaskBoard([{ id: 'main', subject: 'Formalize ' + currentMode, status: 'pending' }]);
      connectPipelineWs(data.pipeline_id);
      setStatus('Pipeline running...', 'running');
    }
  } catch (err) {
    setStatus('Submit failed: ' + err.message, 'failed');
  }
}

function getInputContent() {
  const cfg = MODES[currentMode];
  if (cfg.inputType === 'drop') {
    const zone = document.getElementById('drop-zone');
    return zone && zone._base64 ? zone._base64 : null;
  } else if (cfg.inputType === 'textarea') {
    return document.getElementById('nl-input')?.value.trim() || null;
  } else if (inputEditor) {
    return inputEditor.getValue().trim() || null;
  }
  return null;
}

// ─── Pipeline WebSocket ───────────────────────────────────────────────────

function connectPipelineWs(pipelineId) {
  if (pipelineWs) pipelineWs.close();
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  pipelineWs = new WebSocket(`${proto}//${location.host}${API}/pipeline/${pipelineId}/ws`);
  pipelineWs.onmessage = (evt) => {
    try {
      const msg = JSON.parse(evt.data);
      if (msg.type === 'pipeline_update') {
        renderPipelineGraph(msg.phases || []);
        renderTaskBoard(msg.tasks || []);
        if (msg.status === 'completed') {
          setStatus('Pipeline completed', 'completed');
          fetchPipelineOutput(pipelineId);
        } else if (msg.status === 'failed') {
          setStatus('Pipeline failed', 'failed');
        }
      } else if (msg.type === 'output_update') {
        setOutput(msg.lean_code || '', msg.explanation || '');
      }
    } catch (e) { /* ignore parse errors */ }
  };
  pipelineWs.onerror = () => setStatus('WebSocket error', 'failed');
}

async function fetchPipelineOutput(pipelineId) {
  try {
    const res = await fetch(`${API}/pipeline/${pipelineId}/output`);
    const data = await res.json();
    if (data.lean_code) {
      setOutput(data.lean_code, data.explanation || '');
    }
  } catch (e) { /* ignore */ }
}

// ─── Pipeline Graph (SVG) ─────────────────────────────────────────────────

function renderPipelineGraph(phases) {
  const container = document.getElementById('pipeline-graph');
  if (!phases || phases.length === 0) {
    container.innerHTML = '<div class="pipeline-idle">No pipeline phases</div>';
    return;
  }

  const svgNS = 'http://www.w3.org/2000/svg';
  const width = container.clientWidth || 500;
  const height = 80;
  const nodeR = 16;
  const padding = 50;
  const spacing = (width - 2 * padding) / Math.max(phases.length - 1, 1);

  const svg = document.createElementNS(svgNS, 'svg');
  svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
  svg.setAttribute('width', '100%');
  svg.setAttribute('height', height);

  // Draw edges
  for (let i = 0; i < phases.length - 1; i++) {
    const x1 = padding + i * spacing + nodeR;
    const x2 = padding + (i + 1) * spacing - nodeR;
    const y = height / 2;
    const line = document.createElementNS(svgNS, 'line');
    line.setAttribute('x1', x1);
    line.setAttribute('y1', y);
    line.setAttribute('x2', x2);
    line.setAttribute('y2', y);
    const active = phases[i].status === 'complete' || phases[i].status === 'active';
    line.setAttribute('class', 'pipeline-edge' + (active ? ' active' : ''));
    svg.appendChild(line);
  }

  // Draw nodes
  phases.forEach((phase, i) => {
    const cx = padding + i * spacing;
    const cy = height / 2;
    const g = document.createElementNS(svgNS, 'g');
    g.setAttribute('class', 'pipeline-node' + (phase.status === 'active' ? ' node-active' : ''));

    const circle = document.createElementNS(svgNS, 'circle');
    circle.setAttribute('cx', cx);
    circle.setAttribute('cy', cy);
    circle.setAttribute('r', nodeR);
    circle.setAttribute('class', 'pipeline-node-circle');

    const colors = {
      waiting: { fill: '#1a1a1a', stroke: '#555' },
      active: { fill: '#0a1a2a', stroke: '#4488ff' },
      complete: { fill: '#0a200a', stroke: '#35ff3e' },
      failed: { fill: '#200a0a', stroke: '#ff4444' },
      skipped: { fill: '#1a1a1a', stroke: '#333' },
    };
    const c = colors[phase.status] || colors.waiting;
    circle.setAttribute('fill', c.fill);
    circle.setAttribute('stroke', c.stroke);
    g.appendChild(circle);

    // Check/X mark
    if (phase.status === 'complete') {
      const check = document.createElementNS(svgNS, 'text');
      check.setAttribute('x', cx);
      check.setAttribute('y', cy + 4);
      check.setAttribute('text-anchor', 'middle');
      check.setAttribute('fill', '#35ff3e');
      check.setAttribute('font-size', '14');
      check.textContent = '\u2713';
      g.appendChild(check);
    } else if (phase.status === 'failed') {
      const x = document.createElementNS(svgNS, 'text');
      x.setAttribute('x', cx);
      x.setAttribute('y', cy + 4);
      x.setAttribute('text-anchor', 'middle');
      x.setAttribute('fill', '#ff4444');
      x.setAttribute('font-size', '14');
      x.textContent = '\u2717';
      g.appendChild(x);
    }

    // Label below
    const label = document.createElementNS(svgNS, 'text');
    label.setAttribute('x', cx);
    label.setAttribute('y', cy + nodeR + 14);
    label.setAttribute('class', 'pipeline-node-label');
    label.textContent = phase.name;
    g.appendChild(label);

    svg.appendChild(g);
  });

  container.innerHTML = '';
  container.appendChild(svg);
}

// ─── Task Board ───────────────────────────────────────────────────────────

function renderTaskBoard(tasks) {
  document.querySelectorAll('.kanban-cards').forEach(col => col.innerHTML = '');
  if (!tasks) return;
  tasks.forEach(task => {
    const col = document.querySelector(`.kanban-col[data-status="${task.status}"] .kanban-cards`);
    if (!col) return;
    const card = document.createElement('div');
    card.className = 'kanban-card status-' + task.status;
    card.innerHTML =
      '<div class="card-subject">' + escHtml(task.subject || task.id) + '</div>' +
      (task.agent_id ? '<div class="card-agent">' + escHtml(task.agent_id) + '</div>' : '');
    col.appendChild(card);
  });
}

// ─── Output ───────────────────────────────────────────────────────────────

function setOutput(leanCode, explanation) {
  currentLeanCode = leanCode;
  if (outputEditor) {
    outputEditor.setValue(leanCode);
    // Clear previous markers
    monaco.editor.setModelMarkers(outputEditor.getModel(), 'forge-verify', []);
  }
  document.querySelector('.explanation-content').innerHTML = escHtml(explanation);
  updateOutputButtons();
}

// ─── Verify ───────────────────────────────────────────────────────────────

async function handleVerify() {
  const code = outputEditor ? outputEditor.getValue() : currentLeanCode;
  if (!code.trim()) return;

  setVerifyStatus('running', 'Verifying...');
  document.getElementById('btn-fix').style.display = 'none';
  document.getElementById('btn-prove').style.display = 'none';

  try {
    const res = await fetch(`${API}/verify`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ lean_code: code }),
    });
    const data = await res.json();
    if (data.error) {
      setVerifyStatus('failed', data.error);
      return;
    }

    // Poll for result
    pollVerification(data.verification_id);
  } catch (err) {
    setVerifyStatus('failed', err.message);
  }
}

async function pollVerification(vid) {
  const maxPolls = 60;
  for (let i = 0; i < maxPolls; i++) {
    await sleep(2000);
    try {
      const res = await fetch(`${API}/verify/${vid}/status`);
      const data = await res.json();
      if (data.status === 'running') continue;

      lastVerification = data;
      setVerifyStatus(data.status, statusMessage(data));

      // Set error markers in Monaco
      if (outputEditor && data.errors && data.errors.length > 0) {
        const model = outputEditor.getModel();
        const markers = data.errors.map(e => ({
          severity: monaco.MarkerSeverity.Error,
          startLineNumber: e.line || 1,
          startColumn: e.column || 1,
          endLineNumber: e.line || 1,
          endColumn: model.getLineMaxColumn(e.line || 1),
          message: e.message || 'Error',
        }));
        monaco.editor.setModelMarkers(model, 'forge-verify', markers);
      }

      // Show sorry highlights
      if (outputEditor && data.sorry_locations && data.sorry_locations.length > 0) {
        const decorations = data.sorry_locations.map(s => ({
          range: new monaco.Range(s.line, 1, s.line, 1),
          options: {
            isWholeLine: true,
            className: 'sorry-highlight',
            glyphMarginClassName: 'sorry-glyph',
          }
        }));
        outputEditor.deltaDecorations([], decorations);
      }

      // Show fix/prove buttons based on status
      if (data.status === 'failed') {
        document.getElementById('btn-fix').style.display = '';
        document.getElementById('btn-fix').disabled = false;
      }
      if (data.status === 'partial') {
        document.getElementById('btn-prove').style.display = '';
        document.getElementById('btn-prove').disabled = false;
      }
      return;
    } catch (e) { /* continue polling */ }
  }
  setVerifyStatus('failed', 'Verification timed out');
}

function statusMessage(data) {
  if (data.status === 'verified') return 'Verified — no errors, no sorry';
  if (data.status === 'partial') return 'Partial — ' + (data.sorry_locations?.length || 0) + ' sorry remaining';
  if (data.status === 'failed') return 'Failed — ' + (data.errors?.length || 0) + ' error(s)';
  return data.status;
}

function setVerifyStatus(status, msg) {
  const el = document.getElementById('verify-status');
  el.className = 'verify-status ' + status;
  el.textContent = msg;
  setStatus(msg, status);
}

// ─── Fix ──────────────────────────────────────────────────────────────────

async function handleFix() {
  const code = outputEditor ? outputEditor.getValue() : currentLeanCode;
  const errors = lastVerification?.errors?.map(e => e.message) || [];
  if (!code.trim() || errors.length === 0) return;

  setVerifyStatus('running', 'Fixing (iteration ' + (fixIteration + 1) + ')...');
  document.getElementById('btn-fix').disabled = true;

  try {
    const res = await fetch(`${API}/fix`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ lean_code: code, errors, iteration: fixIteration }),
    });
    const data = await res.json();

    if (data.status === 'exhausted') {
      setVerifyStatus('failed', data.message);
      document.getElementById('forge-fix-count').textContent = 'Fix iterations exhausted';
      return;
    }

    fixIteration = data.iteration || fixIteration + 1;
    document.getElementById('forge-fix-count').textContent = 'Fix iteration: ' + fixIteration + '/5';

    if (data.lean_code) {
      setOutput(data.lean_code, 'AI-proposed fix (iteration ' + fixIteration + '). Click Verify to check.');
      setVerifyStatus('', 'Fix applied — verify to check');
    }
  } catch (err) {
    setVerifyStatus('failed', err.message);
  } finally {
    document.getElementById('btn-fix').disabled = false;
  }
}

// ─── Prove ────────────────────────────────────────────────────────────────

async function handleProve() {
  const code = outputEditor ? outputEditor.getValue() : currentLeanCode;
  const sorrys = lastVerification?.sorry_locations || [];
  if (!code.trim() || sorrys.length === 0) return;

  setVerifyStatus('running', 'Proving ' + sorrys.length + ' sorry(s)...');
  document.getElementById('btn-prove').disabled = true;

  try {
    const res = await fetch(`${API}/prove`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ lean_code: code, sorry_locations: sorrys }),
    });
    const data = await res.json();

    if (data.lean_code) {
      setOutput(data.lean_code, 'Proof search complete. ' + (data.remaining_sorrys || 0) + ' sorry remaining.');
    }
    if (data.status === 'proved') {
      setVerifyStatus('verified', 'All sorrys proved! Click Verify to confirm.');
    } else {
      setVerifyStatus('partial', data.remaining_sorrys + ' sorry(s) remain');
    }
  } catch (err) {
    setVerifyStatus('failed', err.message);
  } finally {
    document.getElementById('btn-prove').disabled = false;
  }
}

// ─── Save ─────────────────────────────────────────────────────────────────

function setupModals() {
  // History
  document.getElementById('btn-history').addEventListener('click', openHistory);
  document.getElementById('btn-history-close').addEventListener('click', () => {
    document.getElementById('history-modal').style.display = 'none';
  });
  document.getElementById('history-modal').addEventListener('click', (e) => {
    if (e.target.id === 'history-modal') e.target.style.display = 'none';
  });

  // Save
  document.getElementById('btn-save-confirm').addEventListener('click', handleSaveConfirm);
  document.getElementById('btn-save-cancel').addEventListener('click', () => {
    document.getElementById('save-modal').style.display = 'none';
  });
  document.getElementById('save-modal').addEventListener('click', (e) => {
    if (e.target.id === 'save-modal') e.target.style.display = 'none';
  });
}

async function handleSaveConfirm() {
  const filePath = document.getElementById('save-path').value.trim();
  const resultEl = document.getElementById('save-result');
  if (!filePath) {
    resultEl.textContent = 'File path is required';
    resultEl.className = 'modal-result error';
    return;
  }

  const code = outputEditor ? outputEditor.getValue() : currentLeanCode;
  resultEl.textContent = 'Saving...';
  resultEl.className = 'modal-result';

  try {
    const res = await fetch(`${API}/save`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ lean_code: code, file_path: filePath }),
    });
    const data = await res.json();
    if (data.ok) {
      resultEl.textContent = 'Saved: ' + data.file_path;
      resultEl.className = 'modal-result ok';
      setTimeout(() => {
        document.getElementById('save-modal').style.display = 'none';
      }, 1500);
    } else {
      resultEl.textContent = data.error || 'Save failed';
      resultEl.className = 'modal-result error';
    }
  } catch (err) {
    resultEl.textContent = err.message;
    resultEl.className = 'modal-result error';
  }
}

// ─── Export ───────────────────────────────────────────────────────────────

function handleExport() {
  const code = outputEditor ? outputEditor.getValue() : currentLeanCode;
  if (!code.trim()) return;

  const blob = new Blob([code], { type: 'text/plain' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'forge_output.lean';
  a.click();
  URL.revokeObjectURL(url);
}

// ─── History ──────────────────────────────────────────────────────────────

async function openHistory() {
  document.getElementById('history-modal').style.display = 'flex';
  const list = document.getElementById('history-list');
  list.innerHTML = '<div style="color:var(--f-text-dim)">Loading...</div>';

  try {
    const res = await fetch(`${API}/history`);
    const data = await res.json();
    const entries = data.history || [];
    if (entries.length === 0) {
      list.innerHTML = '<div style="color:var(--f-text-dim)">No history yet</div>';
      return;
    }
    list.innerHTML = '';
    entries.forEach(entry => {
      const item = document.createElement('div');
      item.className = 'history-item';
      item.innerHTML =
        '<span class="hi-mode">' + escHtml(entry.mode) + '</span>' +
        '<span class="hi-preview">' + escHtml(entry.output_preview || entry.input_preview) + '</span>' +
        '<span class="hi-time">' + timeAgo(entry.timestamp) + '</span>';
      list.appendChild(item);
    });
  } catch (err) {
    list.innerHTML = '<div style="color:var(--f-error)">' + escHtml(err.message) + '</div>';
  }
}

// ─── Templates ────────────────────────────────────────────────────────────

async function loadTemplates() {
  try {
    const res = await fetch(`${API}/templates`);
    const data = await res.json();
    const select = document.getElementById('template-select');
    (data.templates || []).forEach(t => {
      const opt = document.createElement('option');
      opt.value = JSON.stringify(t);
      opt.textContent = t.name;
      select.appendChild(opt);
    });
    select.addEventListener('change', () => {
      if (!select.value) return;
      const tpl = JSON.parse(select.value);
      // Switch to the template's mode
      switchMode(tpl.mode);
      // Fill content
      setTimeout(() => {
        if (inputEditor) {
          inputEditor.setValue(tpl.content || '');
        } else {
          const textarea = document.getElementById('nl-input');
          if (textarea) textarea.value = tpl.content || '';
        }
      }, 100);
      select.value = '';
    });
  } catch (e) { /* ignore */ }
}

// ─── Status ───────────────────────────────────────────────────────────────

function setStatus(msg, level) {
  document.getElementById('forge-status').textContent = 'Status: ' + msg;
}

// ─── Helpers ──────────────────────────────────────────────────────────────

function escHtml(str) {
  const d = document.createElement('div');
  d.textContent = str || '';
  return d.innerHTML;
}

function timeAgo(unixSecs) {
  const now = Math.floor(Date.now() / 1000);
  const diff = now - unixSecs;
  if (diff < 60) return diff + 's ago';
  if (diff < 3600) return Math.floor(diff / 60) + 'm ago';
  if (diff < 86400) return Math.floor(diff / 3600) + 'h ago';
  return Math.floor(diff / 86400) + 'd ago';
}

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }
