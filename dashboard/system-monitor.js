/* System Monitor — AgentHALO Dashboard
 * Real hardware monitoring for NVIDIA DGX Spark.
 * Uses DOM-based in-place updates (cached element refs) for smooth animation.
 * Ported from HeytingLean System Dashboard gauge/sparkline/process pattern.
 */
'use strict';
(function() {

  var DGX_URL = 'https://www.nvidia.com/en-us/products/workstations/dgx-spark/';
  var POLL_MS = 2000;
  var HIST_MAX = 60;
  var NVIDIA_THERMAL = { gpu:{n:83,t:90,c:100}, cpu:{n:85,t:95,c:105}, nvme:{n:70,t:75,c:85}, soc:{n:85,t:90,c:95} };
  var st = { live:false, timer:null, isDgx:false, sim:false, data:null, hist:{cpu:[],mem:[],gpu:[]},
    gaugeCache:{}, sparkCache:{}, expanded:{}, initialized:false };

  function esc(s) { var d=document.createElement('div'); d.textContent=s; return d.innerHTML; }

  // ── Fetch ──────────────────────────────────────────────────
  async function snap() {
    try {
      var r = await fetch('/api/system/snapshot');
      if (!r.ok) throw 0;
      var d = await r.json();
      st.isDgx = !!d.is_dgx; st.sim = !st.isDgx;
      var mT=d.mem_total_kb||1, mU=d.mem_used_kb||0;
      d.mem_pct=(mU/mT)*100; d.mem_used_gb=(mU/1048576).toFixed(1); d.mem_total_gb=(mT/1048576).toFixed(1);
      d.cpu_pct=0;
      if(d.cores&&d.cores.length) d.cpu_pct=d.cores.reduce(function(s,c){return s+(c.pct||0);},0)/d.cores.length;
      else if(d.load_1m&&d.cpu_cores) d.cpu_pct=Math.min(100,(d.load_1m/d.cpu_cores)*100);
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
      gpu_processes:[], is_dgx:false};
  }

  // ── Colors ─────────────────────────────────────────────────
  function pctC(p){return p>=90?'#ef4444':p>=70?'#f59e0b':p>=40?'#00ee00':'#00bfff';}
  function tempC(t,type){var s=NVIDIA_THERMAL[type||'cpu']||NVIDIA_THERMAL.cpu;if(t>=s.c)return'#dc2660';if(t>=s.t)return'#ef4444';if(t>=s.n)return'#f59e0b';if(t>=s.n*0.6)return'#00ee00';return'#00bfff';}
  function tempRgb(t,type){var c=tempC(t,type);if(c.startsWith('#')){var h=c.slice(1);return{r:parseInt(h.substr(0,2),16),g:parseInt(h.substr(2,2),16),b:parseInt(h.substr(4,2),16)};}return{r:0,g:238,b:0};}
  function heatI(t,type){var s=NVIDIA_THERMAL[type||'cpu']||NVIDIA_THERMAL.cpu;return Math.max(0,Math.min(1,(t-30)/(s.c-30)));}
  function polarToCart(cx,cy,r,deg){var rad=(deg-90)*Math.PI/180;return{x:cx+r*Math.cos(rad),y:cy+r*Math.sin(rad)};}
  function describeArc(x,y,r,sa,ea){var s=polarToCart(x,y,r,ea),e=polarToCart(x,y,r,sa);return'M '+s.x+' '+s.y+' A '+r+' '+r+' 0 '+(ea-sa<=180?'0':'1')+' 0 '+e.x+' '+e.y;}

  // ── DOM-based Gauge (cached refs, in-place update) ─────────
  function gaugeDOM(id, pct, label, sub) {
    pct = Math.max(0, Math.min(100, pct || 0));
    var size=90, sw=8, r=(size-sw)/2, circ=r*Math.PI, off=circ-(pct/100)*circ;
    var color = pctC(pct);
    var cached = st.gaugeCache[id];
    if (cached && cached.progress && document.body.contains(cached.progress)) {
      cached.progress.style.strokeDashoffset = off;
      cached.progress.style.stroke = color;
      cached.valueText.textContent = Math.round(pct) + '%';
      if (cached.subEl && sub) cached.subEl.textContent = sub;
      return cached.container;
    }
    var wrap = document.createElement('div');
    wrap.className = 'sm-gauge-wrap';
    var svg = document.createElementNS('http://www.w3.org/2000/svg','svg');
    svg.setAttribute('width', size); svg.setAttribute('height', size/2+10);
    svg.setAttribute('viewBox', '0 0 '+size+' '+(size/2+10));
    var arcD = describeArc(size/2, size/2, r, 180, 360);
    var bg = document.createElementNS('http://www.w3.org/2000/svg','path');
    bg.setAttribute('d',arcD); bg.setAttribute('fill','none'); bg.setAttribute('stroke','rgba(255,255,255,0.08)');
    bg.setAttribute('stroke-width',sw); bg.setAttribute('stroke-linecap','round');
    svg.appendChild(bg);
    var prog = document.createElementNS('http://www.w3.org/2000/svg','path');
    prog.setAttribute('d',arcD); prog.setAttribute('fill','none'); prog.setAttribute('stroke',color);
    prog.setAttribute('stroke-width',sw); prog.setAttribute('stroke-linecap','round');
    prog.setAttribute('stroke-dasharray',circ); prog.setAttribute('stroke-dashoffset',off);
    prog.style.transition = 'stroke-dashoffset 0.5s ease-out, stroke 0.3s ease';
    svg.appendChild(prog);
    var txt = document.createElementNS('http://www.w3.org/2000/svg','text');
    txt.setAttribute('x',size/2); txt.setAttribute('y',size/2+5); txt.setAttribute('text-anchor','middle');
    txt.setAttribute('fill','#fafafa'); txt.setAttribute('font-size','14'); txt.setAttribute('font-weight','700');
    txt.textContent = Math.round(pct) + '%';
    svg.appendChild(txt);
    wrap.appendChild(svg);
    var lbl = document.createElement('div');
    lbl.className = 'sm-gauge-label'; lbl.textContent = label;
    wrap.appendChild(lbl);
    var subEl = null;
    if (sub) { subEl = document.createElement('div'); subEl.className = 'sm-gauge-sub'; subEl.textContent = sub; wrap.appendChild(subEl); }
    st.gaugeCache[id] = { container:wrap, progress:prog, valueText:txt, subEl:subEl };
    return wrap;
  }

  // ── DOM-based Sparkline (cached refs, in-place update) ─────
  function sparkDOM(id, data) {
    var w=140,h=36,pad=2;
    if(!data||data.length<2) return document.createElement('div');
    var series = data.length > HIST_MAX ? data.slice(-HIST_MAX) : data;
    var offset = HIST_MAX - series.length;
    var pts = series.map(function(v,i){
      var x = pad + ((offset+i)/(HIST_MAX-1))*(w-pad*2);
      var y = h - pad - (Math.max(0,Math.min(100,v))/100)*(h-pad*2);
      return x.toFixed(1)+','+y.toFixed(1);
    });
    var color = pctC(data[data.length-1]);
    var fillColor = 'rgba(0,238,0,0.1)';
    var cached = st.sparkCache[id];
    if (cached && cached.svg && document.body.contains(cached.svg)) {
      cached.fill.setAttribute('points', pad+','+(h-pad)+' '+pts.join(' ')+' '+(w-pad)+','+(h-pad));
      cached.fill.setAttribute('fill', fillColor);
      cached.line.setAttribute('points', pts.join(' '));
      cached.line.setAttribute('stroke', color);
      var lp = pts[pts.length-1].split(',');
      cached.dot.setAttribute('cx', lp[0]); cached.dot.setAttribute('cy', lp[1]); cached.dot.setAttribute('fill', color);
      return cached.svg;
    }
    var svg = document.createElementNS('http://www.w3.org/2000/svg','svg');
    svg.setAttribute('width',w); svg.setAttribute('height',h);
    svg.setAttribute('viewBox','0 0 '+w+' '+h);
    svg.style.cssText = 'display:block;margin:4px auto 0;';
    // Grid lines
    [0,50,100].forEach(function(v){
      var y = h-pad-(v/100)*(h-pad*2);
      var gl = document.createElementNS('http://www.w3.org/2000/svg','line');
      gl.setAttribute('x1',pad); gl.setAttribute('x2',w-pad); gl.setAttribute('y1',y); gl.setAttribute('y2',y);
      gl.setAttribute('stroke','rgba(255,255,255,'+(v===50?'0.05':'0.08')+')'); gl.setAttribute('stroke-width','1');
      svg.appendChild(gl);
    });
    var fill = document.createElementNS('http://www.w3.org/2000/svg','polygon');
    fill.setAttribute('points', pad+','+(h-pad)+' '+pts.join(' ')+' '+(w-pad)+','+(h-pad));
    fill.setAttribute('fill', fillColor);
    svg.appendChild(fill);
    var line = document.createElementNS('http://www.w3.org/2000/svg','polyline');
    line.setAttribute('points', pts.join(' ')); line.setAttribute('fill','none');
    line.setAttribute('stroke', color); line.setAttribute('stroke-width','1.5');
    line.setAttribute('stroke-linejoin','round'); line.setAttribute('stroke-linecap','round');
    svg.appendChild(line);
    var lp = pts[pts.length-1].split(',');
    var dot = document.createElementNS('http://www.w3.org/2000/svg','circle');
    dot.setAttribute('cx',lp[0]); dot.setAttribute('cy',lp[1]); dot.setAttribute('r','3'); dot.setAttribute('fill',color);
    svg.appendChild(dot);
    st.sparkCache[id] = { svg:svg, fill:fill, line:line, dot:dot };
    return svg;
  }

  // ── Update functions (modify existing DOM, no innerHTML) ───
  function updateGaugeCard(cardId, pct, label, sub, sparkData) {
    var card = document.getElementById(cardId);
    if (!card) return;
    var gWrap = card.querySelector('.sm-gauge-wrap');
    if (gWrap) {
      gaugeDOM(cardId+'-g', pct, label, sub);
    } else {
      var gaugeArea = card.querySelector('.sm-gauge-area');
      if (gaugeArea) { gaugeArea.innerHTML = ''; gaugeArea.appendChild(gaugeDOM(cardId+'-g', pct, label, sub)); }
    }
    var sparkArea = card.querySelector('.sm-spark-area');
    if (sparkArea) { sparkArea.innerHTML = ''; sparkArea.appendChild(sparkDOM(cardId+'-s', sparkData)); }
  }

  function updateCores(cores) {
    if (!cores) return;
    cores.forEach(function(c,i) {
      var el = document.getElementById('sm-core-'+i);
      if (!el) return;
      var p = c.pct||0, col = pctC(p);
      var fill = el.querySelector('.sm-core-fill');
      if (fill) { fill.style.height = p.toFixed(0)+'%'; fill.style.background = col; }
      var pctEl = el.querySelector('.sm-core-pct');
      if (pctEl) { pctEl.textContent = Math.round(p); pctEl.style.color = col; }
      el.style.borderColor = col;
      el.style.boxShadow = p > 60 ? '0 0 8px '+col : 'none';
    });
  }

  function updateThermalMap(d) {
    var zones = d.thermals || [];
    var labels = ['X925-A','X925-B','A725-A','A725-B','VRM','SOC','PWR'];
    var types = ['cpu','cpu','cpu','cpu','soc','soc','soc'];
    zones.forEach(function(z,i) {
      if(i>=labels.length) return;
      var el = document.getElementById('sm-tz-'+i);
      if(!el) return;
      var t=z.temp_c||0, type=types[i], col=tempC(t,type), rgb=tempRgb(t,type), hi=heatI(t,type);
      var glow = Math.round(2+hi*8);
      var cell = el.querySelector('.sm-sensor-cell');
      if(cell) {
        cell.style.background = 'linear-gradient(180deg,rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.12+hi*0.25)+'),rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.04+hi*0.08)+'))';
        cell.style.borderColor = 'rgba('+rgb.r+','+rgb.g+','+rgb.b+',0.6)';
        cell.style.boxShadow = '0 0 '+glow+'px rgba('+rgb.r+','+rgb.g+','+rgb.b+','+hi*0.3+')';
      }
      var tempEl = el.querySelector('.sm-tz-temp');
      if(tempEl) { tempEl.textContent = Math.round(t)+'\u00B0C'; tempEl.style.color = col; }
      var bar = el.querySelector('.sm-tz-bar-fill');
      if(bar) { bar.style.width = Math.round(hi*100)+'%'; bar.style.background = col; }
    });
    // GPU chip
    var gpuT=d.gpu_temp_c||0, gpuCol=tempC(gpuT,'gpu'), gpuRgb=tempRgb(gpuT,'gpu'), gpuHi=heatI(gpuT,'gpu');
    var gpuChip = document.getElementById('sm-gpu-chip');
    if(gpuChip) {
      gpuChip.style.background = 'linear-gradient(180deg,rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.1+gpuHi*0.25)+'),rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.03+gpuHi*0.07)+'))';
      gpuChip.style.borderColor = 'rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+',0.5)';
      gpuChip.style.boxShadow = '0 0 '+(3+gpuHi*10)+'px rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+gpuHi*0.25+')';
      var gTemp = gpuChip.querySelector('.sm-gpu-temp');
      if(gTemp) { gTemp.textContent = Math.round(gpuT)+'\u00B0C'; gTemp.style.color = gpuCol; }
      var gBar = gpuChip.querySelector('.sm-gpu-util-fill');
      if(gBar) { gBar.style.width = (d.gpu_pct||0).toFixed(0)+'%'; gBar.style.background = gpuCol; }
      var gSub = gpuChip.querySelector('.sm-gpu-sub');
      if(gSub) gSub.textContent = (d.gpu_pct||0).toFixed(0)+'% \u2022 '+(d.gpu_power_w||0)+'W';
    }
    // Memory
    var memChip = document.getElementById('sm-mem-chip');
    if(memChip) {
      var memLabel = memChip.querySelector('.sm-mem-label');
      if(memLabel) memLabel.textContent = (d.mem_used_gb||0)+' / '+(d.mem_total_gb||0)+' GiB';
      var memBar = memChip.querySelector('.sm-mem-bar-fill');
      if(memBar) memBar.style.width = (d.mem_pct||0).toFixed(0)+'%';
    }
    // Timestamp
    var ts = document.getElementById('sm-timestamp');
    if(ts) ts.textContent = 'Updated ' + new Date().toLocaleTimeString();
  }

  // ── Process expand toggle ──────────────────────────────────
  function toggleExpand(id) {
    st.expanded[id] = !st.expanded[id];
    var body = document.getElementById('sm-procs-'+id);
    var label = document.querySelector('[data-sm-expand="'+id+'"] .sm-expand-label');
    var chevron = document.querySelector('[data-sm-expand="'+id+'"] .sm-expand-chevron');
    if(!body) return;
    if(st.expanded[id]) {
      body.style.maxHeight = '350px';
      if(label) label.textContent = 'Hide ' + id.toUpperCase() + ' Processes';
      if(chevron) chevron.classList.add('open');
      // Populate processes
      var procs = id==='cpu' ? st.data.cpu_processes : id==='mem' ? st.data.mem_processes : st.data.gpu_processes;
      var inner = body.querySelector('.sm-procs-inner');
      if(inner && procs) {
        var html = '<table><thead><tr><th>PID</th><th>CPU%</th><th>MEM%</th><th>Command</th></tr></thead><tbody>';
        (procs||[]).slice(0,20).forEach(function(p){
          var cpuVal=parseFloat(p.cpu||0), cpuCol=cpuVal>100?'var(--red)':cpuVal>50?'var(--amber)':'var(--green)';
          var cmd=(p.cmd||p.name||'').split('/').pop(); if(cmd.length>35) cmd=cmd.slice(0,32)+'...';
          html+='<tr><td class="mono">'+esc(p.pid||'')+'</td><td style="color:'+cpuCol+';font-weight:600">'+esc(p.cpu||'0')+'</td><td>'+esc(p.mem||'0')+'</td><td class="mono cmd" title="'+esc(p.cmd||p.name||'')+'">'+esc(cmd)+'</td></tr>';
        });
        html += '</tbody></table>';
        inner.innerHTML = html;
      }
    } else {
      body.style.maxHeight = '0';
      if(label) label.textContent = 'Show ' + id.toUpperCase() + ' Processes';
      if(chevron) chevron.classList.remove('open');
    }
  }

  // ── Initial render (builds full DOM once) ──────────────────
  function initialRender(d) {
    var root = document.getElementById('sysmon-root');
    if(!root) return;
    root.innerHTML = '';

    // Watermark
    if(st.sim) { var wm=document.createElement('div'); wm.className='sysmon-watermark'; wm.textContent='SIMULATION'; root.appendChild(wm); }

    // Header
    var hdr = document.createElement('div'); hdr.className='sm-hdr';
    var badge = st.isDgx
      ? '<div class="sm-badge live"><span class="sm-badge-dot"></span>DGX Spark \u2014 GB10 Connected</div>'
      : '<a href="'+DGX_URL+'" target="_blank" rel="noopener" class="sm-badge link">Learn about NVIDIA DGX Spark \u2197</a>';
    hdr.innerHTML = '<div><div class="sm-title">System <span style="color:var(--green)">Monitor</span></div>'+
      '<div class="sm-sub" id="sm-timestamp">'+esc(d.gpu_name)+' \u2014 '+(st.isDgx?'Grace Blackwell Architecture':'Simulation Mode')+'</div></div>'+
      '<div class="sm-hdr-r">'+badge+'<div class="sm-live"><span class="sm-live-dot'+(st.live?' on':'')+'"></span><button class="btn btn-sm'+(st.live?' btn-primary':'')+'" id="sm-toggle">'+(st.live?'Stop':'Start Live')+'</button></div></div>';
    root.appendChild(hdr);

    // Gauge cards
    var gauges = document.createElement('div'); gauges.className='sm-gauges';
    ['cpu','mem','gpu'].forEach(function(id) {
      var card = document.createElement('div'); card.className='sm-gauge-card'; card.id='sm-card-'+id;
      var gaugeArea = document.createElement('div'); gaugeArea.className='sm-gauge-area';
      var pct = id==='cpu'?d.cpu_pct:id==='mem'?d.mem_pct:d.gpu_pct;
      var label = id==='cpu'?'CPU':id==='mem'?'MEMORY':'GPU';
      var sub = id==='cpu'?(d.load_1m||'0')+' / '+(d.cpu_cores||20)+' cores':id==='mem'?(d.mem_used_gb||'0')+' / '+(d.mem_total_gb||'0')+' GiB':Math.round(d.gpu_temp_c)+'\u00B0C \u00B7 '+(d.gpu_power_w||0)+'W';
      gaugeArea.appendChild(gaugeDOM('sm-card-'+id+'-g', pct, label, sub));
      card.appendChild(gaugeArea);
      var sparkArea = document.createElement('div'); sparkArea.className='sm-spark-area';
      sparkArea.appendChild(sparkDOM('sm-card-'+id+'-s', st.hist[id==='mem'?'mem':id]));
      card.appendChild(sparkArea);
      // Expand toggle
      var expandDiv = document.createElement('div'); expandDiv.className='sm-expand';
      expandDiv.innerHTML = '<div class="sm-expand-toggle" data-sm-expand="'+id+'"><span class="sm-expand-label">Show '+label+' Processes</span><svg class="sm-expand-chevron" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6,9 12,15 18,9"/></svg></div>';
      var expandBody = document.createElement('div'); expandBody.id='sm-procs-'+id; expandBody.className='sm-expand-body-wrap';
      expandBody.style.maxHeight='0'; expandBody.style.overflow='hidden'; expandBody.style.transition='max-height 0.3s ease-out';
      expandBody.innerHTML='<div class="sm-procs-inner"></div>';
      expandDiv.appendChild(expandBody);
      card.appendChild(expandDiv);
      gauges.appendChild(card);
    });
    root.appendChild(gauges);

    // CPU Cores
    var coresSection = document.createElement('div'); coresSection.className='sm-section';
    coresSection.innerHTML = '<div class="sm-sec-hdr">\u{1F9E0} CPU Cores (20) <span class="sm-sec-sub">10\u00D7 X925 Performance + 10\u00D7 A725 Efficiency</span></div>';
    var coresGrid = document.createElement('div'); coresGrid.className='sm-cores';
    (d.cores||[]).forEach(function(c,i) {
      var p=c.pct||0, col=pctC(p), isP=i<10;
      var cell = document.createElement('div'); cell.className='sm-core'; cell.id='sm-core-'+i;
      cell.style.borderColor=col; if(p>60) cell.style.boxShadow='0 0 8px '+col;
      cell.innerHTML = '<div class="sm-core-fill" style="height:'+p.toFixed(0)+'%;background:'+col+'"></div>'+
        '<div class="sm-core-info"><span class="sm-core-pct" style="color:'+col+'">'+Math.round(p)+'</span><span class="sm-core-type">'+(isP?'X925':'A725')+'</span></div>'+
        '<span class="sm-core-id">'+i+'</span>';
      coresGrid.appendChild(cell);
    });
    coresSection.appendChild(coresGrid);
    root.appendChild(coresSection);

    // Thermal Map
    var thermalSection = document.createElement('div'); thermalSection.className='sm-section';
    thermalSection.innerHTML = '<div class="sm-sec-hdr">\u{1F321} Thermal Map <span class="sm-sec-sub">DGX Spark board layout</span></div>';
    var board = document.createElement('div'); board.className='sm-board';
    board.innerHTML = '<div class="sm-board-grid"></div><div class="sm-board-label">DGX SPARK GB10</div>';
    var zones = d.thermals||[];
    var labels = ['X925-A','X925-B','A725-A','A725-B','VRM','SOC','PWR'];
    var types = ['cpu','cpu','cpu','cpu','soc','soc','soc'];
    var positions = [{x:20,y:22},{x:38,y:22},{x:20,y:48},{x:38,y:48},{x:50,y:35},{x:30,y:68},{x:8,y:45}];
    zones.forEach(function(z,i) {
      if(i>=labels.length) return;
      var t=z.temp_c||0, type=types[i], col=tempC(t,type), rgb=tempRgb(t,type), hi=heatI(t,type);
      var isLg=i<2, sz=isLg?48:38, glow=Math.round(2+hi*8);
      var sensor = document.createElement('div'); sensor.id='sm-tz-'+i; sensor.className='sm-sensor';
      sensor.style.cssText = 'left:'+positions[i].x+'%;top:'+positions[i].y+'%;width:'+sz+'px;height:'+sz+'px';
      sensor.innerHTML = '<div class="sm-sensor-cell" style="background:linear-gradient(180deg,rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.12+hi*0.25)+'),rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.04+hi*0.08)+'));border-color:rgba('+rgb.r+','+rgb.g+','+rgb.b+',0.6);box-shadow:0 0 '+glow+'px rgba('+rgb.r+','+rgb.g+','+rgb.b+','+hi*0.3+')">'+
        '<div class="sm-tz-temp" style="font-size:'+(isLg?12:10)+'px;color:'+col+';font-weight:600">'+Math.round(t)+'\u00B0C</div>'+
        '<div style="width:75%;height:2px;background:rgba(255,255,255,0.1);border-radius:1px;margin:3px 0;overflow:hidden"><div class="sm-tz-bar-fill" style="width:'+Math.round(hi*100)+'%;height:100%;background:'+col+';transition:width 0.4s"></div></div>'+
        '<div style="font-size:7px;color:var(--text-dim)">'+labels[i]+'</div></div>';
      board.appendChild(sensor);
    });
    // GPU chip
    var gpuT=d.gpu_temp_c||0, gpuCol=tempC(gpuT,'gpu'), gpuRgb=tempRgb(gpuT,'gpu'), gpuHi=heatI(gpuT,'gpu');
    var gpuChip = document.createElement('div'); gpuChip.id='sm-gpu-chip'; gpuChip.className='sm-gpu-chip';
    gpuChip.style.cssText = 'background:linear-gradient(180deg,rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.1+gpuHi*0.25)+'),rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.03+gpuHi*0.07)+'));border-color:rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+',0.5);box-shadow:0 0 '+(3+gpuHi*10)+'px rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+gpuHi*0.25+')';
    gpuChip.innerHTML = '<div class="sm-gpu-temp" style="font-size:20px;color:'+gpuCol+';font-weight:600">'+Math.round(gpuT)+'\u00B0C</div>'+
      '<div style="font-size:10px;color:var(--text-dim);margin:4px 0 2px">GPU</div>'+
      '<div style="width:65%;height:3px;background:rgba(255,255,255,0.1);border-radius:2px;overflow:hidden"><div class="sm-gpu-util-fill" style="width:'+(d.gpu_pct||0).toFixed(0)+'%;height:100%;background:'+gpuCol+';transition:width 0.4s"></div></div>'+
      '<div class="sm-gpu-sub" style="font-size:9px;color:var(--text-dim);margin-top:4px">'+(d.gpu_pct||0).toFixed(0)+'% \u2022 '+(d.gpu_power_w||0)+'W</div>';
    board.appendChild(gpuChip);
    // Memory
    var memChip = document.createElement('div'); memChip.id='sm-mem-chip'; memChip.className='sm-mem-chip';
    memChip.innerHTML = '<div class="sm-mem-label" style="font-size:11px;color:var(--green);font-weight:600">'+(d.mem_used_gb||0)+' / '+(d.mem_total_gb||0)+' GiB</div>'+
      '<div style="width:100%;height:3px;background:rgba(255,255,255,0.15);border-radius:2px;margin-top:4px;overflow:hidden"><div class="sm-mem-bar-fill" style="width:'+(d.mem_pct||0).toFixed(0)+'%;height:100%;background:var(--green);transition:width 0.3s"></div></div>'+
      '<div style="font-size:8px;color:var(--text-dim);margin-top:2px">Memory</div>';
    board.appendChild(memChip);
    board.innerHTML += '<div class="sm-board-legend"><span>Safe</span><div class="sm-legend-bar"></div><span>Critical</span></div>';
    thermalSection.appendChild(board);
    root.appendChild(thermalSection);

    // Specs
    var specs = document.createElement('div'); specs.className='sm-specs';
    [['Architecture','Grace Blackwell'],['GPU','GB10 Superchip'],['Unified RAM','128 GB'],['AI Performance','1 PFLOP FP4'],['Inference','1,000 TOPS'],['CPU','20-core Grace']].forEach(function(s){
      specs.innerHTML += '<div class="sm-spec"><span>'+s[0]+'</span><strong>'+s[1]+'</strong></div>';
    });
    root.appendChild(specs);

    // Bind events
    document.getElementById('sm-toggle')?.addEventListener('click', toggleLive);
    document.querySelectorAll('[data-sm-expand]').forEach(function(tog){
      tog.addEventListener('click', function(){ toggleExpand(tog.dataset.smExpand); });
    });

    st.initialized = true;
  }

  // ── Tick (update existing DOM or create) ───────────────────
  async function tick() {
    st.data = await snap();
    if(!st.data) return;
    st.hist.cpu.push(st.data.cpu_pct||0); st.hist.mem.push(st.data.mem_pct||0); st.hist.gpu.push(st.data.gpu_pct||0);
    if(st.hist.cpu.length>HIST_MAX){st.hist.cpu.shift();st.hist.mem.shift();st.hist.gpu.shift();}

    if(!st.initialized || !document.getElementById('sm-card-cpu')) {
      initialRender(st.data);
      return;
    }
    // In-place updates (no innerHTML rebuild)
    var d = st.data;
    updateGaugeCard('sm-card-cpu', d.cpu_pct, 'CPU', (d.load_1m||'0')+' / '+(d.cpu_cores||20)+' cores', st.hist.cpu);
    updateGaugeCard('sm-card-mem', d.mem_pct, 'MEMORY', (d.mem_used_gb||'0')+' / '+(d.mem_total_gb||'0')+' GiB', st.hist.mem);
    updateGaugeCard('sm-card-gpu', d.gpu_pct, 'GPU', Math.round(d.gpu_temp_c)+'\u00B0C \u00B7 '+(d.gpu_power_w||0)+'W', st.hist.gpu);
    updateCores(d.cores);
    updateThermalMap(d);
  }

  function toggleLive() {
    st.live=!st.live;
    if(st.live){tick();st.timer=setInterval(tick,POLL_MS);}
    else{clearInterval(st.timer);st.timer=null;}
    var btn=document.getElementById('sm-toggle');
    if(btn){btn.textContent=st.live?'Stop':'Start Live';btn.className='btn btn-sm'+(st.live?' btn-primary':'');}
    var dot=document.querySelector('.sm-live-dot');
    if(dot){dot.className='sm-live-dot'+(st.live?' on':'');}
  }
  function stop(){st.live=false;if(st.timer){clearInterval(st.timer);st.timer=null;}st.initialized=false;st.gaugeCache={};st.sparkCache={};}

  window.renderSystemMonitorPage = async function(){
    var c=document.getElementById('content');if(!c)return;
    c.innerHTML='<div class="sysmon-page"><div id="sysmon-root"><div class="loading">Connecting to hardware...</div></div></div>';
    st.initialized=false; st.gaugeCache={}; st.sparkCache={};
    st.live=true; await tick(); st.timer=setInterval(tick,POLL_MS);
  };
  window.stopSystemMonitor = stop;
})();
