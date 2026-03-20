/* System Monitor — AgentHALO Dashboard
 * Real hardware monitoring for NVIDIA DGX Spark.
 * Falls back to simulation with watermark if nvidia-smi unavailable.
 * Visual style: animated SVG gauges, per-core heat grid, thermal zones.
 */
'use strict';
(function() {

  var DGX_SPARK_URL = 'https://www.nvidia.com/en-us/products/workstations/dgx-spark/';
  var POLL_MS = 2000;
  var state = { live: false, timer: null, isDgx: false, sim: false, data: null, history: { cpu: [], mem: [], gpu: [] }, maxHist: 60 };

  function esc(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }

  // ── Fetch real data from backend ───────────────────────────
  async function fetchSnapshot() {
    try {
      var res = await fetch('/api/system/snapshot');
      if (!res.ok) throw new Error('HTTP ' + res.status);
      var d = await res.json();
      state.isDgx = !!d.is_dgx;
      state.sim = !state.isDgx;
      // Compute derived values
      var memTotalKb = d.mem_total_kb || 1;
      var memUsedKb = d.mem_used_kb || 0;
      d.mem_pct = (memUsedKb / memTotalKb) * 100;
      d.mem_used_gb = (memUsedKb / 1048576).toFixed(1);
      d.mem_total_gb = (memTotalKb / 1048576).toFixed(1);
      d.cpu_pct = 0;
      if (d.cores && d.cores.length) {
        d.cpu_pct = d.cores.reduce(function(s, c) { return s + (c.pct || 0); }, 0) / d.cores.length;
      } else if (d.load_1m && d.cpu_cores) {
        d.cpu_pct = Math.min(100, (d.load_1m / d.cpu_cores) * 100);
      }
      d.gpu_pct = d.gpu_pct || 0;
      d.gpu_name = d.gpu_name || 'GPU';
      d.gpu_temp_c = d.gpu_temp_c || 0;
      d.gpu_power_w = d.gpu_power_w || 0;
      return d;
    } catch (_e) {
      // Backend unavailable — use simulation
      state.sim = true;
      state.isDgx = false;
      return generateSim();
    }
  }

  function generateSim() {
    var t = Date.now() / 1000;
    var cores = [];
    for (var i = 0; i < 20; i++) {
      var base = i < 10 ? 30 + Math.random() * 40 : 10 + Math.random() * 25;
      cores.push({ id: i, pct: Math.min(100, base + Math.sin(t * 0.2 + i) * 15) });
    }
    return {
      gpu_name: 'NVIDIA GB10 Superchip', gpu_pct: 35 + Math.sin(t*0.08)*20 + Math.random()*10,
      gpu_temp_c: Math.round(45 + Math.sin(t*0.06)*12 + Math.random()*5),
      gpu_power_w: (80 + Math.sin(t*0.07)*30 + Math.random()*10).toFixed(1),
      gpu_mem_used: 'N/A', gpu_mem_total: 'N/A',
      cpu_pct: 25 + Math.sin(t*0.1)*15 + Math.random()*8,
      cpu_cores: 20, cores: cores,
      load_1m: (5 + Math.sin(t*0.1)*3).toFixed(2), load_5m: (4.5 + Math.sin(t*0.08)*2).toFixed(2), load_15m: (4 + Math.sin(t*0.05)*1.5).toFixed(2),
      mem_pct: 42 + Math.sin(t*0.05)*5, mem_used_gb: ((42+Math.sin(t*0.05)*5)/100*128).toFixed(1), mem_total_gb: '128.0',
      thermals: [
        { label: 'GPU', temp_c: 45 + Math.sin(t*0.06)*12 + Math.random()*5 },
        { label: 'CPU-X925', temp_c: 52 + Math.sin(t*0.09)*8 + Math.random()*3 },
        { label: 'CPU-A725', temp_c: 44 + Math.sin(t*0.11)*6 + Math.random()*2 },
        { label: 'NVMe', temp_c: 38 + Math.sin(t*0.04)*4 + Math.random()*2 },
        { label: 'SoC-VRM', temp_c: 55 + Math.sin(t*0.07)*10 + Math.random()*3 },
        { label: 'Network', temp_c: 42 + Math.sin(t*0.05)*5 + Math.random()*2 },
      ],
      is_dgx: false
    };
  }

  // ── SVG Gauge Arc ──────────────────────────────────────────
  function svgGauge(pct, label, sub, colorFn) {
    var color = colorFn(pct);
    var r = 52, cx = 60, cy = 60, stroke = 8;
    var circ = 2 * Math.PI * r;
    var dash = circ * (pct / 100);
    return '<div class="sysmon-gauge">' +
      '<svg viewBox="0 0 120 120" class="sysmon-gauge-svg">' +
        '<circle cx="' + cx + '" cy="' + cy + '" r="' + r + '" fill="none" stroke="rgba(255,255,255,0.05)" stroke-width="' + stroke + '"/>' +
        '<circle cx="' + cx + '" cy="' + cy + '" r="' + r + '" fill="none" stroke="' + color + '" stroke-width="' + stroke + '" ' +
          'stroke-dasharray="' + dash.toFixed(1) + ' ' + circ.toFixed(1) + '" stroke-linecap="round" ' +
          'transform="rotate(-90 ' + cx + ' ' + cy + ')" style="transition:stroke-dasharray 0.8s ease;filter:drop-shadow(0 0 4px ' + color + ')"/>' +
        '<text x="' + cx + '" y="' + (cy - 4) + '" text-anchor="middle" fill="' + color + '" font-size="22" font-weight="700" font-family="var(--font)">' + Math.round(pct) + '%</text>' +
        '<text x="' + cx + '" y="' + (cy + 14) + '" text-anchor="middle" fill="var(--text-dim)" font-size="9" font-family="var(--font)">' + esc(label) + '</text>' +
      '</svg>' +
      '<div class="sysmon-gauge-sub">' + esc(sub) + '</div>' +
    '</div>';
  }

  // ── Sparkline ──────────────────────────────────────────────
  function sparkline(data, colorFn) {
    if (!data.length) return '';
    var w = 100, h = 24;
    var max = Math.max.apply(null, data.concat([1]));
    var pts = data.map(function(v, i) {
      return (i / (data.length - 1 || 1) * w).toFixed(1) + ',' + (h - v / max * h).toFixed(1);
    }).join(' ');
    var lastColor = colorFn(data[data.length - 1]);
    return '<svg viewBox="0 0 ' + w + ' ' + h + '" class="sysmon-spark">' +
      '<polyline points="' + pts + '" fill="none" stroke="' + lastColor + '" stroke-width="1.5" stroke-linejoin="round" style="filter:drop-shadow(0 0 2px ' + lastColor + ')"/>' +
    '</svg>';
  }

  // ── Color helpers ──────────────────────────────────────────
  function pctColor(p) { return p >= 90 ? '#ef4444' : p >= 70 ? '#f59e0b' : p >= 40 ? '#00ee00' : '#00bfff'; }
  function tempColor(t) { return t >= 85 ? '#dc2660' : t >= 75 ? '#ef4444' : t >= 60 ? '#f59e0b' : t >= 40 ? '#00ee00' : '#00bfff'; }

  // ── Per-core heat grid ─────────────────────────────────────
  function coresGrid(cores) {
    if (!cores || !cores.length) return '';
    var html = '<div class="sysmon-cores">';
    cores.forEach(function(c, i) {
      var pct = c.pct || 0;
      var color = pctColor(pct);
      var glow = pct > 60 ? 'box-shadow:0 0 8px ' + color + ';' : '';
      var isPerf = i < 10;
      html += '<div class="sysmon-core" style="border-color:' + color + ';' + glow + '">' +
        '<div class="sysmon-core-bar-bg"><div class="sysmon-core-bar-fill" style="height:' + pct.toFixed(0) + '%;background:' + color + '"></div></div>' +
        '<div class="sysmon-core-pct" style="color:' + color + '">' + Math.round(pct) + '</div>' +
        '<div class="sysmon-core-label">' + (isPerf ? 'X925' : 'A725') + '</div>' +
        '<div class="sysmon-core-id">' + i + '</div>' +
      '</div>';
    });
    html += '</div>';
    return html;
  }

  // ── Thermal zones ──────────────────────────────────────────
  function thermalZones(zones) {
    if (!zones || !zones.length) return '';
    var html = '<div class="sysmon-thermals">';
    zones.forEach(function(z) {
      var t = z.temp_c || 0;
      var color = tempColor(t);
      var pct = Math.min(100, t);
      html += '<div class="sysmon-thermal">' +
        '<div class="sysmon-thermal-ring" style="border-color:' + color + ';box-shadow:0 0 ' + (t > 60 ? '12' : '4') + 'px ' + color + '">' +
          '<div class="sysmon-thermal-val" style="color:' + color + '">' + Math.round(t) + '\u00B0</div>' +
        '</div>' +
        '<div class="sysmon-thermal-name">' + esc(z.label) + '</div>' +
        '<div class="sysmon-thermal-bar"><div style="width:' + pct + '%;height:100%;background:' + color + ';border-radius:2px;box-shadow:0 0 4px ' + color + '"></div></div>' +
      '</div>';
    });
    html += '</div>';
    return html;
  }

  // ── Process table ───────────────────────────────────────────
  function processTable(procs) {
    if (!procs || !procs.length) return '<div style="padding:12px;color:var(--text-dim);font-size:11px">No process data</div>';
    var html = '<div class="sysmon-procs"><table><thead><tr>' +
      '<th>PID</th><th>User</th><th>CPU%</th><th>MEM%</th><th>Command</th></tr></thead><tbody>';
    procs.forEach(function(p) {
      var cpuVal = parseFloat(p.cpu) || 0;
      var cpuColor = cpuVal > 50 ? 'var(--red)' : cpuVal > 20 ? 'var(--amber)' : 'var(--green)';
      var cmd = (p.cmd || '').length > 60 ? p.cmd.slice(0, 57) + '...' : p.cmd || '';
      html += '<tr>' +
        '<td class="mono">' + esc(p.pid || '') + '</td>' +
        '<td>' + esc(p.user || '') + '</td>' +
        '<td style="color:' + cpuColor + ';font-weight:600">' + esc(p.cpu || '0') + '</td>' +
        '<td>' + esc(p.mem || '0') + '</td>' +
        '<td class="mono cmd" title="' + esc(p.cmd || '') + '">' + esc(cmd) + '</td>' +
      '</tr>';
    });
    html += '</tbody></table></div>';
    return html;
  }

  // ── Main render ────────────────────────────────────────────
  function render() {
    var el = document.getElementById('sysmon-root');
    if (!el) return;
    var d = state.data;
    if (!d) { el.innerHTML = '<div class="loading">Connecting to hardware...</div>'; return; }

    var watermark = state.sim ? '<div class="sysmon-watermark">SIMULATION</div>' : '';
    var badge = state.isDgx
      ? '<div class="sysmon-badge live"><span class="sysmon-badge-dot"></span>DGX Spark \u2014 GB10 Connected</div>'
      : '<a href="' + DGX_SPARK_URL + '" target="_blank" rel="noopener" class="sysmon-badge link">Learn about NVIDIA DGX Spark \u2197</a>';

    el.innerHTML = watermark +
      // Header
      '<div class="sysmon-hdr">' +
        '<div><div class="sysmon-title">System <span style="color:var(--green)">Monitor</span></div>' +
        '<div class="sysmon-sub">' + esc(d.gpu_name) + ' \u2014 ' + (state.isDgx ? 'Grace Blackwell Architecture' : 'Simulation Mode') + '</div></div>' +
        '<div class="sysmon-hdr-right">' + badge +
          '<div class="sysmon-live-ctl">' +
            '<span class="sysmon-live-dot' + (state.live ? ' on' : '') + '"></span>' +
            '<button class="btn btn-sm' + (state.live ? ' btn-primary' : '') + '" id="sysmon-toggle">' + (state.live ? 'Stop' : 'Start Live') + '</button>' +
          '</div>' +
        '</div>' +
      '</div>' +

      // Gauges row
      '<div class="sysmon-gauges">' +
        svgGauge(d.cpu_pct, 'CPU', (d.load_1m || '0') + ' / ' + (d.cpu_cores || 20) + ' cores', pctColor) +
        '<div class="sysmon-gauge-spark">' + sparkline(state.history.cpu, pctColor) + '</div>' +
        svgGauge(d.mem_pct, 'MEMORY', (d.mem_used_gb || '0') + ' / ' + (d.mem_total_gb || '0') + ' GiB', pctColor) +
        '<div class="sysmon-gauge-spark">' + sparkline(state.history.mem, pctColor) + '</div>' +
        svgGauge(d.gpu_pct, 'GPU', Math.round(d.gpu_temp_c) + '\u00B0C \u00B7 ' + (d.gpu_power_w || 0) + 'W', pctColor) +
        '<div class="sysmon-gauge-spark">' + sparkline(state.history.gpu, pctColor) + '</div>' +
      '</div>' +

      // CPU cores
      '<div class="sysmon-section">' +
        '<div class="sysmon-sec-hdr">\u{1F9E0} CPU Cores <span class="sysmon-sec-sub">' + (d.cpu_cores || 20) + '-core Grace \u2014 10\u00D7 X925 + 10\u00D7 A725</span></div>' +
        coresGrid(d.cores) +
      '</div>' +

      // Thermals
      '<div class="sysmon-section">' +
        '<div class="sysmon-sec-hdr">\u{1F321} Thermal Zones <span class="sysmon-sec-sub">DGX Spark thermal monitoring</span></div>' +
        thermalZones(d.thermals) +
      '</div>' +

      // Active processes
      '<div class="sysmon-section">' +
        '<div class="sysmon-sec-hdr">\u{1F4BB} Active Processes <span class="sysmon-sec-sub">Top 15 by CPU usage</span></div>' +
        processTable(d.processes) +
      '</div>' +

      // Specs bar
      '<div class="sysmon-specs">' +
        '<div class="sysmon-spec"><span>Architecture</span><strong>Grace Blackwell</strong></div>' +
        '<div class="sysmon-spec"><span>GPU</span><strong>GB10 Superchip</strong></div>' +
        '<div class="sysmon-spec"><span>Unified RAM</span><strong>128 GB</strong></div>' +
        '<div class="sysmon-spec"><span>AI Performance</span><strong>1 PFLOP FP4</strong></div>' +
        '<div class="sysmon-spec"><span>Inference</span><strong>1,000 TOPS</strong></div>' +
        '<div class="sysmon-spec"><span>CPU</span><strong>20-core Grace</strong></div>' +
      '</div>';

    document.getElementById('sysmon-toggle')?.addEventListener('click', toggleLive);
  }

  // ── Update cycle ───────────────────────────────────────────
  async function tick() {
    state.data = await fetchSnapshot();
    if (state.data) {
      state.history.cpu.push(state.data.cpu_pct || 0);
      state.history.mem.push(state.data.mem_pct || 0);
      state.history.gpu.push(state.data.gpu_pct || 0);
      if (state.history.cpu.length > state.maxHist) { state.history.cpu.shift(); state.history.mem.shift(); state.history.gpu.shift(); }
    }
    render();
  }

  function toggleLive() {
    state.live = !state.live;
    if (state.live) { tick(); state.timer = setInterval(tick, POLL_MS); }
    else { clearInterval(state.timer); state.timer = null; render(); }
  }

  function stop() { state.live = false; if (state.timer) { clearInterval(state.timer); state.timer = null; } }

  window.renderSystemMonitorPage = async function() {
    var content = document.getElementById('content');
    if (!content) return;
    content.innerHTML = '<div class="sysmon-page"><div id="sysmon-root"><div class="loading">Connecting to hardware...</div></div></div>';
    state.live = true;
    await tick();
    state.timer = setInterval(tick, POLL_MS);
  };
  window.stopSystemMonitor = stop;

})();
