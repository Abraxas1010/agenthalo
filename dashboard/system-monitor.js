/* System Monitor — AgentHALO Dashboard
 * Real hardware monitoring for NVIDIA DGX Spark with thermal map,
 * expandable CPU/Memory/GPU process sections, and animated gauges.
 * Ported from HeytingLean System Dashboard SYSTEM tab.
 */
'use strict';
(function() {

  var DGX_URL = 'https://www.nvidia.com/en-us/products/workstations/dgx-spark/';
  var POLL_MS = 2000;
  var NVIDIA_THERMAL = { gpu:{n:83,t:90,c:100}, cpu:{n:85,t:95,c:105}, nvme:{n:70,t:75,c:85}, soc:{n:85,t:90,c:95} };
  var st = { live:false, timer:null, isDgx:false, sim:false, data:null, hist:{cpu:[],mem:[],gpu:[]}, maxH:60, expanded:{} };

  function esc(s) { var d=document.createElement('div'); d.textContent=s; return d.innerHTML; }

  // ── Fetch real data ────────────────────────────────────────
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
      load_1m:(5+Math.sin(t*0.1)*3).toFixed(2),load_5m:(4.5).toFixed(2),load_15m:(4).toFixed(2),
      mem_pct:42+Math.sin(t*0.05)*5,mem_used_gb:((42+Math.sin(t*0.05)*5)/100*128).toFixed(1),mem_total_gb:'128.0',
      thermals:[{label:'acpitz',temp_c:76+Math.sin(t*0.06)*5},{label:'acpitz',temp_c:58+Math.sin(t*0.09)*4},{label:'acpitz',temp_c:75+Math.sin(t*0.07)*5},{label:'acpitz',temp_c:57+Math.sin(t*0.11)*3},{label:'acpitz',temp_c:76+Math.sin(t*0.08)*5},{label:'acpitz',temp_c:57+Math.sin(t*0.1)*3},{label:'acpitz',temp_c:66+Math.sin(t*0.05)*4}],
      cpu_processes:[{pid:'1234',user:'abraxas',cpu:'120',mem:'0.8',rss:'1078864',cmd:'lean proof_check.lean'},{pid:'5678',user:'abraxas',cpu:'99',mem:'14.8',rss:'18676652',cmd:'full_kernel_sky_service'}],
      mem_processes:[{pid:'5678',user:'abraxas',cpu:'99',mem:'14.8',rss:'18676652',cmd:'full_kernel_sky_service'}],
      gpu_processes:[], is_dgx:false};
  }

  // ── Colors ─────────────────────────────────────────────────
  function pctC(p){return p>=90?'#ef4444':p>=70?'#f59e0b':p>=40?'#00ee00':'#00bfff';}
  function tempC(t,type){
    var s=NVIDIA_THERMAL[type||'cpu']||NVIDIA_THERMAL.cpu;
    if(t>=s.c) return '#dc2660';
    if(t>=s.t) return '#ef4444';
    if(t>=s.n) return '#f59e0b';
    if(t>=s.n*0.6) return '#00ee00';
    return '#00bfff';
  }
  function tempRgb(t,type){
    var c=tempC(t,type); if(c.startsWith('#')){var h=c.slice(1);return{r:parseInt(h.substr(0,2),16),g:parseInt(h.substr(2,2),16),b:parseInt(h.substr(4,2),16)};} return{r:0,g:238,b:0};
  }
  function heatI(t,type){var s=NVIDIA_THERMAL[type||'cpu']||NVIDIA_THERMAL.cpu;return Math.max(0,Math.min(1,(t-30)/(s.c-30)));}

  // ── SVG Gauge ──────────────────────────────────────────────
  function gauge(pct,label,sub) {
    var c=pctC(pct),r=48,cx=55,cy=55,sw=7,circ=2*Math.PI*r,dash=circ*(pct/100);
    return '<div class="sm-gauge"><svg viewBox="0 0 110 110" class="sm-gauge-svg">' +
      '<circle cx="'+cx+'" cy="'+cy+'" r="'+r+'" fill="none" stroke="rgba(255,255,255,0.05)" stroke-width="'+sw+'"/>' +
      '<circle cx="'+cx+'" cy="'+cy+'" r="'+r+'" fill="none" stroke="'+c+'" stroke-width="'+sw+'" stroke-dasharray="'+dash.toFixed(1)+' '+circ.toFixed(1)+'" stroke-linecap="round" transform="rotate(-90 '+cx+' '+cy+')" style="transition:stroke-dasharray 0.8s;filter:drop-shadow(0 0 4px '+c+')"/>' +
      '<text x="'+cx+'" y="'+(cy-2)+'" text-anchor="middle" fill="'+c+'" font-size="20" font-weight="700">'+Math.round(pct)+'%</text>' +
      '<text x="'+cx+'" y="'+(cy+14)+'" text-anchor="middle" fill="var(--text-dim)" font-size="9">'+esc(label)+'</text>' +
    '</svg><div class="sm-gauge-sub">'+esc(sub)+'</div></div>';
  }

  // ── Sparkline SVG ──────────────────────────────────────────
  function spark(data) {
    if(!data.length) return '';
    var w=120,h=28,mx=Math.max.apply(null,data.concat([1]));
    var pts=data.map(function(v,i){return(i/(data.length-1||1)*w).toFixed(1)+','+(h-v/mx*h).toFixed(1);}).join(' ');
    var c=pctC(data[data.length-1]);
    return '<svg viewBox="0 0 '+w+' '+h+'" class="sm-spark"><polyline points="'+pts+'" fill="none" stroke="'+c+'" stroke-width="1.5" stroke-linejoin="round" style="filter:drop-shadow(0 0 2px '+c+')"/></svg>';
  }

  // ── Expandable Process Section ─────────────────────────────
  function processSection(id, label, procs) {
    var isExp = !!st.expanded[id];
    var count = (procs||[]).length;
    var html = '<div class="sm-expand">' +
      '<div class="sm-expand-toggle" data-expand="'+id+'">' +
        '<span class="sm-expand-label">'+esc(isExp?'Hide':'Show')+' '+esc(label)+' Processes</span>' +
        '<span class="sm-expand-count">'+count+'</span>' +
        '<svg class="sm-expand-chevron'+(isExp?' open':'')+'" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="6,9 12,15 18,9"/></svg>' +
      '</div>';
    if(isExp && procs && procs.length) {
      html += '<div class="sm-expand-body"><table><thead><tr><th>PID</th><th>User</th><th>CPU%</th><th>MEM%</th>';
      if(id==='gpu') html += '<th>GPU Mem</th>';
      html += '<th>Command</th></tr></thead><tbody>';
      procs.forEach(function(p) {
        var cpuVal=parseFloat(p.cpu||0),cpuCol=cpuVal>100?'var(--red)':cpuVal>50?'var(--amber)':'var(--green)';
        var cmd=(p.cmd||p.name||'').split('/').pop();
        if(cmd.length>30) cmd=cmd.slice(0,27)+'...';
        html += '<tr><td class="mono">'+esc(p.pid||'')+'</td><td>'+esc(p.user||'')+'</td><td style="color:'+cpuCol+';font-weight:600">'+esc(p.cpu||'0')+'</td><td>'+esc(p.mem||'0')+'</td>';
        if(id==='gpu') html += '<td>'+esc(p.gpu_mem_mib||'0')+' MiB</td>';
        html += '<td class="mono cmd" title="'+esc(p.cmd||p.name||'')+'">'+esc(cmd)+'</td></tr>';
      });
      html += '</tbody></table></div>';
    } else if(isExp) {
      html += '<div class="sm-expand-body"><div class="sm-expand-empty">No processes</div></div>';
    }
    html += '</div>';
    return html;
  }

  // ── CPU Cores Grid ─────────────────────────────────────────
  function coresGrid(cores) {
    if(!cores||!cores.length) return '';
    var html = '<div class="sm-cores">';
    cores.forEach(function(c,i) {
      var p=c.pct||0,col=pctC(p),isP=i<10,glow=p>60?'box-shadow:0 0 8px '+col+';':'';
      html += '<div class="sm-core" style="border-color:'+col+';'+glow+'">' +
        '<div class="sm-core-fill" style="height:'+p.toFixed(0)+'%;background:'+col+'"></div>' +
        '<div class="sm-core-info"><span class="sm-core-pct" style="color:'+col+'">'+Math.round(p)+'</span><span class="sm-core-type">'+(isP?'X925':'A725')+'</span></div>' +
        '<span class="sm-core-id">'+i+'</span>' +
      '</div>';
    });
    html += '</div>';
    return html;
  }

  // ── Thermal Map (DGX Spark board layout) ───────────────────
  function thermalMap(d) {
    var zones = d.thermals || [];
    if(!zones.length) return '';
    // Map ACPI zones to DGX Spark components
    var labels = ['X925-A','X925-B','A725-A','A725-B','VRM','SOC','PWR'];
    var types = ['cpu','cpu','cpu','cpu','soc','soc','soc'];
    var positions = [{x:20,y:22},{x:38,y:22},{x:20,y:48},{x:38,y:48},{x:50,y:35},{x:30,y:68},{x:8,y:45}];

    var html = '<div class="sm-board">' +
      '<div class="sm-board-grid"></div>' +
      '<div class="sm-board-label">DGX SPARK GB10</div>';

    // CPU + peripheral thermal zones
    zones.forEach(function(z,i) {
      if(i>=labels.length) return;
      var t=z.temp_c||0, type=types[i]||'cpu', col=tempC(t,type), rgb=tempRgb(t,type), hi=heatI(t,type);
      var pos=positions[i], isLarge=i<2;
      var size=isLarge?48:38, glow=Math.round(2+hi*8);
      html += '<div class="sm-sensor" style="left:'+pos.x+'%;top:'+pos.y+'%;width:'+size+'px;height:'+size+'px">' +
        '<div class="sm-sensor-cell" style="background:linear-gradient(180deg,rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.12+hi*0.25)+') 0%,rgba('+rgb.r+','+rgb.g+','+rgb.b+','+(0.04+hi*0.08)+') 100%);border-color:rgba('+rgb.r+','+rgb.g+','+rgb.b+',0.6);box-shadow:0 0 '+glow+'px rgba('+rgb.r+','+rgb.g+','+rgb.b+','+hi*0.3+')">' +
          '<div style="font-size:'+(isLarge?12:10)+'px;color:'+col+';font-weight:600">'+Math.round(t)+'\u00B0C</div>' +
          '<div style="width:75%;height:2px;background:rgba(255,255,255,0.1);border-radius:1px;margin:3px 0;overflow:hidden"><div style="width:'+Math.round(hi*100)+'%;height:100%;background:'+col+';transition:width 0.4s"></div></div>' +
          '<div style="font-size:7px;color:var(--text-dim)">'+labels[i]+'</div>' +
        '</div>' +
      '</div>';
    });

    // GPU (large component)
    var gpuT=d.gpu_temp_c||0, gpuCol=tempC(gpuT,'gpu'), gpuRgb=tempRgb(gpuT,'gpu'), gpuHi=heatI(gpuT,'gpu');
    html += '<div class="sm-gpu-chip" style="background:linear-gradient(180deg,rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.1+gpuHi*0.25)+'),rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+(0.03+gpuHi*0.07)+'));border-color:rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+',0.5);box-shadow:0 0 '+(3+gpuHi*10)+'px rgba('+gpuRgb.r+','+gpuRgb.g+','+gpuRgb.b+','+gpuHi*0.25+')">' +
      '<div style="font-size:20px;color:'+gpuCol+';font-weight:600">'+Math.round(gpuT)+'\u00B0C</div>' +
      '<div style="font-size:10px;color:var(--text-dim);margin:4px 0 2px">GPU</div>' +
      '<div style="width:65%;height:3px;background:rgba(255,255,255,0.1);border-radius:2px;overflow:hidden"><div style="width:'+(d.gpu_pct||0).toFixed(0)+'%;height:100%;background:'+gpuCol+';transition:width 0.4s"></div></div>' +
      '<div style="font-size:9px;color:var(--text-dim);margin-top:4px">'+(d.gpu_pct||0).toFixed(0)+'% \u2022 '+(d.gpu_power_w||0)+'W</div>' +
    '</div>';

    // Memory chip
    var memP=d.mem_pct||0,memI=memP/100;
    html += '<div class="sm-mem-chip" style="box-shadow:0 0 '+(4+memI*8)+'px rgba(0,238,0,'+memI*0.3+')">' +
      '<div style="font-size:11px;color:var(--green);font-weight:600">'+(d.mem_used_gb||0)+' / '+(d.mem_total_gb||0)+' GiB</div>' +
      '<div style="width:100%;height:3px;background:rgba(255,255,255,0.15);border-radius:2px;margin-top:4px;overflow:hidden"><div style="width:'+memP.toFixed(0)+'%;height:100%;background:var(--green);transition:width 0.3s"></div></div>' +
      '<div style="font-size:8px;color:var(--text-dim);margin-top:2px">Memory</div>' +
    '</div>';

    // Legend
    html += '<div class="sm-board-legend"><span>Safe</span><div class="sm-legend-bar"></div><span>Critical</span></div>';

    html += '</div>';
    return html;
  }

  // ── Main Render ────────────────────────────────────────────
  function render() {
    var el=document.getElementById('sysmon-root');
    if(!el) return;
    var d=st.data;
    if(!d){el.innerHTML='<div class="loading">Connecting...</div>';return;}

    var wm = st.sim ? '<div class="sysmon-watermark">SIMULATION</div>' : '';
    var badge = st.isDgx
      ? '<div class="sm-badge live"><span class="sm-badge-dot"></span>DGX Spark \u2014 GB10 Connected</div>'
      : '<a href="'+DGX_URL+'" target="_blank" rel="noopener" class="sm-badge link">Learn about NVIDIA DGX Spark \u2197</a>';

    el.innerHTML = wm +
      '<div class="sm-hdr">' +
        '<div><div class="sm-title">System <span style="color:var(--green)">Monitor</span></div>' +
        '<div class="sm-sub">'+esc(d.gpu_name)+' \u2014 '+(st.isDgx?'Grace Blackwell Architecture':'Simulation Mode')+'</div></div>' +
        '<div class="sm-hdr-r">'+badge+'<div class="sm-live"><span class="sm-live-dot'+(st.live?' on':'')+'"></span><button class="btn btn-sm'+(st.live?' btn-primary':'')+'" id="sm-toggle">'+(st.live?'Stop':'Start Live')+'</button></div></div>' +
      '</div>' +

      // Gauges + sparklines
      '<div class="sm-gauges">' +
        '<div class="sm-gauge-card">' + gauge(d.cpu_pct,'CPU',(d.load_1m||'0')+' / '+(d.cpu_cores||20)+' cores') + spark(st.hist.cpu) +
          processSection('cpu','CPU',d.cpu_processes) + '</div>' +
        '<div class="sm-gauge-card">' + gauge(d.mem_pct,'MEMORY',(d.mem_used_gb||'0')+' / '+(d.mem_total_gb||'0')+' GiB') + spark(st.hist.mem) +
          processSection('mem','Memory',d.mem_processes) + '</div>' +
        '<div class="sm-gauge-card">' + gauge(d.gpu_pct,'GPU',Math.round(d.gpu_temp_c)+'\u00B0C \u00B7 '+(d.gpu_power_w||0)+'W') + spark(st.hist.gpu) +
          processSection('gpu','GPU',d.gpu_processes) + '</div>' +
      '</div>' +

      // CPU Cores
      '<div class="sm-section"><div class="sm-sec-hdr">\u{1F9E0} CPU Cores (20) <span class="sm-sec-sub">10\u00D7 X925 Performance + 10\u00D7 A725 Efficiency</span></div>' +
        coresGrid(d.cores) + '</div>' +

      // Thermal Map
      '<div class="sm-section"><div class="sm-sec-hdr">\u{1F321} Thermal Map <span class="sm-sec-sub">DGX Spark board layout \u2014 NVIDIA thermal specifications</span></div>' +
        thermalMap(d) + '</div>' +

      // Specs
      '<div class="sm-specs">' +
        '<div class="sm-spec"><span>Architecture</span><strong>Grace Blackwell</strong></div>' +
        '<div class="sm-spec"><span>GPU</span><strong>GB10 Superchip</strong></div>' +
        '<div class="sm-spec"><span>Unified RAM</span><strong>128 GB</strong></div>' +
        '<div class="sm-spec"><span>AI Performance</span><strong>1 PFLOP FP4</strong></div>' +
        '<div class="sm-spec"><span>Inference</span><strong>1,000 TOPS</strong></div>' +
        '<div class="sm-spec"><span>CPU</span><strong>20-core Grace</strong></div>' +
      '</div>';

    // Bind events
    document.getElementById('sm-toggle')?.addEventListener('click', toggleLive);
    document.querySelectorAll('[data-expand]').forEach(function(tog) {
      tog.addEventListener('click', function() {
        var id=tog.dataset.expand;
        st.expanded[id]=!st.expanded[id];
        render();
      });
    });
  }

  async function tick() {
    st.data = await snap();
    if(st.data){st.hist.cpu.push(st.data.cpu_pct||0);st.hist.mem.push(st.data.mem_pct||0);st.hist.gpu.push(st.data.gpu_pct||0);
      if(st.hist.cpu.length>st.maxH){st.hist.cpu.shift();st.hist.mem.shift();st.hist.gpu.shift();}}
    render();
  }
  function toggleLive(){st.live=!st.live;if(st.live){tick();st.timer=setInterval(tick,POLL_MS);}else{clearInterval(st.timer);st.timer=null;render();}}
  function stop(){st.live=false;if(st.timer){clearInterval(st.timer);st.timer=null;}}

  window.renderSystemMonitorPage = async function(){
    var c=document.getElementById('content');if(!c) return;
    c.innerHTML='<div class="sysmon-page"><div id="sysmon-root"><div class="loading">Connecting to hardware...</div></div></div>';
    st.live=true; await tick(); st.timer=setInterval(tick,POLL_MS);
  };
  window.stopSystemMonitor = stop;
})();
