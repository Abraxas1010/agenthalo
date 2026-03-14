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
      this.refreshTimer = null;
      this.logBuffer = '';
      this.customSlot = null;
      this.identity = null;
      this.attestations = null;
      this.agentType = null;

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
          { label: 'View Identity', onClick: () => this.viewIdentity() },
          { label: 'View Attestations', onClick: () => this.viewAttestations() },
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
      setTimeout(() => {
        this.fit();
        this.focusTerminal();
      }, 0);

      this.term.onData((data) => {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
          this.ws.send(data);
        }
      });

      this.body.addEventListener('mousedown', () => this.focusTerminal());

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
      this.ws.onopen = () => {
        this.fit();
        this.focusTerminal();
      };
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

    focusTerminal() {
      if (!this.term) return;
      try {
        this.term.focus();
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

    setRefreshTimer(callback, intervalMs) {
      if (this.refreshTimer) clearInterval(this.refreshTimer);
      this.refreshTimer = setInterval(() => {
        callback().catch(() => {});
      }, intervalMs);
    }

    setIdentity(identity) {
      this.identity = identity || null;
    }

    setAttestations(attestations) {
      this.attestations = attestations || null;
    }

    viewIdentity() {
      if (!this.identity) {
        alert('No DID identity is attached to this panel yet.');
        return;
      }
      showJsonModal('Agent Identity', this.identity);
    }

    viewAttestations() {
      if (!this.attestations) {
        alert('No attestation or capability data is attached to this panel.');
        return;
      }
      showJsonModal('Agent Attestations', this.attestations);
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
      if (this.refreshTimer) {
        clearInterval(this.refreshTimer);
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
      this.meshCollapsed = localStorage.getItem('cockpit_mesh_collapsed') === '1';
      const cfgPollMs = Number(window.__cockpitConfig?.meshPollMs);
      const cfgMetricsPollMs = Number(window.__cockpitConfig?.metricsPollMs);
      this.meshPollMs = Number.isFinite(cfgPollMs) && cfgPollMs >= 1000 ? cfgPollMs : 10000;
      this.metricsPollMs = Number.isFinite(cfgMetricsPollMs) && cfgMetricsPollMs >= 1000 ? cfgMetricsPollMs : 5000;
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
      this.meshTimer = null;
      this.metricsTimer = null;
      this.meshSidebarEl = null;
      this.meshBodyEl = null;
      this.meshSelfEl = null;
      this.meshPeerListEl = null;
      this.diversityScoreEl = null;
      this.diversityMetaEl = null;
      this.diversityCanvasEl = null;
      this.diversityChart = null;
      this.topologyCanvasEl = null;
      this.topologyEmptyEl = null;
      this.topologyChart = null;
    }

    mount(hostEl) {
      this.root = hostEl;
      hostEl.innerHTML = this.renderSkeleton();
      this.tabsEl = hostEl.querySelector('#cockpit-tabs');
      this.gridEl = hostEl.querySelector('#cockpit-grid');
      this.meshSidebarEl = hostEl.querySelector('#cockpit-mesh-sidebar');
      this.meshBodyEl = hostEl.querySelector('#cockpit-mesh-body');
      this.meshSelfEl = hostEl.querySelector('#cockpit-mesh-self');
      this.meshPeerListEl = hostEl.querySelector('#cockpit-mesh-peers');
      this.diversityScoreEl = hostEl.querySelector('#cockpit-diversity-score');
      this.diversityMetaEl = hostEl.querySelector('#cockpit-diversity-meta');
      this.diversityCanvasEl = hostEl.querySelector('#cockpit-diversity-chart');
      this.topologyCanvasEl = hostEl.querySelector('#cockpit-topology-chart');
      this.topologyEmptyEl = hostEl.querySelector('#cockpit-topology-empty');
      this.setMeshCollapsed(this.meshCollapsed);
      this.bindUi(hostEl);
      this.restoreSessions();
      this.consumePendingLaunch();
      this.bindShortcuts();
      this.startStatusPoll();
      this.stopMeshPoll();
      this.startMeshPoll();
      this.stopMetricsPoll();
      this.startMetricsPoll();
    }

    renderSkeleton() {
      return `
        <div class="cockpit-container">
          <div class="cockpit-toolbar" id="cockpit-toolbar">
            ${this.layoutOrder.map(k => `<button type="button" class="layout-btn ${this.layout === k ? 'active' : ''}" data-layout="${k}">${k}</button>`).join('')}
            <button type="button" class="btn btn-sm cockpit-new-btn" id="cockpit-new">+ New</button>
          </div>
          <div class="cockpit-main">
            <div class="cockpit-stage">
              <div class="cockpit-tabs" id="cockpit-tabs"></div>
              <div class="cockpit-grid" id="cockpit-grid"></div>
            </div>
            <aside class="cockpit-mesh-sidebar" id="cockpit-mesh-sidebar">
              <div class="cockpit-mesh-header">
                <span class="cockpit-mesh-title">⬡ Mesh Network</span>
                <button type="button" class="cockpit-mesh-toggle" id="cockpit-mesh-toggle" title="Collapse mesh sidebar">◀</button>
              </div>
              <div class="cockpit-mesh-body" id="cockpit-mesh-body">
                <div class="cockpit-mesh-self" id="cockpit-mesh-self"></div>
                <div class="cockpit-mesh-peers" id="cockpit-mesh-peers"></div>
                <div class="cockpit-diversity-card">
                  <div class="mesh-section-title">Strategy Diversity</div>
                  <div class="cockpit-diversity-score" id="cockpit-diversity-score">--</div>
                  <canvas id="cockpit-diversity-chart" height="110"></canvas>
                  <div class="cockpit-diversity-meta" id="cockpit-diversity-meta">Waiting for data...</div>
                </div>
                <div class="cockpit-topology-card">
                  <div class="mesh-section-title">Trace Topology (H₀)</div>
                  <canvas id="cockpit-topology-chart" height="180"></canvas>
                  <div class="cockpit-topology-empty" id="cockpit-topology-empty">No persistent patterns yet</div>
                </div>
              </div>
            </aside>
          </div>
        </div>`;
    }

    bindUi(hostEl) {
      hostEl.querySelectorAll('[data-layout]').forEach((btn) => {
        btn.addEventListener('click', () => this.setLayout(btn.dataset.layout));
      });
      hostEl.querySelector('#cockpit-new').addEventListener('click', (ev) => this.toggleNewDropdown(ev.currentTarget));
      hostEl.querySelector('#cockpit-mesh-toggle')?.addEventListener('click', () => {
        this.setMeshCollapsed(!this.meshCollapsed);
      });
      document.addEventListener('click', () => this.hideDropdown());
    }

    setMeshCollapsed(collapsed) {
      this.meshCollapsed = !!collapsed;
      this.meshSidebarEl?.classList.toggle('collapsed', this.meshCollapsed);
      try {
        localStorage.setItem('cockpit_mesh_collapsed', this.meshCollapsed ? '1' : '0');
      } catch (_e) {}
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

    startMeshPoll() {
      if (this.meshTimer) return;
      this.refreshMeshStatus().catch(() => {});
      this.meshTimer = setInterval(() => {
        this.refreshMeshStatus().catch(() => {});
      }, this.meshPollMs);
    }

    stopMeshPoll() {
      if (this.meshTimer) {
        clearInterval(this.meshTimer);
        this.meshTimer = null;
      }
    }

    startMetricsPoll() {
      if (this.metricsTimer) return;
      this.refreshDiversityStatus().catch(() => {});
      this.refreshTraceTopology().catch(() => {});
      this.metricsTimer = setInterval(() => {
        this.refreshDiversityStatus().catch(() => {});
        this.refreshTraceTopology().catch(() => {});
      }, this.metricsPollMs);
    }

    stopMetricsPoll() {
      if (this.metricsTimer) {
        clearInterval(this.metricsTimer);
        this.metricsTimer = null;
      }
    }

    async refreshMeshStatus() {
      if (!this.meshSidebarEl || !this.meshSelfEl || !this.meshPeerListEl) return;
      let payload = null;
      try {
        const res = await fetch('/api/orchestrator/mesh');
        if (res.ok) {
          payload = await res.json();
        }
      } catch (_e) {
        payload = null;
      }
      this.renderMeshStatus(payload);
    }

    renderMeshStatus(payload) {
      if (!this.meshSelfEl || !this.meshPeerListEl) return;
      if (!payload || payload.enabled !== true) {
        this.meshSelfEl.innerHTML = '<div class="mesh-disabled">Mesh not configured</div>';
        this.meshPeerListEl.innerHTML = '';
        return;
      }

      const selfId = payload.self_agent_id || 'this node';
      this.meshSelfEl.innerHTML = `
        <div class="mesh-self-row">
          <span class="mesh-indicator mesh-online">●</span>
          <span class="mesh-self-id">${escapeHtml(selfId)}</span>
        </div>
      `;
      const peers = Array.isArray(payload.peers) ? payload.peers : [];
      if (peers.length === 0) {
        this.meshPeerListEl.innerHTML = '<div class="mesh-empty">No remote peers detected</div>';
        return;
      }
      this.meshPeerListEl.innerHTML = peers.map((peer) => {
        const online = peer && peer.reachable === true;
        const name = peer && peer.agent_id ? peer.agent_id : 'unknown';
        const latency = online && Number.isFinite(Number(peer.latency_ms))
          ? `${Number(peer.latency_ms)}ms`
          : 'unreachable';
        const did = peer && peer.did_uri ? ' · DID' : '';
        return `
          <div class="mesh-peer ${online ? 'mesh-peer-online' : 'mesh-peer-offline'}">
            <span class="mesh-indicator ${online ? 'mesh-online' : 'mesh-offline'}">${online ? '●' : '○'}</span>
            <div class="mesh-peer-info">
              <div class="mesh-peer-name">${escapeHtml(name)}</div>
              <div class="mesh-peer-detail">${escapeHtml(latency)}${did}</div>
            </div>
          </div>
        `;
      }).join('');
    }

    async refreshDiversityStatus() {
      if (!this.diversityScoreEl) return;
      try {
        const res = await fetch('/api/metrics/diversity?window_seconds=300');
        if (!res.ok) {
          this.renderDiversityStatus(null);
          return;
        }
        const payload = await res.json();
        this.renderDiversityStatus(payload);
      } catch (_e) {
        this.renderDiversityStatus(null);
      }
    }

    renderDiversityStatus(payload) {
      if (!this.diversityScoreEl || !this.diversityMetaEl) return;
      if (!payload || !Number.isFinite(Number(payload.score))) {
        this.diversityScoreEl.textContent = '--';
        this.diversityMetaEl.textContent = 'No diversity data available';
        this.updateDiversityChart(0);
        return;
      }

      const score = clamp(Number(payload.score), 0, 100);
      this.diversityScoreEl.textContent = `${score.toFixed(1)} / 100`;
      const totalCalls = Number(payload.total_calls || 0);
      const toolCount = payload.tool_counts && typeof payload.tool_counts === 'object'
        ? Object.keys(payload.tool_counts).length
        : 0;
      const label = score < 30
        ? 'Mode collapse risk'
        : score < 70
          ? 'Normal exploration'
          : 'Healthy exploration';
      this.diversityMetaEl.textContent = `${label} · ${toolCount} tools · ${totalCalls} calls`;
      this.updateDiversityChart(score);
    }

    updateDiversityChart(score) {
      if (!this.diversityCanvasEl || typeof Chart === 'undefined') return;
      const color = score < 30 ? '#ff3030' : (score < 70 ? '#ffb830' : '#00ff41');
      const data = [score, Math.max(0, 100 - score)];
      if (!this.diversityChart) {
        this.diversityChart = new Chart(this.diversityCanvasEl, {
          type: 'doughnut',
          data: {
            labels: ['Diversity', 'Remaining'],
            datasets: [{
              data,
              backgroundColor: [color, 'rgba(23, 40, 20, 0.65)'],
              borderColor: ['rgba(255, 255, 255, 0.1)', 'rgba(255, 255, 255, 0.08)'],
              borderWidth: 1,
            }],
          },
          options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: {
              legend: { display: false },
              tooltip: { enabled: false },
            },
            cutout: '70%',
          },
        });
        return;
      }
      this.diversityChart.data.datasets[0].data = data;
      this.diversityChart.data.datasets[0].backgroundColor = [color, 'rgba(23, 40, 20, 0.65)'];
      this.diversityChart.update('none');
    }

    async refreshTraceTopology() {
      if (!this.topologyCanvasEl) return;
      try {
        const res = await fetch('/api/metrics/trace-topology?window_seconds=300&max_chain_degree=2&max_entries=8');
        if (!res.ok) {
          this.renderTraceTopology(null);
          return;
        }
        const payload = await res.json();
        this.renderTraceTopology(payload);
      } catch (_e) {
        this.renderTraceTopology(null);
      }
    }

    renderTraceTopology(payload) {
      if (!this.topologyCanvasEl || typeof Chart === 'undefined') return;
      const entries = Array.isArray(payload && payload.entries) ? payload.entries : [];
      if (!entries.length) {
        if (this.topologyEmptyEl) this.topologyEmptyEl.style.display = 'block';
        this.updateTopologyChart([], []);
        return;
      }

      if (this.topologyEmptyEl) this.topologyEmptyEl.style.display = 'none';
      const labels = [];
      const values = [];
      entries.forEach((entry, idx) => {
        const rep = Array.isArray(entry.representative) ? entry.representative : [];
        const title = rep.length ? rep.join(' → ') : `feature-${idx + 1}`;
        const persistence = Number.isFinite(Number(entry.persistence))
          ? Number(entry.persistence)
          : Number(payload.window_seconds || 300);
        labels.push(title);
        values.push(Math.max(0, persistence));
      });
      this.updateTopologyChart(labels, values);
    }

    updateTopologyChart(labels, values) {
      if (!this.topologyCanvasEl || typeof Chart === 'undefined') return;
      if (!this.topologyChart) {
        this.topologyChart = new Chart(this.topologyCanvasEl, {
          type: 'bar',
          data: {
            labels,
            datasets: [{
              label: 'Persistence',
              data: values,
              backgroundColor: 'rgba(0, 255, 65, 0.35)',
              borderColor: '#00ff41',
              borderWidth: 1,
            }],
          },
          options: {
            responsive: true,
            maintainAspectRatio: false,
            indexAxis: 'y',
            plugins: { legend: { display: false } },
            scales: {
              x: { beginAtZero: true, ticks: { color: '#7bb07b' }, grid: { color: 'rgba(45, 66, 40, 0.5)' } },
              y: { ticks: { color: '#8ecf8e', font: { size: 10 } }, grid: { display: false } },
            },
          },
        });
        return;
      }
      this.topologyChart.data.labels = labels;
      this.topologyChart.data.datasets[0].data = values;
      this.topologyChart.update('none');
    }

    async toggleNewDropdown(anchor) {
      if (this.newDropdown) {
        this.hideDropdown();
        return;
      }
      const items = [
        { id: 'claude', label: 'Claude', icon: '⚡', needsPreflight: true },
        { id: 'codex', label: 'Codex', icon: '⌁', needsPreflight: true },
        { id: 'gemini', label: 'Gemini', icon: '◇', needsPreflight: true },
        { id: 'shell', label: 'Shell', icon: '▣', needsPreflight: false },
        { id: 'admin', label: 'Admin Panel', icon: '⚙', needsPreflight: false },
        { id: 'containers', label: 'Containers', icon: '⬒', needsPreflight: false },
        { id: 'workflow', label: 'Workflow Builder', icon: '🔀', needsPreflight: false },
        { id: 'channel', label: 'Agent Channel', icon: '⬡', needsPreflight: false },
        { id: 'metrics', label: 'Metrics Panel', icon: '📊', needsPreflight: false },
        { id: 'log', label: 'Log Stream', icon: '📜', needsPreflight: false },
        { id: 'custom', label: 'Custom', icon: '⚙', needsPreflight: false },
      ];
      const menu = document.createElement('div');
      menu.className = 'cockpit-new-dropdown';
      menu.innerHTML = items.map((it) => `
        <div class="dropdown-item" data-agent="${it.id}">
          <span class="dropdown-icon">${it.icon || ''}</span>
          <span class="dropdown-label">${escapeHtml(it.label)}</span>
          ${it.needsPreflight ? `<span class="dropdown-status loading" data-status-for="${it.id}">…</span>` : ''}
        </div>
      `).join('');
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
      this.populateNewDropdownStatuses(menu).catch(() => {});
    }

    async populateNewDropdownStatuses(menu) {
      let catalog = null;
      try {
        const res = await fetch('/api/deploy/catalog');
        if (res.ok) catalog = await res.json();
      } catch (_e) {}
      const agents = Array.isArray(catalog?.agents) ? catalog.agents : [];
      await Promise.all(agents.map(async (agent) => {
        const statusEl = menu.querySelector(`[data-status-for="${CSS.escape(agent.id)}"]`);
        if (!statusEl) return;
        try {
          const pre = await this.fetchDeployPreflight(agent.id);
          statusEl.classList.remove('loading');
          if (pre.cli_installed && pre.keys_configured) {
            statusEl.classList.add('ready');
            statusEl.textContent = '● ready';
          } else if (pre.cli_installed) {
            statusEl.classList.add('partial');
            statusEl.textContent = '◐ keys';
          } else {
            statusEl.classList.add('missing');
            statusEl.textContent = '○ install';
          }
        } catch (_e) {
          statusEl.classList.remove('loading');
          statusEl.classList.add('missing');
          statusEl.textContent = '○ unavailable';
        }
      }));
    }

    async fetchDeployPreflight(agentId) {
      const res = await fetch('/api/deploy/preflight', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ agent_id: agentId }),
      });
      if (!res.ok) {
        throw await buildApiError(res, '/api/deploy/preflight');
      }
      return res.json();
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
      if (agent === 'admin' || agent === 'containers' || agent === 'workflow' || agent === 'channel') {
        this.attachSystemPanel(agent);
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
        claude: { command: 'claude', args: [], agentType: 'claude' },
        codex: { command: 'codex', args: [], agentType: 'codex' },
        gemini: { command: 'gemini', args: [], agentType: 'gemini' },
      };
      const cfg = map[agent] || map.shell;
      await this.createSession(cfg.command, cfg.args, cfg.agentType);
    }

    attachSystemPanel(kind) {
      const id = `${kind}-${Date.now().toString(36)}`;
      const titles = {
        metrics: 'metrics',
        log: 'events',
        admin: 'admin',
        containers: 'containers',
        workflow: 'workflow',
        channel: 'channel',
      };
      const title = titles[kind] || kind;
      const panel = new CockpitPanel(id, kind, title, this);
      const tab = this.createTab(id, title);
      panel.el.querySelector('[data-action="close"]').addEventListener('click', () => this.detachSession(id, true));
      if (kind === 'metrics') {
        panel.attachMetrics();
        panel.updateMetrics(this.lastSessionSnapshot);
      } else if (kind === 'log') {
        panel.attachLogStream();
      } else if (kind === 'admin') {
        panel.attachAdmin = () => attachAdminPanel(panel);
        panel.attachAdmin();
      } else if (kind === 'containers') {
        panel.attachContainers = () => attachContainersPanel(panel);
        panel.attachContainers();
      } else if (kind === 'workflow') {
        panel.attachWorkflow = () => attachWorkflowPanel(panel);
        panel.attachWorkflow();
      } else if (kind === 'channel') {
        panel.attachChannel = () => attachChannelPanel(panel);
        panel.attachChannel();
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
      panel.agentType = session.agent_type || 'session';
      if (session.identity) panel.setIdentity(session.identity);
      if (session.attestations) panel.setAttestations(session.attestations);
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
      this.sessions.get(sessionId)?.panel?.focusTerminal?.();
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
      const autoAgent = localStorage.getItem('cockpit_autolaunch_agent');
      if (autoAgent) {
        localStorage.removeItem('cockpit_autolaunch_agent');
        setTimeout(() => {
          this.createFromPreset(autoAgent).catch(() => {});
        }, 350);
      }
      const raw = localStorage.getItem('cockpit_pending_launch');
      if (!raw) return;
      localStorage.removeItem('cockpit_pending_launch');
      try {
        const launch = JSON.parse(raw);
        if (launch.session_id) {
          this.attachSession({
            id: launch.session_id,
            agent_type: launch.agent || 'launch',
            ws_url: `/api/cockpit/sessions/${encodeURIComponent(launch.session_id)}/ws`,
            status: { state: 'active' },
          });
        }
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

  async function fetchJson(path) {
    const res = await fetch(path);
    if (!res.ok) throw await buildApiError(res, path);
    return res.json();
  }

  async function postJson(path, body) {
    const res = await fetch(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body || {}),
    });
    if (!res.ok) throw await buildApiError(res, path);
    return res.json();
  }

  async function deleteJson(path) {
    const res = await fetch(path, { method: 'DELETE' });
    if (!res.ok) throw await buildApiError(res, path);
    return res.json();
  }

  function showJsonModal(title, payload) {
    const old = document.getElementById('cockpit-json-modal');
    if (old) old.remove();
    const wrap = document.createElement('div');
    wrap.id = 'cockpit-json-modal';
    wrap.className = 'cockpit-json-modal';
    wrap.innerHTML = `
      <div class="cockpit-json-card">
        <div class="cockpit-json-head">
          <strong>${escapeHtml(title)}</strong>
          <button type="button" class="btn btn-sm" data-close="1">Close</button>
        </div>
        <pre class="cockpit-json-body">${escapeHtml(JSON.stringify(payload, null, 2))}</pre>
      </div>
    `;
    wrap.addEventListener('click', (ev) => {
      if (ev.target === wrap || ev.target.closest('[data-close="1"]')) wrap.remove();
    });
    document.body.appendChild(wrap);
  }

  function didBadge(didUri) {
    if (!didUri) return '<span class="badge badge-muted">no DID</span>';
    return `<span class="badge badge-info">${escapeHtml(truncateDid(didUri))}</span>`;
  }

  function truncateDid(didUri) {
    const value = String(didUri || '');
    if (value.length <= 24) return value;
    return `${value.slice(0, 14)}…${value.slice(-8)}`;
  }

  function agentStatusBadge(status) {
    const normalized = String(status || 'unknown').toLowerCase();
    const cls = normalized === 'idle'
      ? 'badge-ok'
      : normalized === 'busy' || normalized === 'running'
        ? 'badge-info'
        : normalized === 'stopped' || normalized === 'failed'
          ? 'badge-warn'
          : 'badge-muted';
    return `<span class="badge ${cls}">${escapeHtml(normalized)}</span>`;
  }

  function lockStateBadge(lockState) {
    const normalized = String(lockState || 'empty').toLowerCase();
    const cls = normalized === 'locked'
      ? 'badge-ok'
      : normalized === 'initialized' || normalized === 'reusable'
        ? 'badge-info'
        : normalized === 'error'
          ? 'badge-warn'
          : 'badge-muted';
    return `<span class="badge ${cls}">${escapeHtml(normalized)}</span>`;
  }

  function attachAdminPanel(panel) {
    const render = async () => {
      const [agentsRes, tasksRes, graphRes] = await Promise.all([
        fetchJson('/api/orchestrator/agents'),
        fetchJson('/api/orchestrator/tasks'),
        fetchJson('/api/orchestrator/graph'),
      ]);
      const agents = Array.isArray(agentsRes.agents) ? agentsRes.agents : [];
      const tasks = Array.isArray(tasksRes.tasks) ? tasksRes.tasks : [];
      const graph = graphRes.graph || { nodes: {}, edges: [] };
      panel.body.innerHTML = `
        <div class="cockpit-admin">
          <div class="cockpit-admin-grid">
            <form class="cockpit-form" id="admin-launch-${panel.id}">
              <div class="mesh-section-title">Launch Agent</div>
              <label>Agent
                <select class="input" name="agent">
                  <option value="claude">claude</option>
                  <option value="codex">codex</option>
                  <option value="gemini">gemini</option>
                  <option value="shell">shell</option>
                </select>
              </label>
              <label>Name <input class="input" name="agent_name" value="worker"></label>
              <label>Dispatch
                <select class="input" name="dispatch_mode">
                  <option value="pty">pty</option>
                  <option value="container">container</option>
                </select>
              </label>
              <label>Hookup
                <select class="input" name="hookup_kind">
                  <option value="cli">cli</option>
                  <option value="api">api</option>
                  <option value="local_model">local_model</option>
                </select>
              </label>
              <label>Hookup Value <input class="input" name="hookup_value" placeholder="codex | provider | model id"></label>
              <label>Model <input class="input" name="model" placeholder="optional model or repo id"></label>
              <label>API Key Source <input class="input" name="api_key_source" value="vault:openrouter"></label>
              <button class="btn btn-sm btn-primary" type="submit">Launch</button>
            </form>
            <form class="cockpit-form" id="admin-task-${panel.id}">
              <div class="mesh-section-title">Dispatch Task</div>
              <label>Agent
                <select class="input" name="agent_id">
                  ${agents.map((agent) => `<option value="${escapeHtml(agent.agent_id)}">${escapeHtml(agent.agent_name || agent.agent_id)}</option>`).join('')}
                </select>
              </label>
              <label>Task <textarea class="input" name="task" rows="4" placeholder="Review src/main.rs"></textarea></label>
              <button class="btn btn-sm btn-primary" type="submit">Run</button>
            </form>
          </div>
          <div class="mesh-section-title">Agents</div>
          <div class="table-wrap"><table>
            <thead><tr><th>Name</th><th>Status</th><th>DID</th><th>Cost</th><th>Actions</th></tr></thead>
            <tbody>
              ${agents.length ? agents.map((agent) => `
                <tr>
                  <td>${escapeHtml(agent.agent_name || agent.agent_id)}</td>
                  <td>${agentStatusBadge(agent.status)}</td>
                  <td>${didBadge(agent.did_uri)}</td>
                  <td>${formatUsd(Number(agent.total_cost_usd || 0))}</td>
                  <td>
                    <button class="btn btn-sm" data-stop-agent="${escapeHtml(agent.agent_id)}">Stop</button>
                    <button class="btn btn-sm" data-show-identity="${escapeHtml(agent.did_uri || '')}">Identity</button>
                  </td>
                </tr>
              `).join('') : '<tr><td colspan="5" class="muted">No orchestrated agents.</td></tr>'}
            </tbody>
          </table></div>
          <div class="mesh-section-title">Tasks</div>
          <div class="table-wrap"><table>
            <thead><tr><th>Task</th><th>Agent</th><th>Status</th><th>Result</th></tr></thead>
            <tbody>
              ${tasks.length ? tasks.slice(-8).reverse().map((task) => `
                <tr>
                  <td><code>${escapeHtml(task.task_id || '')}</code></td>
                  <td>${escapeHtml(task.agent_id || '')}</td>
                  <td>${agentStatusBadge(task.status)}</td>
                  <td>${escapeHtml(String(task.result || task.error || '').slice(0, 96) || '-')}</td>
                </tr>
              `).join('') : '<tr><td colspan="4" class="muted">No tasks yet.</td></tr>'}
            </tbody>
          </table></div>
          <div class="cockpit-admin-summary">Graph: ${Object.keys(graph.nodes || {}).length} nodes · ${(graph.edges || []).length} edges</div>
        </div>
      `;
      panel.body.querySelector(`#admin-launch-${CSS.escape(panel.id)}`)?.addEventListener('submit', async (ev) => {
        ev.preventDefault();
        const fd = new FormData(ev.currentTarget);
        await postJson('/api/orchestrator/launch', {
          ...(() => {
            const agent = String(fd.get('agent') || 'codex');
            const dispatchMode = String(fd.get('dispatch_mode') || 'pty');
            const hookupKind = String(fd.get('hookup_kind') || 'cli');
            const hookupValue = String(fd.get('hookup_value') || '').trim();
            const model = String(fd.get('model') || '').trim();
            const apiKeySource = String(fd.get('api_key_source') || 'vault:openrouter').trim();
            const payload = {
              agent,
              agent_name: String(fd.get('agent_name') || 'worker'),
              timeout_secs: 600,
              trace: true,
              capabilities: ['memory_read', 'memory_write'],
              dispatch_mode: dispatchMode,
            };
            if (dispatchMode === 'container') {
              if (hookupKind === 'api') {
                payload.container_hookup = {
                  kind: 'api',
                  provider: hookupValue || agent,
                  model: model || 'default',
                  api_key_source: apiKeySource || 'vault:openrouter',
                };
              } else if (hookupKind === 'local_model') {
                payload.container_hookup = {
                  kind: 'local_model',
                  model_id: hookupValue || model || 'Qwen/Qwen2.5-Coder-7B-Instruct',
                };
              } else {
                payload.container_hookup = {
                  kind: 'cli',
                  cli_name: hookupValue || agent,
                  model: model || null,
                };
              }
            }
            return payload;
          })(),
        });
        await render();
      });
      panel.body.querySelector(`#admin-task-${CSS.escape(panel.id)}`)?.addEventListener('submit', async (ev) => {
        ev.preventDefault();
        const fd = new FormData(ev.currentTarget);
        await postJson('/api/orchestrator/task', {
          agent_id: String(fd.get('agent_id') || ''),
          task: String(fd.get('task') || ''),
          timeout_secs: 300,
          wait: false,
        });
        await render();
      });
      panel.body.querySelectorAll('[data-stop-agent]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          await postJson('/api/orchestrator/stop', {
            agent_id: btn.dataset.stopAgent,
            force: true,
          });
          await render();
        });
      });
      panel.body.querySelectorAll('[data-show-identity]').forEach((btn) => {
        btn.addEventListener('click', () => {
          const didUri = btn.dataset.showIdentity;
          if (!didUri) {
            alert('No DID identity is attached to this agent.');
            return;
          }
          showJsonModal('Agent DID', { did_uri: didUri });
        });
      });
    };
    panel.setRefreshTimer(render, 3000);
    render().catch((e) => {
      panel.body.innerHTML = `<div class="cockpit-panel-error">${escapeHtml(String(e.message || e))}</div>`;
    });
  }

  function attachContainersPanel(panel) {
    const render = async () => {
      const payload = await fetchJson('/api/containers');
      const sessions = Array.isArray(payload.sessions) ? payload.sessions : [];
      panel.body.innerHTML = `
        <div class="cockpit-admin">
          <form class="cockpit-form cockpit-form-inline" id="containers-provision-${panel.id}">
            <div class="mesh-section-title">Provision Container</div>
            <input class="input" name="image" value="nucleusdb:latest" />
            <input class="input" name="agent_id" placeholder="optional-agent-id" />
            <button class="btn btn-sm btn-primary" type="submit">Provision</button>
          </form>
          <div class="table-wrap"><table>
            <thead><tr><th>Session</th><th>Agent</th><th>Lock</th><th>DID</th><th>Actions</th></tr></thead>
            <tbody>
              ${sessions.length ? sessions.map((session) => `
                <tr>
                  <td><code>${escapeHtml(session.session_id || '')}</code></td>
                  <td>${escapeHtml(session.agent_id || '')}</td>
                  <td>${agentStatusBadge(session.lock_state || 'empty')}</td>
                  <td>${didBadge(session.identity?.did_uri || session.did_uri)}</td>
                  <td>
                    <button class="btn btn-sm" data-init-session="${escapeHtml(session.session_id || '')}">Init</button>
                    <button class="btn btn-sm" data-deinit-session="${escapeHtml(session.session_id || '')}">Deinit</button>
                    <button class="btn btn-sm" data-logs-session="${escapeHtml(session.session_id || '')}">Logs</button>
                    <button class="btn btn-sm" data-identity-session="${escapeHtml(session.session_id || '')}">Identity</button>
                    <button class="btn btn-sm" data-destroy-session="${escapeHtml(session.session_id || '')}">Destroy</button>
                  </td>
                </tr>
              `).join('') : '<tr><td colspan="5" class="muted">No containers provisioned.</td></tr>'}
            </tbody>
          </table></div>
        </div>
      `;
      panel.body.querySelector(`#containers-provision-${CSS.escape(panel.id)}`)?.addEventListener('submit', async (ev) => {
        ev.preventDefault();
        const fd = new FormData(ev.currentTarget);
        await postJson('/api/containers/provision', {
          image: String(fd.get('image') || 'nucleusdb:latest'),
          agent_id: String(fd.get('agent_id') || '').trim() || null,
        });
        await render();
      });
      panel.body.querySelectorAll('[data-init-session]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const cliName = prompt('CLI hookup name', 'codex');
          if (!cliName) return;
          await postJson('/api/containers/initialize', {
            session_id: btn.dataset.initSession,
            hookup: { kind: 'cli', cli_name: cliName.trim().toLowerCase() },
            reuse_policy: 'reusable',
          });
          await render();
        });
      });
      panel.body.querySelectorAll('[data-deinit-session]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          await postJson('/api/containers/deinitialize', {
            session_id: btn.dataset.deinitSession,
          });
          await render();
        });
      });
      panel.body.querySelectorAll('[data-logs-session]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const logs = await fetchJson(`/api/containers/${encodeURIComponent(btn.dataset.logsSession)}/logs`);
          showJsonModal(`Container Logs: ${btn.dataset.logsSession}`, logs);
        });
      });
      panel.body.querySelectorAll('[data-identity-session]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const fresh = await fetchJson('/api/containers');
          const session = (fresh.sessions || []).find((item) => item.session_id === btn.dataset.identitySession);
          if (!session?.identity) {
            alert('No persisted identity document for this container.');
            return;
          }
          panel.setIdentity(session.identity);
          showJsonModal(`Container Identity: ${session.session_id}`, session.identity);
        });
      });
      panel.body.querySelectorAll('[data-destroy-session]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          if (!confirm(`Destroy ${btn.dataset.destroySession}?`)) return;
          await deleteJson(`/api/containers/${encodeURIComponent(btn.dataset.destroySession)}`);
          await render();
        });
      });
    };
    panel.setRefreshTimer(render, 5000);
    render().catch((e) => {
      panel.body.innerHTML = `<div class="cockpit-panel-error">${escapeHtml(String(e.message || e))}</div>`;
    });
  }

  function attachWorkflowPanel(panel) {
    const applyTemplate = async (template, agents, tasks) => {
      const completed = tasks.filter((task) => String(task.status || '').toLowerCase() === 'complete');
      if (!completed.length) {
        alert('Complete at least one task before applying a workflow template.');
        return;
      }
      if (template === 'review-fix' && agents.length >= 2) {
        await postJson('/api/orchestrator/pipe', {
          source_task_id: completed[0].task_id,
          target_agent_id: agents[1].agent_id,
          transform: 'claude_answer',
          task_prefix: 'Fix the following findings:\n\n',
        });
      } else if (template === 'research-implement-test' && agents.length >= 3) {
        await postJson('/api/orchestrator/pipe', {
          source_task_id: completed[0].task_id,
          target_agent_id: agents[1].agent_id,
          transform: 'claude_answer',
          task_prefix: 'Implement this plan:\n\n',
        });
        await postJson('/api/orchestrator/pipe', {
          source_task_id: completed[0].task_id,
          target_agent_id: agents[2].agent_id,
          transform: 'claude_answer',
          task_prefix: 'Test the resulting changes:\n\n',
        });
      } else if (template === 'consensus' && agents.length >= 3) {
        for (const agent of agents.slice(1, 3)) {
          await postJson('/api/orchestrator/pipe', {
            source_task_id: completed[0].task_id,
            target_agent_id: agent.agent_id,
            transform: 'identity',
            task_prefix: 'Independently solve the same task:\n\n',
          });
        }
      }
    };

    const drawWorkflow = (canvas, agents, graph) => {
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      const width = canvas.width = canvas.clientWidth || 600;
      const height = canvas.height = canvas.clientHeight || 240;
      ctx.clearRect(0, 0, width, height);
      const positions = new Map();
      const gap = width / Math.max(agents.length, 1);
      agents.forEach((agent, idx) => {
        const x = gap * idx + gap / 2;
        const y = height / 2;
        positions.set(agent.agent_id, { x, y });
      });
      (graph.edges || []).forEach((edge) => {
        const sourceNode = graph.nodes?.[edge.source_task_id];
        const from = positions.get(sourceNode?.agent_id);
        const to = positions.get(edge.target_agent_id);
        if (!from || !to) return;
        ctx.strokeStyle = '#ffb830';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.lineTo(to.x, to.y);
        ctx.stroke();
      });
      agents.forEach((agent) => {
        const pos = positions.get(agent.agent_id);
        if (!pos) return;
        ctx.fillStyle = '#0d1408';
        ctx.strokeStyle = '#00ff41';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.arc(pos.x, pos.y, 26, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = '#00ff41';
        ctx.font = '11px "Share Tech Mono", monospace';
        ctx.textAlign = 'center';
        ctx.fillText((agent.agent_name || agent.agent_id).slice(0, 12), pos.x, pos.y + 4);
        ctx.fillStyle = '#7bb07b';
        ctx.fillText(truncateDid(agent.did_uri || 'no DID'), pos.x, pos.y + 42);
      });
    };

    const render = async () => {
      const [agentsRes, tasksRes, graphRes] = await Promise.all([
        fetchJson('/api/orchestrator/agents'),
        fetchJson('/api/orchestrator/tasks'),
        fetchJson('/api/orchestrator/graph'),
      ]);
      const agents = Array.isArray(agentsRes.agents) ? agentsRes.agents : [];
      const tasks = Array.isArray(tasksRes.tasks) ? tasksRes.tasks : [];
      const graph = graphRes.graph || { nodes: {}, edges: [] };
      panel.body.innerHTML = `
        <div class="cockpit-workflow">
          <div class="workflow-toolbar">
            <select class="input" id="workflow-source-${panel.id}">
              ${tasks.map((task) => `<option value="${escapeHtml(task.task_id || '')}">${escapeHtml(task.task_id || '')}</option>`).join('')}
            </select>
            <select class="input" id="workflow-target-${panel.id}">
              ${agents.map((agent) => `<option value="${escapeHtml(agent.agent_id || '')}">${escapeHtml(agent.agent_name || agent.agent_id)}</option>`).join('')}
            </select>
            <select class="input" id="workflow-transform-${panel.id}">
              <option value="identity">identity</option>
              <option value="claude_answer">claude_answer</option>
              <option value="json_extract:.result">json_extract:.result</option>
            </select>
            <button class="btn btn-sm btn-primary" id="workflow-add-${panel.id}">+ Add Pipe</button>
            <select class="input" id="workflow-template-${panel.id}">
              <option value="">Template...</option>
              <option value="review-fix">Review → Fix</option>
              <option value="research-implement-test">Research → Implement → Test</option>
              <option value="consensus">Consensus</option>
            </select>
          </div>
          <canvas id="workflow-canvas-${panel.id}" class="workflow-canvas"></canvas>
          <div class="table-wrap">
            <table>
              <thead><tr><th>Task</th><th>Agent</th><th>Status</th><th>Edge Count</th></tr></thead>
              <tbody>
                ${tasks.slice(-8).reverse().map((task) => {
                  const edgeCount = (graph.edges || []).filter((edge) => edge.source_task_id === task.task_id).length;
                  return `<tr>
                    <td><code>${escapeHtml(task.task_id || '')}</code></td>
                    <td>${escapeHtml(task.agent_id || '')}</td>
                    <td>${agentStatusBadge(task.status)}</td>
                    <td>${edgeCount}</td>
                  </tr>`;
                }).join('') || '<tr><td colspan="4" class="muted">No workflow tasks yet.</td></tr>'}
              </tbody>
            </table>
          </div>
        </div>
      `;
      const canvas = panel.body.querySelector(`#workflow-canvas-${CSS.escape(panel.id)}`);
      if (canvas) drawWorkflow(canvas, agents, graph);
      panel.body.querySelector(`#workflow-add-${CSS.escape(panel.id)}`)?.addEventListener('click', async () => {
        const source = panel.body.querySelector(`#workflow-source-${CSS.escape(panel.id)}`)?.value;
        const target = panel.body.querySelector(`#workflow-target-${CSS.escape(panel.id)}`)?.value;
        const transform = panel.body.querySelector(`#workflow-transform-${CSS.escape(panel.id)}`)?.value;
        if (!source || !target) return;
        await postJson('/api/orchestrator/pipe', {
          source_task_id: source,
          target_agent_id: target,
          transform,
        });
        await render();
      });
      panel.body.querySelector(`#workflow-template-${CSS.escape(panel.id)}`)?.addEventListener('change', async (ev) => {
        const template = ev.target.value;
        if (!template) return;
        await applyTemplate(template, agents, tasks);
        ev.target.value = '';
        await render();
      });
    };
    panel.setRefreshTimer(render, 3000);
    render().catch((e) => {
      panel.body.innerHTML = `<div class="cockpit-panel-error">${escapeHtml(String(e.message || e))}</div>`;
    });
  }

  function attachChannelPanel(panel) {
    panel.channelEvents = [];
    panel.channelSnapshot = { peers: [], edgeCount: 0 };
    if (panel.eventSource) {
      try { panel.eventSource.close(); } catch (_e) {}
      panel.eventSource = null;
    }
    const pushEvent = (entry) => {
      panel.channelEvents.unshift(entry);
      panel.channelEvents = panel.channelEvents.slice(0, 40);
    };
    const renderEvents = () => {
      panel.body.innerHTML = `
        <div class="cockpit-channel">
          <div class="mesh-section-title">Agent Channel</div>
          <div class="cockpit-channel-list">
            ${panel.channelEvents.length ? panel.channelEvents.map((entry) => `
              <div class="cockpit-channel-entry">
                <div><strong>${escapeHtml(entry.type)}</strong> · ${escapeHtml(entry.timestamp)}</div>
                <div>${escapeHtml(entry.message)}</div>
              </div>
            `).join('') : '<div class="muted">Waiting for mesh or workflow traffic.</div>'}
          </div>
        </div>
      `;
    };
    const refresh = async () => {
      const [mesh, graph] = await Promise.all([
        fetchJson('/api/orchestrator/mesh'),
        fetchJson('/api/orchestrator/graph'),
      ]);
      const peers = Array.isArray(mesh.peers) ? mesh.peers.map((peer) => peer.agent_id).sort() : [];
      const edgeCount = Array.isArray(graph.graph?.edges) ? graph.graph.edges.length : 0;
      if (JSON.stringify(peers) !== JSON.stringify(panel.channelSnapshot.peers)) {
        pushEvent({
          type: 'peer_announce',
          timestamp: new Date().toLocaleTimeString(),
          message: peers.length ? `mesh peers: ${peers.join(', ')}` : 'mesh peers cleared',
        });
      }
      if (edgeCount !== panel.channelSnapshot.edgeCount) {
        pushEvent({
          type: 'task_pipe',
          timestamp: new Date().toLocaleTimeString(),
          message: `workflow edge count changed to ${edgeCount}`,
        });
      }
      pushEvent({
        type: 'heartbeat',
        timestamp: new Date().toLocaleTimeString(),
        message: `mesh ${mesh.enabled ? 'enabled' : 'disabled'} · ${peers.length} peer(s)`,
      });
      panel.channelSnapshot = { peers, edgeCount };
      renderEvents();
    };
    try {
      panel.eventSource = new EventSource('/events');
      panel.eventSource.addEventListener('heartbeat', () => {
        pushEvent({
          type: 'heartbeat',
          timestamp: new Date().toLocaleTimeString(),
          message: 'dashboard event stream heartbeat',
        });
        renderEvents();
      });
      panel.eventSource.addEventListener('session_update', (ev) => {
        pushEvent({
          type: 'session_update',
          timestamp: new Date().toLocaleTimeString(),
          message: String(ev.data || '').slice(0, 180),
        });
        renderEvents();
      });
    } catch (_e) {}
    panel.setRefreshTimer(refresh, 5000);
    refresh().catch((e) => {
      panel.body.innerHTML = `<div class="cockpit-panel-error">${escapeHtml(String(e.message || e))}</div>`;
    });
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
