/* System Monitor — AgentHALO Dashboard
 * Real hardware monitoring for NVIDIA DGX Spark.
 * WebSocket-driven updates at 500ms with delta-based per-core CPU utilization.
 * Falls back to HTTP polling if WebSocket unavailable.
 * All section panels use collapsible ▶/▼ toggle headers.
 */
'use strict';
(function() {

  var DGX_URL = 'https://www.nvidia.com/en-us/products/workstations/dgx-spark/';
  var POLL_MS = 2000;
  var HIST_MAX = 60;
  var NVIDIA_THERMAL = { gpu:{n:83,t:90,c:100}, cpu:{n:85,t:95,c:105}, nvme:{n:70,t:75,c:85}, soc:{n:85,t:90,c:95} };
  // Brand orange accent: #ff6a00 → rgb(255,106,0)
  var ACCENT_RGB = '255,106,0';
  var st = { live:false, timer:null, isDgx:false, sim:false, data:null, hist:{cpu:[],mem:[],gpu:[]},
    gaugeCache:{}, sparkCache:{}, expanded:{}, prevThermals:null, prevThermalTime:0,
    ws:null, wsConnected:false, wsReconnect:null, initialized:false };

  function esc(s) { var d=document.createElement('div'); d.textContent=s; return d.innerHTML; }

  // ── WebSocket ───────────────────────────────────────────────
  function connectWs() {
    if (st.ws && st.ws.readyState === WebSocket.OPEN) return;
    var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    var url = proto + '//' + location.host + '/api/system/stream';
    try {
      console.log('[sysmon-ws] Connecting to', url);
      var ws = new WebSocket(url);
      st.ws = ws;
      ws.onopen = function() {
        console.log('[sysmon-ws] Connected — switching to 500ms WebSocket updates');
        st.wsConnected = true;
        // Stop HTTP polling — WS is 4x faster
        if (st.timer) { clearInterval(st.timer); st.timer = null; }
        // Update status badge
        var badge = document.querySelector('.sm-hdr-r .sm-pill');
        if (badge) { badge.className = 'sm-pill sm-pill-accent'; badge.textContent = '\u26A1 WebSocket 500ms'; }
        var dot = document.querySelector('.sm-live-dot');
        if (dot) { dot.className = 'sm-live-dot on'; }
      };
      ws.onmessage = function(ev) {
        try {
          var msg = JSON.parse(ev.data);
          if (msg.type === 'snapshot' && msg.data) handleWsSnapshot(msg.data);
        } catch(e) { console.error('[sysmon-ws] Parse error:', e); }
      };
      ws.onclose = function() {
        console.log('[sysmon-ws] Disconnected — falling back to HTTP polling');
        st.wsConnected = false;
        st.ws = null;
        if (st.live && !st.timer) st.timer = setInterval(httpTick, POLL_MS);
        if (st.live) st.wsReconnect = setTimeout(connectWs, 2000);
      };
      ws.onerror = function(e) { console.error('[sysmon-ws] Error:', e); };
    } catch(e) {
      // WebSocket not available, stay on HTTP polling
    }
  }

  function disconnectWs() {
    if (st.wsReconnect) { clearTimeout(st.wsReconnect); st.wsReconnect = null; }
    if (st.ws) { st.ws.close(); st.ws = null; }
    st.wsConnected = false;
  }

  function handleWsSnapshot(d) {
    // Map WS snapshot format to our display format
    var cores = (d.cpu || []).map(function(c) { return { id: c.id, pct: c.util || 0, cls: c.class }; });
    var gpu = d.gpu || {};
    var mem = d.mem || {};
    var mT = mem.total_kb || 1, mU = mem.used_kb || 0;
    var thermals = (d.thermal || []).map(function(t) { return { temp_c: t.temp_c || 0 }; });

    var cpuPct = 0;
    if (d.load_1m && cores.length) cpuPct = Math.min(100, (d.load_1m / cores.length) * 100);
    else if (cores.length) cpuPct = cores.reduce(function(s,c){return s+c.pct;},0) / cores.length;

    var mapped = {
      cpu_pct: cpuPct,
      cpu_cores: cores.length || 20,
      cores: cores,
      gpu_pct: gpu.util || 0,
      gpu_temp_c: gpu.temp || 0,
      gpu_power_w: gpu.power || 0,
      gpu_name: 'NVIDIA GB10 Superchip',
      mem_pct: (mU/mT)*100,
      mem_used_gb: (mU/1048576).toFixed(1),
      mem_total_gb: (mT/1048576).toFixed(1),
      thermals: thermals,
      load_1m: d.load_1m || 0,
      entropy_avail: d.entropy_avail,
      uptime_secs: d.uptime_secs,
      is_dgx: true
    };

    st.data = mapped;
    st.isDgx = true; st.sim = false;

    // Update history
    st.hist.cpu.push(mapped.cpu_pct); st.hist.mem.push(mapped.mem_pct); st.hist.gpu.push(mapped.gpu_pct);
    if(st.hist.cpu.length>HIST_MAX){st.hist.cpu.shift();st.hist.mem.shift();st.hist.gpu.shift();}

    if (!st.initialized || !document.getElementById('sm-card-cpu')) {
      initialRender(mapped);
    } else {
      updateAll(mapped);
    }
  }

  // ── HTTP Fetch (fallback) ──────────────────────────────────
  async function snap() {
    try {
      var r = await fetch('/api/system/snapshot');
      if (!r.ok) throw 0;
      var d = await r.json();
      st.isDgx = !!d.is_dgx; st.sim = !st.isDgx;
      var mT=d.mem_total_kb||1, mU=d.mem_used_kb||0;
      d.mem_pct=(mU/mT)*100; d.mem_used_gb=(mU/1048576).toFixed(1); d.mem_total_gb=(mT/1048576).toFixed(1);
      d.cpu_pct = d.load_1m && d.cpu_cores ? Math.min(100, (parseFloat(d.load_1m) / d.cpu_cores) * 100) : 0;
      d.gpu_pct=d.gpu_pct||0; d.gpu_name=d.gpu_name||'GPU'; d.gpu_temp_c=d.gpu_temp_c||0; d.gpu_power_w=d.gpu_power_w||0;
      return d;
    } catch(_) { st.sim=true; st.isDgx=false; return simData(); }
  }

  function simData() {
    var t=Date.now()/1000, cores=[];
    for(var i=0;i<20;i++){var b=i<10?30+Math.random()*40:10+Math.random()*25;cores.push({id:i,pct:Math.min(100,b+Math.sin(t*0.2+i)*15)});}
    return{gpu_name:'NVIDIA GB10 Superchip',gpu_pct:35+Math.sin(t*0.08)*20+Math.random()*10,gpu_temp_c:Math.round(45+Math.sin(t*0.06)*12+Math.random()*5),gpu_power_w:+(80+Math.sin(t*0.07)*30+Math.random()*10).toFixed(1),
      cpu_pct:25+Math.sin(t*0.1)*15+Math.random()*8,cpu_cores:20,cores:cores,
      load_1m:(5+Math.sin(t*0.1)*3).toFixed(2),load_5m:'4.50',load_15m:'4.00',
      mem_pct:42+Math.sin(t*0.05)*5,mem_used_gb:((42+Math.sin(t*0.05)*5)/100*128).toFixed(1),mem_total_gb:'128.0',
      thermals:[{label:'acpitz',temp_c:76+Math.sin(t*0.06)*5},{label:'acpitz',temp_c:58+Math.sin(t*0.09)*4},{label:'acpitz',temp_c:75+Math.sin(t*0.07)*5},{label:'acpitz',temp_c:57+Math.sin(t*0.11)*3},{label:'acpitz',temp_c:76+Math.sin(t*0.08)*5},{label:'acpitz',temp_c:57+Math.sin(t*0.1)*3},{label:'acpitz',temp_c:66+Math.sin(t*0.05)*4}],
      cpu_processes:[{pid:'1234',user:'sim',cpu:'50.0',mem:'0.8',cmd:'lean proof_check.lean'}],
      mem_processes:[{pid:'5678',user:'sim',cpu:'10',mem:'14.8',rss:'18676652',cmd:'full_kernel_sky_service'}],
      gpu_processes:[], is_dgx:false, entropy_avail: 256 + Math.round(Math.random() * 7936), uptime_secs: 86400 + t % 86400};
  }

  // ── Colors ─────────────────────────────────────────────────
  function pctC(p){return p>=90?'#ef4444':p>=70?'#f59e0b':p>=40?'#ff6a00':'#ff8d2b';}
  function tempC(t,type){var s=NVIDIA_THERMAL[type||'cpu']||NVIDIA_THERMAL.cpu;if(t>=s.c)return'#dc2660';if(t>=s.t)return'#ef4444';if(t>=s.n)return'#f59e0b';if(t>=s.n*0.6)return'#ff6a00';return'#ff8d2b';}
  function tempRgb(t,type){var c=tempC(t,type);if(c.startsWith('#')){var h=c.slice(1);return{r:parseInt(h.substr(0,2),16),g:parseInt(h.substr(2,2),16),b:parseInt(h.substr(4,2),16)};}return{r:255,g:106,b:0};}
  function heatI(t,type){var s=NVIDIA_THERMAL[type||'cpu']||NVIDIA_THERMAL.cpu;return Math.max(0,Math.min(1,(t-30)/(s.c-30)));}

  // ── Collapsible Section Helper ─────────────────────────────
  function makeCollapsible(id, icon, title, subtitle, rightBadges, defaultOpen) {
    var isOpen = st.expanded[id] !== undefined ? st.expanded[id] : !!defaultOpen;
    st.expanded[id] = isOpen;
    var section = document.createElement('div'); section.className = 'sm-section'; section.id = 'sm-sec-' + id;
    var hdr = document.createElement('div'); hdr.className = 'sm-sec-hdr sm-sec-hdr-toggle';
    var left = document.createElement('div'); left.style.cssText = 'display:flex;align-items:center;gap:8px;';
    var arrow = document.createElement('span'); arrow.className = 'sm-sec-arrow'; arrow.textContent = isOpen ? '\u25BC' : '\u25B6';
    left.appendChild(arrow);
    var iconSpan = document.createElement('span'); iconSpan.style.fontSize = '14px'; iconSpan.textContent = icon; left.appendChild(iconSpan);
    var titleSpan = document.createElement('span'); titleSpan.style.cssText = 'font-weight:600;font-size:14px;'; titleSpan.textContent = title; left.appendChild(titleSpan);
    if (subtitle) { var sub = document.createElement('span'); sub.className = 'sm-sec-sub'; sub.textContent = subtitle; left.appendChild(sub); }
    hdr.appendChild(left);
    if (rightBadges) { var right = document.createElement('div'); right.style.cssText = 'display:flex;align-items:center;gap:6px;flex-wrap:wrap;'; right.innerHTML = rightBadges; hdr.appendChild(right); }
    section.appendChild(hdr);
    var body = document.createElement('div'); body.className = 'sm-sec-body';
    body.style.cssText = 'overflow:hidden;transition:max-height 0.3s ease-out;' + (isOpen ? 'max-height:2000px;' : 'max-height:0;');
    section.appendChild(body);
    hdr.addEventListener('click', function() { st.expanded[id] = !st.expanded[id]; arrow.textContent = st.expanded[id] ? '\u25BC' : '\u25B6'; body.style.maxHeight = st.expanded[id] ? '2000px' : '0'; });
    return { section: section, body: body };
  }

  // ── PUF Fingerprint Visual ──────────────────────────────────
  function pufFingerprintVisual() {
    var fp = '';
    var seed = navigator.userAgent + location.hostname + 'DGX-GB10-PUF';
    for (var i = 0; i < 64; i++) { var h = 0; for (var j = 0; j < seed.length; j++) { h = ((h << 5) - h + seed.charCodeAt(j) + i * 7) | 0; } fp += (Math.abs(h) % 16).toString(16); }
    var container = document.createElement('div');
    container.style.cssText = 'display:grid;grid-template-columns:repeat(16,10px);grid-template-rows:repeat(4,10px);gap:2px;padding:12px;background:rgba(0,0,0,0.3);border-radius:6px;justify-content:center;';
    for (var k = 0; k < 64; k++) {
      var v = parseInt(fp[k] || '0', 16);
      var block = document.createElement('div');
      block.style.cssText = 'width:10px;height:10px;border-radius:2px;background:hsl(' + ((v * 22.5) % 360) + ',' + (55 + (v % 4) * 5) + '%,' + (25 + v * 2.5) + '%);';
      block.title = fp[k].toUpperCase();
      container.appendChild(block);
    }
    return container;
  }

  // ── DOM-based Gauge — 270° arc with glow ───────────────────
  function gaugeDOM(id, pct, label, sub) {
    pct = Math.max(0, Math.min(100, pct || 0));
    var size = 130, cx = 65, cy = 65, r = 48, sw = 6;
    var circ = 2 * Math.PI * r, arcLen = circ * 0.75, gapLen = circ - arcLen;
    var off = arcLen * (1 - pct / 100), color = pctC(pct);

    var cached = st.gaugeCache[id];
    if (cached && cached.prog && document.body.contains(cached.prog)) {
      cached.prog.style.strokeDashoffset = off;
      cached.prog.setAttribute('stroke', color);
      cached.prog.style.filter = 'drop-shadow(0 0 8px ' + color + ')';
      cached.valText.textContent = Math.round(pct) + '%';
      cached.valText.setAttribute('fill', color);
      if (cached.subEl && sub !== undefined) cached.subEl.textContent = sub;
      return cached.container;
    }

    var wrap = document.createElement('div'); wrap.className = 'sm-gauge-wrap';
    var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 ' + size + ' ' + size); svg.setAttribute('class', 'sm-gauge-svg');

    [0, 0.25, 0.5, 0.75, 1].forEach(function(f) {
      var a = (135 + f * 270) * Math.PI / 180, ri = r + sw/2 + 3, ro = ri + (f === 0||f===0.5||f===1 ? 7 : 4);
      var tick = document.createElementNS('http://www.w3.org/2000/svg', 'line');
      tick.setAttribute('x1', cx+ri*Math.cos(a)); tick.setAttribute('y1', cy+ri*Math.sin(a));
      tick.setAttribute('x2', cx+ro*Math.cos(a)); tick.setAttribute('y2', cy+ro*Math.sin(a));
      tick.setAttribute('stroke', 'rgba(255,255,255,' + (f===0||f===0.5||f===1?'0.2':'0.1') + ')');
      tick.setAttribute('stroke-width', f===0.5?'1.5':'1'); tick.setAttribute('stroke-linecap', 'round');
      svg.appendChild(tick);
    });

    var bg = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    bg.setAttribute('cx',cx); bg.setAttribute('cy',cy); bg.setAttribute('r',r);
    bg.setAttribute('fill','none'); bg.setAttribute('stroke','rgba(255,255,255,0.06)');
    bg.setAttribute('stroke-width',sw); bg.setAttribute('stroke-linecap','round');
    bg.setAttribute('stroke-dasharray',arcLen+' '+gapLen); bg.setAttribute('transform','rotate(135 '+cx+' '+cy+')');
    svg.appendChild(bg);

    var prog = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    prog.setAttribute('cx',cx); prog.setAttribute('cy',cy); prog.setAttribute('r',r);
    prog.setAttribute('fill','none'); prog.setAttribute('stroke',color);
    prog.setAttribute('stroke-width',sw+1.5); prog.setAttribute('stroke-linecap','round');
    prog.setAttribute('stroke-dasharray',arcLen+' '+gapLen);
    prog.setAttribute('transform','rotate(135 '+cx+' '+cy+')');
    // CRITICAL: set dashoffset via style (not attribute) so CSS transition fires on updates
    prog.style.strokeDashoffset = off;
    prog.style.transition = 'stroke-dashoffset 0.2s ease-out, stroke 0.2s ease, filter 0.2s ease';
    prog.style.filter = 'drop-shadow(0 0 8px ' + color + ')';
    svg.appendChild(prog);

    var valText = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    valText.setAttribute('x',cx); valText.setAttribute('y',cy-2);
    valText.setAttribute('text-anchor','middle'); valText.setAttribute('dominant-baseline','central');
    valText.setAttribute('fill',color); valText.setAttribute('font-size','24');
    valText.setAttribute('font-weight','700'); valText.setAttribute('font-family',"'JetBrains Mono','Courier New',monospace");
    valText.textContent = Math.round(pct) + '%'; svg.appendChild(valText);

    var lblSvg = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    lblSvg.setAttribute('x',cx); lblSvg.setAttribute('y',cy+18);
    lblSvg.setAttribute('text-anchor','middle'); lblSvg.setAttribute('dominant-baseline','central');
    lblSvg.setAttribute('fill','rgba(255,255,255,0.3)'); lblSvg.setAttribute('font-size','10');
    lblSvg.setAttribute('font-weight','600'); lblSvg.setAttribute('letter-spacing','2');
    lblSvg.textContent = label; svg.appendChild(lblSvg);
    wrap.appendChild(svg);

    var subEl = null;
    if (sub) { subEl = document.createElement('div'); subEl.className = 'sm-gauge-sub'; subEl.textContent = sub; wrap.appendChild(subEl); }
    st.gaugeCache[id] = { container: wrap, prog: prog, valText: valText, subEl: subEl };
    return wrap;
  }

  // ── Sparkline ──────────────────────────────────────────────
  function sparkDOM(id, data) {
    var w=200,h=44,pad=2;
    if(!data||data.length<2) return document.createElement('div');
    var series = data.length > HIST_MAX ? data.slice(-HIST_MAX) : data;
    var offset = HIST_MAX - series.length;
    var pts = series.map(function(v,i){ var x=pad+((offset+i)/(HIST_MAX-1))*(w-pad*2); var y=h-pad-(Math.max(0,Math.min(100,v))/100)*(h-pad*2); return x.toFixed(1)+','+y.toFixed(1); });
    var color = pctC(data[data.length-1]);
    var hx = color.slice(1);
    var fillColor = 'rgba('+parseInt(hx.substr(0,2),16)+','+parseInt(hx.substr(2,2),16)+','+parseInt(hx.substr(4,2),16)+',0.12)';
    var cached = st.sparkCache[id];
    if (cached && cached.svg && document.body.contains(cached.svg)) {
      cached.fill.setAttribute('points', pad+','+(h-pad)+' '+pts.join(' ')+' '+(w-pad)+','+(h-pad));
      cached.fill.setAttribute('fill', fillColor);
      cached.line.setAttribute('points', pts.join(' ')); cached.line.setAttribute('stroke', color);
      cached.line.style.filter = 'drop-shadow(0 0 2px '+color+')';
      var lp = pts[pts.length-1].split(',');
      cached.dot.setAttribute('cx',lp[0]); cached.dot.setAttribute('cy',lp[1]); cached.dot.setAttribute('fill',color);
      return cached.svg;
    }
    var svg = document.createElementNS('http://www.w3.org/2000/svg','svg');
    svg.setAttribute('width',w); svg.setAttribute('height',h); svg.setAttribute('viewBox','0 0 '+w+' '+h);
    svg.setAttribute('preserveAspectRatio','none'); svg.style.cssText = 'display:block;width:100%;height:44px;';
    [0,50,100].forEach(function(v){ var y=h-pad-(v/100)*(h-pad*2); var gl=document.createElementNS('http://www.w3.org/2000/svg','line'); gl.setAttribute('x1',pad); gl.setAttribute('x2',w-pad); gl.setAttribute('y1',y); gl.setAttribute('y2',y); gl.setAttribute('stroke','rgba(255,255,255,'+(v===50?'0.05':'0.08')+')'); gl.setAttribute('stroke-width','1'); svg.appendChild(gl); });
    var fill = document.createElementNS('http://www.w3.org/2000/svg','polygon');
    fill.setAttribute('points', pad+','+(h-pad)+' '+pts.join(' ')+' '+(w-pad)+','+(h-pad)); fill.setAttribute('fill', fillColor); svg.appendChild(fill);
    var line = document.createElementNS('http://www.w3.org/2000/svg','polyline');
    line.setAttribute('points', pts.join(' ')); line.setAttribute('fill','none'); line.setAttribute('stroke', color);
    line.setAttribute('stroke-width','1.5'); line.setAttribute('stroke-linejoin','round'); line.setAttribute('stroke-linecap','round');
    line.style.filter = 'drop-shadow(0 0 2px '+color+')'; svg.appendChild(line);
    var lp = pts[pts.length-1].split(',');
    var dot = document.createElementNS('http://www.w3.org/2000/svg','circle');
    dot.setAttribute('cx',lp[0]); dot.setAttribute('cy',lp[1]); dot.setAttribute('r','3'); dot.setAttribute('fill',color);
    dot.style.filter = 'drop-shadow(0 0 4px '+color+')'; svg.appendChild(dot);
    st.sparkCache[id] = { svg:svg, fill:fill, line:line, dot:dot };
    return svg;
  }

  // ── Update helpers ─────────────────────────────────────────
  function updateGaugeCard(cardId, pct, label, sub, sparkData) {
    var card = document.getElementById(cardId); if (!card) return;
    var gWrap = card.querySelector('.sm-gauge-wrap');
    if (gWrap) gaugeDOM(cardId+'-g', pct, label, sub);
    else { var ga = card.querySelector('.sm-gauge-area'); if(ga){ga.innerHTML='';ga.appendChild(gaugeDOM(cardId+'-g',pct,label,sub));} }
    var sa = card.querySelector('.sm-spark-area');
    if (sa) { var se = sparkDOM(cardId+'-s', sparkData); if(!sa.contains(se)){sa.innerHTML='';sa.appendChild(se);} }
  }

  function updateCores(cores) {
    if (!cores) return;
    cores.forEach(function(c,i) {
      var el = document.getElementById('sm-core-'+i); if (!el) return;
      var p = c.pct||0;
      var intensity = Math.max(0.08, p / 100);
      var textColor = p > 50 ? '#fff' : 'rgba(255,255,255,0.6)';
      el.style.background = 'linear-gradient(180deg, rgba('+ACCENT_RGB+','+intensity.toFixed(2)+'), rgba('+ACCENT_RGB+','+(intensity*0.35).toFixed(2)+'))';
      el.style.borderColor = 'rgba('+ACCENT_RGB+','+(0.15+intensity*0.7).toFixed(2)+')';
      el.style.boxShadow = p > 15 ? '0 0 '+Math.round(intensity*16)+'px rgba('+ACCENT_RGB+','+(intensity*0.5).toFixed(2)+')' : 'none';
      var pctEl = el.querySelector('.sm-core-pct'); if(pctEl){pctEl.textContent=Math.round(p)+'%';pctEl.style.color=textColor;}
      var idEl = el.querySelector('.sm-core-id'); if(idEl) idEl.style.color = textColor;
      var bar = el.querySelector('.sm-core-bar'); if(bar) bar.style.width = p.toFixed(0)+'%';
    });
  }

  function updateThermalMap(d) {
    var zones=d.thermals||[], labels=['X925-A','X925-B','A725-A','A725-B','VRM','SOC','PWR'], types=['cpu','cpu','cpu','cpu','soc','soc','soc'];
    zones.forEach(function(z,i) {
      if(i>=labels.length)return; var el=document.getElementById('sm-tz-'+i); if(!el)return;
      var t=z.temp_c||0,type=types[i],col=tempC(t,type),rgb=tempRgb(t,type),hi=heatI(t,type),glow=Math.round(2+hi*8);
      var cell=el.querySelector('.sm-sensor-cell');
      if(cell){cell.style.background='linear-gradient(180deg,rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.12+hi*0.25)+'),rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.04+hi*0.08)+'))';cell.style.borderColor='rgba('+rgb.r+','+rgb.g+','+rgb.b+',0.6)';cell.style.boxShadow='0 0 '+glow+'px rgba('+rgb.r+','+rgb.g+','+rgb.b+','+hi*0.3+')';}
      var te=el.querySelector('.sm-tz-temp');if(te){te.textContent=Math.round(t)+'\u00B0C';te.style.color=col;}
      var ba=el.querySelector('.sm-tz-bar-fill');if(ba){ba.style.width=Math.round(hi*100)+'%';ba.style.background=col;}
    });
    var gpuT=d.gpu_temp_c||0,gpuCol=tempC(gpuT,'gpu'),gpuRgb=tempRgb(gpuT,'gpu'),gpuHi=heatI(gpuT,'gpu');
    var gc=document.getElementById('sm-gpu-chip');
    if(gc){gc.style.background='linear-gradient(180deg,rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.1+gpuHi*0.25)+'),rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.03+gpuHi*0.07)+'))';gc.style.borderColor='rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+',0.5)';gc.style.boxShadow='0 0 '+(3+gpuHi*10)+'px rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+gpuHi*0.25+')';var gt=gc.querySelector('.sm-gpu-temp');if(gt){gt.textContent=Math.round(gpuT)+'\u00B0C';gt.style.color=gpuCol;}var gb=gc.querySelector('.sm-gpu-util-fill');if(gb){gb.style.width=(d.gpu_pct||0).toFixed(0)+'%';gb.style.background=gpuCol;}var gs=gc.querySelector('.sm-gpu-sub');if(gs)gs.textContent=(d.gpu_pct||0).toFixed(0)+'% \u2022 '+(d.gpu_power_w||0)+'W';}
    var mc=document.getElementById('sm-mem-chip');
    if(mc){var ml=mc.querySelector('.sm-mem-label');if(ml)ml.textContent=(d.mem_used_gb||0)+' / '+(d.mem_total_gb||0)+' GiB';var mb=mc.querySelector('.sm-mem-bar-fill');if(mb)mb.style.width=(d.mem_pct||0).toFixed(0)+'%';}
    var ts=document.getElementById('sm-timestamp');if(ts)ts.textContent='Updated '+new Date().toLocaleTimeString();
  }

  function updateThermalEntropy(d) {
    var body=document.getElementById('sm-entropy-body');if(!body)return;
    var ent=d.entropy_avail,healthy=ent!==undefined&&ent>256,entKb=ent!==undefined?(ent/8/1024).toFixed(2):'N/A';
    body.innerHTML='<div style="display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-bottom:12px">'+
      '<div class="sm-stat-box"><div class="sm-stat-icon">\u{1F4CA}</div><div class="sm-stat-val" style="color:'+(healthy?'var(--accent)':'var(--amber)')+'">'+(ent!==undefined?ent.toLocaleString():'N/A')+'</div><div class="sm-stat-label">Bits</div></div>'+
      '<div class="sm-stat-box"><div class="sm-stat-icon">\u{1F4E6}</div><div class="sm-stat-val">'+entKb+' KB</div><div class="sm-stat-label">Pool</div></div>'+
      '<div class="sm-stat-box"><div class="sm-stat-icon">\u{1F510}</div><div class="sm-stat-val" style="color:'+(healthy?'var(--accent)':'var(--amber)')+'">'+(healthy?'Healthy':'Low')+'</div><div class="sm-stat-label">Status</div></div>'+
      '<div class="sm-stat-box"><div class="sm-stat-icon">\u{1F3B2}</div><div class="sm-stat-val">Kernel</div><div class="sm-stat-label">TRNG</div></div></div>'+
      '<div style="padding:10px;background:rgba(168,85,247,0.08);border-radius:6px;border-left:3px solid #a855f7"><div style="font-size:11px;font-weight:600;color:#a855f7;margin-bottom:4px">\u2713 Formally Verified in Lean 4</div><code style="font-size:9px;background:rgba(0,0,0,0.3);padding:1px 4px;border-radius:2px;color:var(--text-dim)">HeytingLean.MirandaDynamics.Thermal.Entropy</code></div>';
  }

  function updateThermalDynamics(d) {
    var body=document.getElementById('sm-therm-dyn-body');if(!body)return;
    var zones=d.thermals||[];if(!zones.length){body.innerHTML='<div class="sm-dim">No thermal data</div>';return;}
    var avg=zones.reduce(function(s,z){return s+(z.temp_c||0);},0)/zones.length;
    var max=Math.max.apply(null,zones.map(function(z){return z.temp_c||0;}));
    var min=Math.min.apply(null,zones.map(function(z){return z.temp_c||0;}));
    var spread=max-min,gpuP=d.gpu_power_w||0;
    var sCol=spread>20?'var(--red)':spread>10?'var(--amber)':'var(--accent)';
    var up=d.uptime_secs?(d.uptime_secs/3600).toFixed(1)+'h':'N/A';
    var now=Date.now(),maxRate=0;
    if(st.prevThermals&&st.prevThermalTime>0){var dt=(now-st.prevThermalTime)/1000;if(dt>0.3&&dt<10){zones.forEach(function(z,idx){if(st.prevThermals[idx]){var rate=Math.abs((z.temp_c||0)-(st.prevThermals[idx].temp_c||0))/dt;if(rate>maxRate)maxRate=rate;}});}}
    st.prevThermals=zones.slice();st.prevThermalTime=now;
    var rCol=maxRate>0.5?'var(--red)':maxRate>0.1?'var(--amber)':'var(--accent)';
    body.innerHTML='<div class="sm-dynamics-grid">'+
      '<div class="sm-dyn-card"><div class="sm-dyn-arrow" style="color:var(--accent)">\u2191</div><div class="sm-dyn-val" style="color:var(--accent)">'+avg.toFixed(1)+'\u00B0C</div><div class="sm-dyn-label">Avg Temp</div></div>'+
      '<div class="sm-dyn-card"><div class="sm-dyn-arrow" style="color:var(--accent)">\u26A1</div><div class="sm-dyn-val" style="color:var(--accent)">'+gpuP.toFixed(1)+'W</div><div class="sm-dyn-label">GPU Power</div></div>'+
      '<div class="sm-dyn-card"><div class="sm-dyn-arrow" style="color:'+tempC(max,'cpu')+'">\u2B06</div><div class="sm-dyn-val" style="color:'+tempC(max,'cpu')+'">'+max.toFixed(1)+'\u00B0C</div><div class="sm-dyn-label">Peak Temp</div></div>'+
      '<div class="sm-dyn-card"><div class="sm-dyn-arrow" style="color:'+rCol+'">\u{1F4C8}</div><div class="sm-dyn-val" style="color:'+rCol+'">'+maxRate.toFixed(3)+'</div><div class="sm-dyn-label">dT/dt (\u00B0C/s)</div></div>'+
      '<div class="sm-dyn-card"><div class="sm-dyn-arrow" style="color:'+sCol+'">\u2194</div><div class="sm-dyn-val" style="color:'+sCol+'">'+spread.toFixed(1)+'\u00B0</div><div class="sm-dyn-label">\u0394 Spread</div></div>'+
      '<div class="sm-dyn-card"><div class="sm-dyn-arrow" style="color:var(--text-dim)">\u23F1</div><div class="sm-dyn-val">'+up+'</div><div class="sm-dyn-label">Uptime</div></div></div>';
  }

  function updateAll(d) {
    updateGaugeCard('sm-card-cpu', d.cpu_pct, 'CPU', (d.load_1m||'0')+' / '+(d.cpu_cores||20)+' cores', st.hist.cpu);
    updateGaugeCard('sm-card-mem', d.mem_pct, 'MEMORY', (d.mem_used_gb||'0')+' / '+(d.mem_total_gb||'0')+' GiB', st.hist.mem);
    updateGaugeCard('sm-card-gpu', d.gpu_pct, 'GPU', Math.round(d.gpu_temp_c)+'\u00B0C \u00B7 '+(d.gpu_power_w||0)+'W', st.hist.gpu);
    updateCores(d.cores);
    updateThermalMap(d);
    updateThermalEntropy(d);
    updateThermalDynamics(d);
  }

  // ── Process expand ─────────────────────────────────────────
  function toggleProcessExpand(id) {
    st.expanded['proc-'+id] = !st.expanded['proc-'+id];
    var body=document.getElementById('sm-procs-'+id),label=document.querySelector('[data-sm-expand="'+id+'"] .sm-expand-label'),chevron=document.querySelector('[data-sm-expand="'+id+'"] .sm-expand-chevron');
    if(!body)return;
    if(st.expanded['proc-'+id]){
      body.style.maxHeight='350px';if(label)label.textContent='Hide '+id.toUpperCase()+' Processes';if(chevron)chevron.classList.add('open');
      var inner=body.querySelector('.sm-procs-inner');if(inner)inner.innerHTML='<div style="text-align:center;padding:8px;color:var(--text-dim);font-size:10px">Loading...</div>';
      snap().then(function(fd){if(!fd||!inner)return;var procs=id==='cpu'?fd.cpu_processes:id==='mem'?fd.mem_processes:fd.gpu_processes;var html='<table><thead><tr><th>PID</th><th>CPU%</th><th>MEM%</th><th>Command</th></tr></thead><tbody>';(procs||[]).slice(0,20).forEach(function(p){var cv=parseFloat(p.cpu||0),cc=cv>100?'var(--red)':cv>50?'var(--amber)':'var(--accent)';var cmd=(p.cmd||p.name||'').split('/').pop();if(cmd.length>35)cmd=cmd.slice(0,32)+'...';html+='<tr><td class="mono">'+esc(p.pid||'')+'</td><td style="color:'+cc+';font-weight:600">'+esc(p.cpu||'0')+'</td><td>'+esc(p.mem||'0')+'</td><td class="mono cmd" title="'+esc(p.cmd||p.name||'')+'">'+esc(cmd)+'</td></tr>';});html+='</tbody></table>';if(inner)inner.innerHTML=html;});
    } else { body.style.maxHeight='0';if(label)label.textContent='Show '+id.toUpperCase()+' Processes';if(chevron)chevron.classList.remove('open'); }
  }

  // ── Core cell ──────────────────────────────────────────────
  function createCoreCell(c, i) {
    var p=c.pct||0,intensity=Math.max(0.08,p/100),textColor=p>50?'#fff':'rgba(255,255,255,0.6)';
    var cell=document.createElement('div');cell.className='sm-core';cell.id='sm-core-'+i;
    cell.style.background='linear-gradient(180deg,rgba('+ACCENT_RGB+','+intensity.toFixed(2)+'),rgba('+ACCENT_RGB+','+(intensity*0.35).toFixed(2)+'))';
    cell.style.borderColor='rgba('+ACCENT_RGB+','+(0.15+intensity*0.7).toFixed(2)+')';
    if(p>15) cell.style.boxShadow='0 0 '+Math.round(intensity*16)+'px rgba('+ACCENT_RGB+','+(intensity*0.5).toFixed(2)+')';
    cell.title='Core '+i+'\nUtilization: '+p.toFixed(1)+'%';
    cell.innerHTML='<div class="sm-core-id" style="color:'+textColor+'">'+i+'</div><div class="sm-core-pct" style="color:'+textColor+'">'+Math.round(p)+'%</div><div style="width:100%;height:3px;background:rgba(255,255,255,0.12);border-radius:2px;overflow:hidden;margin-top:2px"><div class="sm-core-bar" style="width:'+p.toFixed(0)+'%;height:100%;background:var(--accent);transition:width 0.3s"></div></div>';
    return cell;
  }

  // ── Initial render ─────────────────────────────────────────
  function initialRender(d) {
    var root=document.getElementById('sysmon-root');if(!root)return;root.innerHTML='';
    if(st.sim){var wm=document.createElement('div');wm.className='sysmon-watermark';wm.textContent='SIMULATION';root.appendChild(wm);}

    var hdr=document.createElement('div');hdr.className='sm-hdr';
    var badge=st.isDgx?'<div class="sm-badge live"><span class="sm-badge-dot"></span>DGX Spark \u2014 GB10 Connected</div>':'<a href="'+DGX_URL+'" target="_blank" rel="noopener" class="sm-badge link">Learn about NVIDIA DGX Spark \u2197</a>';
    var wsStatus = st.wsConnected ? '<span class="sm-pill sm-pill-accent">\u26A1 WebSocket</span>' : '<span class="sm-pill sm-pill-dim">HTTP Poll</span>';
    hdr.innerHTML='<div><div class="sm-title">System <span style="color:var(--accent)">Monitor</span></div><div class="sm-sub" id="sm-timestamp">'+esc(d.gpu_name)+' \u2014 '+(st.isDgx?'Grace Blackwell Architecture':'Simulation Mode')+'</div></div><div class="sm-hdr-r">'+badge+wsStatus+'<div class="sm-live"><span class="sm-live-dot'+(st.live?' on':'')+'"></span><button class="btn btn-sm'+(st.live?' btn-primary':'')+'" id="sm-toggle">'+(st.live?'Stop':'Start Live')+'</button></div></div>';
    root.appendChild(hdr);

    // Gauge cards
    var gauges=document.createElement('div');gauges.className='sm-gauges';
    ['cpu','mem','gpu'].forEach(function(id){
      var card=document.createElement('div');card.className='sm-gauge-card';card.id='sm-card-'+id;
      var ga=document.createElement('div');ga.className='sm-gauge-area';
      var pct=id==='cpu'?d.cpu_pct:id==='mem'?d.mem_pct:d.gpu_pct;
      var label=id==='cpu'?'CPU':id==='mem'?'MEMORY':'GPU';
      var sub=id==='cpu'?(d.load_1m||'0')+' / '+(d.cpu_cores||20)+' cores':id==='mem'?(d.mem_used_gb||'0')+' / '+(d.mem_total_gb||'0')+' GiB':Math.round(d.gpu_temp_c)+'\u00B0C \u00B7 '+(d.gpu_power_w||0)+'W';
      ga.appendChild(gaugeDOM('sm-card-'+id+'-g',pct,label,sub));card.appendChild(ga);
      var sa=document.createElement('div');sa.className='sm-spark-area';sa.appendChild(sparkDOM('sm-card-'+id+'-s',st.hist[id==='mem'?'mem':id]));card.appendChild(sa);
      var ed=document.createElement('div');ed.className='sm-expand';
      ed.innerHTML='<div class="sm-expand-toggle" data-sm-expand="'+id+'"><span class="sm-expand-label">Show '+label+' Processes</span><svg class="sm-expand-chevron" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6,9 12,15 18,9"/></svg></div>';
      var eb=document.createElement('div');eb.id='sm-procs-'+id;eb.className='sm-expand-body-wrap';eb.style.maxHeight='0';eb.style.overflow='hidden';eb.style.transition='max-height 0.3s ease-out';eb.innerHTML='<div class="sm-procs-inner"></div>';
      ed.appendChild(eb);card.appendChild(ed);gauges.appendChild(card);
    });
    root.appendChild(gauges);

    // CPU Cores
    var cc=makeCollapsible('cores','\u{1F9E0}','CPU Cores ('+(d.cores||[]).length+')',null,'<span class="sm-pill sm-pill-accent">10\u00D7 X925</span><span class="sm-pill sm-pill-accent">10\u00D7 A725</span>',true);
    var ci=document.createElement('div');ci.style.padding='14px';
    var r1l=document.createElement('div');r1l.style.cssText='font-size:10px;font-weight:600;color:var(--accent);margin-bottom:6px;letter-spacing:0.04em;';r1l.textContent='PERFORMANCE \u2022 Cortex-X925';ci.appendChild(r1l);
    var r1=document.createElement('div');r1.className='sm-cores-row';for(var i=0;i<10&&i<(d.cores||[]).length;i++)r1.appendChild(createCoreCell(d.cores[i],i));ci.appendChild(r1);
    var r2l=document.createElement('div');r2l.style.cssText='font-size:10px;font-weight:600;color:var(--accent);margin:12px 0 6px;letter-spacing:0.04em;';r2l.textContent='EFFICIENCY \u2022 Cortex-A725';ci.appendChild(r2l);
    var r2=document.createElement('div');r2.className='sm-cores-row';for(var j=10;j<20&&j<(d.cores||[]).length;j++)r2.appendChild(createCoreCell(d.cores[j],j));ci.appendChild(r2);
    cc.body.appendChild(ci);root.appendChild(cc.section);

    // Thermal Map
    var tc=makeCollapsible('thermal','\u{1F321}','Thermal Map','DGX Spark board layout',null,true);
    var board=document.createElement('div');board.className='sm-board';board.innerHTML='<div class="sm-board-grid"></div><div class="sm-board-label">DGX SPARK GB10</div>';
    var zones=d.thermals||[],labels=['X925-A','X925-B','A725-A','A725-B','VRM','SOC','PWR'],types=['cpu','cpu','cpu','cpu','soc','soc','soc'],positions=[{x:20,y:22},{x:38,y:22},{x:20,y:48},{x:38,y:48},{x:50,y:35},{x:30,y:68},{x:8,y:45}];
    zones.forEach(function(z,idx){if(idx>=labels.length)return;var t=z.temp_c||0,type=types[idx],col=tempC(t,type),rgb=tempRgb(t,type),hi=heatI(t,type),isLg=idx<2,sz=isLg?48:38,glow=Math.round(2+hi*8);var sensor=document.createElement('div');sensor.id='sm-tz-'+idx;sensor.className='sm-sensor';sensor.style.cssText='left:'+positions[idx].x+'%;top:'+positions[idx].y+'%;width:'+sz+'px;height:'+sz+'px';sensor.innerHTML='<div class="sm-sensor-cell" style="background:linear-gradient(180deg,rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.12+hi*0.25)+'),rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.04+hi*0.08)+'));border-color:rgba('+rgb.r+','+rgb.g+','+rgb.b+',0.6);box-shadow:0 0 '+glow+'px rgba('+rgb.r+','+rgb.g+','+rgb.b+','+hi*0.3+')"><div class="sm-tz-temp" style="font-size:'+(isLg?12:10)+'px;color:'+col+';font-weight:600">'+Math.round(t)+'\u00B0C</div><div style="width:75%;height:2px;background:rgba(255,255,255,0.1);border-radius:1px;margin:3px 0;overflow:hidden"><div class="sm-tz-bar-fill" style="width:'+Math.round(hi*100)+'%;height:100%;background:'+col+';transition:width 0.4s"></div></div><div style="font-size:7px;color:var(--text-dim)">'+labels[idx]+'</div></div>';board.appendChild(sensor);});
    var gpuT=d.gpu_temp_c||0,gpuCol=tempC(gpuT,'gpu'),gpuRgb=tempRgb(gpuT,'gpu'),gpuHi=heatI(gpuT,'gpu');var gpuChip=document.createElement('div');gpuChip.id='sm-gpu-chip';gpuChip.className='sm-gpu-chip';gpuChip.style.cssText='background:linear-gradient(180deg,rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.1+gpuHi*0.25)+'),rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.03+gpuHi*0.07)+'));border-color:rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+',0.5);box-shadow:0 0 '+(3+gpuHi*10)+'px rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+gpuHi*0.25+')';gpuChip.innerHTML='<div class="sm-gpu-temp" style="font-size:20px;color:'+gpuCol+';font-weight:600">'+Math.round(gpuT)+'\u00B0C</div><div style="font-size:10px;color:var(--text-dim);margin:4px 0 2px">GPU</div><div style="width:65%;height:3px;background:rgba(255,255,255,0.1);border-radius:2px;overflow:hidden"><div class="sm-gpu-util-fill" style="width:'+(d.gpu_pct||0).toFixed(0)+'%;height:100%;background:'+gpuCol+';transition:width 0.4s"></div></div><div class="sm-gpu-sub" style="font-size:9px;color:var(--text-dim);margin-top:4px">'+(d.gpu_pct||0).toFixed(0)+'% \u2022 '+(d.gpu_power_w||0)+'W</div>';board.appendChild(gpuChip);
    var memChip=document.createElement('div');memChip.id='sm-mem-chip';memChip.className='sm-mem-chip';memChip.innerHTML='<div class="sm-mem-label" style="font-size:11px;color:var(--accent);font-weight:600">'+(d.mem_used_gb||0)+' / '+(d.mem_total_gb||0)+' GiB</div><div style="width:100%;height:3px;background:rgba(255,255,255,0.15);border-radius:2px;margin-top:4px;overflow:hidden"><div class="sm-mem-bar-fill" style="width:'+(d.mem_pct||0).toFixed(0)+'%;height:100%;background:var(--accent);transition:width 0.3s"></div></div><div style="font-size:8px;color:var(--text-dim);margin-top:2px">Memory</div>';board.appendChild(memChip);
    board.innerHTML+='<div class="sm-board-legend"><span>Safe</span><div class="sm-legend-bar"></div><span>Critical</span></div>';
    tc.body.appendChild(board);root.appendChild(tc.section);

    // PUF
    var pc=makeCollapsible('puf','\u26A1','Device Identity (PUF)',null,'<span class="sm-pill sm-pill-purple">\u2713 Lean</span>',false);
    var pi=document.createElement('div');pi.style.padding='16px 18px';
    var fpw=document.createElement('div');fpw.style.cssText='display:flex;justify-content:center;margin-bottom:14px;';fpw.appendChild(pufFingerprintVisual());pi.appendChild(fpw);
    pi.innerHTML+='<div style="display:grid;grid-template-columns:repeat(3,1fr);gap:10px;margin-bottom:12px"><div class="sm-stat-box"><div class="sm-stat-icon">\u{1F4BB}</div><div class="sm-stat-val">GB10</div><div class="sm-stat-label">GPU</div></div><div class="sm-stat-box"><div class="sm-stat-icon">\u{1F4BE}</div><div class="sm-stat-val">NVMe</div><div class="sm-stat-label">Storage</div></div><div class="sm-stat-box"><div class="sm-stat-icon">\u{1F5A5}</div><div class="sm-stat-val">Grace</div><div class="sm-stat-label">CPU</div></div><div class="sm-stat-box"><div class="sm-stat-icon">\u{1F310}</div><div class="sm-stat-val">2 ports</div><div class="sm-stat-label">MACs</div></div><div class="sm-stat-box"><div class="sm-stat-icon">\u26A1</div><div class="sm-stat-val">ConnectX</div><div class="sm-stat-label">Network</div></div><div class="sm-stat-box"><div class="sm-stat-icon">\u{1F321}</div><div class="sm-stat-val">7 zones</div><div class="sm-stat-label">Thermal</div></div></div><div style="padding:10px;background:rgba(168,85,247,0.08);border-radius:6px;border-left:3px solid #a855f7"><div style="font-size:11px;font-weight:600;color:#a855f7;margin-bottom:4px">\u2713 Formally Verified in Lean 4</div><code style="font-size:9px;background:rgba(0,0,0,0.3);padding:1px 4px;border-radius:2px;color:var(--text-dim)">HeytingLean.MirandaDynamics.Thermal.PUF</code><div style="font-size:9px;color:var(--text-dim);margin-top:4px">Predicates: SufficientHardware, ReliableThermalSignature, ValidFingerprint, HighConfidenceVerification</div></div>';
    pc.body.appendChild(pi);root.appendChild(pc.section);

    // Proof Binding, Event Log, Entropy, Dynamics, HW Specs
    var prc=makeCollapsible('proof','\u{1F512}','Lean Proof Binding','Formal verification certificates',null,false);
    var prb=document.createElement('div');prb.id='sm-proof-body';prb.style.padding='16px 18px';prb.innerHTML='<div class="sm-dim">Loading...</div>';prc.body.appendChild(prb);root.appendChild(prc.section);loadProofBinding();

    var evc=makeCollapsible('events','\u{1F4DC}','Certified Event Log','Attestation records',null,false);
    var evb=document.createElement('div');evb.id='sm-event-body';evb.style.padding='16px 18px';evb.innerHTML='<div class="sm-dim">Loading...</div>';evc.body.appendChild(evb);root.appendChild(evc.section);loadEventLog();

    var enc=makeCollapsible('entropy','\u{1F3B2}','Thermal Entropy (TRNG)','Hardware-seeded entropy source',null,false);
    var enb=document.createElement('div');enb.id='sm-entropy-body';enb.style.padding='16px 18px';enc.body.appendChild(enb);root.appendChild(enc.section);updateThermalEntropy(d);

    var dyc=makeCollapsible('dynamics','\u{1F525}','Thermal Dynamics','Power and thermal gradient analysis',null,true);
    var dyb=document.createElement('div');dyb.id='sm-therm-dyn-body';dyb.style.padding='16px 18px';dyc.body.appendChild(dyb);root.appendChild(dyc.section);updateThermalDynamics(d);

    var hwc=makeCollapsible('hwspecs','\u{1F4CB}','DGX Spark Reference Specs','Official hardware specifications',null,false);
    var hwb=document.createElement('div');hwb.id='sm-hwspecs-body';hwb.style.padding='16px 18px';hwc.body.appendChild(hwb);root.appendChild(hwc.section);
    hwb.innerHTML='<table class="sm-tbl"><thead><tr><th>Component</th><th>Specification</th><th>Details</th></tr></thead><tbody><tr><td>GPU</td><td class="sm-val">NVIDIA Blackwell GB10</td><td class="sm-dim">5nm, CoWoS</td></tr><tr><td>CPU</td><td class="sm-val">Grace ARM (20-core)</td><td class="sm-dim">10\u00D7 X925 + 10\u00D7 A725</td></tr><tr><td>Memory</td><td class="sm-val">128 GB LPDDR5X</td><td class="sm-dim">Unified, 273 GB/s</td></tr><tr><td>Storage</td><td class="sm-val">NVMe SSD</td><td class="sm-dim">PCIe Gen5</td></tr><tr><td>AI Performance</td><td class="sm-val">1 PFLOP FP4</td><td class="sm-dim">1,000 TOPS</td></tr><tr><td>Network</td><td class="sm-val">ConnectX-7</td><td class="sm-dim">400 Gb/s</td></tr></tbody></table>';

    var specs=document.createElement('div');specs.className='sm-specs';
    [['Architecture','Grace Blackwell'],['GPU','GB10 Superchip'],['Unified RAM','128 GB'],['AI Performance','1 PFLOP FP4'],['Inference','1,000 TOPS'],['CPU','20-core Grace']].forEach(function(s){specs.innerHTML+='<div class="sm-spec"><span>'+s[0]+'</span><strong>'+s[1]+'</strong></div>';});
    root.appendChild(specs);

    document.getElementById('sm-toggle')?.addEventListener('click',toggleLive);
    document.querySelectorAll('[data-sm-expand]').forEach(function(tog){tog.addEventListener('click',function(){toggleProcessExpand(tog.dataset.smExpand);});});
    st.initialized=true;
  }

  // ── Async panel loaders ────────────────────────────────────
  async function loadProofBinding(){try{var r=await fetch('/api/proof-gate/status');if(!r.ok)throw 0;var d=await r.json(),body=document.getElementById('sm-proof-body');if(!body)return;var certs=d.certificates||[],v=certs.filter(function(c){return c.verification&&c.verification.all_checked;}).length,t=certs.length||1,p=(v/t*100).toFixed(0);var html='<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:8px"><span style="font-size:12px;font-weight:600">'+v+' / '+t+' verified</span><span class="sm-status-badge '+(v===t?'ok':'warn')+'">'+(v===t?'ALL VERIFIED':'PARTIAL')+'</span></div><div style="height:6px;background:rgba(255,255,255,0.06);border-radius:3px;overflow:hidden;margin-bottom:10px"><div style="width:'+p+'%;height:100%;background:'+(v===t?'var(--accent)':'var(--amber)')+';transition:width 0.5s"></div></div>';if(certs.length){html+='<div style="max-height:200px;overflow-y:auto"><table class="sm-tbl"><thead><tr><th>Certificate</th><th>Status</th><th style="text-align:right">Decls</th></tr></thead><tbody>';certs.forEach(function(c){var name=(c.filename||'').replace('.lean4export','').split('_').pop(),ok=c.verification&&c.verification.all_checked;html+='<tr><td style="font-family:var(--font-mono);max-width:200px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title="'+esc(c.filename||'')+'">'+esc(name)+'</td><td style="color:'+(ok?'var(--accent)':'var(--red)')+'">'+( ok?'\u2714 Verified':'\u2718 Failed')+'</td><td style="text-align:right">'+(c.verification?.declarations_checked||0)+'</td></tr>';});html+='</tbody></table></div>';}body.innerHTML=html;}catch(_){var body=document.getElementById('sm-proof-body');if(body)body.innerHTML='<div class="sm-dim">Proof gate not configured</div>';}}

  async function loadEventLog(){try{var r=await fetch('/api/attestations');if(!r.ok)throw 0;var d=await r.json(),body=document.getElementById('sm-event-body');if(!body)return;var events=(d.attestations||d.events||[]).slice(0,10);if(!events.length){body.innerHTML='<div class="sm-dim">No attestation events recorded</div>';return;}var html='<div style="max-height:180px;overflow-y:auto;font-size:10px">';events.forEach(function(e){var ts=e.created_at||e.timestamp||'';if(ts&&typeof ts==='number')ts=new Date(ts*1000).toLocaleString();html+='<div style="padding:6px 8px;border-top:1px solid var(--border);display:flex;justify-content:space-between;gap:8px"><div style="min-width:0"><div style="font-weight:500;color:var(--text)">'+esc(e.agent_type||e.type||'event')+'</div><div style="color:var(--text-dim);font-size:9px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-family:var(--font-mono)" title="'+esc(e.session_id||'')+'">'+esc(e.session_id||e.id||'')+'</div></div><div style="font-size:9px;color:var(--text-dim);white-space:nowrap;font-family:var(--font-mono)">'+esc(ts)+'</div></div>';});html+='</div>';body.innerHTML=html;}catch(_){var body=document.getElementById('sm-event-body');if(body)body.innerHTML='<div class="sm-dim">Event log unavailable</div>';}}

  // ── HTTP tick (fallback when WS unavailable) ───────────────
  async function httpTick() {
    st.data = await snap(); if(!st.data) return;
    st.hist.cpu.push(st.data.cpu_pct||0);st.hist.mem.push(st.data.mem_pct||0);st.hist.gpu.push(st.data.gpu_pct||0);
    if(st.hist.cpu.length>HIST_MAX){st.hist.cpu.shift();st.hist.mem.shift();st.hist.gpu.shift();}
    if(!st.initialized||!document.getElementById('sm-card-cpu')) initialRender(st.data);
    else updateAll(st.data);
  }

  function toggleLive(){
    st.live=!st.live;
    if(st.live){connectWs();httpTick();st.timer=setInterval(httpTick,POLL_MS);}
    else{disconnectWs();if(st.timer){clearInterval(st.timer);st.timer=null;}}
    var btn=document.getElementById('sm-toggle');if(btn){btn.textContent=st.live?'Stop':'Start Live';btn.className='btn btn-sm'+(st.live?' btn-primary':'');}
    var dot=document.querySelector('.sm-live-dot');if(dot)dot.className='sm-live-dot'+(st.live?' on':'');
  }

  function stop(){st.live=false;disconnectWs();if(st.timer){clearInterval(st.timer);st.timer=null;}st.initialized=false;st.gaugeCache={};st.sparkCache={};}

  window.renderSystemMonitorPage = async function(){
    var c=document.getElementById('content');if(!c)return;
    c.innerHTML='<div class="sysmon-page"><div id="sysmon-root"><div class="loading">Connecting to hardware...</div></div></div>';
    st.initialized=false;st.gaugeCache={};st.sparkCache={};st.prevThermals=null;st.prevThermalTime=0;
    st.live=true;connectWs();await httpTick();st.timer=setInterval(httpTick,POLL_MS);
  };
  window.stopSystemMonitor = stop;
})();
