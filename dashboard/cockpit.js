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
    '8':  [{ x: 0, y: 0, w: 0.25, h: 0.5 }, { x: 0.25, y: 0, w: 0.25, h: 0.5 }, { x: 0.5, y: 0, w: 0.25, h: 0.5 }, { x: 0.75, y: 0, w: 0.25, h: 0.5 },
           { x: 0, y: 0.5, w: 0.25, h: 0.5 }, { x: 0.25, y: 0.5, w: 0.25, h: 0.5 }, { x: 0.5, y: 0.5, w: 0.25, h: 0.5 }, { x: 0.75, y: 0.5, w: 0.25, h: 0.5 }],
    '9':  [{ x: 0, y: 0, w: 1/3, h: 1/3 }, { x: 1/3, y: 0, w: 1/3, h: 1/3 }, { x: 2/3, y: 0, w: 1/3, h: 1/3 },
           { x: 0, y: 1/3, w: 1/3, h: 1/3 }, { x: 1/3, y: 1/3, w: 1/3, h: 1/3 }, { x: 2/3, y: 1/3, w: 1/3, h: 1/3 },
           { x: 0, y: 2/3, w: 1/3, h: 1/3 }, { x: 1/3, y: 2/3, w: 1/3, h: 1/3 }, { x: 2/3, y: 2/3, w: 1/3, h: 1/3 }],
    '10': [{ x: 0, y: 0, w: 0.2, h: 0.5 }, { x: 0.2, y: 0, w: 0.2, h: 0.5 }, { x: 0.4, y: 0, w: 0.2, h: 0.5 }, { x: 0.6, y: 0, w: 0.2, h: 0.5 }, { x: 0.8, y: 0, w: 0.2, h: 0.5 },
           { x: 0, y: 0.5, w: 0.2, h: 0.5 }, { x: 0.2, y: 0.5, w: 0.2, h: 0.5 }, { x: 0.4, y: 0.5, w: 0.2, h: 0.5 }, { x: 0.6, y: 0.5, w: 0.2, h: 0.5 }, { x: 0.8, y: 0.5, w: 0.2, h: 0.5 }],
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
      this.terminalInputEl = null;

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
          <button type="button" data-action="new" title="New lane">+</button>
          <button type="button" data-action="maximize" title="Maximize">□</button>
          <button type="button" data-action="reset" title="Reset agent" class="is-hidden">↺</button>
          <button type="button" data-action="close" title="Close">×</button>
        </div>`;
      this.body = document.createElement('div');
      this.body.className = 'cockpit-panel-body';

      this.el.appendChild(header);
      this.el.appendChild(this.body);

      header.addEventListener('dblclick', () => this.el.classList.toggle('maximized'));
      header.querySelector('[data-action="new"]').addEventListener('click', (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        this.manager?.toggleNewDropdown(ev.currentTarget, {
          sourcePanelId: this.id,
          sourceAgentType: this.agentType || null,
        });
      });
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

    setResetAction(handler) {
      const btn = this.el.querySelector('[data-action="reset"]');
      if (!btn) return;
      btn.classList.toggle('is-hidden', typeof handler !== 'function');
      btn.onclick = null;
      if (typeof handler === 'function') {
        btn.onclick = (ev) => {
          ev.preventDefault();
          ev.stopPropagation();
          handler();
        };
      }
    }

    attachChat(agentId, agentType, options = {}) {
      const panelSelf = this;
      this.agentId = agentId;
      this.agentType = agentType;
      this.chatMessages = [];
      const history = Array.isArray(options.history) ? options.history : [];
      const subtitle = options.workingDir
        ? `Headless API mode · ${options.workingDir}`
        : 'Headless API mode · type a task below';
      this.body.innerHTML = `
        <div class="cockpit-chat-shell">
          <div class="cockpit-chat-thread" id="chat-thread-${escapeHtml(this.id)}">
            <div class="chat-welcome">
              <div class="chat-welcome-icon">${agentType === 'claude' ? '⚡' : agentType === 'codex' ? '⌁' : agentType === 'gemini' ? '◇' : '▣'}</div>
              <div class="chat-welcome-title">${escapeHtml(agentType)} agent ready</div>
              <div class="chat-welcome-sub">${escapeHtml(subtitle)}</div>
            </div>
          </div>
          <form class="cockpit-chat-composer" id="chat-form-${escapeHtml(this.id)}">
            <textarea class="input cockpit-chat-input" rows="2" placeholder="Describe a task for ${escapeHtml(agentType)}…"></textarea>
            <div class="cockpit-chat-actions">
              <button type="submit" class="btn btn-sm btn-primary">Send</button>
              <button type="button" class="btn btn-sm" data-chat-clear="1">Clear</button>
            </div>
          </form>
        </div>
      `;
      const thread = this.body.querySelector(`#chat-thread-${CSS.escape(this.id)}`);
      const composer = this.body.querySelector(`#chat-form-${CSS.escape(this.id)}`);
      const input = this.body.querySelector('.cockpit-chat-input');
      const renderWelcome = () => {
        thread.innerHTML = `
          <div class="chat-welcome">
            <div class="chat-welcome-icon">${agentType === 'claude' ? '⚡' : agentType === 'codex' ? '⌁' : agentType === 'gemini' ? '◇' : '▣'}</div>
            <div class="chat-welcome-title">${escapeHtml(agentType)} agent ready</div>
            <div class="chat-welcome-sub">${escapeHtml(subtitle)}</div>
          </div>
        `;
      };

      const appendMessage = (role, content, meta) => {
        const msg = document.createElement('div');
        msg.className = `chat-msg chat-msg-${role}`;
        const metaHtml = meta ? `<div class="chat-msg-meta">${escapeHtml(meta)}</div>` : '';
        if (role === 'user') {
          msg.innerHTML = `<div class="chat-msg-bubble chat-bubble-user">${escapeHtml(content)}</div>${metaHtml}`;
        } else if (role === 'thinking') {
          msg.innerHTML = `<div class="chat-msg-bubble chat-bubble-thinking"><span class="chat-thinking-dot">●</span> Thinking…</div>`;
        } else if (role === 'error') {
          msg.innerHTML = `<div class="chat-msg-bubble chat-bubble-error"><pre class="chat-agent-text">${escapeHtml(content)}</pre></div>${metaHtml}`;
        } else {
          msg.innerHTML = `<div class="chat-msg-bubble chat-bubble-agent"><pre class="chat-agent-text">${escapeHtml(content)}</pre></div>${metaHtml}`;
        }
        thread.appendChild(msg);
        thread.scrollTop = thread.scrollHeight;
        return msg;
      };
      const hydrateHistory = (items) => {
        thread.innerHTML = '';
        if (!items.length) {
          renderWelcome();
          return;
        }
        items.forEach((item) => appendMessage(item.role, item.content, item.meta));
      };
      this.focusTerminal = () => {
        try { input?.focus(); } catch (_e) {}
      };
      hydrateHistory(history);

      const sendTask = async (text) => {
        appendMessage('user', text);
        const thinkingEl = appendMessage('thinking', '');
        input.disabled = true;
        try {
          const res = await fetch('/api/orchestrator/task', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              agent_id: agentId,
              task: text,
              timeout_secs: 600,
              wait: true,
            }),
          });
          thinkingEl.remove();
          if (!res.ok) {
            const err = await res.text();
            let errMsg = err;
            try { errMsg = JSON.parse(err).error || err; } catch (_e) {}
            appendMessage('error', `Error: ${errMsg}`, `HTTP ${res.status}`);
            return;
          }
          const payload = await res.json();
          const task = payload.task || payload;
          const answer = task.result || task.answer || task.error || '(no response)';
          const usage = task.usage || {};
          const tokens = (Number(usage.input_tokens || 0) + Number(usage.output_tokens || 0));
          const cost = Number(usage.cost_usd || 0);
          const parts = [];
          if (task.status) parts.push(task.status);
          if (tokens > 0) parts.push(`${tokens} tokens`);
          if (cost > 0) parts.push(`$${cost.toFixed(4)}`);
          if (task.task_id) parts.push(task.task_id);
          appendMessage('agent', answer, parts.join(' · '));
        } catch (e) {
          thinkingEl.remove();
          appendMessage('error', `Network error: ${e.message || e}`);
        } finally {
          input.disabled = false;
          input.focus();
        }
      };

      composer?.addEventListener('submit', (ev) => {
        ev.preventDefault();
        const value = String(input?.value || '').trim();
        if (!value) return;
        input.value = '';
        // Route @LETTER messages to the target agent's orchestrator task
        const atMatch = value.match(/^@([A-Z][A-Z0-9]*)\s+([\s\S]+)$/i);
        const mgr = panelSelf.manager;
        if (atMatch && mgr) {
          const targetLetter = atMatch[1].toUpperCase();
          const message = atMatch[2];
          const targetSessionId = mgr.findSessionByLetter(targetLetter);
          if (targetSessionId) {
            const entry = mgr.sessions.get(targetSessionId);
            const targetAgentId = entry?.panel?.agentType || entry?.agentId || 'shell';
            appendMessage('user', `\u2192 Agent ${targetLetter}: ${message}`);
            fetch('/api/orchestrator/task', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ agent_id: targetAgentId, task: message, timeout_secs: 600, wait: false }),
            }).then(res => {
              if (!res.ok) appendMessage('error', `Failed to route to Agent ${targetLetter}: HTTP ${res.status}`);
              else appendMessage('agent', `Message routed to Agent ${targetLetter}`);
            }).catch(e => appendMessage('error', `Route error: ${e.message}`));
          } else {
            appendMessage('error', `No agent with letter "${targetLetter}". Active: ${Array.from(mgr.agentLetters.values()).join(', ') || 'none'}`);
          }
          return;
        }
        sendTask(value);
      });
      input?.addEventListener('keydown', (ev) => {
        if (ev.key === 'Enter' && !ev.shiftKey) {
          ev.preventDefault();
          composer?.requestSubmit();
        }
      });
      this.body.querySelector('[data-chat-clear="1"]')?.addEventListener('click', () => {
        renderWelcome();
      });
    }

    attachTerminal(sessionId, wsUrl, onStatus) {
      this.sessionId = sessionId;
      this.wsUrl = wsUrl;
      this.body.innerHTML = `
        <div class="cockpit-terminal-shell">
          <div class="cockpit-terminal-host"></div>
          <form class="cockpit-terminal-composer">
            <textarea class="input cockpit-terminal-input" rows="2" placeholder="Type input for this session. Enter sends. Shift+Enter inserts a newline."></textarea>
            <div class="cockpit-terminal-actions">
              <button type="submit" class="btn btn-sm btn-primary">Send</button>
              <button type="button" class="btn btn-sm" data-terminal-enter="1">Enter</button>
              <button type="button" class="btn btn-sm" data-terminal-ctrlc="1">Ctrl-C</button>
              <button type="button" class="btn btn-sm" data-terminal-focus="1">Focus</button>
            </div>
          </form>
        </div>
      `;
      const terminalHost = this.body.querySelector('.cockpit-terminal-host');
      const composer = this.body.querySelector('.cockpit-terminal-composer');
      const input = this.body.querySelector('.cockpit-terminal-input');
      this.terminalInputEl = input;

      if (!window.Terminal || !window.FitAddon) {
        terminalHost.innerHTML = '<pre style="padding:10px;color:#ff3030">xterm.js not loaded.</pre>';
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

      this.term.open(terminalHost);
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
      composer?.addEventListener('submit', (ev) => {
        ev.preventDefault();
        const value = String(input?.value || '');
        if (!value.trim()) return;
        this.sendTerminalText(value, true);
        if (input) input.value = '';
      });
      input?.addEventListener('keydown', (ev) => {
        if (ev.key === 'Enter' && !ev.shiftKey) {
          ev.preventDefault();
          composer?.requestSubmit();
        }
      });
      this.body.querySelector('[data-terminal-enter="1"]')?.addEventListener('click', () => {
        this.sendTerminalText('\n', false);
      });
      this.body.querySelector('[data-terminal-ctrlc="1"]')?.addEventListener('click', () => {
        this.sendTerminalText('\u0003', false);
      });
      this.body.querySelector('[data-terminal-focus="1"]')?.addEventListener('click', () => {
        this.focusTerminal();
        try { input?.focus(); } catch (_e) {}
      });

      this.resizeObs = new ResizeObserver(() => this.fit());
      this.resizeObs.observe(terminalHost);

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
      this.body.innerHTML = `
        <div class="cockpit-log-toolbar">
          <select class="input cockpit-log-filter">
            <option value="all">All severities</option>
            <option value="info">Info</option>
            <option value="warn">Warn</option>
            <option value="error">Error</option>
          </select>
          <input class="input cockpit-log-search" placeholder="Filter log text">
        </div>
        <pre class="cockpit-log-stream"></pre>
      `;
      const out = this.body.querySelector('.cockpit-log-stream');
      const filterEl = this.body.querySelector('.cockpit-log-filter');
      const searchEl = this.body.querySelector('.cockpit-log-search');
      this.logEntries = [];
      const classify = (label, payload) => {
        const hay = `${label} ${payload || ''}`.toLowerCase();
        if (hay.includes('error') || hay.includes('failed')) return 'error';
        if (hay.includes('warn')) return 'warn';
        return 'info';
      };
      const renderLog = () => {
        const severity = filterEl?.value || 'all';
        const query = String(searchEl?.value || '').trim().toLowerCase();
        const lines = this.logEntries
          .filter((entry) => severity === 'all' || entry.severity === severity)
          .filter((entry) => !query || entry.line.toLowerCase().includes(query))
          .map((entry) => entry.line);
        out.textContent = lines.join('');
        out.scrollTop = out.scrollHeight;
      };
      const writeLine = (label, payload) => {
        const ts = new Date().toLocaleTimeString();
        const line = `[${ts}] ${label} ${payload || ''}\n`;
        this.logBuffer += line;
        this.logEntries.push({ severity: classify(label, payload), line });
        if (this.logEntries.length > 600) this.logEntries.shift();
        renderLog();
      };

      filterEl?.addEventListener('change', renderLog);
      searchEl?.addEventListener('input', renderLog);

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
          <div class="metric-row"><span>Total Tokens</span><span id="metric-in-${escapeHtml(this.id)}">0</span></div>
          <div class="metric-row"><span>Resolved Models</span><span id="metric-out-${escapeHtml(this.id)}">0</span></div>
          <div class="metric-row"><span>Total Cost</span><span id="metric-cost-${escapeHtml(this.id)}">$0.00</span></div>
          <div class="metric-row"><span>Telemetry</span><span id="metric-mode-${escapeHtml(this.id)}">estimated</span></div>
        </div>
        <div class="cockpit-metric-charts">
          <div class="cockpit-metric-chart-card">
            <div class="card-sub">Session sparkline</div>
            <canvas id="metric-sessions-chart-${escapeHtml(this.id)}" height="80"></canvas>
          </div>
          <div class="cockpit-metric-chart-card">
            <div class="card-sub">Cost sparkline</div>
            <canvas id="metric-cost-chart-${escapeHtml(this.id)}" height="80"></canvas>
          </div>
        </div>
      `;
      this.metricHistory = [];
      this.sessionChartEl = this.body.querySelector(`#metric-sessions-chart-${CSS.escape(this.id)}`);
      this.costChartEl = this.body.querySelector(`#metric-cost-chart-${CSS.escape(this.id)}`);
    }

    updateMetrics(rows) {
      if (this.type !== 'metrics') return;
      const list = Array.isArray(rows) ? rows : [];
      const sessions = list.length;
      const input = list.reduce((acc, s) => {
        const actual = Number(s.actual_total_tokens || 0);
        const estimated =
          Number(s.estimated_input_tokens || 0) +
          Number(s.estimated_output_tokens || 0);
        return acc + (actual > 0 ? actual : estimated);
      }, 0);
      const resolvedModels = new Set(
        list.map((s) => String(s.resolved_model || '')).filter(Boolean),
      );
      const cost = list.reduce((acc, s) => {
        const actual = Number(s.actual_total_cost_usd || 0);
        const estimated = Number(s.estimated_cost_usd || 0);
        return acc + (actual > 0 ? actual : estimated);
      }, 0);
      const actualCount = list.filter((s) => Number(s.actual_total_tokens || 0) > 0).length;
      const mode = actualCount > 0 ? `traced ${actualCount}/${sessions}` : 'estimated';
      const setText = (id, text) => {
        const el = this.body.querySelector(id);
        if (el) el.textContent = text;
      };
      setText(`#metric-sessions-${CSS.escape(this.id)}`, String(sessions));
      setText(`#metric-in-${CSS.escape(this.id)}`, String(input));
      setText(`#metric-out-${CSS.escape(this.id)}`, String(resolvedModels.size));
      setText(`#metric-cost-${CSS.escape(this.id)}`, formatUsd(cost));
      setText(`#metric-mode-${CSS.escape(this.id)}`, mode);
      this.metricHistory.push({
        label: new Date().toLocaleTimeString(),
        sessions,
        cost,
      });
      if (this.metricHistory.length > 24) this.metricHistory.shift();
      this.updateMetricCharts();
    }

    updateMetricCharts() {
      if (typeof Chart === 'undefined') return;
      const labels = this.metricHistory.map((point) => point.label);
      const sessionValues = this.metricHistory.map((point) => point.sessions);
      const costValues = this.metricHistory.map((point) => point.cost);
      const baseOptions = {
        responsive: true,
        maintainAspectRatio: false,
        plugins: { legend: { display: false } },
        scales: {
          x: { display: false },
          y: { display: false, beginAtZero: true },
        },
        elements: { point: { radius: 0 }, line: { tension: 0.25 } },
      };
      if (!this.sessionSparkline && this.sessionChartEl) {
        this.sessionSparkline = new Chart(this.sessionChartEl, {
          type: 'line',
          data: {
            labels,
            datasets: [{ data: sessionValues, borderColor: '#00ff41', backgroundColor: 'rgba(0,255,65,0.18)', fill: true }],
          },
          options: baseOptions,
        });
      }
      if (!this.costSparkline && this.costChartEl) {
        this.costSparkline = new Chart(this.costChartEl, {
          type: 'line',
          data: {
            labels,
            datasets: [{ data: costValues, borderColor: '#ffb830', backgroundColor: 'rgba(255,184,48,0.18)', fill: true }],
          },
          options: baseOptions,
        });
      }
      if (this.sessionSparkline) {
        this.sessionSparkline.data.labels = labels;
        this.sessionSparkline.data.datasets[0].data = sessionValues;
        this.sessionSparkline.update('none');
      }
      if (this.costSparkline) {
        this.costSparkline.data.labels = labels;
        this.costSparkline.data.datasets[0].data = costValues;
        this.costSparkline.update('none');
      }
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
      try {
        this.term.textarea?.focus();
      } catch (_e) {}
    }

    sendTerminalText(text, ensureNewline) {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return false;
      const payload = ensureNewline && !text.endsWith('\n') ? `${text}\n` : text;
      this.ws.send(payload);
      return true;
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
      if (this.sessionSparkline) {
        try { this.sessionSparkline.destroy(); } catch (_e) {}
      }
      if (this.costSparkline) {
        try { this.costSparkline.destroy(); } catch (_e) {}
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
      this.layoutOrder = ['1', '2h', '2v', '4', '3L', '3T', '6', '8', '9', '10'];
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
      this.noticeEl = null;
      this.noticeTimer = null;

      // Agent lettering system — letters are permanent per session (H4)
      // Restore from sessionStorage on page reload (F6 fix)
      this.nextLetterIndex = 0;
      this.agentLetters = new Map(); // sessionId → letter
      this.agentColors = new Map();  // sessionId → CSS class
      this._restoreLetterState();

      // Workflow sidebar state
      this.wfSidebarEl = null;
      this.selectedWorkflowId = null;
      this.selectedWorkflow = null;
      this.wfMiniGraph = null;
      this.wfMiniCanvas = null;
      this.activeWorkflowInstance = null;
      this.wfPollTimer = null;
      this.wfExecLog = [];
    }

    // ── Agent Lettering (H4: persistent within session, never reassigned) ──

    assignLetter(sessionId) {
      if (this.agentLetters.has(sessionId)) return this.agentLetters.get(sessionId);
      const idx = this.nextLetterIndex++;
      const suffix = idx < 26 ? '' : String(Math.floor(idx / 26));
      const letter = String.fromCharCode(65 + (idx % 26)) + suffix;
      this.agentLetters.set(sessionId, letter);
      this._persistLetterState();
      return letter;
    }

    _persistLetterState() {
      try {
        const data = {
          nextLetterIndex: this.nextLetterIndex,
          letters: Array.from(this.agentLetters.entries()),
        };
        sessionStorage.setItem('halo_agent_letters', JSON.stringify(data));
      } catch (_) { /* storage full or unavailable — non-critical */ }
      // Expose globally so mesh, orchestrator, and other surfaces can look up letters
      this._publishLetterRegistry();
    }

    _publishLetterRegistry() {
      const registry = {};
      this.agentLetters.forEach((letter, sessionId) => {
        const entry = this.sessions.get(sessionId);
        const agentType = entry?.panel?.agentType || entry?.agentType || 'agent';
        registry[sessionId] = { letter, agentType };
      });
      window.__haloAgentLetters = registry;
      document.dispatchEvent(new CustomEvent('halo-agent-letters-changed', {
        detail: { letters: registry },
      }));
    }

    /** Look up a session ID by its letter (case-insensitive). */
    findSessionByLetter(letter) {
      const target = String(letter).toUpperCase();
      for (const [sessionId, assignedLetter] of this.agentLetters) {
        if (assignedLetter === target) return sessionId;
      }
      return null;
    }

    _restoreLetterState() {
      try {
        const raw = sessionStorage.getItem('halo_agent_letters');
        if (!raw) return;
        const data = JSON.parse(raw);
        if (typeof data.nextLetterIndex === 'number') {
          this.nextLetterIndex = data.nextLetterIndex;
        }
        if (Array.isArray(data.letters)) {
          this.agentLetters = new Map(data.letters);
          this._publishLetterRegistry();
        }
      } catch (_) { /* corrupt data — start fresh */ }
    }

    getAgentColorClass(agentType) {
      const type = (agentType || '').toLowerCase();
      if (type === 'claude') return 'agent-claude';
      if (type === 'gemini') return 'agent-gemini';
      if (type === 'codex') return 'agent-codex';
      if (type === 'shell' || type === 'custom') return 'agent-shell';
      return 'agent-other';
    }

    letterBadgeHtml(sessionId, agentType) {
      const letter = this.agentLetters.get(sessionId) || '?';
      const colorClass = this.getAgentColorClass(agentType);
      return `<span class="agent-letter-badge ${colorClass}">${escapeHtml(letter)}</span>`;
    }

    // Get all active agents with letters for dropdown population
    getActiveAgentsForRoles() {
      const agents = [];
      this.sessions.forEach((entry, id) => {
        const letter = this.agentLetters.get(id);
        if (!letter) return;
        const agentType = entry.panel?.agentType || 'session';
        agents.push({ sessionId: id, letter, agentType, agentId: entry.agentId || id });
      });
      return agents;
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
      this.noticeEl = hostEl.querySelector('#cockpit-notice-bar');
      this.wfSidebarEl = hostEl.querySelector('#cockpit-wf-sidebar-body');
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
      this.initWorkflowSidebar();
    }

    renderSkeleton() {
      return `
        <div class="cockpit-container">
          <div class="cockpit-toolbar" id="cockpit-toolbar">
            ${this.layoutOrder.map(k => `<button type="button" class="layout-btn ${this.layout === k ? 'active' : ''}" data-layout="${k}">${k}</button>`).join('')}
            <div class="cockpit-notice-bar" id="cockpit-notice-bar" aria-live="polite"></div>
            <button type="button" class="btn btn-sm cockpit-new-btn" id="cockpit-new">+ New</button>
          </div>
          <div class="cockpit-main">
            <div class="cockpit-stage">
              <div class="cockpit-tabs" id="cockpit-tabs"></div>
              <div class="cockpit-grid" id="cockpit-grid"></div>
            </div>
            <aside class="cockpit-mesh-sidebar" id="cockpit-mesh-sidebar">
              <div class="cockpit-mesh-header">
                <span class="cockpit-mesh-title">&#9851; Workflows</span>
                <button type="button" class="cockpit-mesh-toggle" id="cockpit-mesh-toggle" title="Collapse sidebar">◀</button>
              </div>
              <div class="cockpit-mesh-body" id="cockpit-mesh-body">
                <div class="wf-sidebar-body" id="cockpit-wf-sidebar-body">
                  <div class="wf-section" id="wf-select-section">
                    <div class="wf-section-title">Workflow</div>
                    <select class="wf-select" id="wf-workflow-select">
                      <option value="">— Select Workflow —</option>
                    </select>
                    <div class="wf-mini-diagram" id="wf-mini-diagram">
                      <div class="wf-mini-empty">No workflow selected</div>
                    </div>
                  </div>
                  <div class="wf-section" id="wf-roles-section" style="display:none">
                    <div class="wf-section-title">Role Assignment</div>
                    <div id="wf-roles-list"></div>
                    <div class="wf-actions">
                      <button type="button" class="btn btn-sm btn-primary" id="wf-run-btn" disabled>&#9654; Run</button>
                      <button type="button" class="btn btn-sm" id="wf-stop-btn" disabled>&#9632; Stop</button>
                    </div>
                  </div>
                  <div class="wf-section" id="wf-exec-section" style="display:none">
                    <div class="wf-section-title">Execution Log</div>
                    <div class="wf-exec-log" id="wf-exec-log"></div>
                    <div class="wf-progress-bar" id="wf-progress-bar" style="display:none">
                      <div class="wf-progress-fill" id="wf-progress-fill" style="width:0%"></div>
                    </div>
                  </div>
                  <div class="wf-section" id="wf-agents-section">
                    <div class="wf-section-title">Active Agents</div>
                    <div id="wf-agents-list"></div>
                  </div>
                  <div class="wf-section" id="wf-joint-section" style="display:none">
                    <div class="wf-section-title">Active Workflow</div>
                    <div id="wf-joint-card"></div>
                  </div>
                </div>
              </div>
            </aside>
          </div>
        </div>`;
    }

    showNotice(message, tone = 'info', timeoutMs = 5000) {
      if (!this.noticeEl) return;
      const text = String(message || '').trim();
      if (!text) {
        this.noticeEl.textContent = '';
        this.noticeEl.className = 'cockpit-notice-bar';
        return;
      }
      const safeTone = ['info', 'success', 'warn'].includes(tone) ? tone : 'info';
      this.noticeEl.textContent = text;
      this.noticeEl.className = `cockpit-notice-bar is-visible tone-${safeTone}`;
      if (this.noticeTimer) clearTimeout(this.noticeTimer);
      this.noticeTimer = setTimeout(() => {
        if (!this.noticeEl) return;
        this.noticeEl.textContent = '';
        this.noticeEl.className = 'cockpit-notice-bar';
      }, timeoutMs);
    }

    bindUi(hostEl) {
      hostEl.querySelectorAll('[data-layout]').forEach((btn) => {
        btn.addEventListener('click', () => this.setLayout(btn.dataset.layout));
      });
      hostEl.querySelector('#cockpit-new').addEventListener('click', (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        this.toggleNewDropdown(ev.currentTarget);
      });
      hostEl.querySelector('#cockpit-mesh-toggle')?.addEventListener('click', () => {
        this.setMeshCollapsed(!this.meshCollapsed);
      });
      document.addEventListener('click', (ev) => {
        if (!this.newDropdown) return;
        const target = ev.target;
        const anchor = hostEl.querySelector('#cockpit-new');
        if (this.newDropdown.contains(target) || anchor?.contains(target)) return;
        this.hideDropdown();
      });
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
      const [ptyRes, agentRes] = await Promise.all([
        fetch('/api/cockpit/sessions'),
        fetch('/api/orchestrator/agents').catch(() => null),
      ]);
      if (!ptyRes.ok) return;
      const payload = await ptyRes.json();
      const rows = Array.isArray(payload.sessions) ? payload.sessions : [];
      const agentPayload = agentRes && agentRes.ok ? await agentRes.json() : { agents: [] };
      const agents = Array.isArray(agentPayload.agents) ? agentPayload.agents : [];
      this.lastSessionSnapshot = rows;
      const byId = new Map(rows.map((s) => [s.id, s]));
      const chatsByPanel = new Map(
        agents.filter((agent) => this.isChatAgent(agent)).map((agent) => [this.chatPanelId(agent.agent_id), agent]),
      );

      this.sessions.forEach((entry, id) => {
        const isSystemPanel = entry.panel.type === 'metrics' || entry.panel.type === 'log';
        if (isSystemPanel) return;
        if (entry.panel.type === 'chat') {
          const agent = chatsByPanel.get(id);
          if (!agent) return;
          this.updateTabStatus(id, this.chatTabState(agent.status));
          this.updateTabCost(id, Number(agent.total_cost_usd || 0));
          return;
        }
        const row = byId.get(id);
        if (!row) return;
        this.updateTabStatus(id, row.status || {});
        this.updateTabCost(
          id,
          Number(row.actual_total_cost_usd || row.estimated_cost_usd || 0),
        );
      });

      this.sessions.forEach((entry) => {
        if (entry.panel.type === 'metrics') {
          const chatRows = agents.filter((agent) => this.isChatAgent(agent)).map((agent) => ({
            id: this.chatPanelId(agent.agent_id),
            resolved_model: agent.model || null,
            actual_total_tokens: 0,
            actual_total_cost_usd: Number(agent.total_cost_usd || 0),
          }));
          entry.panel.updateMetrics(rows.concat(chatRows));
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
        // Look up letter badge from the global registry
        const reg = window.__haloAgentLetters || {};
        let badge = '';
        for (const [, info] of Object.entries(reg)) {
          // Match by agent_id prefix in peer name
          if (info.letter && name.toLowerCase().includes(info.agentType.toLowerCase())) {
            badge = `<span class="agent-letter-badge ${this.getAgentColorClass(info.agentType)}">${escapeHtml(info.letter)}</span> `;
            break;
          }
        }
        return `
          <div class="mesh-peer ${online ? 'mesh-peer-online' : 'mesh-peer-offline'}">
            <span class="mesh-indicator ${online ? 'mesh-online' : 'mesh-offline'}">${online ? '●' : '○'}</span>
            <div class="mesh-peer-info">
              <div class="mesh-peer-name">${badge}${escapeHtml(name)}</div>
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

    // ── Workflow Sidebar Methods ──────────────────────────────────

    async initWorkflowSidebar() {
      const selectEl = this.root?.querySelector('#wf-workflow-select');
      if (selectEl) {
        selectEl.addEventListener('change', () => this.onWorkflowSelected(selectEl.value));
      }
      const runBtn = this.root?.querySelector('#wf-run-btn');
      if (runBtn) runBtn.addEventListener('click', () => this.runSelectedWorkflow());
      const stopBtn = this.root?.querySelector('#wf-stop-btn');
      if (stopBtn) stopBtn.addEventListener('click', () => this.stopActiveWorkflow());
      await this.refreshWorkflowList();
      this.refreshAgentsList();
    }

    async refreshWorkflowList() {
      const selectEl = this.root?.querySelector('#wf-workflow-select');
      if (!selectEl) return;
      try {
        const res = await fetch('/api/workflows');
        if (!res.ok) return;
        const payload = await res.json();
        let workflows = Array.isArray(payload.workflows) ? payload.workflows : [];

        // Auto-seed built-in templates if no workflows exist yet
        if (workflows.length === 0) {
          await this._seedBuiltinWorkflows();
          const res2 = await fetch('/api/workflows');
          if (res2.ok) {
            const p2 = await res2.json();
            workflows = Array.isArray(p2.workflows) ? p2.workflows : [];
          }
        }

        const curVal = selectEl.value;
        selectEl.innerHTML = '<option value="">— Select Workflow —</option>';
        workflows.forEach((wf) => {
          const opt = document.createElement('option');
          opt.value = wf.workflow_id;
          opt.textContent = wf.name || wf.workflow_id;
          selectEl.appendChild(opt);
        });
        if (curVal && workflows.some((w) => w.workflow_id === curVal)) {
          selectEl.value = curVal;
        }
      } catch (e) {
        console.warn('[Cockpit] Failed to load workflows:', e);
      }
    }

    async _seedBuiltinWorkflows() {
      // Guard: only one tab seeds at a time (sessionStorage is per-tab, use localStorage for cross-tab)
      const SEED_KEY = 'halo_wf_seed_lock';
      const lockTs = localStorage.getItem(SEED_KEY);
      if (lockTs && Date.now() - Number(lockTs) < 30000) return; // another tab seeding within 30s
      localStorage.setItem(SEED_KEY, String(Date.now()));

      const templates = [
        {
          name: 'Proof + Hostile Audit Loop',
          workflow_id: 'builtin-proof-audit-loop',
          litegraph: {
            nodes: [
              { id: 1, type: 'halo/agent', pos: [100, 200], size: [240, 110], properties: { role_name: 'Prover', agent_type: 'codex', model: 'codex-5.4-high', skill_ref: 'formal-proof', prompt_template: 'Continue proving the current phase. Address all audit findings.', timeout_secs: 600 } },
              { id: 2, type: 'halo/transform', pos: [400, 200], size: [200, 60], properties: { transform_type: 'assistant_answer', transform_value: '' } },
              { id: 3, type: 'halo/agent', pos: [650, 200], size: [240, 110], properties: { role_name: 'Auditor', agent_type: 'claude', model: 'claude-opus-4-6', skill_ref: 'adversarial-audit', prompt_template: 'Perform a hostile audit. If no findings, respond: findings: 0', timeout_secs: 600 } },
              { id: 4, type: 'halo/decision', pos: [950, 200], size: [200, 120], properties: { condition_type: 'contains', condition_value: 'findings: 0', max_iterations: 5 } },
            ],
            links: [[1, 1, 0, 2, 0, 'string'], [2, 2, 0, 3, 0, 'string'], [3, 3, 0, 4, 0, 'string'], [4, 4, 1, 1, 0, 'string']],
            last_link_id: 4, last_node_id: 4,
          },
          halo_meta: { description: 'Prover completes a phase, auditor reviews. Loops until zero findings.', max_iterations: 5, role_definitions: {} },
        },
        {
          name: 'Translation Verification',
          workflow_id: 'builtin-translation-verify',
          litegraph: {
            nodes: [
              { id: 1, type: 'halo/agent', pos: [100, 200], size: [240, 110], properties: { role_name: 'Translator', agent_type: 'claude', model: 'claude-opus-4-6', skill_ref: 'meta-translation', prompt_template: 'Translate the given Lean code to Coq.', timeout_secs: 600 } },
              { id: 2, type: 'halo/agent', pos: [450, 200], size: [240, 110], properties: { role_name: 'Verifier', agent_type: 'codex', model: 'codex-5.4-high', skill_ref: '', prompt_template: 'Compile and verify translated Coq code. If clean: compilation: success', timeout_secs: 600 } },
              { id: 3, type: 'halo/decision', pos: [780, 200], size: [200, 120], properties: { condition_type: 'contains', condition_value: 'compilation: success', max_iterations: 3 } },
            ],
            links: [[1, 1, 0, 2, 0, 'string'], [2, 2, 0, 3, 0, 'string'], [3, 3, 1, 1, 0, 'string']],
            last_link_id: 3, last_node_id: 3,
          },
          halo_meta: { description: 'Translate between proof assistants, then verify compilation.', max_iterations: 3, role_definitions: {} },
        },
        {
          name: 'Paper Formalization Pipeline',
          workflow_id: 'builtin-paper-formalization',
          litegraph: {
            nodes: [
              { id: 1, type: 'halo/phase', pos: [60, 80], size: [220, 80], properties: { phase_name: 'Phase 1: Extract', phase_number: 1, description: 'Extract proof obligations from paper' } },
              { id: 2, type: 'halo/agent', pos: [60, 220], size: [240, 110], properties: { role_name: 'Extractor', agent_type: 'claude', model: 'claude-opus-4-6', skill_ref: 'paper-ingest', prompt_template: 'Extract all proof obligations from the paper.', timeout_secs: 600 } },
              { id: 3, type: 'halo/agent', pos: [380, 220], size: [240, 110], properties: { role_name: 'Planner', agent_type: 'claude', model: 'claude-sonnet-4-6', skill_ref: 'proof-strategy-polya', prompt_template: 'Create a formalization plan.', timeout_secs: 600 } },
              { id: 4, type: 'halo/agent', pos: [700, 220], size: [240, 110], properties: { role_name: 'Prover', agent_type: 'codex', model: 'codex-5.4-high', skill_ref: 'formal-proof', prompt_template: 'Formalize the planned obligations in Lean 4.', timeout_secs: 600 } },
              { id: 5, type: 'halo/agent', pos: [1020, 220], size: [240, 110], properties: { role_name: 'Auditor', agent_type: 'claude', model: 'claude-opus-4-6', skill_ref: 'adversarial-audit', prompt_template: 'Audit the formalization for sorry, vacuity, and mathematical correctness.', timeout_secs: 600 } },
            ],
            links: [[1, 1, 0, 2, 0, 'string'], [2, 2, 0, 3, 0, 'string'], [3, 3, 0, 4, 0, 'string'], [4, 4, 0, 5, 0, 'string']],
            last_link_id: 4, last_node_id: 5,
          },
          halo_meta: { description: 'Extract obligations from paper, plan formalization, prove, then audit.', max_iterations: 10, role_definitions: {} },
        },
      ];

      let seeded = 0;
      let errors = [];
      for (const t of templates) {
        try {
          const res = await fetch('/api/workflows', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ name: t.name, workflow_id: t.workflow_id, created_at: 0, updated_at: 0, version: 1, litegraph: t.litegraph, halo_meta: t.halo_meta }),
          });
          if (res.ok) seeded++;
          else if (res.status === 409) { /* duplicate — already exists, fine */ }
          else errors.push(`${t.name}: HTTP ${res.status}`);
        } catch (e) {
          errors.push(`${t.name}: ${e.message}`);
        }
      }
      localStorage.removeItem(SEED_KEY);
      if (errors.length > 0 && seeded === 0) {
        console.warn('[Cockpit] Failed to seed built-in workflows:', errors);
      }
    }

    async onWorkflowSelected(workflowId) {
      this.selectedWorkflowId = workflowId || null;
      this.selectedWorkflow = null;
      const rolesSection = this.root?.querySelector('#wf-roles-section');
      const miniDiagram = this.root?.querySelector('#wf-mini-diagram');

      if (!workflowId) {
        if (rolesSection) rolesSection.style.display = 'none';
        if (miniDiagram) miniDiagram.innerHTML = '<div class="wf-mini-empty">No workflow selected</div>';
        return;
      }

      try {
        const res = await fetch(`/api/workflows/${encodeURIComponent(workflowId)}`);
        if (!res.ok) return;
        const wf = await res.json();
        this.selectedWorkflow = wf;
        this.renderMiniDiagram(wf);
        this.renderRoleAssignment(wf);
      } catch (_e) {}
    }

    renderMiniDiagram(wf) {
      const container = this.root?.querySelector('#wf-mini-diagram');
      if (!container) return;
      container.innerHTML = '';

      // Use a simplified SVG diagram showing nodes and connections
      const litegraph = wf.litegraph || {};
      const nodes = Array.isArray(litegraph.nodes) ? litegraph.nodes : [];
      const links = Array.isArray(litegraph.links) ? litegraph.links : [];

      if (nodes.length === 0) {
        container.innerHTML = '<div class="wf-mini-empty">Empty workflow</div>';
        return;
      }

      // Compute bounding box of all nodes
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      nodes.forEach((n) => {
        const x = n.pos?.[0] ?? 0, y = n.pos?.[1] ?? 0;
        const w = n.size?.[0] ?? 160, h = n.size?.[1] ?? 60;
        minX = Math.min(minX, x);
        minY = Math.min(minY, y);
        maxX = Math.max(maxX, x + w);
        maxY = Math.max(maxY, y + h);
      });

      const padding = 20;
      const worldW = maxX - minX + 2 * padding;
      const worldH = maxY - minY + 2 * padding;

      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      svg.setAttribute('viewBox', `0 0 ${worldW} ${worldH}`);
      svg.style.width = '100%';
      svg.style.height = '100%';

      const nodeColors = { 'halo/agent': '#7c3aed', 'halo/decision': '#f59e0b', 'halo/transform': '#0ea5e9', 'halo/phase': '#22c55e' };

      // Draw links
      const nodeById = new Map();
      nodes.forEach((n) => nodeById.set(n.id, n));
      links.forEach((link) => {
        // litegraph link format: [linkId, originId, originSlot, targetId, targetSlot, type]
        const originNode = nodeById.get(link[1]);
        const targetNode = nodeById.get(link[3]);
        if (!originNode || !targetNode) return;
        const ox = (originNode.pos?.[0] ?? 0) - minX + padding + (originNode.size?.[0] ?? 160);
        const oy = (originNode.pos?.[1] ?? 0) - minY + padding + (originNode.size?.[1] ?? 60) / 2;
        const tx = (targetNode.pos?.[0] ?? 0) - minX + padding;
        const ty = (targetNode.pos?.[1] ?? 0) - minY + padding + (targetNode.size?.[1] ?? 60) / 2;
        const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
        line.setAttribute('x1', ox); line.setAttribute('y1', oy);
        line.setAttribute('x2', tx); line.setAttribute('y2', ty);
        line.setAttribute('stroke', 'rgba(255,255,255,0.25)');
        line.setAttribute('stroke-width', '1.5');
        svg.appendChild(line);
      });

      // Draw nodes
      nodes.forEach((n) => {
        const x = (n.pos?.[0] ?? 0) - minX + padding;
        const y = (n.pos?.[1] ?? 0) - minY + padding;
        const w = n.size?.[0] ?? 160;
        const h = n.size?.[1] ?? 60;
        const color = nodeColors[n.type] || '#6b7280';

        const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
        rect.setAttribute('x', x); rect.setAttribute('y', y);
        rect.setAttribute('width', w); rect.setAttribute('height', h);
        rect.setAttribute('rx', '4');
        rect.setAttribute('fill', color);
        rect.setAttribute('fill-opacity', '0.3');
        rect.setAttribute('stroke', color);
        rect.setAttribute('stroke-width', '1.5');
        svg.appendChild(rect);

        const title = n.properties?.role_name || n.title || n.type?.split('/')[1] || '';
        if (title) {
          const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
          text.setAttribute('x', x + w / 2);
          text.setAttribute('y', y + h / 2 + 4);
          text.setAttribute('text-anchor', 'middle');
          text.setAttribute('fill', '#fff');
          text.setAttribute('font-size', Math.min(12, w / (title.length * 0.7)));
          text.setAttribute('font-family', 'sans-serif');
          text.textContent = title.length > 12 ? title.slice(0, 11) + '...' : title;
          svg.appendChild(text);
        }
      });

      container.appendChild(svg);
    }

    renderRoleAssignment(wf) {
      const rolesSection = this.root?.querySelector('#wf-roles-section');
      const rolesList = this.root?.querySelector('#wf-roles-list');
      if (!rolesSection || !rolesList) return;

      // Extract roles from workflow — agent nodes define roles
      const roles = [];
      const meta = wf.halo_meta || {};
      const roleDefs = meta.role_definitions || {};
      Object.entries(roleDefs).forEach(([key, def]) => {
        roles.push({ key, name: def.role_name || key, agentType: def.agent_type || '' });
      });

      // Also scan litegraph nodes for agent nodes that might not be in role_definitions
      const litegraph = wf.litegraph || {};
      const nodes = Array.isArray(litegraph.nodes) ? litegraph.nodes : [];
      nodes.forEach((n) => {
        if (n.type === 'halo/agent') {
          const roleName = n.properties?.role_name || `Agent ${n.id}`;
          const key = String(n.id);
          if (!roles.some((r) => r.key === key || r.name === roleName)) {
            roles.push({ key, name: roleName, agentType: n.properties?.agent_type || '' });
          }
        }
      });

      if (roles.length === 0) {
        rolesSection.style.display = 'none';
        return;
      }

      rolesSection.style.display = '';
      const activeAgents = this.getActiveAgentsForRoles();

      rolesList.innerHTML = roles.map((role) => {
        const options = activeAgents.map((a) =>
          `<option value="${escapeHtml(a.agentId)}">${escapeHtml(a.letter)} — ${escapeHtml(a.agentType)}</option>`
        ).join('');
        return `
          <div class="wf-role-row" data-role-key="${escapeHtml(role.key)}">
            <span class="wf-role-label" title="${escapeHtml(role.name)}">${escapeHtml(role.name)}</span>
            <select class="wf-role-select" data-role="${escapeHtml(role.key)}">
              <option value="">— Auto —</option>
              ${options}
            </select>
          </div>
        `;
      }).join('');

      this.updateRunButton();
      rolesList.querySelectorAll('.wf-role-select').forEach((sel) => {
        sel.addEventListener('change', () => this.updateRunButton());
      });
    }

    updateRunButton() {
      const runBtn = this.root?.querySelector('#wf-run-btn');
      if (!runBtn) return;
      const isRunning = this.activeWorkflowInstance && this.activeWorkflowInstance.status === 'running';
      runBtn.disabled = !this.selectedWorkflowId || isRunning;
    }

    async runSelectedWorkflow() {
      if (!this.selectedWorkflowId) return;
      const roleBindings = {};
      this.root?.querySelectorAll('.wf-role-select').forEach((sel) => {
        const role = sel.dataset.role;
        const agentId = sel.value;
        if (role && agentId) roleBindings[role] = agentId;
      });

      try {
        const res = await fetch(`/api/workflows/${encodeURIComponent(this.selectedWorkflowId)}/run`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ role_bindings: roleBindings }),
        });
        if (!res.ok) {
          const errData = await res.json().catch(() => ({}));
          this.showNotice(`Workflow run failed: ${errData.error || res.statusText}`, 'warn', 5000);
          return;
        }
        const payload = await res.json();
        this.activeWorkflowInstance = payload.instance || payload;
        this.showNotice('Workflow started', 'success', 3000);
        this.showExecSection(true);
        this.startWfPoll();
        this.updateRunButton();
        this.updateStopButton(true);
        this.updateJointIndicators();
      } catch (e) {
        this.showNotice(`Workflow run error: ${e.message}`, 'warn', 5000);
      }
    }

    async stopActiveWorkflow() {
      if (!this.activeWorkflowInstance) return;
      const instanceId = this.activeWorkflowInstance.instance_id;
      try {
        await fetch(`/api/workflows/${encodeURIComponent(this.selectedWorkflowId)}/stop`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ instance_id: instanceId }),
        });
        this.showNotice('Workflow stopped', 'info', 3000);
        this.stopWfPoll();
        this.activeWorkflowInstance = null;
        this.updateRunButton();
        this.updateStopButton(false);
        this.updateJointIndicators();
      } catch (_e) {}
    }

    showExecSection(show) {
      const section = this.root?.querySelector('#wf-exec-section');
      if (section) section.style.display = show ? '' : 'none';
    }

    updateStopButton(enabled) {
      const stopBtn = this.root?.querySelector('#wf-stop-btn');
      if (stopBtn) stopBtn.disabled = !enabled;
    }

    startWfPoll() {
      this.stopWfPoll();
      this.pollWorkflowStatus();
      this.wfPollTimer = setInterval(() => this.pollWorkflowStatus(), 3000);
    }

    stopWfPoll() {
      if (this.wfPollTimer) {
        clearInterval(this.wfPollTimer);
        this.wfPollTimer = null;
      }
    }

    async pollWorkflowStatus() {
      if (!this.selectedWorkflowId || !this.activeWorkflowInstance) return;
      try {
        const res = await fetch(`/api/workflows/${encodeURIComponent(this.selectedWorkflowId)}/status`);
        if (!res.ok) return;
        const payload = await res.json();
        const instance = payload.instance || payload;
        this.activeWorkflowInstance = instance;
        this.renderExecLog(instance);
        this.updateJointIndicators();

        const status = (instance.status || '').toLowerCase();
        if (status === 'completed' || status === 'failed' || status === 'stopped' || status === 'max_iterations_exceeded') {
          this.stopWfPoll();
          this.updateRunButton();
          this.updateStopButton(false);
          this.showNotice(`Workflow ${status}`, status === 'completed' ? 'success' : 'warn', 5000);
        }
      } catch (_e) {}
    }

    renderExecLog(instance) {
      const logEl = this.root?.querySelector('#wf-exec-log');
      const progressBar = this.root?.querySelector('#wf-progress-bar');
      const progressFill = this.root?.querySelector('#wf-progress-fill');
      if (!logEl) return;

      const events = Array.isArray(instance.events) ? instance.events : [];
      logEl.innerHTML = events.slice(-20).map((ev) => {
        const letter = ev.agent_letter || '';
        const badgeHtml = letter
          ? `<span class="agent-letter-badge agent-other" style="width:16px;height:16px;font-size:9px">${escapeHtml(letter)}</span>`
          : '';
        const evType = ev.event_type || {};
        let msgClass = '';
        if (typeof evType === 'string') {
          if (evType === 'workflow_completed' || evType === 'node_completed') msgClass = 'event-complete';
          if (evType === 'node_failed' || evType === 'workflow_failed') msgClass = 'event-error';
        }
        return `<div class="wf-exec-entry">${badgeHtml}<span class="wf-exec-msg ${msgClass}">${escapeHtml(ev.message || '')}</span></div>`;
      }).join('');

      logEl.scrollTop = logEl.scrollHeight;

      // Render progress for decision loops
      const iterations = instance.iteration_counts || {};
      const maxIter = this.selectedWorkflow?.halo_meta?.max_iterations || 10;
      const maxCount = Math.max(...Object.values(iterations), 0);
      if (maxCount > 0 && progressBar && progressFill) {
        progressBar.style.display = '';
        const pct = Math.min(100, (maxCount / maxIter) * 100);
        progressFill.style.width = `${pct}%`;
      } else if (progressBar) {
        progressBar.style.display = 'none';
      }
    }

    refreshAgentsList() {
      const listEl = this.root?.querySelector('#wf-agents-list');
      if (!listEl) return;

      const agents = this.getActiveAgentsForRoles();
      if (agents.length === 0) {
        listEl.innerHTML = '<div class="mesh-empty" style="font-size:11px;color:var(--text-dim)">No active agents</div>';
        return;
      }

      listEl.innerHTML = agents.map((a) => {
        const entry = this.sessions.get(a.sessionId);
        const statusState = entry?.status?.state || 'idle';
        const statusClass = statusState === 'active' ? 'status-running' : statusState === 'waiting' ? 'status-waiting' : 'status-idle';
        return `
          <div class="wf-agent-row">
            ${this.letterBadgeHtml(a.sessionId, a.agentType)}
            <span class="wf-agent-name">${escapeHtml(a.agentType)}</span>
            <span class="wf-agent-status ${statusClass}">${escapeHtml(statusState)}</span>
          </div>
        `;
      }).join('');
    }

    // ── Joint Workflow Indicators (H6) ──────────────────────────

    updateJointIndicators() {
      const jointSection = this.root?.querySelector('#wf-joint-section');
      const jointCard = this.root?.querySelector('#wf-joint-card');
      if (!jointSection || !jointCard) return;

      if (!this.activeWorkflowInstance || !this.selectedWorkflow) {
        jointSection.style.display = 'none';
        // Remove workflow indicators from tabs
        this.sessions.forEach((_entry, id) => {
          this.setTabWorkflowBadge(id, null);
        });
        return;
      }

      const inst = this.activeWorkflowInstance;
      const status = (inst.status || '').toLowerCase();
      const isRunning = status === 'running' || status === 'pending';

      if (!isRunning) {
        jointSection.style.display = 'none';
        this.sessions.forEach((_entry, id) => {
          this.setTabWorkflowBadge(id, null);
        });
        return;
      }

      // Find which sessions are bound to this workflow
      const bindings = inst.role_bindings || {};
      const boundAgentIds = new Set(Object.values(bindings));
      const boundSessions = [];

      this.sessions.forEach((entry, id) => {
        const agentId = entry.agentId || id;
        if (boundAgentIds.has(agentId)) {
          boundSessions.push({ sessionId: id, agentId, letter: this.agentLetters.get(id) || '?', agentType: entry.panel?.agentType || 'agent' });
        }
      });

      if (boundSessions.length < 2) {
        jointSection.style.display = 'none';
        return;
      }

      jointSection.style.display = '';
      const wfColor = '#ff6a00'; // accent
      const comboLetters = boundSessions.map((s) => s.letter).join('');
      const wfName = this.selectedWorkflow.name || this.selectedWorkflow.workflow_id || 'Workflow';

      const agentBadges = boundSessions.map((s) =>
        this.letterBadgeHtml(s.sessionId, s.agentType)
      ).join('<span class="wf-joint-arrow">↔</span>');

      const currentNode = inst.current_node ? ` — Node: ${escapeHtml(String(inst.current_node))}` : '';
      const iterations = inst.iteration_counts || {};
      const maxIter = this.selectedWorkflow?.halo_meta?.max_iterations || 10;
      const maxCount = Math.max(...Object.values(iterations), 0);
      const loopText = maxCount > 0 ? ` · Loop ${maxCount}/${maxIter}` : '';

      jointCard.innerHTML = `
        <div class="wf-joint-card">
          <div class="wf-joint-header">
            <span class="wf-joint-dot" style="background:${wfColor}"></span>
            <span class="wf-joint-name">${escapeHtml(wfName)}</span>
          </div>
          <div class="wf-joint-agents">
            ${agentBadges}
            <span class="wf-joint-combo">= ${escapeHtml(comboLetters)}</span>
          </div>
          <div class="wf-joint-status">Status: ${escapeHtml(status)}${currentNode}${loopText}</div>
        </div>
      `;

      // Set workflow badges on participating tabs
      boundSessions.forEach((s) => {
        this.setTabWorkflowBadge(s.sessionId, { comboLetters, color: wfColor, name: wfName });
      });
      // Clear badges from non-participating sessions
      this.sessions.forEach((_entry, id) => {
        if (!boundSessions.some((s) => s.sessionId === id)) {
          this.setTabWorkflowBadge(id, null);
        }
      });
    }

    setTabWorkflowBadge(sessionId, wfInfo) {
      const entry = this.sessions.get(sessionId);
      if (!entry?.tab) return;
      // Remove existing badges
      entry.tab.querySelectorAll('.tab-workflow-indicator,.tab-joint-badge').forEach((el) => el.remove());
      if (!wfInfo) return;
      const dot = document.createElement('span');
      dot.className = 'tab-workflow-indicator';
      dot.style.background = wfInfo.color;
      dot.title = `In workflow: ${wfInfo.name}`;
      entry.tab.appendChild(dot);
      const badge = document.createElement('span');
      badge.className = 'tab-joint-badge';
      badge.style.background = wfInfo.color + '44';
      badge.textContent = wfInfo.comboLetters;
      badge.title = `Joint workflow: ${wfInfo.name}`;
      entry.tab.appendChild(badge);
    }

    async fetchAgentState() {
      const [agentRes, taskRes] = await Promise.all([
        fetch('/api/orchestrator/agents').catch(() => null),
        fetch('/api/orchestrator/tasks').catch(() => null),
      ]);
      const agentPayload = agentRes && agentRes.ok ? await agentRes.json() : { agents: [] };
      const taskPayload = taskRes && taskRes.ok ? await taskRes.json() : { tasks: [] };
      return {
        agents: Array.isArray(agentPayload.agents) ? agentPayload.agents : [],
        tasks: Array.isArray(taskPayload.tasks) ? taskPayload.tasks : [],
      };
    }

    async attachExistingAgent(agentId) {
      const { agents, tasks } = await this.fetchAgentState();
      const agent = agents.find((item) => item && item.agent_id === agentId);
      if (!agent) throw new Error(`Unknown agent ${agentId}`);
      const history = tasks.filter((task) => task && task.agent_id === agentId);
      this.attachChatSession(agent, history);
    }

    async resetChatAgent(sessionId) {
      const entry = this.sessions.get(sessionId);
      const agentId = entry?.agentId || entry?.panel?.agentId;
      if (!agentId) return;
      const res = await fetch('/api/orchestrator/stop', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ agent_id: agentId, force: true, purge: true }),
      });
      if (!res.ok) {
        throw await buildApiError(res, '/api/orchestrator/stop');
      }
      const detached = this.detachSession(sessionId, true, {
        notice: {
          tone: 'success',
          message: `Agent ${agentId} reset. Launch a fresh lane when ready.`,
        },
      });
      if (!detached) {
        await this.restoreSessions();
        this.showNotice(`Agent ${agentId} reset on the server. Cockpit refreshed to recover local state.`, 'warn', 7000);
      }
    }

    async buildNewDropdownSections(context = {}) {
      const sourceAgentType = String(context.sourceAgentType || '').trim().toLowerCase();
      const { agents } = await this.fetchAgentState();
      const persistentAgents = agents
        .filter((agent) => this.isChatAgent(agent))
        .sort((left, right) => {
          const leftMatch = String(left?.agent_type || '').toLowerCase() === sourceAgentType ? -1 : 0;
          const rightMatch = String(right?.agent_type || '').toLowerCase() === sourceAgentType ? -1 : 0;
          if (leftMatch !== rightMatch) return leftMatch - rightMatch;
          return String(left?.agent_name || left?.agent_id || '').localeCompare(String(right?.agent_name || right?.agent_id || ''));
        });
      const sections = [
        {
          title: 'New Agent Lanes',
          items: [
            { id: 'claude', label: 'Claude', detail: 'Anthropic agent — headless API', icon: '⚡', needsPreflight: true },
            { id: 'codex', label: 'Codex', detail: 'OpenAI agent — headless API', icon: '⌁', needsPreflight: true },
            { id: 'gemini', label: 'Gemini', detail: 'Google agent — headless API', icon: '◇', needsPreflight: true },
          ],
        },
      ];
      if (persistentAgents.length) {
        sections.push({
          title: 'Existing Agents',
          items: persistentAgents.map((agent) => ({
            id: `attach:${agent.agent_id}`,
            label: agent.agent_name || agent.agent_id,
            detail: `${agent.agent_type} · ${agent.status || 'idle'} · ${((agent.identity_fingerprint || agent.identity_digest || agent.agent_id || '')).slice(0, 24)}`,
            icon: agent.agent_type === 'claude' ? '⚡' : agent.agent_type === 'codex' ? '⌁' : agent.agent_type === 'gemini' ? '◇' : '▣',
            needsPreflight: false,
          })),
        });
      }
      sections.push(
        {
          title: 'Terminals',
          items: [
            { id: 'shell', label: 'Shell', detail: 'Interactive bash PTY session', icon: '▣', needsPreflight: false },
            { id: 'custom', label: 'Custom PTY', detail: 'Run your own PTY command', icon: '⚙', needsPreflight: false },
          ],
        },
        {
          title: 'Panels',
          items: [
            { id: 'admin', label: 'Admin Panel', detail: 'Operational controls and task dispatch', icon: '⚙', needsPreflight: false },
            { id: 'containers', label: 'Containers', detail: 'Live container state and bootstrap mode', icon: '⬒', needsPreflight: false },
            { id: 'workflow', label: 'Workflow Builder', detail: 'Task graph orchestration', icon: '🔀', needsPreflight: false },
            { id: 'channel', label: 'Agent Channel', detail: 'Inter-agent message surface', icon: '⬡', needsPreflight: false },
            { id: 'metrics', label: 'Metrics Panel', detail: 'Session telemetry', icon: '📊', needsPreflight: false },
            { id: 'log', label: 'Log Stream', detail: 'Live event feed', icon: '📜', needsPreflight: false },
          ],
        },
      );
      return sections;
    }

    async toggleNewDropdown(anchor, context = {}) {
      if (this.newDropdown) {
        this.hideDropdown();
        return;
      }
      const sections = await this.buildNewDropdownSections(context);
      const menu = document.createElement('div');
      menu.className = 'cockpit-new-dropdown';
      sections.forEach((section) => {
        const sectionEl = document.createElement('div');
        sectionEl.className = 'dropdown-section';

        const titleEl = document.createElement('div');
        titleEl.className = 'dropdown-section-title';
        titleEl.textContent = String(section.title || '');
        sectionEl.appendChild(titleEl);

        section.items.forEach((item) => {
          const itemEl = document.createElement('div');
          itemEl.className = 'dropdown-item';
          itemEl.dataset.agent = String(item.id || '');

          const iconEl = document.createElement('span');
          iconEl.className = 'dropdown-icon';
          iconEl.textContent = String(item.icon || '');
          itemEl.appendChild(iconEl);

          const copyEl = document.createElement('span');
          copyEl.className = 'dropdown-copy';

          const labelEl = document.createElement('span');
          labelEl.className = 'dropdown-label';
          labelEl.textContent = String(item.label || '');
          copyEl.appendChild(labelEl);

          const detailEl = document.createElement('span');
          detailEl.className = 'dropdown-detail';
          detailEl.textContent = String(item.detail || '');
          copyEl.appendChild(detailEl);

          itemEl.appendChild(copyEl);

          if (item.needsPreflight) {
            const statusEl = document.createElement('span');
            statusEl.className = 'dropdown-status loading';
            statusEl.dataset.statusFor = String(item.id || '');
            statusEl.textContent = '…';
            itemEl.appendChild(statusEl);
          }

          sectionEl.appendChild(itemEl);
        });

        menu.appendChild(sectionEl);
      });
      const footnoteEl = document.createElement('div');
      footnoteEl.className = 'dropdown-footnote';
      footnoteEl.textContent =
        'Agent lanes use headless API dispatch with isolated AgentHALO homes. Shell and Custom PTY use the host terminal environment.';
      menu.appendChild(footnoteEl);
      menu.addEventListener('click', async (ev) => {
        ev.stopPropagation();
        const item = ev.target.closest('[data-agent]');
        if (!item) return;
        try {
          const target = String(item.dataset.agent || '');
          if (target.startsWith('attach:')) {
            await this.attachExistingAgent(target.slice('attach:'.length));
          } else {
            await this.createFromPreset(target);
          }
        } catch (e) {
          if (!(typeof window.trySetupRedirect === 'function' && window.trySetupRedirect(e, item.dataset.agent, 'cockpit'))) {
            alert(`Launch failed: ${e.message || e}`);
          }
        }
        this.hideDropdown();
      });

      const rect = anchor.getBoundingClientRect();
      menu.style.top = `${rect.bottom + window.scrollY + 4}px`;
      document.body.appendChild(menu);
      const maxLeft = window.scrollX + window.innerWidth - menu.offsetWidth - 12;
      const preferredLeft = rect.left + window.scrollX - 10;
      menu.style.left = `${Math.max(12 + window.scrollX, Math.min(preferredLeft, maxLeft))}px`;
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
          const pre = agent.id === 'shell'
            ? await this.fetchDeployPreflight(agent.id)
            : await this.fetchOrchestratorReadiness(agent.id);
          const readiness = pre.preflight || pre;
          statusEl.classList.remove('loading');
          if (pre.ready === true) {
            statusEl.classList.add('ready');
            statusEl.textContent = '● ready';
          } else if (readiness.cli_installed && readiness.keys_configured) {
            statusEl.classList.add('partial');
            statusEl.textContent = '◐ launch';
          } else if (readiness.cli_installed) {
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

    async fetchOrchestratorReadiness(agentId) {
      const res = await fetch('/api/orchestrator/readiness', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ agent_id: agentId }),
      });
      if (!res.ok) {
        throw await buildApiError(res, '/api/orchestrator/readiness');
      }
      return res.json();
    }

    isChatAgent(agent) {
      return !!(agent && agent.container_session_id && agent.agent_id);
    }

    chatPanelId(agentId) {
      return `chat-${agentId}`;
    }

    chatTabState(status) {
      const state = String(status || 'idle').toLowerCase();
      if (state === 'busy') return { state: 'waiting' };
      if (state === 'stopped') return { state: 'error' };
      return { state: 'active' };
    }

    buildChatHistory(tasks) {
      return [...(Array.isArray(tasks) ? tasks : [])]
        .sort((a, b) => {
          const left = Number(a?.started_at || a?.completed_at || 0);
          const right = Number(b?.started_at || b?.completed_at || 0);
          return left - right;
        })
        .flatMap((task) => {
          const items = [];
          const prompt = String(task?.prompt || '').trim();
          if (prompt) {
            items.push({ role: 'user', content: prompt, meta: task?.task_id || null });
          }
          const usage = task?.usage || {};
          const tokens = Number(usage.input_tokens || 0) + Number(usage.output_tokens || 0);
          const cost = Number(usage.estimated_cost_usd || 0);
          const meta = [];
          if (task?.status) meta.push(String(task.status));
          if (tokens > 0) meta.push(`${tokens} tokens`);
          if (cost > 0) meta.push(`$${cost.toFixed(4)}`);
          if (task?.task_id) meta.push(task.task_id);
          const taskMeta = meta.join(' · ');
          const status = String(task?.status || '').toLowerCase();
          if (status === 'running' || status === 'pending') {
            items.push({ role: 'thinking', content: '', meta: taskMeta });
            return items;
          }
          if (status === 'failed' || status === 'timeout') {
            items.push({
              role: 'error',
              content: task?.error || task?.result || '(no response)',
              meta: taskMeta,
            });
            return items;
          }
          items.push({
            role: 'agent',
            content: task?.result || task?.answer || task?.error || '(no response)',
            meta: taskMeta,
          });
          return items;
        });
    }

    attachChatSession(agent, tasks) {
      const panelId = this.chatPanelId(agent.agent_id);
      if (this.sessions.has(panelId)) {
        this.activateTab(panelId);
        return panelId;
      }

      const agentType = agent.agent_type || 'agent';
      this.assignLetter(panelId);
      const letter = this.agentLetters.get(panelId);

      const panel = new CockpitPanel(panelId, 'chat', `${agentType}:${agent.agent_id.slice(0, 8)}`, this);
      panel.agentType = agentType;

      // Add letter badge to panel header
      const titleEl = panel.el.querySelector('.cockpit-panel-title');
      if (titleEl && letter) {
        const badgeSpan = document.createElement('span');
        badgeSpan.innerHTML = this.letterBadgeHtml(panelId, agentType);
        titleEl.insertBefore(badgeSpan.firstChild, titleEl.firstChild);
      }

      const tab = this.createTab(panelId, agentType, agentType);
      const closeBtn = panel.el.querySelector('[data-action="close"]');
      if (closeBtn) {
        closeBtn.title = 'Close panel only (agent keeps running)';
        closeBtn.addEventListener('click', () => this.closeChatPanel(panelId));
      }
      panel.setResetAction(() => this.resetChatAgent(panelId));
      panel.attachChat(agent.agent_id, agent.agent_type, {
        history: this.buildChatHistory(tasks),
        workingDir: agent.working_dir,
      });

      this.gridEl.appendChild(panel.el);
      this.sessions.set(panelId, {
        panel,
        tab,
        status: this.chatTabState(agent.status),
        cost: Number(agent.total_cost_usd || 0),
        agentId: agent.agent_id,
        sessionKind: 'chat',
      });
      this.updateTabStatus(panelId, this.chatTabState(agent.status));
      this.updateTabCost(panelId, Number(agent.total_cost_usd || 0));
      this.activateTab(panelId);
      if (this.sessions.size === 2 && this.layout === '1') {
        this.setLayout('2h');
      }
      this.applyLayout();
      this.refreshAgentsList();
      return panelId;
    }

    closeChatPanel(sessionId) {
      const entry = this.sessions.get(sessionId);
      if (!entry) return;
      const agentId = entry.agentId || entry.panel?.agentId || sessionId;
      const agentType = entry.panel?.agentType || 'agent';
      this.detachSession(sessionId, true, {
        notice: {
          tone: 'info',
          message: `${agentType} lane detached. Agent ${agentId} is still running. Reattach it from + New -> Existing Agents.`,
        },
      });
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

      // Shell uses PTY; AI agents use headless API dispatch
      if (agent === 'shell') {
        await this.createSession('/bin/bash', [], 'shell');
        return;
      }

      const ready = await this.ensurePresetReady(agent);
      if (!ready) return;

      await this.launchAgentChat(agent);
    }

    async launchAgentChat(agentType) {
      const agentName = `${agentType}-${Date.now().toString(36)}`;
      const res = await fetch('/api/orchestrator/launch', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          agent: agentType,
          agent_name: agentName,
          timeout_secs: 600,
          trace: true,
          capabilities: ['memory_read', 'memory_write'],
          dispatch_mode: 'container',
          container_hookup: {
            kind: 'cli',
            cli_name: agentType,
            model: null,
          },
        }),
      });
      if (!res.ok) {
        throw await buildApiError(res, '/api/orchestrator/launch');
      }
      const payload = await res.json();
      const agentId = payload.agent_id || payload.id || agentName;
      this.attachChatSession({
        agent_id: agentId,
        agent_type: agentType,
        status: 'idle',
        total_cost_usd: 0,
        working_dir: payload.working_dir || null,
      }, []);
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
      const readiness = await this.fetchOrchestratorReadiness(agent);
      const pre = readiness.preflight || {};
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
      if (!readiness.ready) {
        const reasons = [];
        if (!readiness.mesh_network_ready) reasons.push('mesh registry is not writable');
        if (!readiness.mesh_secret_ready) reasons.push('mesh auth token could not be prepared');
        if (!readiness.mcp_server_path) reasons.push('agenthalo-mcp-server is not on PATH');
        alert(`${agent}: orchestrator launch is not ready. ${reasons.join('; ') || 'check native mesh worker bootstrap.'}`);
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
      const [ptyRes, agentRes, taskRes] = await Promise.all([
        fetch('/api/cockpit/sessions'),
        fetch('/api/orchestrator/agents').catch(() => null),
        fetch('/api/orchestrator/tasks').catch(() => null),
      ]);
      if (!ptyRes.ok) return;
      const payload = await ptyRes.json();
      const sessions = payload.sessions || [];
      const agentPayload = agentRes && agentRes.ok ? await agentRes.json() : { agents: [] };
      const taskPayload = taskRes && taskRes.ok ? await taskRes.json() : { tasks: [] };
      const agents = Array.isArray(agentPayload.agents) ? agentPayload.agents : [];
      const tasks = Array.isArray(taskPayload.tasks) ? taskPayload.tasks : [];
      const tasksByAgent = new Map();
      tasks.forEach((task) => {
        const agentId = task && task.agent_id;
        if (!agentId) return;
        if (!tasksByAgent.has(agentId)) tasksByAgent.set(agentId, []);
        tasksByAgent.get(agentId).push(task);
      });

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
      agents.filter((agent) => this.isChatAgent(agent)).forEach((agent) => {
        this.attachChatSession(agent, tasksByAgent.get(agent.agent_id) || []);
      });
      this.setLayout(this.layout);
    }

    attachSession(session) {
      if (this.sessions.has(session.id)) return;

      const agentType = session.agent_type || 'session';
      this.assignLetter(session.id);
      const letter = this.agentLetters.get(session.id);

      const panel = new CockpitPanel(session.id, 'terminal', `${agentType}:${session.id.slice(0, 8)}`, this);
      panel.agentType = agentType;
      if (session.identity) panel.setIdentity(session.identity);
      if (session.attestations) panel.setAttestations(session.attestations);

      // Add letter badge to panel header
      const titleEl = panel.el.querySelector('.cockpit-panel-title');
      if (titleEl && letter) {
        const badgeSpan = document.createElement('span');
        badgeSpan.innerHTML = this.letterBadgeHtml(session.id, agentType);
        titleEl.insertBefore(badgeSpan.firstChild, titleEl.firstChild);
      }

      const tab = this.createTab(session.id, agentType, agentType);

      panel.el.querySelector('[data-action="close"]').addEventListener('click', () => this.destroySession(session.id));
      panel.attachTerminal(session.id, session.ws_url, (statusMsg) => this.updateTabStatus(session.id, statusMsg));

      this.gridEl.appendChild(panel.el);
      this.sessions.set(session.id, { panel, tab, status: session.status || { state: 'active' }, cost: Number(session.actual_total_cost_usd || session.estimated_cost_usd || 0) });
      this.updateTabCost(session.id, Number(session.actual_total_cost_usd || session.estimated_cost_usd || 0));
      this.activateTab(session.id);
      if (this.sessions.size === 2 && this.layout === '1') {
        this.setLayout('2h');
      }
      this.refreshAgentsList();
    }

    createTab(sessionId, label, agentType) {
      const tab = document.createElement('button');
      tab.type = 'button';
      tab.className = 'cockpit-tab tab-active';
      tab.dataset.sessionId = sessionId;
      const letter = this.agentLetters.get(sessionId);
      const letterHtml = letter ? this.letterBadgeHtml(sessionId, agentType) : '<span class="tab-icon">●</span>';
      tab.innerHTML = `
        ${letterHtml}
        <span class="tab-label">${escapeHtml(label)}</span>
        <span class="tab-cost">${formatUsd(0)}</span>`;
      tab.addEventListener('click', () => this.activateTab(sessionId));
      tab.addEventListener('dblclick', () => {
        const panel = this.sessions.get(sessionId)?.panel?.el;
        if (panel) panel.classList.toggle('maximized');
      });
      tab.addEventListener('contextmenu', (ev) => {
        ev.preventDefault();
        const entry = this.sessions.get(sessionId);
        const isChat = entry?.panel?.type === 'chat';
        showContextMenu(ev.clientX, ev.clientY, [
          { label: isChat ? 'Close Panel (agent keeps running)' : 'Close', onClick: () => isChat ? this.closeChatPanel(sessionId) : this.destroySession(sessionId) },
          { label: 'Restart', onClick: () => this.restartSession(sessionId) },
          ...(isChat ? [{ label: 'Reset Agent', onClick: () => this.resetChatAgent(sessionId) }] : []),
          { label: 'Export', onClick: () => this.sessions.get(sessionId)?.panel?.exportLog() },
          { label: isChat ? 'Detach Panel (agent keeps running)' : 'Detach', onClick: () => isChat ? this.detachSession(sessionId, false, {
            notice: {
              tone: 'info',
              message: `Panel detached. Agent ${entry?.agentId || sessionId} is still running in the background.`,
            },
          }) : this.detachSession(sessionId) },
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
        hint.innerHTML = `
          <div class="empty-hint-title">No active sessions.</div>
          <div class="empty-hint-subtitle">Launch a new agent lane, attach an existing persistent agent, or open a shell terminal. Persistent agent lanes keep running after the panel closes and use isolated AgentHALO homes. Use <b>+ New</b> for additional panels.</div>
          <div class="empty-hint-actions">
            <button type="button" class="btn btn-sm btn-primary" data-launch-agent="claude">Start Claude</button>
            <button type="button" class="btn btn-sm" data-launch-agent="codex">Start Codex</button>
            <button type="button" class="btn btn-sm" data-launch-agent="gemini">Start Gemini</button>
            <button type="button" class="btn btn-sm" data-launch-agent="shell">Shell</button>
          </div>
          <div class="empty-hint-note">Agent lanes use the orchestrator API with structured JSON I/O, persistent trace identity, and isolated AgentHALO homes. Shell opens an interactive PTY terminal on the host environment.</div>
        `;
        hint.querySelectorAll('[data-launch-agent]').forEach((btn) => {
          btn.addEventListener('click', async (ev) => {
            ev.preventDefault();
            const agent = btn.dataset.launchAgent;
            try {
              await this.createFromPreset(agent);
            } catch (e) {
              if (!(typeof window.trySetupRedirect === 'function' && window.trySetupRedirect(e, agent, 'cockpit'))) {
                alert(`Launch failed: ${e.message || e}`);
              }
            }
          });
        });
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
      const entry = this.sessions.get(sessionId);
      if (entry && entry.panel.type === 'chat') {
        this.closeChatPanel(sessionId);
        return;
      }
      const res = await fetch(`/api/cockpit/sessions/${encodeURIComponent(sessionId)}`, { method: 'DELETE' });
      if (!res.ok) {
        alert(`Failed to close session ${sessionId}`);
        return;
      }
      this.detachSession(sessionId, true);
    }

    detachSession(sessionId, destroy = false, options = {}) {
      const entry = this.sessions.get(sessionId);
      if (!entry) return false;
      try {
        entry.tab.remove();
        if (destroy) {
          entry.panel.destroy();
        } else {
          entry.panel.el.remove();
        }
      } catch (e) {
        console.error('Failed to detach cockpit panel', sessionId, e);
        this.showNotice(`Cockpit UI failed while closing ${sessionId}. Refreshing restores the server state.`, 'warn', 7000);
      } finally {
        this.sessions.delete(sessionId);
      }
      const next = [...this.sessions.keys()][0] || null;
      this.activeTab = next;
      if (next) this.activateTab(next);
      this.applyLayout();
      this.refreshAgentsList();
      if (options.notice) {
        this.showNotice(options.notice.message, options.notice.tone || 'info');
      }
      return true;
    }

    async restartSession(sessionId) {
      const entry = this.sessions.get(sessionId);
      if (!entry) return;
      if (entry.panel.type === 'chat') {
        const agentType = entry.panel.agentType || 'claude';
        await this.resetChatAgent(sessionId);
        await this.launchAgentChat(agentType);
        return;
      }
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
            <select class="input" name="bootstrap_mode">
              <option value="required">password required</option>
              <option value="optional">password optional</option>
              <option value="disabled">passwordless review</option>
            </select>
            <button class="btn btn-sm btn-primary" type="submit">Provision</button>
          </form>
          <div class="table-wrap"><table>
            <thead><tr><th>Session</th><th>Agent</th><th>Lock</th><th>DID</th><th>Actions</th></tr></thead>
            <tbody>
              ${sessions.length ? sessions.map((session) => `
                <tr>
                  <td><code>${escapeHtml(session.session_id || '')}</code></td>
                  <td>${escapeHtml(session.agent_id || '')}</td>
                  <td>${lockStateBadge(session.lock_state || 'empty')}</td>
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
          bootstrap_mode: String(fd.get('bootstrap_mode') || 'required'),
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
