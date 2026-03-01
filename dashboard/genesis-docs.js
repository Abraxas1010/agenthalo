/* ================================================================
   Overview Hub + Genesis Docs — Documentation Pages
   ================================================================
   Standalone page renderers loaded as a separate script to avoid
   merge conflicts with the Genesis ceremony overlay code in app.js.
   ================================================================ */
'use strict';

function gdocEsc(v) {
  if (v == null) return '';
  return String(v)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

async function gdocFetchGenesisStatus() {
  const res = await fetch('/api/genesis/status');
  if (!res.ok) throw new Error(`genesis status failed (${res.status})`);
  return res.json();
}

async function hydrateGenesisRuntimePanel() {
  const node = document.getElementById('gdoc-genesis-runtime');
  if (!node) return;
  node.innerHTML = '<div class="gdoc-text">Loading latest ceremony status...</div>';
  try {
    const status = await gdocFetchGenesisStatus();
    const summary = status.summary && typeof status.summary === 'object' ? status.summary : {};
    const pulse = status.curby_pulse_id || summary.curby_pulse_id || null;
    const sourcesCount = status.sources_count || (summary.policy && summary.policy.actual_sources) || 0;
    const digest = status.combined_entropy_sha256 || summary.combined_entropy_sha256 || '';
    const digestShort = typeof digest === 'string' && digest.length > 24
      ? `${digest.slice(0, 24)}...`
      : String(digest || 'not available');

    node.innerHTML = `
      <div class="gdoc-card gdoc-card--blue">
        <div class="gdoc-card-head">Latest Ceremony Status</div>
        <div class="gdoc-card-body">
          <div><strong>Genesis:</strong> ${status.completed ? 'Complete' : 'Pending'}</div>
          <div><strong>Quantum number:</strong> ${pulse ? `Pulse #${gdocEsc(String(pulse))}` : 'Unavailable'}</div>
          <div><strong>Sources used:</strong> ${gdocEsc(String(sourcesCount))}</div>
          <div><strong>Entropy digest:</strong> <code>${gdocEsc(digestShort)}</code></div>
        </div>
      </div>
    `;
  } catch (err) {
    node.innerHTML = `
      <div class="gdoc-card gdoc-card--amber">
        <div class="gdoc-card-head">Latest Ceremony Status</div>
        <div class="gdoc-card-body">
          Could not load runtime Genesis status. ${gdocEsc(String(err && err.message || err || 'unknown error'))}
        </div>
      </div>
    `;
  }
}

/* ================================================================
   OVERVIEW HUB — Table of Contents for all doc/protocol pages
   ================================================================ */

// Registry of documentation pages. Add entries here as new pages ship.
const DOCS_PAGES = [
  {
    id: 'genesis',
    title: 'Genesis Protocol',
    subtitle: 'Formally Verified Entropy Harvest',
    icon: '\u2733',
    color: 'blue',
    status: 'live',
    summary: 'The birth ceremony for your agent. Harvests true randomness from 4 independent sources, combines them cryptographically, and records the result in an immutable ledger. 8 theorems proved in Lean 4.',
    stats: [
      { val: '4', lbl: 'Sources' },
      { val: '64 B', lbl: 'Seed' },
      { val: '8', lbl: 'Proofs' },
    ],
  },
  // Future pages will be added here as they are developed:
  // {
  //   id: 'identity',
  //   title: 'Identity & Wallet',
  //   subtitle: 'Post-Quantum Key Management',
  //   icon: '\u26BF',
  //   color: 'green',
  //   status: 'planned',
  //   summary: 'PQ-signed identity ledger, wallet lifecycle, and anonymous mode.',
  //   stats: [],
  // },
];

function renderDocsOverview() {
  const content = document.getElementById('content');

  const cardsHtml = DOCS_PAGES.map(p => {
    const isLive = p.status === 'live';
    const statusBadge = isLive
      ? '<span class="ovw-card-badge ovw-card-badge--live">Live</span>'
      : '<span class="ovw-card-badge ovw-card-badge--planned">Planned</span>';
    const statsHtml = p.stats.length > 0
      ? `<div class="ovw-card-stats">${p.stats.map(s =>
          `<div class="ovw-card-stat"><div class="ovw-card-stat-val">${s.val}</div><div class="ovw-card-stat-lbl">${s.lbl}</div></div>`
        ).join('')}</div>`
      : '';
    const clickAttr = isLive ? `onclick="location.hash='#/${p.id}'" style="cursor:pointer"` : '';

    return `
      <div class="ovw-card ovw-card--${p.color} ${isLive ? 'ovw-card--clickable' : 'ovw-card--dimmed'}" ${clickAttr}>
        <div class="ovw-card-header">
          <div class="ovw-card-icon ovw-card-icon--${p.color}">${p.icon}</div>
          <div class="ovw-card-titles">
            <div class="ovw-card-title">${p.title}</div>
            <div class="ovw-card-subtitle">${p.subtitle}</div>
          </div>
          ${statusBadge}
        </div>
        <div class="ovw-card-body">${p.summary}</div>
        ${statsHtml}
        ${isLive ? '<div class="ovw-card-go">View details \u2192</div>' : ''}
      </div>
    `;
  }).join('');

  content.innerHTML = `
    <!-- Hero -->
    <div class="gdoc-hero">
      <div class="gdoc-hero-img-wrap">
        <img class="gdoc-hero-img" src="img/agent_halo_logo.png" alt="H.A.L.O."
             onerror="this.style.display='none'">
      </div>
      <div class="gdoc-hero-copy">
        <div class="gdoc-hero-kicker">Agent H.A.L.O. // Documentation</div>
        <div class="gdoc-hero-title">Protocol Overview</div>
        <div class="gdoc-hero-subtitle">Formally verified systems powering your agent</div>
        <div class="gdoc-hero-sep"></div>
        <div class="gdoc-hero-stat-row">
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">${DOCS_PAGES.filter(p => p.status === 'live').length}</div>
            <div class="gdoc-hero-stat-lbl">Live Protocols</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">${DOCS_PAGES.length}</div>
            <div class="gdoc-hero-stat-lbl">Total Sections</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">${DOCS_PAGES.reduce((n, p) => n + p.stats.reduce((a, s) => a + (parseInt(s.val) || 0), 0), 0)}</div>
            <div class="gdoc-hero-stat-lbl">Proved Theorems</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Page cards -->
    <div class="gdoc-section">
      <div class="gdoc-section-title">Protocols &amp; Systems</div>
      <div class="ovw-card-list">
        ${cardsHtml}
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">About This Section</div>
      <p class="gdoc-text">
        Each protocol page documents a formally verified subsystem of Agent H.A.L.O.
        Pages include a <strong>high-level overview</strong> (for everyone),
        <strong>technical details</strong> (Lean proofs, architecture diagrams),
        and <strong>agent access</strong> (CLI commands, MCP tools).
        New pages appear here as protocols ship.
      </p>
    </div>
  `;
}


/* ================================================================
   GENESIS PAGE
   ================================================================ */

function renderGenesis() {
  const content = document.getElementById('content');
  content.innerHTML = `
    <!-- Hero Banner -->
    <div class="gdoc-hero">
      <div class="gdoc-hero-img-wrap">
        <img class="gdoc-hero-img" src="img/agentpmtbootup.png" alt="Genesis"
             onerror="this.style.display='none'">
      </div>
      <div class="gdoc-hero-copy">
        <div class="gdoc-hero-kicker">Agent H.A.L.O. // Identity Ceremony</div>
        <div class="gdoc-hero-title">Genesis Protocol</div>
        <div class="gdoc-hero-subtitle">Formally Verified Entropy Harvest</div>
        <div class="gdoc-hero-sep"></div>
        <div class="gdoc-hero-stat-row">
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">4</div>
            <div class="gdoc-hero-stat-lbl">Entropy Sources</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">64 B</div>
            <div class="gdoc-hero-stat-lbl">Combined Seed</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">8</div>
            <div class="gdoc-hero-stat-lbl">Theorems Proved</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">\u2265 2</div>
            <div class="gdoc-hero-stat-lbl">Min Sources</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Tab Bar -->
    <div class="gdoc-tabs">
      <button class="gdoc-tab active" data-tab="overview" onclick="gdocTab('overview')">F1:OVERVIEW</button>
      <button class="gdoc-tab" data-tab="technical" onclick="gdocTab('technical')">F2:TECHNICAL</button>
      <button class="gdoc-tab" data-tab="access" onclick="gdocTab('access')">F3:ACCESS</button>
    </div>

    <div id="gdoc-content"></div>
  `;
  gdocTab('overview');
}

window.gdocTab = function(tab) {
  document.querySelectorAll('.gdoc-tab').forEach(b => {
    b.classList.toggle('active', b.dataset.tab === tab);
  });
  const el = document.getElementById('gdoc-content');
  if (!el) return;

  switch (tab) {
    case 'overview':
      el.innerHTML = gdocOverview();
      hydrateGenesisRuntimePanel();
      break;
    case 'technical': el.innerHTML = gdocTechnical(); break;
    case 'access': el.innerHTML = gdocAccess(); break;
  }
};

/* ================================================================
   TAB 1: HIGH-LEVEL OVERVIEW
   ================================================================ */
function gdocOverview() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Live Runtime Status</div>
      <div id="gdoc-genesis-runtime"></div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">What Is Genesis?</div>
      <p class="gdoc-text">
        Genesis is the <strong>birth ceremony</strong> for your agent. The very first time you launch
        the dashboard, before you can do anything else, the system harvests true randomness from
        multiple independent sources around the world and combines them into a unique cryptographic
        identity seed. This happens <strong>once</strong> and never again.
      </p>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Why Does This Matter?</div>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-icon-lg">\u2731</div>
          <div class="gdoc-card-head">Uniqueness</div>
          <div class="gdoc-card-body">
            Your agent's identity comes from quantum and cryptographic sources that are
            physically impossible to predict or replicate. No two agents share the same seed.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\u26D3</div>
          <div class="gdoc-card-head">Immutability</div>
          <div class="gdoc-card-body">
            The harvest is recorded in a hash-chained ledger entry that cannot be modified or deleted.
            It is your agent's permanent, auditable birth record.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--amber">
          <div class="gdoc-card-icon-lg">\u26A1</div>
          <div class="gdoc-card-head">Security</div>
          <div class="gdoc-card-body">
            Raw entropy never touches the disk. Only a cryptographic hash is stored.
            Multiple independent sources ensure no single point of compromise.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">How It Works</div>

      <!-- Visual flow: 4 source cards -> combine -> record -->
      <div class="gdoc-pipeline">

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">1</div>
          <div class="gdoc-pipeline-label">Gather</div>
        </div>
        <div class="gdoc-source-grid">
          <div class="gdoc-source-card gdoc-source-card--blue">
            <div class="gdoc-source-icon">\u269B</div>
            <div class="gdoc-source-name">CURBy</div>
            <div class="gdoc-source-sub">Quantum vacuum RNG<br>Univ. of Colorado</div>
            <div class="gdoc-source-bytes">64 bytes</div>
          </div>
          <div class="gdoc-source-card gdoc-source-card--green">
            <div class="gdoc-source-icon">\u2637</div>
            <div class="gdoc-source-name">NIST Beacon</div>
            <div class="gdoc-source-sub">National standards<br>Public audit trail</div>
            <div class="gdoc-source-bytes">64 bytes</div>
          </div>
          <div class="gdoc-source-card gdoc-source-card--yellow">
            <div class="gdoc-source-icon">\u2609</div>
            <div class="gdoc-source-name">drand</div>
            <div class="gdoc-source-sub">Distributed network<br>Multi-party threshold</div>
            <div class="gdoc-source-bytes">32 B \u2192 SHA-512 \u2192 64 B</div>
          </div>
          <div class="gdoc-source-card gdoc-source-card--amber">
            <div class="gdoc-source-icon">\u2699</div>
            <div class="gdoc-source-name">OS Entropy</div>
            <div class="gdoc-source-sub">Hardware CSPRNG<br>Always available</div>
            <div class="gdoc-source-bytes">64 bytes</div>
          </div>
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC \u25BC \u25BC \u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">2</div>
          <div class="gdoc-pipeline-label">Normalize</div>
        </div>
        <div class="gdoc-pipeline-box">
          All sources normalized to <strong>64 bytes</strong> (512 bits).
          drand's 32 bytes are expanded via SHA-512. Wrong-width inputs rejected.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">3</div>
          <div class="gdoc-pipeline-label">Combine</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--accent">
          XOR fold in canonical order: <code>curby \u2295 nist \u2295 drand \u2295 os</code><br>
          Even if one source is compromised, the others contribute genuine randomness.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">4</div>
          <div class="gdoc-pipeline-label">Record &amp; Forget</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--green">
          SHA-256 hash written to <strong>identity ledger</strong> (permanent birth record).<br>
          Raw entropy used in memory, then <strong>discarded</strong> \u2014 never reaches disk.
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Safety Net</div>
      <div class="gdoc-callout">
        <div class="gdoc-callout-icon">\u26A0</div>
        <div class="gdoc-callout-body">
          The system requires at least <strong>2 out of 4</strong> sources to succeed. If your internet
          is down, you still have OS entropy. If a beacon is temporarily unavailable, the remaining
          ones cover for it. The ceremony shows exactly what failed and offers a clear retry.
        </div>
      </div>
    </div>
  `;
}

/* ================================================================
   TAB 2: TECHNICAL DETAILS
   ================================================================ */
function gdocTechnical() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Lean 4 Formal Model</div>
      <p class="gdoc-text">
        The Genesis entropy protocol is formally specified and proved correct in Lean 4.
        Four modules under <code>HeytingLean.Genesis.Entropy</code> define the mathematical
        contract that the Rust runtime must obey. No <code>sorry</code> or <code>admit</code>.
      </p>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Architecture</div>

      <!-- CSS diagram replacing ASCII art -->
      <div class="gdoc-arch">
        <div class="gdoc-arch-top">
          <div class="gdoc-arch-box gdoc-arch-box--root">
            <div class="gdoc-arch-box-title">State.lean</div>
            <div class="gdoc-arch-box-sub">Definitions &mdash; "What are the pieces?"</div>
            <div class="gdoc-arch-box-items">
              <span>4 sources: curby | nist | drand | os</span>
              <span>ByteVec64 = exactly 64 bytes</span>
              <span>EntropySample = source + bytes</span>
              <span>HarvestPolicy = min 2 sources</span>
              <span>HarvestTrace = successes + failures</span>
            </div>
          </div>
        </div>

        <div class="gdoc-arch-connectors">
          <div class="gdoc-arch-vline"></div>
          <div class="gdoc-arch-branch">
            <div class="gdoc-arch-hline"></div>
            <div class="gdoc-arch-hline"></div>
            <div class="gdoc-arch-hline"></div>
          </div>
        </div>

        <div class="gdoc-arch-bottom">
          <div class="gdoc-arch-box gdoc-arch-box--blue">
            <div class="gdoc-arch-box-title">Sources.lean</div>
            <div class="gdoc-arch-box-sub">Normalization</div>
            <div class="gdoc-arch-box-items">
              <span>curby: 64\u219264</span>
              <span>nist: 64\u219264</span>
              <span>os: 64\u219264</span>
              <span>drand: 32\u2192SHA-512\u219264</span>
            </div>
          </div>
          <div class="gdoc-arch-box gdoc-arch-box--green">
            <div class="gdoc-arch-box-title">Combiner.lean</div>
            <div class="gdoc-arch-box-sub">XOR Combination</div>
            <div class="gdoc-arch-box-items">
              <span>fold(\u2295, zero64)</span>
              <span>Canonical order</span>
              <span>Commutative</span>
            </div>
          </div>
          <div class="gdoc-arch-box gdoc-arch-box--amber">
            <div class="gdoc-arch-box-title">Gate.lean</div>
            <div class="gdoc-arch-box-sub">Unlock Predicate</div>
            <div class="gdoc-arch-box-items">
              <span>unlock \u2194 policy passes</span>
              <span>\u22652 sources succeeded</span>
              <span>Binary: open or locked</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Module Details &amp; Proved Theorems</div>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>State.lean</code>
          <span class="gdoc-module-tag">Definitions</span>
        </div>
        <div class="gdoc-module-body">
          <p>Defines the vocabulary: <code>EntropySourceId</code> (curby | nist | drand | os),
          <code>ByteVec64</code> (exactly 64 bytes, enforced by the type system),
          <code>EntropySample</code>, <code>HarvestPolicy</code>, and <code>HarvestTrace</code>.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>policyPass_implies_minSources</code><br>
              <span class="gdoc-thm-desc">If the policy says "pass", then the required number of sources actually succeeded. Pins the definition so it cannot be weakened.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>sample_width_64</code><br>
              <span class="gdoc-thm-desc">Every EntropySample is exactly 64 bytes. Impossible to create a wrong-width sample by construction.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>Sources.lean</code>
          <span class="gdoc-module-tag">Normalization</span>
        </div>
        <div class="gdoc-module-body">
          <p>CURBy, NIST, and OS accept only exactly 64-byte inputs.
          drand accepts only exactly 32-byte inputs and expands via SHA-512 to 64 bytes.
          Wrong-width inputs rejected \u2014 no silent truncation or padding.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>normalize_drand_deterministic</code><br>
              <span class="gdoc-thm-desc">Same 32-byte drand input always produces the same 64-byte output. The runtime cannot get creative.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>normalize_non_drand_width_guard</code><br>
              <span class="gdoc-thm-desc">If a non-drand source provides wrong-width bytes, normalization fails. No silent acceptance.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Combiner.lean</code>
          <span class="gdoc-module-tag">XOR Combination</span>
        </div>
        <div class="gdoc-module-body">
          <p>All normalized 64-byte vectors are XOR-folded together starting from a zero vector
          in canonical order.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>combineXor_deterministic</code><br>
              <span class="gdoc-thm-desc">Same inputs always produce the same output.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>xorVec64_comm</code><br>
              <span class="gdoc-thm-desc">XOR is commutative (a \u2295 b = b \u2295 a). Canonical ordering is a convention for reproducibility, not a correctness requirement.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--amber">
        <div class="gdoc-module-header">
          <code>Gate.lean</code>
          <span class="gdoc-module-tag">Unlock Predicate</span>
        </div>
        <div class="gdoc-module-body">
          <p>Boolean gate controlling dashboard unlock. Binary \u2014 open or closed, no partial state.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>gateUnlock_true_iff_policyPass</code><br>
              <span class="gdoc-thm-desc">Gate opens if and only if enough sources succeeded.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>gateUnlock_false_iff_not_policyPass</code><br>
              <span class="gdoc-thm-desc">Gate stays locked if and only if the policy does not pass. No middle ground.</span></div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Immutable Logging</div>
      <p class="gdoc-text">Every Genesis harvest writes to two independent systems:</p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-icon-lg">\u26D3</div>
          <div class="gdoc-card-head">Identity Ledger</div>
          <div class="gdoc-card-body">
            Hash-chained, append-only ledger with post-quantum signatures.
            Stores <code>GenesisEntropyHarvested</code> entry: SHA-256 hash of combined entropy,
            source list, policy outcome. Permanent birth certificate.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-icon-lg">\u2630</div>
          <div class="gdoc-card-head">Runtime Trace</div>
          <div class="gdoc-card-body">
            NucleusDB-backed event log. Every attempt (success or failure) writes a
            <code>GenesisHarvest</code> trace event with timing, source counts, and error codes.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Once-Only Semantics</div>
      <div class="gdoc-callout">
        <div class="gdoc-callout-icon">\u26BF</div>
        <div class="gdoc-callout-body">
          Genesis runs on first launch only. The server reads the identity ledger for a
          <code>GenesisEntropyHarvested</code> entry. If found, the overlay never appears again.
          Server-side check \u2014 no localStorage, no cookies, no bypass. Admin-only reset exists
          but is never exposed in the UI.
        </div>
      </div>
    </div>
  `;
}

/* ================================================================
   TAB 3: AGENT ACCESS
   ================================================================ */
function gdocAccess() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Agent Integration (Track B \u2014 Planned)</div>
      <p class="gdoc-text">
        The Genesis formal model and runtime ceremony are designed for programmatic access
        by agents and CLI tools. The following surfaces are planned for Track B delivery
        (separate approval required).
      </p>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">CLI Commands</div>
      <p class="gdoc-text">
        The <code>agenthalo</code> CLI will expose Genesis operations:
      </p>
      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Query genesis status</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo genesis status
Genesis: COMPLETE
  Timestamp:  2026-03-01T14:23:07Z
  Sources:    4/4 (CURBy, NIST, drand, OS)
  Ledger seq: 1
  Hash:       a3f7c9...d42e</pre>
      </div>
      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Trigger genesis ceremony (first install)</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo genesis run
Harvesting entropy...
  CURBy quantum beacon:  OK  (pulse #7523)
  NIST randomness beacon: OK  (pulse 2847291)
  drand distributed:      OK  (round 4192837, normalized)
  OS CSPRNG:              OK
Combined: 4 sources XOR'd \u2192 64 bytes
Policy: PASS (4 \u2265 2 required)
Ledger: GenesisEntropyHarvested written (seq 1)
Trace:  GenesisHarvest event logged
Genesis complete.</pre>
      </div>
      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Admin reset (guarded)</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo genesis reset --confirm
WARNING: This will delete the genesis record.
Genesis reset. Next launch triggers ceremony.</pre>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">MCP Tool Surface</div>
      <p class="gdoc-text">
        For AI agent access (Claude, Codex, Gemini), Genesis status will be queryable
        through the AgentHALO MCP server:
      </p>
      <div class="gdoc-tool-list">
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo_genesis_status</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--read">Read-only</div>
          </div>
          <div class="gdoc-tool-desc">
            Returns Genesis completion state, source summary, ledger sequence, and entropy hash.
            Safe for any agent to call at any time.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo_genesis_harvest</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--guard">Guarded</div>
          </div>
          <div class="gdoc-tool-desc">
            Triggers entropy harvest ceremony. Only callable when Genesis not completed.
            Writes to identity ledger and runtime trace on success.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo_genesis_reset</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--admin">Admin</div>
          </div>
          <div class="gdoc-tool-desc">
            Deletes Genesis completion marker. Requires admin authorization.
            Logs the reset as a trace event before clearing.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Formal Verification Bridge</div>
      <p class="gdoc-text">
        The Lean formal model serves as the specification both the Rust runtime
        and agent tools must conform to:
      </p>

      <div class="gdoc-bridge">
        <div class="gdoc-bridge-col">
          <div class="gdoc-bridge-heading">Lean Formal Model</div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">State.lean<br><span>EntropySourceId</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">genesis_entropy.rs<br><span>EntropySource enum</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">State.lean<br><span>ByteVec64</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">genesis_entropy.rs<br><span>[u8; 64]</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Sources.lean<br><span>normalizeTo64</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">genesis_entropy.rs<br><span>normalize_source()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Combiner.lean<br><span>combineXor</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">genesis_entropy.rs<br><span>xor_combine()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Gate.lean<br><span>gateUnlock</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">api.rs<br><span>GET /api/genesis/status</span></div>
          </div>
        </div>
      </div>

      <div class="gdoc-callout" style="margin-top:16px">
        <div class="gdoc-callout-icon">\u2693</div>
        <div class="gdoc-callout-body">
          Track B adds a <strong>CAB (Certified Artifact Bundle)</strong> \u2014 a provenance
          package binding proved Lean theorems to the deployed binary, enabling tamper-evident
          verification of the entire Genesis pipeline.
        </div>
      </div>
    </div>
  `;
}
