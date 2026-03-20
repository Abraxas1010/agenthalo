/* HeytingLean Observatory — Per-Panel Push-Based Visualization System
 *
 * Architecture:
 *   - Each CockpitPanel gets its own Observatory drawer on its right edge
 *   - Buttons do NOT fetch data — they are passive receivers
 *   - When an agent sends data (via chat/MCP tool response), the drawer unfurls
 *     and the relevant button lights up as active/clickable
 *   - Clicking an active button spawns a floating window with that data
 *   - Closing a window (X) resets the button for next use
 *   - Multiple floating windows can coexist across panels
 *
 * Visualization types (ATP-focused):
 *   prooftree  — Interactive proof tree (Gentzen-style / tactic tree)
 *   goals      — Current goal state with KaTeX-rendered types
 *   depgraph   — D3 force-directed dependency graph
 *   treemap    — Squarified treemap of file/module health
 *   tactics    — Tactic suggestion list with confidence scores
 *   latex      — LaTeX/KaTeX equation display
 *   flowchart  — Mermaid flowchart / sequence diagram
 *   table      — Sortable data table
 */
'use strict';
(function() {

  // ── Visualization type registry ────────────────────────────
  // endpoint: if set, button fetches data directly (always enabled)
  // endpoint: null → button is push-only (disabled until agent sends data)
  var VIZ_TYPES = [
    { id: 'dashboard', label: 'Dashboard',     icon: '\u{1F4CA}', endpoint: '/api/observatory/status' },
    { id: 'treemap',   label: 'Treemap',       icon: '\u{1F5FA}', endpoint: '/api/observatory/treemap' },
    { id: 'depgraph',  label: 'Dependencies',  icon: '\u{1F578}', endpoint: '/api/observatory/depgraph' },
    { id: 'clusters',  label: 'Clusters',      icon: '\u{1F52C}', endpoint: '/api/observatory/clusters' },
    { id: 'sorrys',    label: 'Sorrys',        icon: '\u{26A0}',  endpoint: '/api/observatory/sorrys' },
    { id: 'prooftree', label: 'Proof Tree',    icon: '\u{1F333}', endpoint: null },
    { id: 'goals',     label: 'Goals',         icon: '\u{1F3AF}', endpoint: null },
    { id: 'tactics',   label: 'Tactics',       icon: '\u{2694}',  endpoint: null },
    { id: 'latex',     label: 'Math',          icon: '\u{222B}',  endpoint: null },
    { id: 'flowchart', label: 'Flowchart',     icon: '\u{1F4C8}', endpoint: null },
    { id: 'table',     label: 'Data',          icon: '\u{1F4CB}', endpoint: '/api/observatory/complexity' },
  ];

  // ── Global state ───────────────────────────────────────────
  var topZ = 600;
  var allWindows = new Map(); // windowId → { el, panelId, vizType }

  function esc(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }

  // ── Mesh Command Registry ──────────────────────────────────
  var MESH_COMMANDS = [
    {
      id: 'clear', label: 'Clear', icon: '\u{1F9F9}',
      getCommandText: function(agentType) {
        if (agentType === 'shell' || agentType === 'custom') return 'clear';
        return '/clear';
      },
    },
    {
      id: 'compact', label: 'Compact', icon: '\u{1F4E6}',
      getCommandText: function(agentType) {
        if (agentType === 'shell' || agentType === 'custom') return null;
        if (agentType === 'gemini') return null;
        return '/compact';
      },
    },
    {
      id: 'credo', label: 'Credo', icon: '\u{1F4DC}',
      getCommandText: function(agentType) {
        if (agentType === 'shell' || agentType === 'custom') return null;
        return 'Invoke .agents/CREDO.md and recalibrate';
      },
    },
    {
      id: 'audit', label: 'Hostile Audit', icon: '\u{1F50D}',
      getCommandText: function(agentType) {
        if (agentType === 'shell' || agentType === 'custom') return null;
        return 'Run adversarial audit on the last commit using the adversarial-audit skill';
      },
    },
  ];

  function computeDirection(mySlot, theirSlot) {
    if (!mySlot || !theirSlot) return '\u25CF';
    var dx = (theirSlot.x + theirSlot.w / 2) - (mySlot.x + mySlot.w / 2);
    var dy = (theirSlot.y + theirSlot.h / 2) - (mySlot.y + mySlot.h / 2);
    if (Math.abs(dx) < 0.01 && Math.abs(dy) < 0.01) return '\u25CF';
    if (Math.abs(dx) >= Math.abs(dy)) return dx > 0 ? '\u261E' : '\u261C';
    return dy > 0 ? '\u261F' : '\u261D';
  }

  function dispatchCommand(sessionId, commandText) {
    var mgr = window.__cockpitManager;
    if (!mgr || typeof mgr.pushCommandToPanel !== 'function') return false;
    return mgr.pushCommandToPanel(sessionId, commandText);
  }

  function getCurrentSlots() {
    var layouts = window.__cockpitLayouts;
    if (!layouts) return [{ x: 0, y: 0, w: 1, h: 1 }];
    var mgr = window.__cockpitManager;
    var key = (mgr && mgr.layout) || '1';
    return layouts[key] || layouts['1'] || [{ x: 0, y: 0, w: 1, h: 1 }];
  }

  function getAgentColorClass(agentType) {
    var t = (agentType || '').toLowerCase();
    if (t === 'claude') return 'agent-claude';
    if (t === 'gemini') return 'agent-gemini';
    if (t === 'codex') return 'agent-codex';
    if (t === 'shell' || t === 'custom') return 'agent-shell';
    return 'agent-other';
  }

  // ══════════════════════════════════════════════════════════════
  // ObservatoryDrawer — one per CockpitPanel
  // ══════════════════════════════════════════════════════════════
  function ObservatoryDrawer(panelId) {
    this.panelId = panelId;
    this.collapsed = true;
    this.pendingData = {};  // vizType → data (waiting for user to click)
    this.activeWindows = new Set(); // vizTypes with open windows
    this.el = null;
    this.btnMap = {};
  }

  ObservatoryDrawer.prototype.render = function() {
    var self = this;
    var el = document.createElement('div');
    el.className = 'obs-drawer' + (this.collapsed ? ' collapsed' : '');

    // Build self-command buttons HTML
    var selfBtnsHtml = '';
    var registry = window.__haloAgentLetters || {};
    var myInfo = registry[this.panelId];
    var myAgentType = (myInfo && myInfo.agentType) || 'agent';
    MESH_COMMANDS.forEach(function(cmd) {
      var text = cmd.getCommandText(myAgentType);
      var isDisabled = text === null;
      selfBtnsHtml += '<button class="obs-cmd-self-btn"' +
        ' data-cmd-id="' + cmd.id + '"' +
        (isDisabled ? ' disabled' : '') +
        ' title="' + esc(cmd.label) + (isDisabled ? ' (N/A for ' + esc(myAgentType) + ')' : '') + '">' +
        '<span class="obs-cmd-icon">' + cmd.icon + '</span>' +
        '<span>' + esc(cmd.label) + '</span>' +
      '</button>';
    });

    el.innerHTML =
      '<div class="obs-drawer-tab" title="Observatory">' +
        '<span class="obs-drawer-tab-icon">\u{1F52D}</span>' +
      '</div>' +
      '<div class="obs-drawer-panel">' +
        '<div class="obs-drawer-tabs">' +
          '<button class="obs-drawer-tab-btn is-active" data-obs-tab="observe" title="Observe">\u{1F52D}</button>' +
          '<button class="obs-drawer-tab-btn" data-obs-tab="command" title="Mesh Commands">\u261E</button>' +
        '</div>' +
        '<div class="obs-tab-content" data-obs-content="observe">' +
          '<div class="obs-drawer-header">' +
            '<span class="obs-drawer-title">Observatory</span>' +
          '</div>' +
          '<div class="obs-drawer-buttons">' +
            VIZ_TYPES.map(function(v) {
              var hasEndpoint = !!v.endpoint;
              return '<button class="obs-viz-btn' + (hasEndpoint ? ' has-endpoint' : '') + '" data-viz="' + v.id + '"' +
                (hasEndpoint ? '' : ' disabled') +
                ' title="' + v.label + (hasEndpoint ? ' \u2014 click to load' : ' \u2014 waiting for agent data') + '">' +
                '<span class="obs-viz-icon">' + v.icon + '</span>' +
                '<span class="obs-viz-label">' + v.label + '</span>' +
                '<span class="obs-viz-dot"></span>' +
              '</button>';
            }).join('') +
          '</div>' +
        '</div>' +
        '<div class="obs-tab-content obs-tab-command" data-obs-content="command" style="display:none">' +
          '<div class="obs-cmd-section">' +
            '<div class="obs-cmd-section-title">Self</div>' +
            '<div class="obs-cmd-self-grid">' + selfBtnsHtml + '</div>' +
          '</div>' +
          '<div class="obs-cmd-section">' +
            '<div class="obs-cmd-section-title">Push to Agent</div>' +
            '<div class="obs-cmd-agent-grid"></div>' +
          '</div>' +
          '<div class="obs-cmd-section">' +
            '<button class="obs-cmd-broadcast-btn" title="Send to all agents">\u{1F4E1} Broadcast All</button>' +
            '<div class="obs-cmd-broadcast-input" style="display:none">' +
              '<textarea class="obs-cmd-custom-input" rows="2" placeholder="Message to all agents\u2026"></textarea>' +
              '<div style="display:flex;gap:3px;margin-top:3px">' +
                '<button class="obs-cmd-broadcast-send">Send</button>' +
                '<button class="obs-cmd-broadcast-cancel">Cancel</button>' +
              '</div>' +
            '</div>' +
          '</div>' +
        '</div>' +
      '</div>';

    // Toggle collapse
    el.querySelector('.obs-drawer-tab').addEventListener('click', function() {
      self.collapsed = !self.collapsed;
      el.classList.toggle('collapsed', self.collapsed);
    });

    // Tab switching
    this.activeTab = 'observe';
    el.querySelectorAll('.obs-drawer-tab-btn').forEach(function(tabBtn) {
      tabBtn.addEventListener('click', function() {
        var target = tabBtn.dataset.obsTab;
        self.activeTab = target;
        el.querySelectorAll('.obs-drawer-tab-btn').forEach(function(b) { b.classList.toggle('is-active', b === tabBtn); });
        el.querySelectorAll('.obs-tab-content').forEach(function(c) {
          c.style.display = c.dataset.obsContent === target ? '' : 'none';
        });
        if (target === 'command') {
          self.refreshAgentGrid();
          self.refreshSelfButtons();
        }
      });
    });

    // Poll agent registry every 2s while command tab is visible
    setInterval(function() {
      if (self.activeTab === 'command' && !self.collapsed) {
        self.refreshAgentGrid();
      }
    }, 2000);

    // Viz button clicks (existing logic — unchanged)
    el.querySelectorAll('.obs-viz-btn').forEach(function(btn) {
      var vizType = btn.dataset.viz;
      var vizMeta = VIZ_TYPES.find(function(v) { return v.id === vizType; });
      self.btnMap[vizType] = btn;
      btn.addEventListener('click', function() {
        if (self.pendingData[vizType]) {
          self.openWindow(vizType, self.pendingData[vizType]);
          return;
        }
        if (vizMeta && vizMeta.endpoint) {
          btn.disabled = true;
          btn.title = vizMeta.label + ' \u2014 loading...';
          fetch(vizMeta.endpoint)
            .then(function(res) { if (!res.ok) throw new Error('HTTP ' + res.status); return res.json(); })
            .then(function(data) {
              self.pendingData[vizType] = data;
              self.openWindow(vizType, data);
            })
            .catch(function(err) {
              btn.title = vizMeta.label + ' \u2014 error: ' + err.message;
            })
            .finally(function() {
              btn.disabled = false;
              btn.title = vizMeta.label + ' \u2014 click to load';
            });
        }
      });
    });

    // Self-command button clicks
    el.querySelectorAll('.obs-cmd-self-btn').forEach(function(btn) {
      btn.addEventListener('click', function() {
        if (btn.disabled) return;
        var cmdId = btn.dataset.cmdId;
        var cmd = MESH_COMMANDS.find(function(c) { return c.id === cmdId; });
        if (!cmd) return;
        var reg = window.__haloAgentLetters || {};
        var info = reg[self.panelId];
        var aType = (info && info.agentType) || 'agent';
        var text = cmd.getCommandText(aType);
        if (!text) return;
        dispatchCommand(self.panelId, text);
        btn.classList.add('obs-cmd-sent');
        setTimeout(function() { btn.classList.remove('obs-cmd-sent'); }, 800);
      });
    });

    // Broadcast button
    var broadcastBtn = el.querySelector('.obs-cmd-broadcast-btn');
    var broadcastInput = el.querySelector('.obs-cmd-broadcast-input');
    var broadcastSend = el.querySelector('.obs-cmd-broadcast-send');
    var broadcastCancel = el.querySelector('.obs-cmd-broadcast-cancel');
    var broadcastTextarea = broadcastInput ? broadcastInput.querySelector('textarea') : null;

    if (broadcastBtn) {
      broadcastBtn.addEventListener('click', function() {
        broadcastInput.style.display = '';
        broadcastBtn.style.display = 'none';
        if (broadcastTextarea) broadcastTextarea.focus();
      });
    }
    if (broadcastCancel) {
      broadcastCancel.addEventListener('click', function() {
        broadcastInput.style.display = 'none';
        broadcastBtn.style.display = '';
        if (broadcastTextarea) broadcastTextarea.value = '';
      });
    }
    if (broadcastSend) {
      broadcastSend.addEventListener('click', function() {
        var text = broadcastTextarea ? broadcastTextarea.value.trim() : '';
        if (!text) return;
        var reg = window.__haloAgentLetters || {};
        var sent = 0;
        Object.keys(reg).forEach(function(sessionId) {
          if (sessionId === self.panelId) return;
          if (dispatchCommand(sessionId, text)) sent++;
        });
        self.showCommandNotice('Broadcast sent to ' + sent + ' agent(s)');
        broadcastTextarea.value = '';
        broadcastInput.style.display = 'none';
        broadcastBtn.style.display = '';
      });
    }

    // Listen for agent letter changes
    document.addEventListener('halo-agent-letters-changed', function() {
      self.refreshAgentGrid();
      self.refreshSelfButtons();
    });

    this.el = el;
    return el;
  };

  // Called when agent pushes data for a visualization type
  ObservatoryDrawer.prototype.pushData = function(vizType, data) {
    this.pendingData[vizType] = data;
    var btn = this.btnMap[vizType];
    if (btn && !this.activeWindows.has(vizType)) {
      btn.disabled = false;
      btn.classList.add('has-data');
      btn.title = VIZ_TYPES.find(function(v) { return v.id === vizType; }).label + ' — click to view';
    }
    // Auto-unfurl the drawer when data arrives
    if (this.collapsed) {
      this.collapsed = false;
      if (this.el) this.el.classList.remove('collapsed');
    }
  };

  // Open a floating window for this viz type
  ObservatoryDrawer.prototype.openWindow = function(vizType, data) {
    var self = this;
    var winId = this.panelId + ':' + vizType;
    var meta = VIZ_TYPES.find(function(v) { return v.id === vizType; });
    if (!meta) return;

    // If already open, bring to front and update
    if (allWindows.has(winId)) {
      var existing = allWindows.get(winId);
      bringToFront(existing.el);
      renderViz(vizType, data, existing.el.querySelector('.obs-float-body'));
      return;
    }

    this.activeWindows.add(vizType);
    var btn = this.btnMap[vizType];
    if (btn) {
      btn.disabled = true;
      btn.classList.remove('has-data');
      btn.classList.add('window-open');
    }

    var win = createFloatingWindow(winId, meta.label, function() {
      // On close callback
      self.activeWindows.delete(vizType);
      allWindows.delete(winId);
      if (btn) {
        btn.classList.remove('window-open');
        // If there's still pending data, re-enable
        if (self.pendingData[vizType]) {
          btn.disabled = false;
          btn.classList.add('has-data');
        }
      }
    });

    renderViz(vizType, data, win.querySelector('.obs-float-body'));
    allWindows.set(winId, { el: win, panelId: this.panelId, vizType: vizType });
  };

  // ── Mesh Command Methods ──────────────────────────────────

  ObservatoryDrawer.prototype.refreshSelfButtons = function() {
    if (!this.el) return;
    var reg = window.__haloAgentLetters || {};
    var info = reg[this.panelId];
    var aType = (info && info.agentType) || 'agent';
    this.el.querySelectorAll('.obs-cmd-self-btn').forEach(function(btn) {
      var cmdId = btn.dataset.cmdId;
      var cmd = MESH_COMMANDS.find(function(c) { return c.id === cmdId; });
      if (!cmd) return;
      btn.disabled = cmd.getCommandText(aType) === null;
    });
  };

  ObservatoryDrawer.prototype.refreshAgentGrid = function() {
    if (!this.el) return;
    var self = this;
    var container = this.el.querySelector('.obs-cmd-agent-grid');
    if (!container) return;
    var registry = window.__haloAgentLetters || {};
    var slots = getCurrentSlots();
    var myEntry = registry[this.panelId];
    var mySlotIdx = myEntry ? myEntry.slotIndex : 0;
    var mySlot = slots[mySlotIdx % slots.length];
    var html = '';
    Object.keys(registry).forEach(function(sessionId) {
      if (sessionId === self.panelId) return;
      var info = registry[sessionId];
      var theirSlot = slots[(info.slotIndex || 0) % slots.length];
      var arrow = computeDirection(mySlot, theirSlot);
      var colorClass = getAgentColorClass(info.agentType);
      html +=
        '<button class="obs-cmd-agent-badge ' + colorClass + '"' +
        ' data-target-session="' + esc(sessionId) + '"' +
        ' data-target-type="' + esc(info.agentType || 'agent') + '"' +
        ' title="Send last response to Agent ' + esc(info.letter) + '">' +
          '<span class="obs-cmd-arrow">' + arrow + '</span>' +
          '<span class="obs-cmd-letter">' + esc(info.letter) + '</span>' +
        '</button>';
    });
    if (!html) html = '<div class="obs-cmd-empty">No other agents active</div>';
    container.innerHTML = html;
    container.querySelectorAll('.obs-cmd-agent-badge').forEach(function(badge) {
      badge.addEventListener('click', function() {
        // Single click = forward last agent message immediately
        var targetSession = badge.dataset.targetSession;
        var letterText = badge.querySelector('.obs-cmd-letter').textContent;
        var mgr = window.__cockpitManager;
        var lastMsg = mgr ? mgr.getLastAgentMessage(self.panelId) : null;
        if (!lastMsg) {
          self.showCommandNotice('No agent response to forward');
          return;
        }
        dispatchCommand(targetSession, lastMsg);
        badge.classList.add('obs-cmd-sent');
        setTimeout(function() { badge.classList.remove('obs-cmd-sent'); }, 800);
        self.showCommandNotice('Forwarded to Agent ' + letterText);
      });
      // Right-click = open command flyout with presets + custom
      badge.addEventListener('contextmenu', function(e) {
        e.preventDefault();
        self.openCommandFlyout(badge);
      });
    });
  };

  ObservatoryDrawer.prototype.openCommandFlyout = function(badgeEl) {
    var self = this;
    var targetSession = badgeEl.dataset.targetSession;
    var targetType = badgeEl.dataset.targetType;
    var existing = this.el.querySelector('.obs-cmd-flyout');
    if (existing) existing.remove();
    var flyout = document.createElement('div');
    flyout.className = 'obs-cmd-flyout';
    var letterText = badgeEl.querySelector('.obs-cmd-letter').textContent;

    // Get last agent message from THIS panel for forwarding
    var mgr = window.__cockpitManager;
    var lastMsg = mgr ? mgr.getLastAgentMessage(self.panelId) : null;
    var hasLastMsg = lastMsg && lastMsg.length > 0;

    var html = '<div class="obs-cmd-flyout-header">' +
      '<span>Push to ' + esc(letterText) + ' (' + esc(targetType) + ')</span>' +
      '<button class="obs-cmd-flyout-close" title="Close">\u2715</button>' +
    '</div>';

    // Forward Last button — prominent at top
    html += '<div class="obs-cmd-flyout-forward">' +
      '<button class="obs-cmd-forward-btn' + (hasLastMsg ? '' : ' is-disabled') + '"' +
      (hasLastMsg ? '' : ' disabled') +
      ' title="' + (hasLastMsg ? 'Forward last response to Agent ' + esc(letterText) : 'No agent response to forward') + '">' +
      '\u{1F4E8} Forward Last Response' +
      '</button>' +
    '</div>';

    html += '<div class="obs-cmd-flyout-presets">';
    MESH_COMMANDS.forEach(function(cmd) {
      var text = cmd.getCommandText(targetType);
      var disabled = text === null;
      html += '<button class="obs-cmd-preset' + (disabled ? ' is-disabled' : '') + '"' +
        ' data-cmd-id="' + cmd.id + '"' +
        (disabled ? ' disabled' : '') +
        ' title="' + esc(cmd.label) + (disabled ? ' (N/A for ' + esc(targetType) + ')' : '') + '">' +
        cmd.icon + ' ' + esc(cmd.label) +
      '</button>';
    });
    html += '</div>';
    html += '<div class="obs-cmd-flyout-custom">' +
      '<textarea class="obs-cmd-custom-input" rows="2" placeholder="Custom message\u2026"></textarea>' +
      '<button class="obs-cmd-custom-send">Send</button>' +
    '</div>';
    flyout.innerHTML = html;
    var rect = badgeEl.getBoundingClientRect();
    var drawerRect = this.el.getBoundingClientRect();
    flyout.style.top = Math.max(0, (rect.top - drawerRect.top)) + 'px';
    var cmdTab = this.el.querySelector('.obs-tab-command');
    if (cmdTab) cmdTab.appendChild(flyout);
    flyout.querySelector('.obs-cmd-flyout-close').addEventListener('click', function() { flyout.remove(); });

    // Forward Last handler
    var forwardBtn = flyout.querySelector('.obs-cmd-forward-btn');
    if (forwardBtn && hasLastMsg) {
      forwardBtn.addEventListener('click', function() {
        dispatchCommand(targetSession, lastMsg);
        forwardBtn.classList.add('obs-cmd-sent');
        setTimeout(function() { forwardBtn.classList.remove('obs-cmd-sent'); }, 800);
        self.showCommandNotice('Forwarded to Agent ' + esc(letterText));
      });
    }

    flyout.querySelectorAll('.obs-cmd-preset:not(.is-disabled)').forEach(function(btn) {
      btn.addEventListener('click', function() {
        var cmdId = btn.dataset.cmdId;
        var cmd = MESH_COMMANDS.find(function(c) { return c.id === cmdId; });
        if (!cmd) return;
        var text = cmd.getCommandText(targetType);
        if (text) {
          dispatchCommand(targetSession, text);
          btn.classList.add('obs-cmd-sent');
          setTimeout(function() { btn.classList.remove('obs-cmd-sent'); }, 800);
        }
      });
    });
    var customInput = flyout.querySelector('.obs-cmd-custom-input');
    flyout.querySelector('.obs-cmd-custom-send').addEventListener('click', function() {
      var text = customInput.value.trim();
      if (!text) return;
      dispatchCommand(targetSession, text);
      customInput.value = '';
      self.showCommandNotice('Sent to Agent ' + esc(letterText));
    });
  };

  ObservatoryDrawer.prototype.showCommandNotice = function(msg) {
    var notice = this.el.querySelector('.obs-cmd-notice');
    if (!notice) {
      notice = document.createElement('div');
      notice.className = 'obs-cmd-notice';
      var cmdTab = this.el.querySelector('.obs-tab-command');
      if (cmdTab) cmdTab.appendChild(notice);
    }
    notice.textContent = msg;
    notice.classList.add('is-visible');
    setTimeout(function() { notice.classList.remove('is-visible'); }, 2000);
  };

  // ══════════════════════════════════════════════════════════════
  // Floating Window — draggable, resizable, closable
  // ══════════════════════════════════════════════════════════════
  function createFloatingWindow(winId, title, onClose) {
    var win = document.createElement('div');
    win.className = 'obs-float-window';
    win.dataset.obsWin = winId;
    win.style.zIndex = topZ++;
    var offset = (allWindows.size % 8) * 30;
    win.style.left = (140 + offset) + 'px';
    win.style.top = (60 + offset) + 'px';
    win.style.width = '700px';
    win.style.height = '500px';
    win.innerHTML =
      '<div class="obs-float-header">' +
        '<span class="obs-float-title">' + esc(title) + '</span>' +
        '<div class="obs-float-actions">' +
          '<button class="obs-float-maximize" title="Maximize">\u25A1</button>' +
          '<button class="obs-float-close" title="Close">\u2715</button>' +
        '</div>' +
      '</div>' +
      '<div class="obs-float-body"></div>' +
      '<div class="obs-float-resize"></div>';

    win.querySelector('.obs-float-close').addEventListener('click', function() {
      win.remove();
      if (onClose) onClose();
    });
    win.querySelector('.obs-float-maximize').addEventListener('click', function() {
      win.classList.toggle('obs-float-maximized');
    });
    win.addEventListener('mousedown', function() { bringToFront(win); });
    installDrag(win, win.querySelector('.obs-float-header'));
    installResize(win, win.querySelector('.obs-float-resize'));
    document.body.appendChild(win);
    return win;
  }

  function bringToFront(el) { el.style.zIndex = topZ++; }

  function installDrag(win, handle) {
    handle.addEventListener('mousedown', function(e) {
      if (e.target.closest('.obs-float-actions')) return;
      e.preventDefault();
      var sx = e.clientX, sy = e.clientY;
      var r = win.getBoundingClientRect();
      var ox = r.left, oy = r.top;
      function mv(ev) { win.style.left = (ox + ev.clientX - sx) + 'px'; win.style.top = Math.max(0, oy + ev.clientY - sy) + 'px'; }
      function up() { document.removeEventListener('mousemove', mv); document.removeEventListener('mouseup', up); win.style.userSelect = ''; }
      win.style.userSelect = 'none';
      document.addEventListener('mousemove', mv);
      document.addEventListener('mouseup', up);
    });
  }

  function installResize(win, handle) {
    handle.addEventListener('mousedown', function(e) {
      e.preventDefault(); e.stopPropagation();
      var sx = e.clientX, sy = e.clientY, ow = win.offsetWidth, oh = win.offsetHeight;
      function mv(ev) { win.style.width = Math.max(360, ow + ev.clientX - sx) + 'px'; win.style.height = Math.max(280, oh + ev.clientY - sy) + 'px'; }
      function up() { document.removeEventListener('mousemove', mv); document.removeEventListener('mouseup', up); win.style.userSelect = ''; }
      win.style.userSelect = 'none';
      document.addEventListener('mousemove', mv);
      document.addEventListener('mouseup', up);
    });
  }

  // ══════════════════════════════════════════════════════════════
  // Visualization Renderers
  // ══════════════════════════════════════════════════════════════

  function renderViz(vizType, data, container) {
    container.innerHTML = '';
    var fn = RENDERERS[vizType];
    if (fn) {
      try { fn(data, container); }
      catch (err) { container.innerHTML = '<div class="obs-error">Render error: ' + esc(err.message) + '</div>'; }
    } else {
      container.innerHTML = '<pre class="obs-code">' + esc(JSON.stringify(data, null, 2)) + '</pre>';
    }
  }

  // ── Proof Tree (Gentzen-style / tactic tree) ──────────────
  // Expects: { nodes: [{id, label, type, children, status}], root: id }
  // or: { steps: [{tactic, goal_before, goal_after, status}] }
  function renderProofTree(data, el) {
    if (data.steps) {
      // Sequential tactic trace
      var html = '<div class="obs-proof-trace">';
      data.steps.forEach(function(s, i) {
        var cls = s.status === 'success' ? 'obs-step-ok' : s.status === 'failed' ? 'obs-step-fail' : 'obs-step-pending';
        html += '<div class="obs-proof-step ' + cls + '">' +
          '<div class="obs-step-num">' + (i + 1) + '</div>' +
          '<div class="obs-step-body">' +
            '<div class="obs-step-tactic"><code>' + esc(s.tactic || '') + '</code></div>';
        if (s.goal_before) html += '<div class="obs-step-goal">' + renderMathSafe(s.goal_before) + '</div>';
        if (s.goal_after) html += '<div class="obs-step-result">\u2192 ' + renderMathSafe(s.goal_after) + '</div>';
        if (s.message) html += '<div class="obs-step-msg">' + esc(s.message) + '</div>';
        html += '</div></div>';
      });
      html += '</div>';
      el.innerHTML = html;
      return;
    }
    // Tree structure — render as nested boxes
    if (data.nodes && data.root !== undefined) {
      var nodeMap = {};
      data.nodes.forEach(function(n) { nodeMap[n.id] = n; });
      el.innerHTML = '<div class="obs-tree-container">' + renderTreeNode(nodeMap, data.root) + '</div>';
      return;
    }
    el.innerHTML = '<pre class="obs-code">' + esc(JSON.stringify(data, null, 2)) + '</pre>';
  }

  function renderTreeNode(nodeMap, id) {
    var n = nodeMap[id];
    if (!n) return '';
    var cls = n.status === 'proved' ? 'obs-node-proved' : n.status === 'sorry' ? 'obs-node-sorry' : 'obs-node-open';
    var childHtml = '';
    if (n.children && n.children.length) {
      childHtml = '<div class="obs-tree-children">' + n.children.map(function(cid) { return renderTreeNode(nodeMap, cid); }).join('') + '</div>';
    }
    return '<div class="obs-tree-node ' + cls + '">' +
      '<div class="obs-tree-label">' + renderMathSafe(n.label || n.type || String(id)) + '</div>' +
      childHtml + '</div>';
  }

  // ── Goals (KaTeX-rendered proof obligations) ──────────────
  // Expects: { goals: [{hyps: [{name, type}], target: string}] } or { goal: string }
  function renderGoals(data, el) {
    if (data.goal && typeof data.goal === 'string') {
      el.innerHTML = '<div class="obs-goal-single">' + renderMathBlock(data.goal) + '</div>';
      return;
    }
    var goals = data.goals || [];
    if (!goals.length) { el.innerHTML = '<div class="obs-success">No goals remaining \u2714</div>'; return; }
    var html = '';
    goals.forEach(function(g, i) {
      html += '<div class="obs-goal-card">';
      if (goals.length > 1) html += '<div class="obs-goal-idx">Goal ' + (i + 1) + '</div>';
      if (g.hyps && g.hyps.length) {
        html += '<div class="obs-goal-hyps">';
        g.hyps.forEach(function(h) {
          html += '<div class="obs-hyp"><span class="obs-hyp-name">' + esc(h.name || '') + '</span> : ' + renderMathSafe(h.type || '') + '</div>';
        });
        html += '</div><div class="obs-goal-turnstile">\u22A2</div>';
      }
      html += '<div class="obs-goal-target">' + renderMathBlock(g.target || g.type || '') + '</div>';
      html += '</div>';
    });
    el.innerHTML = html;
  }

  // ── Dependency Graph (D3 force-directed with zoom/pan) ─────
  // Expects: { nodes: [{id, group?, label?}], edges: [[from, to], ...] }
  function renderDepGraph(data, el) {
    if (!data.nodes || !data.nodes.length) { el.innerHTML = '<p>No dependency data.</p>'; return; }
    if (typeof d3 === 'undefined') { el.innerHTML = '<p class="obs-error">D3.js not loaded.</p>'; return; }

    var width = 800, height = 560;
    var svg = d3.select(el).append('svg')
      .attr('width', '100%').attr('height', '100%')
      .attr('viewBox', '0 0 ' + width + ' ' + height)
      .style('background', 'transparent')
      .style('cursor', 'grab');

    // Zoom container — all graph elements go inside this group
    var g = svg.append('g');

    // Zoom/pan behavior
    var zoom = d3.zoom()
      .scaleExtent([0.1, 8])
      .on('zoom', function(event) { g.attr('transform', event.transform); });
    svg.call(zoom);

    // Zoom controls hint
    svg.append('text')
      .attr('x', 10).attr('y', height - 10)
      .attr('font-size', 9).attr('fill', 'rgba(200,220,180,0.4)')
      .text('Scroll to zoom \u00B7 Drag to pan \u00B7 Drag nodes to rearrange');

    var nodes = data.nodes.map(function(n) {
      return typeof n === 'string' ? { id: n } : { id: n.id || n, group: n.group, label: n.label };
    });
    var nodeIndex = {};
    nodes.forEach(function(n, i) { nodeIndex[n.id] = i; });

    var links = (data.edges || []).filter(function(e) {
      var src = typeof e === 'object' && e.length ? e[0] : e.source;
      var tgt = typeof e === 'object' && e.length ? e[1] : e.target;
      return nodeIndex[src] !== undefined && nodeIndex[tgt] !== undefined;
    }).map(function(e) {
      return { source: typeof e === 'object' && e.length ? e[0] : e.source,
               target: typeof e === 'object' && e.length ? e[1] : e.target };
    });

    if (nodes.length > 300) {
      nodes = nodes.slice(0, 300);
      links = links.filter(function(l) { return nodeIndex[l.source] < 300 && nodeIndex[l.target] < 300; });
    }

    // Arrow markers for directed edges
    svg.append('defs').append('marker')
      .attr('id', 'obs-arrow').attr('viewBox', '0 0 10 10')
      .attr('refX', 18).attr('refY', 5)
      .attr('markerWidth', 5).attr('markerHeight', 5)
      .attr('orient', 'auto')
      .append('path').attr('d', 'M 0 0 L 10 5 L 0 10 z')
      .attr('fill', 'rgba(0,238,0,0.3)');

    var sim = d3.forceSimulation(nodes)
      .force('link', d3.forceLink(links).id(function(d) { return d.id; }).distance(70))
      .force('charge', d3.forceManyBody().strength(-120))
      .force('center', d3.forceCenter(width / 2, height / 2))
      .force('collide', d3.forceCollide(16));

    var link = g.append('g').selectAll('line').data(links).join('line')
      .attr('stroke', 'rgba(0,238,0,0.2)').attr('stroke-width', 1)
      .attr('marker-end', 'url(#obs-arrow)');

    var node = g.append('g').selectAll('circle').data(nodes).join('circle')
      .attr('r', 6).attr('fill', function(d) {
        return d.group !== undefined ? d3.schemeCategory10[d.group % 10] : '#00ee00';
      })
      .attr('stroke', 'rgba(255,255,255,0.4)').attr('stroke-width', 0.8)
      .style('cursor', 'pointer')
      .call(d3.drag().on('start', dragStart).on('drag', dragging).on('end', dragEnd));

    node.append('title').text(function(d) { return d.label || d.id; });

    var label = g.append('g').selectAll('text').data(nodes).join('text')
      .text(function(d) { var s = d.label || d.id; var parts = s.split('.'); return parts.length > 2 ? parts.slice(-2).join('.') : s; })
      .attr('font-size', 8).attr('fill', 'rgba(200,220,180,0.8)').attr('dx', 10).attr('dy', 3)
      .style('pointer-events', 'none');

    sim.on('tick', function() {
      link.attr('x1', function(d) { return d.source.x; }).attr('y1', function(d) { return d.source.y; })
          .attr('x2', function(d) { return d.target.x; }).attr('y2', function(d) { return d.target.y; });
      node.attr('cx', function(d) { return d.x; }).attr('cy', function(d) { return d.y; });
      label.attr('x', function(d) { return d.x; }).attr('y', function(d) { return d.y; });
    });

    function dragStart(event, d) { if (!event.active) sim.alphaTarget(0.3).restart(); d.fx = d.x; d.fy = d.y; }
    function dragging(event, d) { d.fx = event.x; d.fy = event.y; }
    function dragEnd(event, d) { if (!event.active) sim.alphaTarget(0); d.fx = null; d.fy = null; }
  }

  // ── Treemap (squarified) ──────────────────────────────────
  // Expects: { files: [{path, lines, health_score, health_status, sorry_count}] }
  // or: { name, value, children: [...] }
  function renderTreemap(data, el) {
    if (typeof d3 === 'undefined') { el.innerHTML = '<p class="obs-error">D3.js not loaded.</p>'; return; }

    var width = 660, height = 420;
    // Build hierarchy from flat file list
    var root;
    if (data.files) {
      var children = data.files.map(function(f) {
        return { name: f.path.split('/').pop().replace('.lean', ''), fullPath: f.path,
                 value: Math.max(1, f.lines), health: f.health_score || 1, status: f.health_status || 'clean', sorrys: f.sorry_count || 0 };
      });
      root = d3.hierarchy({ name: 'root', children: children }).sum(function(d) { return d.value || 0; });
    } else {
      root = d3.hierarchy(data).sum(function(d) { return d.value || 0; });
    }

    d3.treemap().size([width, height]).padding(1).round(true)(root);

    var svg = d3.select(el).append('svg')
      .attr('width', '100%').attr('height', '100%')
      .attr('viewBox', '0 0 ' + width + ' ' + height)
      .style('background', 'transparent');

    var cell = svg.selectAll('g').data(root.leaves()).join('g')
      .attr('transform', function(d) { return 'translate(' + d.x0 + ',' + d.y0 + ')'; });

    cell.append('rect')
      .attr('width', function(d) { return d.x1 - d.x0; })
      .attr('height', function(d) { return d.y1 - d.y0; })
      .attr('fill', function(d) {
        var s = d.data.status || 'clean';
        return s === 'critical' ? 'rgba(255,48,48,0.55)' : s === 'warning' ? 'rgba(255,184,48,0.45)' : 'rgba(0,200,60,0.35)';
      })
      .attr('stroke', 'rgba(0,0,0,0.3)').attr('stroke-width', 0.5)
      .attr('rx', 2);

    cell.append('title').text(function(d) {
      return (d.data.fullPath || d.data.name) + '\n' + (d.value || 0) + ' lines' +
        (d.data.sorrys ? '\n' + d.data.sorrys + ' sorrys' : '') +
        '\nHealth: ' + ((d.data.health || 1) * 100).toFixed(0) + '%';
    });

    cell.filter(function(d) { return (d.x1 - d.x0) > 30 && (d.y1 - d.y0) > 12; })
      .append('text')
      .attr('x', 3).attr('y', 11)
      .attr('font-size', function(d) { return Math.min(10, Math.max(6, (d.x1 - d.x0) / 8)); })
      .attr('fill', 'rgba(255,255,255,0.8)')
      .text(function(d) { var n = d.data.name || ''; return n.length > 15 ? n.slice(0, 13) + '..' : n; });
  }

  // ── Tactics (suggestion list with confidence) ─────────────
  // Expects: { tactics: [{tactic, confidence, source, description?}] }
  function renderTactics(data, el) {
    var tactics = data.tactics || [];
    if (!tactics.length) { el.innerHTML = '<p>No tactic suggestions.</p>'; return; }
    var html = '<div class="obs-tactics-list">';
    tactics.forEach(function(t, i) {
      var pct = ((t.confidence || 0) * 100).toFixed(0);
      var barW = Math.max(2, t.confidence * 100);
      html += '<div class="obs-tactic-row">' +
        '<div class="obs-tactic-rank">#' + (i + 1) + '</div>' +
        '<div class="obs-tactic-body">' +
          '<code class="obs-tactic-code">' + esc(t.tactic) + '</code>' +
          (t.description ? '<div class="obs-tactic-desc">' + esc(t.description) + '</div>' : '') +
          '<div class="obs-tactic-bar"><div class="obs-tactic-fill" style="width:' + barW + '%"></div><span>' + pct + '%</span></div>' +
          (t.source ? '<div class="obs-tactic-source">' + esc(t.source) + '</div>' : '') +
        '</div>' +
      '</div>';
    });
    html += '</div>';
    el.innerHTML = html;
  }

  // ── LaTeX / KaTeX ─────────────────────────────────────────
  // Expects: { latex: string } or { blocks: [{latex, display?, label?}] }
  function renderLatex(data, el) {
    if (data.latex && typeof data.latex === 'string') {
      el.innerHTML = '<div class="obs-math-display">' + renderMathBlock(data.latex) + '</div>';
      return;
    }
    if (data.blocks) {
      var html = '';
      data.blocks.forEach(function(b) {
        if (b.label) html += '<div class="obs-math-label">' + esc(b.label) + '</div>';
        html += '<div class="obs-math-display">' + renderMathBlock(b.latex || '', b.display !== false) + '</div>';
      });
      el.innerHTML = html;
      return;
    }
    // Fallback: try to render the whole data as a string
    el.innerHTML = '<div class="obs-math-display">' + renderMathBlock(JSON.stringify(data)) + '</div>';
  }

  // ── Flowchart (Mermaid) ───────────────────────────────────
  // Expects: { mermaid: string } or { diagram: string }
  function renderFlowchart(data, el) {
    var src = data.mermaid || data.diagram || data.source || '';
    if (!src) { el.innerHTML = '<pre class="obs-code">' + esc(JSON.stringify(data, null, 2)) + '</pre>'; return; }
    var pre = document.createElement('pre');
    pre.className = 'mermaid';
    pre.textContent = src;
    el.appendChild(pre);
    if (typeof mermaid !== 'undefined') {
      try { mermaid.run({ nodes: [pre] }); } catch (_e) {}
    }
  }

  // ── Table (sortable) ──────────────────────────────────────
  // Expects: { columns: [string], rows: [[...], ...] } or { rows: [{...}, ...] }
  function renderDataTable(data, el) {
    var cols, rows;
    if (data.columns && data.rows) {
      cols = data.columns;
      rows = data.rows.map(function(r) {
        return Array.isArray(r) ? r : cols.map(function(c) { return r[c]; });
      });
    } else if (data.rows && data.rows.length) {
      cols = Object.keys(data.rows[0]);
      rows = data.rows.map(function(r) { return cols.map(function(c) { return r[c]; }); });
    } else {
      el.innerHTML = '<pre class="obs-code">' + esc(JSON.stringify(data, null, 2)) + '</pre>';
      return;
    }

    var wrap = document.createElement('div');
    wrap.className = 'obs-table-wrap';
    var sortCol = -1, sortAsc = true;

    function build() {
      var sorted = rows.slice();
      if (sortCol >= 0) {
        sorted.sort(function(a, b) {
          var va = a[sortCol], vb = b[sortCol];
          if (typeof va === 'number' && typeof vb === 'number') return sortAsc ? va - vb : vb - va;
          return sortAsc ? String(va).localeCompare(String(vb)) : String(vb).localeCompare(String(va));
        });
      }
      var html = '<table><thead><tr>' +
        cols.map(function(c, i) {
          var arrow = sortCol === i ? (sortAsc ? ' \u25B2' : ' \u25BC') : '';
          return '<th data-col="' + i + '">' + esc(c) + arrow + '</th>';
        }).join('') + '</tr></thead><tbody>' +
        sorted.map(function(r) {
          return '<tr>' + r.map(function(v) {
            var s = v == null ? '' : String(v);
            // Render math-like strings via KaTeX
            if (s.indexOf('\\') >= 0 || s.indexOf('\u2200') >= 0) return '<td>' + renderMathSafe(s) + '</td>';
            return '<td>' + esc(s) + '</td>';
          }).join('') + '</tr>';
        }).join('') + '</tbody></table>';
      wrap.innerHTML = html;
      wrap.querySelectorAll('th').forEach(function(th) {
        th.addEventListener('click', function() {
          var ci = parseInt(th.dataset.col);
          if (sortCol === ci) sortAsc = !sortAsc; else { sortCol = ci; sortAsc = true; }
          build();
        });
      });
    }
    build();
    el.appendChild(wrap);
  }

  // ── Dashboard (summary cards) ──────────────────────────────
  function renderDashboard(data, el) {
    var h = (data.health_score || 0);
    var hc = h >= 0.8 ? 'good' : h >= 0.4 ? 'warn' : 'bad';
    function card(val, label, cls) {
      return '<div class="obs-summary-card"><div class="obs-summary-value ' + (cls || '') + '">' +
        esc(String(val)) + '</div><div class="obs-summary-label">' + esc(label) + '</div></div>';
    }
    el.innerHTML = '<div class="obs-summary-grid">' +
      card((data.total_files || 0).toLocaleString(), 'Files') +
      card((data.total_lines || 0).toLocaleString(), 'Lines') +
      card((data.total_decls || 0).toLocaleString(), 'Declarations') +
      card(String(data.total_sorrys || 0), 'Sorrys', data.total_sorrys > 0 ? 'health-bad' : 'health-good') +
      card((h * 100).toFixed(1) + '%', 'Health', 'health-' + hc) +
      card((data.scan_time_ms || 0) + 'ms', 'Scan Time') +
      card(String(data.clusters_count || 0), 'Clusters') +
    '</div>';
  }

  // ── Clusters (D3 packed bubble chart + expandable list + mini proof explorer) ─
  function renderClusters(data, el) {
    var clusters = data.clusters || [];
    if (!clusters.length) { el.innerHTML = '<p>No cluster data.</p>'; return; }

    // D3 packed bubble chart — circles are clickable to open mini proof explorer
    if (typeof d3 !== 'undefined' && clusters.length > 1) {
      var width = 660, height = 400;
      var svg = d3.select(el).append('svg')
        .attr('width', '100%').attr('height', height)
        .attr('viewBox', '0 0 ' + width + ' ' + height)
        .style('background', 'transparent');

      var root = d3.hierarchy({ name: 'root', children: clusters.map(function(c) {
        return { name: c.name, value: Math.max(1, c.total_lines), files: c.files.length,
                 fileList: c.files, sorrys: c.total_sorrys, health: c.health_score };
      })}).sum(function(d) { return d.value || 0; });

      d3.pack().size([width, height]).padding(4)(root);

      var leaf = svg.selectAll('g').data(root.leaves()).join('g')
        .attr('transform', function(d) { return 'translate(' + d.x + ',' + d.y + ')'; })
        .style('cursor', 'pointer')
        .on('click', function(event, d) {
          event.stopPropagation();
          openMiniProofExplorer(d.data.name, d.data.fileList || []);
        });

      leaf.append('circle')
        .attr('r', function(d) { return d.r; })
        .attr('fill', function(d) {
          var h = d.data.health || 1;
          return h >= 0.8 ? 'rgba(0,200,60,0.25)' : h >= 0.4 ? 'rgba(255,184,48,0.25)' : 'rgba(255,48,48,0.3)';
        })
        .attr('stroke', function(d) {
          var h = d.data.health || 1;
          return h >= 0.8 ? 'rgba(0,238,0,0.4)' : h >= 0.4 ? 'rgba(255,184,48,0.4)' : 'rgba(255,48,48,0.5)';
        })
        .attr('stroke-width', 1.5);

      leaf.append('title').text(function(d) {
        return d.data.name + '\n' + d.data.files + ' files, ' + (d.value || 0).toLocaleString() + ' lines' +
          (d.data.sorrys > 0 ? '\n' + d.data.sorrys + ' sorrys' : '') +
          '\nHealth: ' + ((d.data.health || 1) * 100).toFixed(0) + '%' +
          '\nClick to explore';
      });

      leaf.filter(function(d) { return d.r > 20; })
        .append('text')
        .attr('text-anchor', 'middle')
        .attr('dy', '0.3em')
        .attr('font-size', function(d) { return Math.min(11, Math.max(6, d.r / 4)); })
        .attr('fill', 'rgba(200,220,180,0.9)')
        .style('pointer-events', 'none')
        .text(function(d) { var n = d.data.name.split('.').pop(); return n.length > 12 ? n.slice(0, 10) + '..' : n; });

      leaf.filter(function(d) { return d.r > 30; })
        .append('text')
        .attr('text-anchor', 'middle')
        .attr('dy', '1.5em')
        .attr('font-size', function(d) { return Math.min(8, Math.max(5, d.r / 6)); })
        .attr('fill', 'rgba(200,220,180,0.5)')
        .style('pointer-events', 'none')
        .text(function(d) { return d.data.files + ' files'; });

      el.appendChild(document.createElement('hr'));
      el.lastElementChild.style.cssText = 'border:none;border-top:1px solid var(--border-dim);margin:12px 0 8px';
    }

    // Expandable text list — clicking the name opens mini proof explorer
    var listDiv = document.createElement('div');
    listDiv.style.maxHeight = '200px';
    listDiv.style.overflowY = 'auto';
    clusters.sort(function(a, b) { return (b.total_lines || 0) - (a.total_lines || 0); });
    clusters.forEach(function(c) {
      var div = document.createElement('div');
      div.className = 'obs-cluster';
      div.innerHTML =
        '<div class="obs-cluster-header">' +
          '<span class="obs-cluster-name" data-cluster-explore="1" style="cursor:pointer">' + esc(c.name) + '</span>' +
          '<span class="obs-cluster-expand" title="Expand file list" style="cursor:pointer;margin-left:6px;color:var(--text-dim);font-size:9px">\u25BC</span>' +
          '<span class="obs-cluster-meta">' + c.files.length + ' files, ' + (c.total_lines || 0).toLocaleString() + ' lines' +
            (c.total_sorrys > 0 ? ', <span class="obs-sorry-badge">' + c.total_sorrys + ' sorry</span>' : '') +
          '</span>' +
        '</div>' +
        '<div class="obs-cluster-files">' +
          c.files.slice(0, 20).map(function(f) { return '<div class="obs-cluster-file">' + esc(f) + '</div>'; }).join('') +
          (c.files.length > 20 ? '<div class="obs-cluster-file" style="color:var(--text-dim)">... and ' + (c.files.length - 20) + ' more</div>' : '') +
        '</div>';
      // Click name → mini proof explorer
      div.querySelector('[data-cluster-explore]').addEventListener('click', function(e) {
        e.stopPropagation();
        openMiniProofExplorer(c.name, c.files);
      });
      // Click expand arrow → toggle file list
      div.querySelector('.obs-cluster-expand').addEventListener('click', function(e) {
        e.stopPropagation();
        div.classList.toggle('expanded');
      });
      listDiv.appendChild(div);
    });
    el.appendChild(listDiv);
  }

  // ── Mini Proof Explorer popup ─────────────────────────────
  // Opens a floating window with a small 2D canvas showing nodes for one cluster.
  // Data comes from the proof-lattice.json (static) or Observatory API (live).
  function openMiniProofExplorer(clusterName, files) {
    var winId = 'mini-pe:' + clusterName;
    // Close existing if open
    if (allWindows.has(winId)) {
      var ex = allWindows.get(winId);
      ex.el.remove();
      allWindows.delete(winId);
    }

    var win = createFloatingWindow(winId, clusterName + ' — Proof Lattice', function() {
      allWindows.delete(winId);
    });
    var body = win.querySelector('.obs-float-body');
    body.innerHTML = '<div class="obs-loading">Loading lattice for ' + esc(clusterName) + '...</div>';

    // Try to load proof-lattice.json, filter to this cluster
    fetch('/proof-lattice.json?v=1')
      .then(function(r) { if (!r.ok) throw new Error('no data'); return r.json(); })
      .then(function(lattice) {
        if (!lattice.items || !lattice.items.length) throw new Error('empty');
        // Filter items matching this cluster by family or file path prefix
        var matchFiles = new Set(files.map(function(f) { return f.toLowerCase(); }));
        var clusterShort = clusterName.split('.').pop();
        var filtered = [];
        var idxMap = {};
        lattice.items.forEach(function(it, i) {
          var isMatch = (it.family || '').toLowerCase() === clusterShort.toLowerCase()
            || matchFiles.has((it.path || '').toLowerCase());
          if (isMatch) {
            idxMap[i] = filtered.length;
            filtered.push(it);
          }
        });
        if (!filtered.length) throw new Error('no matching nodes');
        // Build edges for filtered subset
        var edges = [];
        (lattice.edges || []).forEach(function(e) {
          if (idxMap[e[0]] !== undefined && idxMap[e[1]] !== undefined) {
            edges.push([idxMap[e[0]], idxMap[e[1]]]);
          }
        });
        renderMiniCanvas(body, filtered, edges, clusterName);
      })
      .catch(function() {
        // Fallback: render file list as simple nodes from Observatory data
        var items = files.map(function(f, i) {
          var angle = (i / files.length) * Math.PI * 2;
          var r = 0.25 + Math.random() * 0.2;
          return {
            name: f.split('/').pop().replace('.lean', ''),
            path: f,
            pos: { x: 0.5 + Math.cos(angle) * r, y: 0.5 + Math.sin(angle) * r },
            family: clusterName.split('.').pop()
          };
        });
        renderMiniCanvas(body, items, [], clusterName);
      });

    allWindows.set(winId, { el: win });
  }

  // Render a 3D Three.js proof lattice for a filtered subset (mini version)
  function renderMiniCanvas(container, items, edges, title) {
    container.innerHTML = '';
    if (!items.length) { container.innerHTML = '<p style="color:var(--text-dim)">No nodes for this cluster.</p>'; return; }

    // 3D mount
    var mount = document.createElement('div');
    mount.style.cssText = 'width:100%;height:340px;background:#050508;border-radius:4px;position:relative';
    container.appendChild(mount);

    // Stats + detail below
    var stats = document.createElement('div');
    stats.style.cssText = 'font-size:10px;color:var(--text-dim);padding:6px 0 0;font-family:var(--font)';
    stats.textContent = items.length + ' nodes, ' + edges.length + ' edges in ' + title;
    container.appendChild(stats);
    var detail = document.createElement('div');
    detail.style.cssText = 'font-size:11px;min-height:28px;padding:4px 0;color:var(--text-dim)';
    container.appendChild(detail);

    // Hint
    var hint = document.createElement('div');
    hint.style.cssText = 'position:absolute;top:6px;left:8px;font-size:9px;color:rgba(0,238,0,0.3);font-family:monospace;z-index:2;pointer-events:none';
    hint.textContent = 'Drag orbit \u00B7 Scroll zoom \u00B7 Hover for details';
    mount.appendChild(hint);

    var colors = ['#00ee00','#00ccff','#39ff14','#00ff88','#7fff00','#00fa9a','#32cd32','#48d1cc','#66cdaa','#00bfff'];

    requestAnimationFrame(function() {
      import('/vendor/three.module.js').then(function(THREE) {
        return import('/vendor/three-addons/OrbitControls.js').then(function(mod) {
          var OrbitControls = mod.OrbitControls;
          var w = mount.clientWidth || 500, h = mount.clientHeight || 340;
          var scene = new THREE.Scene();
          scene.background = new THREE.Color(0x050508);
          scene.fog = new THREE.FogExp2(0x050508, 0.2);
          var camera = new THREE.PerspectiveCamera(55, w / h, 0.1, 100);
          camera.position.set(0, 0, 2.2);
          var renderer = new THREE.WebGLRenderer({ antialias: true });
          renderer.setSize(w, h);
          mount.appendChild(renderer.domElement);

          var geom = new THREE.SphereGeometry(0.04, 14, 14);
          var meshArr = [];
          items.forEach(function(it, i) {
            var col = new THREE.Color(colors[i % colors.length]);
            var mat = new THREE.MeshPhongMaterial({ color: col, emissive: col, emissiveIntensity: 0.35 });
            var mesh = new THREE.Mesh(geom, mat);
            var p = it.pos3 || it.pos || { x: Math.random(), y: Math.random(), z: Math.random() * 0.5 + 0.25 };
            mesh.position.set((p.x - 0.5) * 2, ((p.y || 0.5) - 0.5) * 2, ((p.z || 0.5) - 0.5) * 2);
            mesh._idx = i;
            scene.add(mesh);
            meshArr.push(mesh);
          });
          // Edges — thick and bright
          edges.forEach(function(e) {
            var a = items[e[0]], b = items[e[1]];
            if (!a || !b) return;
            var pa = a.pos3 || a.pos || { x: 0.5, y: 0.5, z: 0.5 };
            var pb = b.pos3 || b.pos || { x: 0.5, y: 0.5, z: 0.5 };
            var edgeCol = new THREE.Color(colors[e[0] % colors.length]);
            var lm = new THREE.LineBasicMaterial({ color: edgeCol, transparent: true, opacity: 0.6, linewidth: 2 });
            var lg = new THREE.BufferGeometry().setFromPoints([
              new THREE.Vector3((pa.x-.5)*2,((pa.y||.5)-.5)*2,((pa.z||.5)-.5)*2),
              new THREE.Vector3((pb.x-.5)*2,((pb.y||.5)-.5)*2,((pb.z||.5)-.5)*2)
            ]);
            scene.add(new THREE.Line(lg, lm));
          });
          scene.add(new THREE.AmbientLight(0xffffff, 1.5));
          var pl = new THREE.PointLight(0xffffff, 0.5, 10); pl.position.set(2, 2, 2); scene.add(pl);
          var controls = new OrbitControls(camera, renderer.domElement);
          controls.enableDamping = true; controls.dampingFactor = 0.08;

          var raycaster = new THREE.Raycaster(), mouse = new THREE.Vector2();
          renderer.domElement.addEventListener('mousemove', function(ev) {
            var r = renderer.domElement.getBoundingClientRect();
            mouse.x = ((ev.clientX - r.left) / r.width) * 2 - 1;
            mouse.y = -((ev.clientY - r.top) / r.height) * 2 + 1;
            raycaster.setFromCamera(mouse, camera);
            var hits = raycaster.intersectObjects(meshArr);
            if (hits.length) {
              var it = items[hits[0].object._idx];
              detail.innerHTML = '<span style="color:var(--green)">' + esc(it.name || '') + '</span>' +
                (it.path ? ' <span style="color:var(--text-dim);font-size:9px">' + esc(it.path) + '</span>' : '') +
                (it.kind ? ' <span style="color:var(--text-dim)">[' + esc(it.kind) + ']</span>' : '');
              renderer.domElement.style.cursor = 'pointer';
            } else {
              detail.textContent = '';
              renderer.domElement.style.cursor = 'grab';
            }
          });

          var animId = 0;
          var tick = function() { controls.update(); renderer.render(scene, camera); animId = requestAnimationFrame(tick); };
          tick();
          // Cleanup when window is closed — the floating window's onClose will remove the DOM
          // which stops the animation frame naturally, but let's be explicit:
          var obs = new MutationObserver(function() {
            if (!document.body.contains(mount)) { cancelAnimationFrame(animId); renderer.dispose(); obs.disconnect(); }
          });
          obs.observe(document.body, { childList: true, subtree: true });
        });
      }).catch(function(e) {
        mount.innerHTML = '<div style="color:var(--red);padding:20px;font-size:11px">3D unavailable: ' + esc(e.message) + '</div>';
      });
    });
  }

  // ── Sorrys (location list) ────────────────────────────────
  function renderSorrys(data, el) {
    var sorrys = data.sorrys || [];
    if (!sorrys.length) { el.innerHTML = '<div class="obs-success">No sorrys found \u2714</div>'; return; }
    var header = document.createElement('div');
    header.style.cssText = 'margin-bottom:10px;font-size:13px;font-weight:700;color:var(--red)';
    header.textContent = sorrys.length + ' sorry' + (sorrys.length !== 1 ? 's' : '') + ' found';
    el.appendChild(header);
    renderGenericTable(sorrys.map(function(s) {
      return { File: s.file, Line: s.line, Declaration: s.decl_name, Kind: s.kind };
    }), el);
  }

  function renderGenericTable(rows, container) {
    if (!rows.length) return;
    var keys = Object.keys(rows[0]);
    var wrap = document.createElement('div');
    wrap.className = 'obs-table-wrap';
    var html = '<table><thead><tr>' + keys.map(function(k) { return '<th>' + esc(k) + '</th>'; }).join('') + '</tr></thead><tbody>';
    rows.forEach(function(row) {
      html += '<tr>' + keys.map(function(k) { return '<td>' + esc(String(row[k] || '')) + '</td>'; }).join('') + '</tr>';
    });
    html += '</tbody></table>';
    wrap.innerHTML = html;
    container.appendChild(wrap);
  }

  var RENDERERS = {
    dashboard:  renderDashboard,
    prooftree:  renderProofTree,
    goals:      renderGoals,
    depgraph:   renderDepGraph,
    treemap:    renderTreemap,
    clusters:   renderClusters,
    sorrys:     renderSorrys,
    tactics:    renderTactics,
    latex:      renderLatex,
    flowchart:  renderFlowchart,
    table:      renderDataTable,
  };

  // ── KaTeX helpers ──────────────────────────────────────────
  function renderMathBlock(tex, display) {
    if (typeof katex !== 'undefined') {
      try { return katex.renderToString(tex, { displayMode: display !== false, throwOnError: false, trust: true }); }
      catch (_e) {}
    }
    return '<code>' + esc(tex) + '</code>';
  }

  function renderMathSafe(tex) {
    // Convert Lean unicode to LaTeX
    var latex = tex
      .replace(/\u2200/g, '\\forall ').replace(/\u2203/g, '\\exists ')
      .replace(/\u2192/g, '\\to ').replace(/\u2190/g, '\\leftarrow ')
      .replace(/\u2194/g, '\\leftrightarrow ').replace(/\u00D7/g, '\\times ')
      .replace(/\u2227/g, '\\land ').replace(/\u2228/g, '\\lor ')
      .replace(/\u00AC/g, '\\lnot ').replace(/\u22A2/g, '\\vdash ')
      .replace(/\u22A5/g, '\\bot ').replace(/\u22A4/g, '\\top ')
      .replace(/\u2208/g, '\\in ').replace(/\u2286/g, '\\subseteq ')
      .replace(/\u2260/g, '\\neq ').replace(/\u2264/g, '\\leq ').replace(/\u2265/g, '\\geq ')
      .replace(/\u2115/g, '\\mathbb{N}').replace(/\u2124/g, '\\mathbb{Z}')
      .replace(/\u211D/g, '\\mathbb{R}').replace(/\u211A/g, '\\mathbb{Q}')
      .replace(/\u03B1/g, '\\alpha ').replace(/\u03B2/g, '\\beta ')
      .replace(/\u03B3/g, '\\gamma ').replace(/\u03B4/g, '\\delta ')
      .replace(/\u03B5/g, '\\varepsilon ').replace(/\u03BB/g, '\\lambda ')
      .replace(/\u03C3/g, '\\sigma ').replace(/\u03C4/g, '\\tau ')
      .replace(/\u03C6/g, '\\varphi ').replace(/\u03C8/g, '\\psi ')
      .replace(/\u03A9/g, '\\Omega ').replace(/\u03A3/g, '\\Sigma ')
      .replace(/\u03A0/g, '\\Pi ');

    if (typeof katex !== 'undefined') {
      try { return katex.renderToString(latex, { displayMode: false, throwOnError: false, trust: true }); }
      catch (_e) {}
    }
    return '<code>' + esc(tex) + '</code>';
  }

  // ══════════════════════════════════════════════════════════════
  // Public API — called by CockpitPanel and agents
  // ══════════════════════════════════════════════════════════════
  // Initialize Mermaid for dark theme if available
  if (typeof mermaid !== 'undefined') {
    try {
      mermaid.initialize({ startOnLoad: false, theme: 'dark', themeVariables: {
        primaryColor: '#1a3a12', primaryTextColor: '#c8dcb4', primaryBorderColor: '#2a5a1a',
        lineColor: '#00ee00', secondaryColor: '#0d1408', tertiaryColor: '#0a0e08'
      }});
    } catch (_e) {}
  }

  window.Observatory = {
    /** Create a drawer for a panel. Returns the drawer instance. */
    createDrawer: function(panelId) {
      var drawer = new ObservatoryDrawer(panelId);
      return drawer;
    },
    /** Push data to a panel's drawer. Auto-unfurls and lights up the button. */
    pushToPanel: function(panelId, vizType, data) {
      // Find the drawer for this panel
      var panel = document.querySelector('[data-panel-id="' + panelId + '"]');
      if (panel && panel._obsDrawer) {
        panel._obsDrawer.pushData(vizType, data);
      }
    },
    /** Available viz types */
    VIZ_TYPES: VIZ_TYPES,
    /** Render a viz into any container (for testing) */
    renderViz: renderViz,
  };

})();
