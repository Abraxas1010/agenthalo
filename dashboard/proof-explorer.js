/* Proof Explorer — AgentHALO Dashboard
 * Interactive 2D canvas + 3D Three.js visualization of the HeytingLean proof lattice.
 * Ported from apoth3osis_webapp ProofExplorerViz.tsx, adapted for vanilla JS + CRT green theme.
 */
'use strict';
(function() {

  // ── Family colors — Green neon CRT theme (orange→green swap) ──
  var FAMILY_COLORS = {
    // TOP FAMILIES — Green/neon theme (was orange)
    TaoAnalysis:'#00ee00',Crypto:'#00cc44',Numbers:'#00dd33',Blockchain:'#00bb22',Logic:'#00ff41',
    // High frequency — cool greens + accents
    Tests:'#2e8b57',CLI:'#3cb371',Quantum:'#39ff14',ProofWidgets:'#66cdaa',LoF:'#dc143c',
    // Medium frequency — varied
    DarwinsCage:'#20b2aa',Payments:'#00fa9a',LambdaIR:'#9acd32',Economics:'#7fff00',Noneism:'#4b0082',
    Privacy:'#00ced1',DeFi:'#ffd700',Governance:'#8fbc8f',Probability:'#48d1cc',
    // Blues/teals
    Graph:'#00bfff',Analysis:'#00ff88',Clifford:'#7b68ee',Tensor:'#1e90ff',CCI:'#00ced1',
    Bridges:'#6495ed',Process:'#00fa9a',Epistemic:'#9932cc',Nucleus:'#00ee44',Contracts:'#32cd32',
    // Accent colors
    Theorems:'#00ff66',Topos:'#4169e1',WPP:'#00cc88',Calculus:'#66ff99',ATheory:'#00ffaa',
    EndToEnd:'#708090',Lens:'#00ff7f',MiniC:'#7fff00',Util:'#a9a9a9',Generative:'#9370db',
    Computing:'#40e0d0',Math:'#00ee66',Ontology:'#663399',Metrics:'#5f9ea0',Geo:'#8b008b',
    C:'#228b22',LeanCoreV2:'#708090',Physics:'#32cd32',Visual:'#00cc99',Runtime:'#00ff7f',
    Computation:'#48d1cc',Cybernetics:'#8b008b',Examples:'#556b2f',PTS:'#00ff41',
    ATP:'#00ccff',HeytingVeil:'#aa55cc',Topology:'#ff69b4',Chem:'#32cd32',Other:'#607060'
  };
  var KIND_COLORS = {
    theorem:'#00ff44',lemma:'#00cc66',def:'#00aaff',example:'#00ff66',axiom:'#ffff00',
    structure:'#ff00ff',class:'#00ffff',instance:'#ff88ff',opaque:'#888888',abbrev:'#aaaaaa',inductive:'#ff69b4'
  };
  var CERT_LEVELS = [
    {key:'endToEnd',label:'End-to-End',color:'#39ff14'},
    {key:'miniC',label:'MiniC',color:'#00bfff'},
    {key:'lambdaIR',label:'Lambda IR',color:'#ffd700'},
    {key:'leanCore',label:'LeanCore',color:'#ff8c00'},
    {key:'none',label:'Standard',color:'#444444'}
  ];

  // ── State ──────────────────────────────────────────────────
  var data = null, loading = true, error = null;
  var viewMode = '3d'; // '2d' | '3d' — default 3D
  var layoutMode = 'umap', edgeMode = 'knn', sizeMode = 'none', colorMode = 'family';
  var enabledFamilies = {}, enabledKinds = {};
  var view = { s: 1, ox: 0, oy: 0 };
  var hoverIdx = null, selectedIdx = null;
  var drag = null;
  var canvasW = 800, canvasH = 500;
  var mdsCache = null;
  var filteredIdx = [];
  var threeCleanup = null;
  var allFamilies = [], allKinds = [];

  function esc(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }

  // Convert any CSS color to rgba with given alpha (0-1)
  function rgba(color, alpha) {
    // Expand 3-digit hex to 6-digit
    var c = color;
    if (/^#[0-9a-fA-F]{3}$/.test(c)) {
      c = '#' + c[1] + c[1] + c[2] + c[2] + c[3] + c[3];
    }
    if (/^#[0-9a-fA-F]{6}$/.test(c)) {
      var r = parseInt(c.slice(1, 3), 16);
      var g = parseInt(c.slice(3, 5), 16);
      var b = parseInt(c.slice(5, 7), 16);
      return 'rgba(' + r + ',' + g + ',' + b + ',' + alpha + ')';
    }
    return color; // fallback
  }

  // ── Color / Size helpers ───────────────────────────────────
  function colorOf(it) {
    if (colorMode === 'kind') return KIND_COLORS[it.kind || 'def'] || '#888';
    if (colorMode === 'cert') { var l = CERT_LEVELS.find(function(c) { return c.key === (it.certLevel || 'none'); }); return l ? l.color : '#444'; }
    return FAMILY_COLORS[it.family || 'Other'] || '#888';
  }
  function sizeOf(it, base) {
    if (sizeMode === 'depIn') return base * (1 + (it.depStats?.in || 0) * 0.5);
    if (sizeMode === 'depOut') return base * (1 + (it.depStats?.out || 0) * 0.6);
    if (sizeMode === 'pageRank') return base * (1 + (it.depStats?.pageRank || 0) * 3000);
    return base;
  }

  // ── Layout points ──────────────────────────────────────────
  function getPts() {
    if (!data) return [];
    if (layoutMode === 'mds' && mdsCache) return mdsCache;
    return data.items.map(function(it) { return it.pos ? { x: it.pos.x, y: it.pos.y } : { x: 0.5, y: 0.5 }; });
  }

  // ── MDS computation ────────────────────────────────────────
  function computeMDS() {
    if (!data || mdsCache) return;
    var items = data.items.slice(0, 2000);
    var n = items.length;
    if (!n) return;
    var D = [], B = [];
    for (var i = 0; i < n; i++) { D[i] = []; B[i] = []; for (var j = 0; j < n; j++) { var d = l1(items[i].features, items[j].features); D[i][j] = d; B[i][j] = -0.5 * d * d; } }
    var rand = function() { return Array.from({ length: n }, Math.random); };
    var mv = function(M, v) { return M.map(function(r) { return r.reduce(function(s, x, k) { return s + x * v[k]; }, 0); }); };
    var nm = function(v) { return Math.hypot.apply(null, v) || 1; };
    var po = function(v, u) { var dot = v.reduce(function(s, x, i) { return s + x * u[i]; }, 0); var den = u.reduce(function(s, x) { return s + x * x; }, 0) || 1; return v.map(function(x, i) { return x - (dot / den) * u[i]; }); };
    var u1 = rand(); for (var it = 0; it < 30; it++) { u1 = mv(B, u1); var nr = nm(u1); u1 = u1.map(function(x) { return x / nr; }); }
    var u2 = rand(); for (var it2 = 0; it2 < 30; it2++) { u2 = po(mv(B, u2), u1); var nr2 = nm(u2); u2 = u2.map(function(x) { return x / nr2; }); }
    var pts = u1.map(function(x, i) { return { x: x, y: u2[i] }; });
    var minx = Math.min.apply(null, pts.map(function(p) { return p.x; }));
    var maxx = Math.max.apply(null, pts.map(function(p) { return p.x; }));
    var miny = Math.min.apply(null, pts.map(function(p) { return p.y; }));
    var maxy = Math.max.apply(null, pts.map(function(p) { return p.y; }));
    var norm = pts.map(function(p) { return { x: (p.x - minx) / ((maxx - minx) || 1), y: (p.y - miny) / ((maxy - miny) || 1) }; });
    mdsCache = data.items.map(function(_, i) { return i < norm.length ? norm[i] : { x: 0.5, y: 0.5 }; });
  }
  function l1(a, b) {
    if (!a || !b) return 0;
    return Math.abs((a.implies||0)-(b.implies||0))+Math.abs((a.not||0)-(b.not||0))+Math.abs((a.and||0)-(b.and||0))+
      Math.abs((a.or||0)-(b.or||0))+Math.abs((a.forall||0)-(b.forall||0))+Math.abs((a.exists||0)-(b.exists||0))+
      Math.abs((a.eq||0)-(b.eq||0))+Math.abs((a.tactics||0)-(b.tactics||0))+Math.abs((a.parenDepth||0)-(b.parenDepth||0));
  }

  // ── Filter ─────────────────────────────────────────────────
  function refilter() {
    if (!data) { filteredIdx = []; return; }
    filteredIdx = [];
    data.items.forEach(function(it, i) {
      if (enabledFamilies[it.family || 'Other'] !== false && enabledKinds[it.kind || 'def'] !== false) filteredIdx.push(i);
    });
  }

  // ── 2D Canvas rendering ────────────────────────────────────
  function draw2D() {
    var c = document.getElementById('pe-canvas');
    if (!c || !data) return;
    var ctx = c.getContext('2d');
    var W = c.width, H = c.height;
    var pts = getPts();
    ctx.fillStyle = '#050508';
    ctx.fillRect(0, 0, W, H);
    var m = 40, iW = W - 2 * m, iH = H - 2 * m;
    var toX = function(px) { return m + px * iW * view.s + view.ox; };
    var toY = function(py) { return m + (1 - py) * iH * view.s + view.oy; };
    // Grid
    ctx.strokeStyle = 'rgba(0, 238, 0, 0.03)';
    ctx.lineWidth = 1;
    for (var gx = m; gx <= W - m; gx += 50) { ctx.beginPath(); ctx.moveTo(gx, m); ctx.lineTo(gx, H - m); ctx.stroke(); }
    for (var gy = m; gy <= H - m; gy += 50) { ctx.beginPath(); ctx.moveTo(m, gy); ctx.lineTo(W - m, gy); ctx.stroke(); }
    // Edges
    if (edgeMode !== 'none') {
      ctx.lineWidth = edgeMode === 'bridges' ? 1.5 : 0.6;
      var edges = edgeMode === 'bridges' ? data.edgesSemantic.map(function(e) { return [e.a, e.b]; }) :
                  edgeMode === 'dag' ? data.edges.filter(function(e) { return e[0] < e[1]; }).slice(0, 3000) :
                  data.edges.slice(0, 5000);
      edges.forEach(function(e) {
        var i = e[0], j = e[1];
        if (filteredIdx.indexOf(i) < 0 || filteredIdx.indexOf(j) < 0) return;
        var a = pts[i], b = pts[j];
        if (!a || !b) return;
        var x1 = toX(a.x), y1 = toY(a.y), x2 = toX(b.x), y2 = toY(b.y);
        var grad = ctx.createLinearGradient(x1, y1, x2, y2);
        grad.addColorStop(0, rgba(colorOf(data.items[i]), 0.25));
        grad.addColorStop(1, rgba(colorOf(data.items[j]), 0.25));
        ctx.strokeStyle = grad;
        if (edgeMode === 'dag') {
          ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke();
          var ang = Math.atan2(y2 - y1, x2 - x1);
          ctx.fillStyle = rgba(colorOf(data.items[j]), 0.38);
          ctx.beginPath(); ctx.moveTo(x2, y2);
          ctx.lineTo(x2 - 6 * Math.cos(ang - 0.52), y2 - 6 * Math.sin(ang - 0.52));
          ctx.lineTo(x2 - 6 * Math.cos(ang + 0.52), y2 - 6 * Math.sin(ang + 0.52));
          ctx.closePath(); ctx.fill();
        } else {
          ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke();
        }
      });
    }
    // Nodes
    filteredIdx.forEach(function(i) {
      if (selectedIdx === i) return;
      var it = data.items[i], p = pts[i];
      if (!p) return;
      var x = toX(p.x), y = toY(p.y);
      var r = sizeOf(it, hoverIdx === i ? 4 : 2.5);
      ctx.beginPath(); ctx.fillStyle = colorOf(it); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill();
    });
    // Selected
    if (selectedIdx !== null && filteredIdx.indexOf(selectedIdx) >= 0) {
      var sit = data.items[selectedIdx], sp = pts[selectedIdx];
      if (sit && sp) {
        var sx = toX(sp.x), sy = toY(sp.y);
        ctx.beginPath(); ctx.fillStyle = '#ffffff'; ctx.arc(sx, sy, 7, 0, Math.PI * 2); ctx.fill();
        ctx.beginPath(); ctx.fillStyle = colorOf(sit); ctx.arc(sx, sy, 5, 0, Math.PI * 2); ctx.fill();
      }
    }
  }

  // ── 3D Three.js ────────────────────────────────────────────
  function init3D() {
    if (threeCleanup) { threeCleanup(); threeCleanup = null; }
    var mount = document.getElementById('pe-three-mount');
    if (!mount || !data) return;
    import('/vendor/three.module.js').then(function(THREE) {
      return import('/vendor/three-addons/OrbitControls.js').then(function(mod) {
        var OrbitControls = mod.OrbitControls;
        mount.innerHTML = '';
        var w = mount.clientWidth || 800, h = mount.clientHeight || 500;
        var scene = new THREE.Scene();
        scene.background = new THREE.Color(0x050508);
        scene.fog = new THREE.FogExp2(0x050508, 0.3);
        var camera = new THREE.PerspectiveCamera(60, w / h, 0.1, 1000);
        camera.position.set(0, 0, 2.5);
        var renderer = new THREE.WebGLRenderer({ antialias: true });
        renderer.setSize(w, h);
        mount.appendChild(renderer.domElement);
        var geom = new THREE.SphereGeometry(0.012, 12, 12);
        var meshes = new Map(), meshArr = [];
        filteredIdx.forEach(function(i) {
          var it = data.items[i];
          var mat = new THREE.MeshLambertMaterial({ color: new THREE.Color(colorOf(it)) });
          var mesh = new THREE.Mesh(geom, mat);
          var p = it.pos3 || { x: 0.5, y: 0.5, z: 0.5 };
          mesh.position.set((p.x - 0.5) * 2, (p.y - 0.5) * 2, (p.z - 0.5) * 2);
          var s = sizeOf(it, 1); mesh.scale.setScalar(s);
          mesh._itemIndex = i;
          scene.add(mesh); meshes.set(i, mesh); meshArr.push(mesh);
        });
        // Edges
        if (edgeMode !== 'none') {
          var edges = edgeMode === 'bridges' ? data.edgesSemantic.map(function(e) { return [e.a, e.b]; }) :
                      edgeMode === 'dag' ? data.edges.filter(function(e) { return e[0] < e[1]; }).slice(0, 5000) :
                      data.edges.slice(0, 5000);
          edges.forEach(function(e) {
            if (filteredIdx.indexOf(e[0]) < 0 || filteredIdx.indexOf(e[1]) < 0) return;
            var a = data.items[e[0]]?.pos3, b = data.items[e[1]]?.pos3;
            if (!a || !b) return;
            var lm = new THREE.LineBasicMaterial({ color: new THREE.Color(colorOf(data.items[e[0]])), transparent: true, opacity: 0.3 });
            var lg = new THREE.BufferGeometry().setFromPoints([
              new THREE.Vector3((a.x-.5)*2,(a.y-.5)*2,(a.z-.5)*2),
              new THREE.Vector3((b.x-.5)*2,(b.y-.5)*2,(b.z-.5)*2)
            ]);
            scene.add(new THREE.Line(lg, lm));
          });
        }
        scene.add(new THREE.AmbientLight(0xffffff, 1.5));
        var pl = new THREE.PointLight(0xffffff, 0.5, 10); pl.position.set(3, 3, 3); scene.add(pl);
        var controls = new OrbitControls(camera, renderer.domElement);
        controls.enableDamping = true; controls.dampingFactor = 0.05;
        var raycaster = new THREE.Raycaster(), mouse = new THREE.Vector2();
        var onMove = function(ev) {
          var r = renderer.domElement.getBoundingClientRect();
          mouse.x = ((ev.clientX - r.left) / r.width) * 2 - 1;
          mouse.y = -((ev.clientY - r.top) / r.height) * 2 + 1;
          raycaster.setFromCamera(mouse, camera);
          var hits = raycaster.intersectObjects(meshArr);
          if (hits.length) { hoverIdx = hits[0].object._itemIndex; renderer.domElement.style.cursor = 'pointer'; }
          else { hoverIdx = null; renderer.domElement.style.cursor = 'grab'; }
          updateDetail();
        };
        var onClick = function(ev) {
          var r = renderer.domElement.getBoundingClientRect();
          mouse.x = ((ev.clientX - r.left) / r.width) * 2 - 1;
          mouse.y = -((ev.clientY - r.top) / r.height) * 2 + 1;
          raycaster.setFromCamera(mouse, camera);
          var hits = raycaster.intersectObjects(meshArr);
          if (hits.length) { selectedIdx = hits[0].object._itemIndex; updateDetail(); }
        };
        renderer.domElement.addEventListener('mousemove', onMove);
        renderer.domElement.addEventListener('click', onClick);
        var animId = 0;
        var tick = function() { controls.update(); renderer.render(scene, camera); animId = requestAnimationFrame(tick); };
        tick();
        var onResize = function() { var nw = mount.clientWidth || 800, nh = mount.clientHeight || 500; renderer.setSize(nw, nh); camera.aspect = nw / nh; camera.updateProjectionMatrix(); };
        window.addEventListener('resize', onResize);
        threeCleanup = function() {
          window.removeEventListener('resize', onResize);
          renderer.domElement.removeEventListener('mousemove', onMove);
          renderer.domElement.removeEventListener('click', onClick);
          cancelAnimationFrame(animId);
          renderer.dispose(); mount.innerHTML = '';
        };
      });
    }).catch(function(e) {
      mount.innerHTML = '<div style="color:var(--red);padding:20px">3D unavailable: ' + esc(e.message) + '</div>';
    });
  }

  // ── Detail panel update ────────────────────────────────────
  function updateDetail() {
    var el = document.getElementById('pe-detail');
    if (!el || !data) return;
    var idx = selectedIdx !== null ? selectedIdx : hoverIdx;
    if (idx === null) { el.innerHTML = '<div class="pe-detail-empty">Hover or click a node to see details</div>'; return; }
    var it = data.items[idx];
    if (!it) return;
    var col = colorOf(it);
    var cert = CERT_LEVELS.find(function(c) { return c.key === (it.certLevel || 'none'); }) || CERT_LEVELS[4];
    el.innerHTML =
      '<div class="pe-detail-content" style="border-color:' + rgba(col, 0.27) + '">' +
        '<div class="pe-detail-name" style="color:' + col + '">' + esc(it.name) + '</div>' +
        '<div class="pe-detail-meta">' +
          '<span><span class="pe-dot" style="background:' + (FAMILY_COLORS[it.family||'Other']||'#888') + '"></span>' + esc(it.family || 'Other') + '</span>' +
          '<span><span class="pe-dot" style="background:' + (KIND_COLORS[it.kind||'def']||'#888') + '"></span>' + esc(it.kind || 'def') + '</span>' +
          '<span><span class="pe-dot" style="background:' + cert.color + '"></span><span style="color:' + cert.color + '">' + cert.label + '</span></span>' +
        '</div>' +
        (it.path ? '<div class="pe-detail-path">' + esc(it.path.split('/').slice(-3).join('/')) + ':' + (it.line || '') + '</div>' : '') +
        (it.depStats ? '<div class="pe-detail-deps">in=' + (it.depStats.in||0) + ' out=' + (it.depStats.out||0) + ' pageRank=' + ((it.depStats.pageRank||0)*1000).toFixed(2) + '</div>' : '') +
        (it.snippet ? '<pre class="pe-detail-snippet" style="border-color:' + rgba(col, 0.2) + '">' + esc(it.snippet.slice(0, 300)) + (it.snippet.length > 300 ? '...' : '') + '</pre>' : '') +
      '</div>';
  }

  // ── Settings panel ─────────────────────────────────────────
  function renderSettings() {
    var el = document.getElementById('pe-settings');
    if (!el || !data) return;
    var html = '';
    // View
    html += '<div class="pe-section"><div class="pe-section-title">View</div><div class="pe-btn-row">';
    ['2d','3d'].forEach(function(m) { html += '<button class="pe-btn' + (viewMode === m ? ' active' : '') + '" data-pe-view="' + m + '">' + m.toUpperCase() + '</button>'; });
    html += '</div></div>';
    // Layout (2D only)
    if (viewMode === '2d') {
      html += '<div class="pe-section"><div class="pe-section-title">Layout</div><div class="pe-btn-row">';
      ['umap','mds'].forEach(function(m) { html += '<button class="pe-btn' + (layoutMode === m ? ' active' : '') + '" data-pe-layout="' + m + '">' + m.toUpperCase() + '</button>'; });
      html += '</div></div>';
    }
    // Edges
    html += '<div class="pe-section"><div class="pe-section-title">Edges</div><div class="pe-btn-row pe-btn-wrap">';
    ['none','knn','dag','bridges'].forEach(function(m) { html += '<button class="pe-btn' + (edgeMode === m ? ' active' : '') + '" data-pe-edge="' + m + '">' + m + '</button>'; });
    html += '</div></div>';
    // Size
    html += '<div class="pe-section"><div class="pe-section-title">Size</div><div class="pe-btn-row pe-btn-wrap">';
    ['none','depIn','depOut','pageRank'].forEach(function(m) { html += '<button class="pe-btn' + (sizeMode === m ? ' active' : '') + '" data-pe-size="' + m + '">' + m + '</button>'; });
    html += '</div></div>';
    // Color
    html += '<div class="pe-section"><div class="pe-section-title">Color</div><div class="pe-btn-row pe-btn-wrap">';
    ['family','kind','cert'].forEach(function(m) { html += '<button class="pe-btn' + (colorMode === m ? ' active' : '') + '" data-pe-color="' + m + '">' + m + '</button>'; });
    html += '</div></div>';
    // Families
    html += '<div class="pe-section"><div class="pe-section-title-row"><span class="pe-section-title">Families</span><span class="pe-toggle-btns"><button data-pe-fam-all="1">All</button><button data-pe-fam-none="1">None</button></span></div>';
    html += '<div class="pe-filter-grid">';
    allFamilies.forEach(function(f) {
      html += '<label class="pe-filter-label"><input type="checkbox" data-pe-fam="' + esc(f) + '"' + (enabledFamilies[f] !== false ? ' checked' : '') + '><span class="pe-dot" style="background:' + (FAMILY_COLORS[f]||'#888') + '"></span><span>' + esc(f) + '</span></label>';
    });
    html += '</div></div>';
    // Kinds
    html += '<div class="pe-section"><div class="pe-section-title-row"><span class="pe-section-title">Kinds</span><span class="pe-toggle-btns"><button data-pe-kind-all="1">All</button><button data-pe-kind-none="1">None</button></span></div>';
    html += '<div class="pe-filter-row">';
    allKinds.forEach(function(k) {
      html += '<label class="pe-filter-label"><input type="checkbox" data-pe-kind="' + esc(k) + '"' + (enabledKinds[k] !== false ? ' checked' : '') + '><span class="pe-dot" style="background:' + (KIND_COLORS[k]||'#888') + '"></span><span>' + esc(k) + '</span></label>';
    });
    html += '</div></div>';
    // Cert legend
    html += '<div class="pe-section"><div class="pe-section-title">Certification</div><div class="pe-cert-legend">';
    CERT_LEVELS.forEach(function(c) {
      var count = data.items.filter(function(it) { return (it.certLevel || 'none') === c.key; }).length;
      html += '<div class="pe-cert-row"><span class="pe-dot" style="background:' + c.color + '"></span><span style="color:' + c.color + '">' + c.label + '</span><span class="pe-cert-count">' + count + '</span></div>';
    });
    html += '</div></div>';
    // Stats
    html += '<div class="pe-section pe-stats">' + filteredIdx.length.toLocaleString() + ' / ' + data.items.length.toLocaleString() + ' nodes<br>' + data.edges.length.toLocaleString() + ' edges | ' + data.edgesSemantic.length + ' bridges</div>';
    el.innerHTML = html;
    bindSettingsEvents();
  }

  function bindSettingsEvents() {
    document.querySelectorAll('[data-pe-view]').forEach(function(b) { b.onclick = function() { viewMode = b.dataset.peView; rebuild(); }; });
    document.querySelectorAll('[data-pe-layout]').forEach(function(b) { b.onclick = function() { layoutMode = b.dataset.peLayout; if (layoutMode === 'mds') computeMDS(); redraw(); renderSettings(); }; });
    document.querySelectorAll('[data-pe-edge]').forEach(function(b) { b.onclick = function() { edgeMode = b.dataset.peEdge; rebuild(); }; });
    document.querySelectorAll('[data-pe-size]').forEach(function(b) { b.onclick = function() { sizeMode = b.dataset.peSize; redraw(); renderSettings(); }; });
    document.querySelectorAll('[data-pe-color]').forEach(function(b) { b.onclick = function() { colorMode = b.dataset.peColor; redraw(); renderSettings(); }; });
    document.querySelectorAll('[data-pe-fam]').forEach(function(cb) { cb.onchange = function() { enabledFamilies[cb.dataset.peFam] = cb.checked; refilter(); rebuild(); }; });
    document.querySelectorAll('[data-pe-kind]').forEach(function(cb) { cb.onchange = function() { enabledKinds[cb.dataset.peKind] = cb.checked; refilter(); rebuild(); }; });
    var famAll = document.querySelector('[data-pe-fam-all]');
    var famNone = document.querySelector('[data-pe-fam-none]');
    if (famAll) famAll.onclick = function() { allFamilies.forEach(function(f) { enabledFamilies[f] = true; }); refilter(); rebuild(); renderSettings(); };
    if (famNone) famNone.onclick = function() { allFamilies.forEach(function(f) { enabledFamilies[f] = false; }); refilter(); rebuild(); renderSettings(); };
    var kindAll = document.querySelector('[data-pe-kind-all]');
    var kindNone = document.querySelector('[data-pe-kind-none]');
    if (kindAll) kindAll.onclick = function() { allKinds.forEach(function(k) { enabledKinds[k] = true; }); refilter(); rebuild(); renderSettings(); };
    if (kindNone) kindNone.onclick = function() { allKinds.forEach(function(k) { enabledKinds[k] = false; }); refilter(); rebuild(); renderSettings(); };
  }

  // ── Canvas mouse events ────────────────────────────────────
  function bind2DEvents() {
    var c = document.getElementById('pe-canvas');
    if (!c || !data) return;
    var m = 40;
    c.onmousemove = function(e) {
      var pts = getPts();
      if (drag) {
        view.ox = drag.ox + e.clientX - drag.x;
        view.oy = drag.oy + e.clientY - drag.y;
        draw2D();
        return;
      }
      var r = c.getBoundingClientRect(), mx = e.clientX - r.left, my = e.clientY - r.top;
      var iW = c.width - 2 * m, iH = c.height - 2 * m;
      var best = -1, bestD = 15;
      filteredIdx.forEach(function(i) {
        var p = pts[i]; if (!p) return;
        var px = m + p.x * iW * view.s + view.ox, py = m + (1 - p.y) * iH * view.s + view.oy;
        var d = Math.hypot(px - mx, py - my);
        if (d < bestD) { bestD = d; best = i; }
      });
      hoverIdx = best >= 0 ? best : null;
      updateDetail();
      draw2D();
    };
    c.onmousedown = function(e) { drag = { x: e.clientX, y: e.clientY, ox: view.ox, oy: view.oy }; };
    window.onmouseup = function(e) {
      if (drag && Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) {
        // Click — select nearest
        var pts = getPts(), r = c.getBoundingClientRect(), mx = e.clientX - r.left, my = e.clientY - r.top;
        var iW = c.width - 2 * m, iH = c.height - 2 * m;
        var best = -1, bestD = 15;
        filteredIdx.forEach(function(i) {
          var p = pts[i]; if (!p) return;
          var px = m + p.x * iW * view.s + view.ox, py = m + (1 - p.y) * iH * view.s + view.oy;
          var d = Math.hypot(px - mx, py - my);
          if (d < bestD) { bestD = d; best = i; }
        });
        if (best >= 0) { selectedIdx = best; updateDetail(); draw2D(); }
      }
      drag = null;
    };
    c.onwheel = function(e) {
      e.preventDefault();
      var r = c.getBoundingClientRect(), sx = e.clientX - r.left, sy = e.clientY - r.top;
      var iW = c.width - 2 * m, iH = c.height - 2 * m;
      var factor = e.deltaY < 0 ? 1.15 : 0.87;
      var newS = Math.max(0.3, Math.min(8, view.s * factor));
      var wx = (sx - m - view.ox) / (iW * view.s), wy = 1 - (sy - m - view.oy) / (iH * view.s);
      view.ox = sx - m - wx * iW * newS;
      view.oy = sy - m - (1 - wy) * iH * newS;
      view.s = newS;
      draw2D();
    };
  }

  // ── Rebuild (view/filter change) ───────────────────────────
  function rebuild() {
    renderSettings();
    if (viewMode === '2d') {
      if (threeCleanup) { threeCleanup(); threeCleanup = null; }
      var container = document.getElementById('pe-viz-container');
      if (container) {
        container.innerHTML = '<canvas id="pe-canvas" style="width:100%;height:100%;cursor:grab"></canvas>';
        var c = document.getElementById('pe-canvas');
        var rect = container.getBoundingClientRect();
        c.width = Math.floor(rect.width); c.height = Math.floor(rect.height);
        bind2DEvents();
        draw2D();
      }
    } else {
      var container = document.getElementById('pe-viz-container');
      if (container) {
        container.innerHTML = '<div id="pe-three-mount" style="width:100%;height:100%"></div>';
        init3D();
      }
    }
    updateDetail();
  }

  function redraw() {
    if (viewMode === '2d') draw2D();
    // 3D color/size updates handled by rebuild for now
  }

  // ── Data fetching — build proof lattice from live Observatory data ──
  function fetchProofData() {
    // Try static file first (pre-generated with UMAP positions)
    return fetch('/proof-lattice.json?v=' + Date.now())
      .then(function(r) {
        if (!r.ok) throw new Error('no static file');
        return r.json();
      })
      .then(function(d) {
        if (d.items && d.items.length > 0) return d;
        throw new Error('empty static file');
      })
      .catch(function() {
        // Fall back to live Observatory API — build proof lattice dynamically
        return Promise.all([
          fetch('/api/observatory/treemap').then(function(r) { return r.json(); }),
          fetch('/api/observatory/depgraph').then(function(r) { return r.json(); }),
        ]).then(function(results) {
          var treemap = results[0], depgraph = results[1];
          var files = treemap.files || [];
          if (!files.length) throw new Error('No Lean files found. Configure a Lean project path first.');
          // Build items from file data — each file becomes a node
          var items = files.map(function(f, i) {
            var parts = f.path.split('/');
            var family = parts.length > 1 ? parts[0] : 'Other';
            var name = parts[parts.length - 1].replace('.lean', '');
            // Distribute in a circle with jitter for visual spread
            var angle = (i / files.length) * Math.PI * 2;
            var radius = 0.3 + Math.random() * 0.15;
            var x = 0.5 + Math.cos(angle) * radius;
            var y = 0.5 + Math.sin(angle) * radius;
            var z = 0.5 + (Math.random() - 0.5) * 0.4;
            return {
              id: f.path, name: name, kind: f.sorry_count > 0 ? 'axiom' : 'theorem',
              path: f.path, line: 1, family: family,
              snippet: f.path + ' — ' + f.lines + ' lines, ' + f.decl_count + ' declarations',
              features: { implies: 0, not: 0, and: 0, or: 0, forall: 0, exists: 0, eq: 0, tactics: 0, parenDepth: 0, length: f.lines },
              depStats: { out: 0, in: 0, pageRank: f.lines / files.reduce(function(s, ff) { return s + ff.lines; }, 1), minHopCore: -1 },
              pos: { x: x, y: y },
              pos3: { x: x, y: y, z: z },
              importance: f.health_score || 0.5,
              certLevel: f.health_status === 'clean' ? 'leanCore' : 'none'
            };
          });
          // Build edges from dependency graph
          var nodeIdx = {};
          items.forEach(function(it, i) { nodeIdx[it.id] = i; });
          var edges = [];
          (depgraph.edges || []).forEach(function(e) {
            // Map module names to file paths
            var fromMod = e[0], toMod = e[1];
            var fromPath = fromMod.replace(/\./g, '/') + '.lean';
            var toPath = toMod.replace(/\./g, '/') + '.lean';
            var fi = nodeIdx[fromPath], ti = nodeIdx[toPath];
            if (fi !== undefined && ti !== undefined) edges.push([fi, ti]);
          });
          return { items: items, edges: edges, edgesSemantic: [] };
        });
      });
  }

  // ── Page render ────────────────────────────────────────────
  window.renderProofExplorerPage = function() {
    var content = document.getElementById('content');
    if (!content) return;
    content.innerHTML =
      '<div class="pe-page">' +
        '<div class="pe-header">' +
          '<div><div class="pe-title">Proof <span class="pe-title-accent">Explorer</span></div>' +
          '<div class="pe-subtitle">Navigate the HeytingLean proof lattice. Each node is a theorem, lemma, or definition. Explore connections, filter by category, and discover structural patterns.</div></div>' +
        '</div>' +
        '<div class="pe-layout">' +
          '<div class="pe-sidebar card" id="pe-settings"><div class="loading">Loading...</div></div>' +
          '<div class="pe-main">' +
            '<div class="pe-viz-container" id="pe-viz-container"><div class="loading">Loading proof lattice...</div></div>' +
            '<div class="pe-detail-panel" id="pe-detail"><div class="pe-detail-empty">Hover or click a node to see details</div></div>' +
          '</div>' +
        '</div>' +
      '</div>';

    if (!data) {
      // Try live Observatory API first (works for any user's Lean project),
      // fall back to static proof-lattice.json
      fetchProofData()
        .then(function(d) {
          data = d;
          allFamilies = []; allKinds = [];
          var famSet = {}, kindSet = {};
          d.items.forEach(function(it) { famSet[it.family || 'Other'] = 1; kindSet[it.kind || 'def'] = 1; });
          allFamilies = Object.keys(famSet).sort();
          allKinds = Object.keys(kindSet).sort();
          allFamilies.forEach(function(f) { if (enabledFamilies[f] === undefined) enabledFamilies[f] = true; });
          allKinds.forEach(function(k) { if (enabledKinds[k] === undefined) enabledKinds[k] = true; });
          refilter();
          loading = false;
          rebuild();
        })
        .catch(function(e) { error = e.message; document.getElementById('pe-viz-container').innerHTML = '<div class="obs-error">Failed to load proof data: ' + esc(e.message) + '</div>'; });
    } else {
      refilter();
      rebuild();
    }
  };

})();
