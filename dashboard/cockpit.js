/* AgentHALO Cockpit */
'use strict';

(function() {
  const TAB_STATES = {
    STARTING:  { icon: '⟳', color: 'var(--blue)', class: 'tab-starting', anim: 'spin' },
    ACTIVE:    { icon: '●', color: 'var(--green)', class: 'tab-active', anim: 'pulse' },
    WAITING:   { icon: '◐', color: 'var(--amber)', class: 'tab-waiting', anim: 'blink' },
    COMPLETED: { icon: '✓', color: 'var(--green)', class: 'tab-completed', anim: 'none' },
    ERROR:     { icon: '✗', color: 'var(--red)', class: 'tab-error', anim: 'none' },
    IDLE:      { icon: '○', color: 'var(--text-dim)', class: 'tab-idle', anim: 'none' },
  };

  const LAYOUTS = {
    '1':  [{ x: 0, y: 0, w: 1, h: 1 }],
    '2h': [{ x: 0, y: 0, w: 0.5, h: 1 }, { x: 0.5, y: 0, w: 0.5, h: 1 }],
    '2v': [{ x: 0, y: 0, w: 1, h: 0.5 }, { x: 0, y: 0.5, w: 1, h: 0.5 }],
    '4':  [{ x: 0, y: 0, w: 0.5, h: 0.5 }, { x: 0.5, y: 0, w: 0.5, h: 0.5 },
           { x: 0, y: 0.5, w: 0.5, h: 0.5 }, { x: 0.5, y: 0.5, w: 0.5, h: 0.5 }],
    '3L': [{ x: 0, y: 0, w: 0.6, h: 1 },
           { x: 0.6, y: 0, w: 0.4, h: 0.5 }, { x: 0.6, y: 0.5, w: 0.4, h: 0.5 }],
    '3T': [{ x: 0, y: 0, w: 1, h: 0.6 },
           { x: 0, y: 0.6, w: 0.5, h: 0.4 }, { x: 0.5, y: 0.6, w: 0.5, h: 0.4 }],
    '6':  [{ x: 0, y: 0, w: 1/3, h: 0.5 }, { x: 1/3, y: 0, w: 1/3, h: 0.5 }, { x: 2/3, y: 0, w: 1/3, h: 0.5 },
           { x: 0, y: 0.5, w: 1/3, h: 0.5 }, { x: 1/3, y: 0.5, w: 1/3, h: 0.5 }, { x: 2/3, y: 0.5, w: 1/3, h: 0.5 }],
  };

  class CockpitPanel {
    constructor(id, type, title, manager) {
      this.id = id;
      this.type = type;
      this.title = title || id;
      this.manager = manager || null;
      this.ws = null;
      this.term = null;
      this.fitAddon = null;
      this.resizeObs = null;
      this.eventSource = null;
      this.logBuffer = '';
      this.customSlot = null;

      this.el = document.createElement('div');
      this.el.className = 'cockpit-panel';
      this.el.dataset.panelId = id;

      const header = document.createElement('div');
      header.className = 'cockpit-panel-header';
      header.innerHTML = `
        <div class="cockpit-panel-title">
          <span>▣</span>
          <span class="title-label">${escapeHtml(this.title)}</span>
          <span class="title-status" id="panel-status-${escapeHtml(id)}">active</span>
        </div>
        <div class="cockpit-panel-actions">
          <button type="button" data-action="maximize" title="Maximize">□</button>
          <button type="button" data-action="close" title="Close">×</button>
        </div>`;
      this.body = document.createElement('div');
      this.body.className = 'cockpit-panel-body';

      this.el.appendChild(header);
      this.el.appendChild(this.body);

      header.addEventListener('dblclick', () => this.el.classList.toggle('maximized'));
      header.querySelector('[data-action="maximize"]').addEventListener('click', () => {
        this.el.classList.toggle('maximized');
      });

      this.el.addEventListener('contextmenu', (ev) => {
        ev.preventDefault();
        showContextMenu(ev.clientX, ev.clientY, [
          { label: 'Copy', onClick: () => this.copySelection() },
          { label: 'Paste', onClick: () => this.pasteClipboard() },
          { label: 'Clear Terminal', onClick: () => this.clearTerminal() },
          { label: 'Export Log', onClick: () => this.exportLog() },
          { label: 'Reconnect WS', onClick: () => this.reconnect() },
        ]);
      });

      this.installResizeHandles();
    }

    attachTerminal(sessionId, wsUrl, onStatus) {
      this.sessionId = sessionId;
      this.wsUrl = wsUrl;

      if (!window.Terminal || !window.FitAddon) {
        this.body.innerHTML = '<pre style="padding:10px;color:#ff3030">xterm.js not loaded.</pre>';
        return;
      }

      this.term = new window.Terminal({
        cursorBlink: true,
        convertEol: true,
        fontFamily: "'Share Tech Mono','Courier New',monospace",
        theme: {
          background: '#0a0a0a',
          foreground: '#00ff41',
          cursor: '#ffb830',
          selectionBackground: 'rgba(255, 184, 48, 0.25)',
        },
      });

      this.fitAddon = new window.FitAddon.FitAddon();
      this.term.loadAddon(this.fitAddon);
      if (window.WebglAddon && window.WebglAddon.WebglAddon) {
        try {
          this.term.loadAddon(new window.WebglAddon.WebglAddon());
        } catch (_e) {
          // no-op fallback
        }
      }

      this.term.open(this.body);
      setTimeout(() => this.fit(), 0);

      this.term.onData((data) => {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
          this.ws.send(data);
        }
      });

      this.resizeObs = new ResizeObserver(() => this.fit());
      this.resizeObs.observe(this.body);

      this.connect(onStatus);
    }

    attachIframe(url) {
      this.body.innerHTML = '';
      const frame = document.createElement('iframe');
      frame.src = url;
      frame.loading = 'lazy';
      this.body.appendChild(frame);
    }

    attachLogStream() {
      this.body.innerHTML = '<pre class="cockpit-log-stream"></pre>';
      const out = this.body.querySelector('.cockpit-log-stream');
      const writeLine = (label, payload) => {
        const ts = new Date().toLocaleTimeString();
        const line = `[${ts}] ${label} ${payload || ''}\n`;
        this.logBuffer += line;
        out.textContent += line;
        out.scrollTop = out.scrollHeight;
      };

      writeLine('log', 'connected');
      try {
        this.eventSource = new EventSource('/events');
        this.eventSource.addEventListener('session_update', (ev) => writeLine('session_update', ev.data));
        this.eventSource.addEventListener('heartbeat', () => writeLine('heartbeat'));
        this.eventSource.onerror = () => writeLine('error', 'event stream disconnected');
      } catch (e) {
        writeLine('error', String(e && e.message ? e.message : e));
      }
    }

    attachMetrics() {
      this.body.innerHTML = `
        <div class="cockpit-metrics">
          <div class="metric-row"><span>Sessions</span><span id="metric-sessions-${escapeHtml(this.id)}">0</span></div>
          <div class="metric-row"><span>Input Tokens</span><span id="metric-in-${escapeHtml(this.id)}">0</span></div>
          <div class="metric-row"><span>Output Tokens</span><span id="metric-out-${escapeHtml(this.id)}">0</span></div>
          <div class="metric-row"><span>Estimated Cost</span><span id="metric-cost-${escapeHtml(this.id)}">$0.00</span></div>
        </div>
      `;
    }

    updateMetrics(rows) {
      if (this.type !== 'metrics') return;
      const list = Array.isArray(rows) ? rows : [];
      const sessions = list.length;
      const input = list.reduce((acc, s) => acc + Number(s.estimated_input_tokens || 0), 0);
      const output = list.reduce((acc, s) => acc + Number(s.estimated_output_tokens || 0), 0);
      const cost = list.reduce((acc, s) => acc + Number(s.estimated_cost_usd || 0), 0);
      const setText = (id, text) => {
        const el = this.body.querySelector(id);
        if (el) el.textContent = text;
      };
      setText(`#metric-sessions-${CSS.escape(this.id)}`, String(sessions));
      setText(`#metric-in-${CSS.escape(this.id)}`, String(input));
      setText(`#metric-out-${CSS.escape(this.id)}`, String(output));
      setText(`#metric-cost-${CSS.escape(this.id)}`, formatUsd(cost));
    }

    installResizeHandles() {
      ['e', 's', 'se'].forEach((dir) => {
        const handle = document.createElement('div');
        handle.className = `cockpit-resize-handle ${dir}`;
        handle.dataset.dir = dir;
        handle.addEventListener('mousedown', (ev) => this.beginResize(ev, dir));
        this.el.appendChild(handle);
      });
    }

    beginResize(ev, dir) {
      if (window.matchMedia('(max-width: 768px)').matches) return;
      ev.preventDefault();
      ev.stopPropagation();
      const grid = this.el.parentElement;
      if (!grid) return;
      const startRect = this.el.getBoundingClientRect();
      const gridRect = grid.getBoundingClientRect();
      const minW = 220;
      const minH = 160;

      const onMove = (moveEv) => {
        let newLeft = startRect.left;
        let newTop = startRect.top;
        let newWidth = startRect.width;
        let newHeight = startRect.height;

        if (dir.includes('e')) {
          newWidth = Math.max(minW, startRect.width + (moveEv.clientX - startRect.right));
        }
        if (dir.includes('s')) {
          newHeight = Math.max(minH, startRect.height + (moveEv.clientY - startRect.bottom));
        }

        const maxWidth = gridRect.right - newLeft;
        const maxHeight = gridRect.bottom - newTop;
        newWidth = Math.min(newWidth, maxWidth);
        newHeight = Math.min(newHeight, maxHeight);

        const slot = {
          x: clamp((newLeft - gridRect.left) / gridRect.width, 0, 0.95),
          y: clamp((newTop - gridRect.top) / gridRect.height, 0, 0.95),
          w: clamp(newWidth / gridRect.width, 0.05, 1),
          h: clamp(newHeight / gridRect.height, 0.05, 1),
        };
        this.customSlot = slot;
        placePanel(this.el, slot);
        this.fit();
      };

      const onUp = () => {
        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
      };

      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp);
    }

    connect(onStatus) {
      if (!this.wsUrl) return;
      if (this.ws) {
        try { this.ws.close(); } catch (_e) {}
      }

      const proto = location.protocol === 'https:' ? 'wss://' : 'ws://';
      const wsFull = this.wsUrl.startsWith('ws://') || this.wsUrl.startsWith('wss://')
        ? this.wsUrl
        : `${proto}${location.host}${this.wsUrl}`;

      this.ws = new WebSocket(wsFull);
      this.ws.binaryType = 'arraybuffer';
      this.ws.onopen = () => this.fit();
      this.ws.onmessage = (ev) => {
        if (typeof ev.data === 'string') {
          try {
            const msg = JSON.parse(ev.data);
            if (msg.type === 'status' && onStatus) {
              onStatus(msg);
            }
          } catch (_e) {}
          return;
        }

        if (this.term) {
          const text = new TextDecoder().decode(new Uint8Array(ev.data));
          this.logBuffer += text;
          this.term.write(text);
        }
      };
      this.ws.onclose = () => {
        if (onStatus) onStatus({ type: 'status', state: 'done', exit_code: 0 });
      };
      this.ws.onerror = () => {
        if (onStatus) onStatus({ type: 'status', state: 'error', message: 'websocket error' });
      };
    }

    fit() {
      if (!this.fitAddon || !this.term) return;
      try {
        this.fitAddon.fit();
        const cols = this.term.cols || 80;
        const rows = this.term.rows || 24;
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
          this.ws.send(JSON.stringify({ type: 'resize', cols, rows }));
        }
      } catch (_e) {}
    }

    reconnect() {
      this.connect(() => {});
    }

    copySelection() {
      if (!this.term) return;
      const text = this.term.getSelection();
      if (!text) return;
      navigator.clipboard?.writeText(text).catch(() => {});
    }

    async pasteClipboard() {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
      try {
        const text = await navigator.clipboard.readText();
        if (text) {
          const encoder = new TextEncoder();
          this.ws.send(encoder.encode(text));
        }
      } catch (_e) {}
    }

    clearTerminal() {
      if (this.term) this.term.clear();
    }

    exportLog() {
      const blob = new Blob([this.logBuffer], { type: 'text/plain' });
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = `cockpit_${this.id}.txt`;
      a.click();
      URL.revokeObjectURL(a.href);
    }

    destroy() {
      if (this.resizeObs) {
        try { this.resizeObs.disconnect(); } catch (_e) {}
      }
      if (this.eventSource) {
        try { this.eventSource.close(); } catch (_e) {}
      }
      if (this.ws) {
        try { this.ws.close(); } catch (_e) {}
      }
      if (this.term) {
        try { this.term.dispose(); } catch (_e) {}
      }
      this.el.remove();
    }
  }

  class CockpitManager {
    constructor() {
      this.layout = localStorage.getItem('cockpit_layout') || '1';
      this.sessions = new Map();
      this.activeTab = null;
      this.root = null;
      this.tabsEl = null;
      this.gridEl = null;
      this.newDropdown = null;
      this.pendingLaunch = null;
      this.layoutOrder = ['1', '2h', '2v', '4', '3L', '3T', '6'];
      this.statsTimer = null;
      this.lastSessionSnapshot = [];
    }

    mount(hostEl) {
      this.root = hostEl;
      hostEl.innerHTML = this.renderSkeleton();
      this.tabsEl = hostEl.querySelector('#cockpit-tabs');
      this.gridEl = hostEl.querySelector('#cockpit-grid');
      this.bindUi(hostEl);
      this.restoreSessions();
      this.consumePendingLaunch();
      this.bindShortcuts();
      this.startStatusPoll();
    }

    renderSkeleton() {
      return `
        <div class="cockpit-container">
          <div class="cockpit-toolbar" id="cockpit-toolbar">
            ${this.layoutOrder.map(k => `<button type="button" class="layout-btn ${this.layout === k ? 'active' : ''}" data-layout="${k}">${k}</button>`).join('')}
            <button type="button" class="btn btn-sm cockpit-new-btn" id="cockpit-new">+ New</button>
          </div>
          <div class="cockpit-tabs" id="cockpit-tabs"></div>
          <div class="cockpit-grid" id="cockpit-grid"></div>
        </div>`;
    }

    bindUi(hostEl) {
      hostEl.querySelectorAll('[data-layout]').forEach((btn) => {
        btn.addEventListener('click', () => this.setLayout(btn.dataset.layout));
      });
      hostEl.querySelector('#cockpit-new').addEventListener('click', (ev) => this.toggleNewDropdown(ev.currentTarget));
      document.addEventListener('click', () => this.hideDropdown());
    }

    bindShortcuts() {
      if (this._shortcutsBound) return;
      this._shortcutsBound = true;
      document.addEventListener('keydown', (ev) => {
        if (location.hash.split('/')[1] !== 'cockpit') return;
        if (ev.ctrlKey && !ev.shiftKey && /^Digit[1-9]$/.test(ev.code)) {
          ev.preventDefault();
          const idx = Number(ev.code.slice(-1)) - 1;
          const ids = [...this.sessions.keys()];
          if (ids[idx]) this.activateTab(ids[idx]);
        } else if (ev.ctrlKey && ev.key === '\\') {
          ev.preventDefault();
          const idx = this.layoutOrder.indexOf(this.layout);
          const next = this.layoutOrder[(idx + 1) % this.layoutOrder.length];
          this.setLayout(next);
        } else if (ev.ctrlKey && ev.shiftKey && ev.key.toLowerCase() === 'n') {
          ev.preventDefault();
          const btn = document.getElementById('cockpit-new');
          if (btn) this.toggleNewDropdown(btn);
        } else if (ev.key === 'Escape') {
          this.hideDropdown();
          const panel = this.gridEl?.querySelector('.cockpit-panel.maximized');
          if (panel) panel.classList.remove('maximized');
        }
      });
    }

    startStatusPoll() {
      if (this.statsTimer) return;
      this.statsTimer = setInterval(() => {
        this.refreshSessionStats().catch(() => {});
      }, 2000);
      this.refreshSessionStats().catch(() => {});
    }

    async refreshSessionStats() {
      const res = await fetch('/api/cockpit/sessions');
      if (!res.ok) return;
      const payload = await res.json();
      const rows = Array.isArray(payload.sessions) ? payload.sessions : [];
      this.lastSessionSnapshot = rows;
      const byId = new Map(rows.map((s) => [s.id, s]));

      this.sessions.forEach((entry, id) => {
        const isSystemPanel = entry.panel.type === 'metrics' || entry.panel.type === 'log';
        if (isSystemPanel) return;
        const row = byId.get(id);
        if (!row) return;
        this.updateTabStatus(id, row.status || {});
        this.updateTabCost(id, Number(row.estimated_cost_usd || 0));
      });

      this.sessions.forEach((entry) => {
        if (entry.panel.type === 'metrics') {
          entry.panel.updateMetrics(rows);
        }
      });
    }

    toggleNewDropdown(anchor) {
      if (this.newDropdown) {
        this.hideDropdown();
        return;
      }
      const items = [
        { id: 'claude', label: 'Claude' },
        { id: 'codex', label: 'Codex' },
        { id: 'gemini', label: 'Gemini' },
        { id: 'openclaw', label: 'OpenClaw' },
        { id: 'shell', label: 'Shell' },
        { id: 'metrics', label: 'Metrics Panel' },
        { id: 'log', label: 'Log Stream' },
        { id: 'custom', label: 'Custom' },
      ];
      const menu = document.createElement('div');
      menu.className = 'cockpit-new-dropdown';
      menu.innerHTML = items.map(it => `<div class="dropdown-item" data-agent="${it.id}">${it.label}</div>`).join('');
      menu.addEventListener('click', async (ev) => {
        const item = ev.target.closest('[data-agent]');
        if (!item) return;
        try {
          await this.createFromPreset(item.dataset.agent);
        } catch (e) {
          if (!(typeof window.trySetupRedirect === 'function' && window.trySetupRedirect(e, item.dataset.agent, 'cockpit'))) {
            alert(`Launch failed: ${e.message || e}`);
          }
        }
        this.hideDropdown();
      });

      const rect = anchor.getBoundingClientRect();
      menu.style.top = `${rect.bottom + window.scrollY + 4}px`;
      menu.style.left = `${Math.max(12, rect.left + window.scrollX - 10)}px`;
      document.body.appendChild(menu);
      this.newDropdown = menu;
    }

    hideDropdown() {
      if (this.newDropdown) {
        this.newDropdown.remove();
        this.newDropdown = null;
      }
      hideContextMenu();
    }

    async createFromPreset(agent) {
      if (agent === 'metrics') {
        this.attachSystemPanel('metrics');
        return;
      }
      if (agent === 'log') {
        this.attachSystemPanel('log');
        return;
      }
      if (agent === 'custom') {
        const command = prompt('Command to run in PTY:', '/bin/bash');
        if (!command) return;
        await this.createSession(command, [], 'custom');
        return;
      }

      const ready = await this.ensurePresetReady(agent);
      if (!ready) return;

      const map = {
        shell: { command: '/bin/bash', args: [], agentType: 'shell' },
        claude: { command: 'claude', args: ['--output-format', 'stream-json', '--verbose'], agentType: 'claude' },
        codex: { command: 'codex', args: ['--json'], agentType: 'codex' },
        gemini: { command: 'gemini', args: ['--output-format', 'stream-json'], agentType: 'gemini' },
        openclaw: { command: 'openclaw', args: [], agentType: 'openclaw' },
      };
      const cfg = map[agent] || map.shell;
      await this.createSession(cfg.command, cfg.args, cfg.agentType);
    }

    attachSystemPanel(kind) {
      const id = `${kind}-${Date.now().toString(36)}`;
      const title = kind === 'metrics' ? 'metrics' : 'events';
      const panel = new CockpitPanel(id, kind, title, this);
      const tab = this.createTab(id, title);
      panel.el.querySelector('[data-action="close"]').addEventListener('click', () => this.detachSession(id, true));
      if (kind === 'metrics') {
        panel.attachMetrics();
        panel.updateMetrics(this.lastSessionSnapshot);
      } else {
        panel.attachLogStream();
      }
      this.gridEl.appendChild(panel.el);
      this.sessions.set(id, { panel, tab, status: { state: 'active' }, cost: 0 });
      this.activateTab(id);
    }

    async ensurePresetReady(agent) {
      if (agent === 'shell' || agent === 'custom') return true;
      const res = await fetch('/api/deploy/preflight', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ agent_id: agent }),
      });
      if (!res.ok) {
        throw await buildApiError(res, '/api/deploy/preflight');
      }
      const pre = await res.json();
      if (!pre.cli_installed) {
        const hint = pre.install_hint || `Install ${agent} CLI and retry.`;
        alert(`${agent}: CLI missing. ${hint}`);
        return false;
      }
      if (!pre.keys_configured) {
        const providers = Array.isArray(pre.missing_keys) ? pre.missing_keys : [];
        const redirected = typeof window.trySetupRedirect === 'function' && window.trySetupRedirect({
          status: 400,
          message: `missing API keys: ${providers.join(', ')}`,
          body: { missing_keys: providers },
        }, agent, 'cockpit');
        if (redirected) {
          return false;
        }
        location.hash = '#/config';
        return false;
      }
      return true;
    }

    async createSession(command, args, agentType) {
      const res = await fetch('/api/cockpit/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ command, args, cols: 120, rows: 36, agent_type: agentType || null }),
      });
      if (!res.ok) {
        throw await buildApiError(res, '/api/cockpit/sessions');
      }
      const payload = await res.json();
      this.attachSession({ id: payload.id, agent_type: agentType, ws_url: payload.ws_url, status: { state: 'active' } });
      this.setLayout(this.layout);
    }

    async restoreSessions() {
      const res = await fetch('/api/cockpit/sessions');
      if (!res.ok) return;
      const payload = await res.json();
      const sessions = payload.sessions || [];

      // Rebuild from server truth whenever mounting.
      this.sessions.forEach((entry) => entry.panel.destroy());
      this.sessions.clear();
      this.tabsEl.innerHTML = '';
      this.gridEl.innerHTML = '';

      sessions.forEach((s) => {
        this.attachSession({
          id: s.id,
          agent_type: s.agent_type,
          ws_url: `/api/cockpit/sessions/${encodeURIComponent(s.id)}/ws`,
          status: s.status,
        });
      });
      this.setLayout(this.layout);
    }

    attachSession(session) {
      if (this.sessions.has(session.id)) return;

      const panel = new CockpitPanel(session.id, 'terminal', `${session.agent_type || 'session'}:${session.id.slice(0, 8)}`, this);
      const tab = this.createTab(session.id, session.agent_type || 'session');

      panel.el.querySelector('[data-action="close"]').addEventListener('click', () => this.destroySession(session.id));
      panel.attachTerminal(session.id, session.ws_url, (statusMsg) => this.updateTabStatus(session.id, statusMsg));

      this.gridEl.appendChild(panel.el);
      this.sessions.set(session.id, { panel, tab, status: session.status || { state: 'active' }, cost: Number(session.estimated_cost_usd || 0) });
      this.updateTabCost(session.id, Number(session.estimated_cost_usd || 0));
      this.activateTab(session.id);
    }

    createTab(sessionId, label) {
      const tab = document.createElement('button');
      tab.type = 'button';
      tab.className = 'cockpit-tab tab-active';
      tab.dataset.sessionId = sessionId;
      tab.innerHTML = `
        <span class="tab-icon">●</span>
        <span class="tab-label">${escapeHtml(label)}</span>
        <span class="tab-cost">${formatUsd(0)}</span>`;
      tab.addEventListener('click', () => this.activateTab(sessionId));
      tab.addEventListener('dblclick', () => {
        const panel = this.sessions.get(sessionId)?.panel?.el;
        if (panel) panel.classList.toggle('maximized');
      });
      tab.addEventListener('contextmenu', (ev) => {
        ev.preventDefault();
        showContextMenu(ev.clientX, ev.clientY, [
          { label: 'Close', onClick: () => this.destroySession(sessionId) },
          { label: 'Restart', onClick: () => this.restartSession(sessionId) },
          { label: 'Export', onClick: () => this.sessions.get(sessionId)?.panel?.exportLog() },
          { label: 'Detach', onClick: () => this.detachSession(sessionId) },
        ]);
      });
      this.tabsEl.appendChild(tab);
      return tab;
    }

    activateTab(sessionId) {
      this.activeTab = sessionId;
      this.sessions.forEach((entry, id) => {
        entry.tab.classList.toggle('active', id === sessionId);
        entry.panel.el.classList.toggle('active', id === sessionId);
      });
      this.applyLayout();
      localStorage.setItem('cockpit_active_tab', sessionId);
    }

    setLayout(layout) {
      if (!LAYOUTS[layout]) return;
      this.layout = layout;
      localStorage.setItem('cockpit_layout', layout);
      this.sessions.forEach((entry) => { entry.panel.customSlot = null; });
      this.root.querySelectorAll('[data-layout]').forEach((btn) => {
        btn.classList.toggle('active', btn.dataset.layout === layout);
      });
      this.applyLayout();
    }

    renderLayoutFrames(slots, entryCount) {
      this.gridEl.querySelectorAll('.cockpit-slot-frame,.cockpit-empty-hint').forEach((el) => el.remove());
      slots.forEach((slot, idx) => {
        const frame = document.createElement('div');
        frame.className = 'cockpit-slot-frame';
        frame.innerHTML = `<span class="slot-label">Slot ${idx + 1}</span>`;
        placePanel(frame, slot);
        this.gridEl.appendChild(frame);
      });
      if (entryCount === 0) {
        const hint = document.createElement('div');
        hint.className = 'cockpit-empty-hint';
        hint.innerHTML = 'No active sessions. Use <b>+ New</b> to launch one.';
        this.gridEl.appendChild(hint);
      }
    }

    applyLayout() {
      const entries = [...this.sessions.values()];

      const mobileSingle = window.matchMedia('(max-width: 768px)').matches;
      const layoutKey = mobileSingle ? '1' : this.layout;
      const slots = LAYOUTS[layoutKey] || LAYOUTS['1'];
      this.renderLayoutFrames(slots, entries.length);

      if (entries.length === 0) return;

      if (layoutKey === '1') {
        const activeEntry = this.sessions.get(this.activeTab) || entries[0];
        entries.forEach((entry) => {
          if (entry === activeEntry) {
            placePanel(entry.panel.el, { x: 0, y: 0, w: 1, h: 1 });
            entry.panel.el.style.display = '';
          } else {
            entry.panel.el.style.display = 'none';
          }
          entry.panel.fit();
        });
        return;
      }

      entries.forEach((entry, idx) => {
        const slot = entry.panel.customSlot || slots[idx % slots.length];
        entry.panel.el.style.display = '';
        placePanel(entry.panel.el, slot);
        entry.panel.fit();
      });
    }

    async destroySession(sessionId) {
      const res = await fetch(`/api/cockpit/sessions/${encodeURIComponent(sessionId)}`, { method: 'DELETE' });
      if (!res.ok) {
        alert(`Failed to close session ${sessionId}`);
        return;
      }
      this.detachSession(sessionId, true);
    }

    detachSession(sessionId, destroy = false) {
      const entry = this.sessions.get(sessionId);
      if (!entry) return;
      entry.tab.remove();
      if (destroy) {
        entry.panel.destroy();
      } else {
        entry.panel.el.remove();
      }
      this.sessions.delete(sessionId);
      const next = [...this.sessions.keys()][0] || null;
      this.activeTab = next;
      if (next) this.activateTab(next);
      this.applyLayout();
    }

    async restartSession(sessionId) {
      const entry = this.sessions.get(sessionId);
      if (!entry) return;
      const cmd = prompt('Restart command:', '/bin/bash');
      if (!cmd) return;
      await this.destroySession(sessionId);
      await this.createSession(cmd, [], entry.panel.agentType || 'shell');
    }

    updateTabStatus(sessionId, statusMsg) {
      const entry = this.sessions.get(sessionId);
      if (!entry) return;

      const state = String(statusMsg?.state || 'active').toUpperCase();
      const mapped = state === 'DONE' ? 'COMPLETED' : state === 'ERROR' ? 'ERROR' : state === 'WAITING' ? 'WAITING' : 'ACTIVE';
      const cfg = TAB_STATES[mapped] || TAB_STATES.ACTIVE;

      entry.tab.className = `cockpit-tab ${cfg.class} ${entry.tab.classList.contains('active') ? 'active' : ''}`;
      const icon = entry.tab.querySelector('.tab-icon');
      if (icon) {
        icon.textContent = cfg.icon;
        icon.style.color = cfg.color;
      }
      const panelStatus = entry.panel.el.querySelector(`#panel-status-${CSS.escape(sessionId)}`);
      if (panelStatus) panelStatus.textContent = state.toLowerCase();
    }

    updateTabCost(sessionId, usd) {
      const entry = this.sessions.get(sessionId);
      if (!entry) return;
      entry.cost = Number.isFinite(usd) ? usd : 0;
      const el = entry.tab.querySelector('.tab-cost');
      if (el) el.textContent = formatUsd(entry.cost);
    }

    consumePendingLaunch() {
      const raw = localStorage.getItem('cockpit_pending_launch');
      if (!raw) return;
      localStorage.removeItem('cockpit_pending_launch');
      try {
        const launch = JSON.parse(raw);
        (launch.panels || []).forEach((panel) => {
          if (panel.panel_type === 'terminal' && panel.id && panel.ws_url) {
            this.attachSession({
              id: panel.id,
              agent_type: 'launch',
              ws_url: panel.ws_url,
              status: { state: 'active' },
            });
          }
          if (panel.panel_type === 'iframe' && panel.iframe_url) {
            const pid = panel.id || `iframe-${Date.now()}`;
            const cockpitPanel = new CockpitPanel(pid, 'iframe', `gui:${pid.slice(0, 6)}`, this);
            const tab = this.createTab(pid, 'gui');
            cockpitPanel.attachIframe(panel.iframe_url);
            cockpitPanel.el.querySelector('[data-action="close"]').addEventListener('click', () => {
              this.detachSession(pid, true);
            });
            this.gridEl.appendChild(cockpitPanel.el);
            this.sessions.set(pid, { panel: cockpitPanel, tab, status: { state: 'active' }, cost: 0 });
            this.activateTab(pid);
          }
        });
        this.applyLayout();
      } catch (_e) {}
    }
  }

  function placePanel(el, slot) {
    const gutter = 4;
    el.style.left = `calc(${(slot.x * 100).toFixed(4)}% + ${gutter}px)`;
    el.style.top = `calc(${(slot.y * 100).toFixed(4)}% + ${gutter}px)`;
    el.style.width = `calc(${(slot.w * 100).toFixed(4)}% - ${gutter * 2}px)`;
    el.style.height = `calc(${(slot.h * 100).toFixed(4)}% - ${gutter * 2}px)`;
  }

  function clamp(v, min, max) {
    return Math.max(min, Math.min(max, v));
  }

  function formatUsd(usd) {
    const n = Number(usd || 0);
    if (!Number.isFinite(n) || n <= 0) return '$0.00';
    if (n < 0.01) return '<$0.01';
    return `$${n.toFixed(2)}`;
  }

  function showContextMenu(x, y, items) {
    hideContextMenu();
    const menu = document.createElement('div');
    menu.id = 'cockpit-context-menu';
    menu.className = 'cockpit-context-menu';
    menu.innerHTML = items.map((it, i) => `<div class="dropdown-item" data-idx="${i}">${escapeHtml(it.label)}</div>`).join('');
    menu.style.left = `${x}px`;
    menu.style.top = `${y}px`;
    menu.addEventListener('click', (ev) => {
      const idx = Number(ev.target.closest('[data-idx]')?.dataset?.idx);
      if (!Number.isNaN(idx) && items[idx]?.onClick) items[idx].onClick();
      hideContextMenu();
    });
    document.body.appendChild(menu);
    setTimeout(() => document.addEventListener('click', hideContextMenu, { once: true }), 0);
  }

  function hideContextMenu() {
    document.getElementById('cockpit-context-menu')?.remove();
  }

  function escapeHtml(s) {
    if (typeof window.__escapeHtml === 'function') {
      return window.__escapeHtml(s);
    }
    return String(s || '')
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  async function buildApiError(res, path) {
    const raw = await res.text();
    let body = null;
    try { body = raw ? JSON.parse(raw) : null; } catch (_e) {}
    const message = (body && body.error) || raw || `${path} => ${res.status}`;
    const err = new Error(message);
    err.status = res.status;
    err.body = body;
    err.path = path;
    return err;
  }

  const cockpitPage = {
    manager: null,
    mount(hostEl) {
      if (!this.manager) this.manager = new CockpitManager();
      this.manager.mount(hostEl);
    },
    queueLaunch(launchResult) {
      localStorage.setItem('cockpit_pending_launch', JSON.stringify(launchResult));
    },
  };

  window.CockpitPage = cockpitPage;
})();
