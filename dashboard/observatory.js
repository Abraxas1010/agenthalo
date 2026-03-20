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
    el.innerHTML =
      '<div class="obs-drawer-tab" title="Observatory">' +
        '<span class="obs-drawer-tab-icon">\u{1F52D}</span>' +
      '</div>' +
      '<div class="obs-drawer-panel">' +
        '<div class="obs-drawer-header">' +
          '<span class="obs-drawer-title">Observatory</span>' +
        '</div>' +
        '<div class="obs-drawer-buttons">' +
          VIZ_TYPES.map(function(v) {
            var hasEndpoint = !!v.endpoint;
            return '<button class="obs-viz-btn' + (hasEndpoint ? ' has-endpoint' : '') + '" data-viz="' + v.id + '"' +
              (hasEndpoint ? '' : ' disabled') +
              ' title="' + v.label + (hasEndpoint ? ' — click to load' : ' — waiting for agent data') + '">' +
              '<span class="obs-viz-icon">' + v.icon + '</span>' +
              '<span class="obs-viz-label">' + v.label + '</span>' +
              '<span class="obs-viz-dot"></span>' +
            '</button>';
          }).join('') +
        '</div>' +
      '</div>';

    // Toggle collapse
    el.querySelector('.obs-drawer-tab').addEventListener('click', function() {
      self.collapsed = !self.collapsed;
      el.classList.toggle('collapsed', self.collapsed);
    });

    // Button clicks
    el.querySelectorAll('.obs-viz-btn').forEach(function(btn) {
      var vizType = btn.dataset.viz;
      var vizMeta = VIZ_TYPES.find(function(v) { return v.id === vizType; });
      self.btnMap[vizType] = btn;
      btn.addEventListener('click', function() {
        // If there's pending agent data, use that
        if (self.pendingData[vizType]) {
          self.openWindow(vizType, self.pendingData[vizType]);
          return;
        }
        // If this type has an API endpoint, fetch directly
        if (vizMeta && vizMeta.endpoint) {
          btn.disabled = true;
          btn.title = vizMeta.label + ' — loading...';
          fetch(vizMeta.endpoint)
            .then(function(res) { if (!res.ok) throw new Error('HTTP ' + res.status); return res.json(); })
            .then(function(data) {
              self.pendingData[vizType] = data;
              self.openWindow(vizType, data);
            })
            .catch(function(err) {
              btn.title = vizMeta.label + ' — error: ' + err.message;
            })
            .finally(function() {
              btn.disabled = false;
              btn.title = vizMeta.label + ' — click to load';
            });
        }
      });
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

  // ── Clusters (D3 packed bubble chart + expandable list) ────
  function renderClusters(data, el) {
    var clusters = data.clusters || [];
    if (!clusters.length) { el.innerHTML = '<p>No cluster data.</p>'; return; }

    // D3 packed bubble chart
    if (typeof d3 !== 'undefined' && clusters.length > 1) {
      var width = 660, height = 400;
      var svg = d3.select(el).append('svg')
        .attr('width', '100%').attr('height', height)
        .attr('viewBox', '0 0 ' + width + ' ' + height)
        .style('background', 'transparent');

      var root = d3.hierarchy({ name: 'root', children: clusters.map(function(c) {
        return { name: c.name, value: Math.max(1, c.total_lines), files: c.files.length,
                 sorrys: c.total_sorrys, health: c.health_score };
      })}).sum(function(d) { return d.value || 0; });

      d3.pack().size([width, height]).padding(4)(root);

      var leaf = svg.selectAll('g').data(root.leaves()).join('g')
        .attr('transform', function(d) { return 'translate(' + d.x + ',' + d.y + ')'; });

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
          '\nHealth: ' + ((d.data.health || 1) * 100).toFixed(0) + '%';
      });

      leaf.filter(function(d) { return d.r > 20; })
        .append('text')
        .attr('text-anchor', 'middle')
        .attr('dy', '0.3em')
        .attr('font-size', function(d) { return Math.min(11, Math.max(6, d.r / 4)); })
        .attr('fill', 'rgba(200,220,180,0.9)')
        .text(function(d) { var n = d.data.name.split('.').pop(); return n.length > 12 ? n.slice(0, 10) + '..' : n; });

      // File count inside smaller text
      leaf.filter(function(d) { return d.r > 30; })
        .append('text')
        .attr('text-anchor', 'middle')
        .attr('dy', '1.5em')
        .attr('font-size', function(d) { return Math.min(8, Math.max(5, d.r / 6)); })
        .attr('fill', 'rgba(200,220,180,0.5)')
        .text(function(d) { return d.data.files + ' files'; });

      el.appendChild(document.createElement('hr'));
      el.lastElementChild.style.cssText = 'border:none;border-top:1px solid var(--border-dim);margin:12px 0 8px';
    }

    // Expandable text list below the chart
    var listDiv = document.createElement('div');
    listDiv.style.maxHeight = '200px';
    listDiv.style.overflowY = 'auto';
    clusters.sort(function(a, b) { return (b.total_lines || 0) - (a.total_lines || 0); });
    clusters.forEach(function(c) {
      var div = document.createElement('div');
      div.className = 'obs-cluster';
      div.innerHTML =
        '<div class="obs-cluster-header">' +
          '<span class="obs-cluster-name">' + esc(c.name) + '</span>' +
          '<span class="obs-cluster-meta">' + c.files.length + ' files, ' + (c.total_lines || 0).toLocaleString() + ' lines' +
            (c.total_sorrys > 0 ? ', <span class="obs-sorry-badge">' + c.total_sorrys + ' sorry</span>' : '') +
          '</span>' +
        '</div>' +
        '<div class="obs-cluster-files">' +
          c.files.slice(0, 20).map(function(f) { return '<div class="obs-cluster-file">' + esc(f) + '</div>'; }).join('') +
          (c.files.length > 20 ? '<div class="obs-cluster-file" style="color:var(--text-dim)">... and ' + (c.files.length - 20) + ' more</div>' : '') +
        '</div>';
      div.querySelector('.obs-cluster-header').addEventListener('click', function() { div.classList.toggle('expanded'); });
      listDiv.appendChild(div);
    });
    el.appendChild(listDiv);
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
