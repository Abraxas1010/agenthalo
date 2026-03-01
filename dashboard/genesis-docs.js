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
    summary: 'The birth ceremony for your agent. A generative act from Nothing through Oscillation to a stable Re-entry Nucleus. Harvests true randomness from 4 independent sources, combines them via commutative XOR fold (proved associative and commutative in Lean), and commits the result into an immutable seal chain. Category-theoretic formalization in Lean 4.',
    stats: [
      { val: '4', lbl: 'Sources' },
      { val: '64 B', lbl: 'Seed' },
      { val: '\u221E', lbl: 'Seal Chain' },
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
            <!-- Actual theorem count from lean/NucleusDB/ (grep -rc '^theorem'). TODO: dynamic API -->
            <div class="gdoc-hero-stat-val">117</div>
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
      <div class="gdoc-section-title">Why Category Theory?</div>
      <p class="gdoc-text">
        Every protocol in H.A.L.O. is formalized using <strong>category theory</strong> &mdash;
        the universal language of mathematical structure. Where traditional software defines
        data types and functions, categorical formalization defines <em>objects</em> (states),
        <em>morphisms</em> (transitions), and <em>functors</em> (structure-preserving maps
        between systems). This gives us three things other approaches cannot:
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Universality</div>
          <div class="gdoc-card-body">
            Category theory is the broadest mathematical framework. Any mathematical
            object &mdash; sets, groups, topological spaces, blockchains, proof trees &mdash;
            can be expressed as a category. This means our formal proofs compose with
            <em>any</em> future mathematical structure we need.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Composability</div>
          <div class="gdoc-card-body">
            Functors preserve structure across boundaries. The genesis seed, the identity
            ledger, the database seal chain, and the sheaf-coherence layer are designed
            around the same categorical architecture &mdash; objects, morphisms, and functors
            &mdash; so that formal properties proved in one subsystem compose naturally with the others.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">Tamper Evidence</div>
          <div class="gdoc-card-body">
            The seal chain follows the structure of a categorical diagram: each commit
            extends the previous seal via a one-way hash. Monotone extension is the key
            property &mdash; every commit proves the new state includes all previous data.
            Deletion would require finding a SHA-256 preimage (a 2<sup>128</sup> operation)
            &mdash; computationally infeasible.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">The Generative Ontology</div>
      <p class="gdoc-text">
        H.A.L.O. protocols follow a common generative pattern rooted in the eigenform framework:
        <strong>Nothing \u2192 Oscillation \u2192 Re-entry \u2192 Nucleus</strong>.
        Genesis is the first and most literal instance: from the void of pre-existence,
        independent entropy sources create oscillatory perturbations, the XOR fold is the
        re-entrant combination, and the committed hash is the stable nucleus &mdash; the
        agent's fixed-point identity. Each protocol page explains how this pattern manifests
        in its specific domain.
      </p>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">About This Section</div>
      <p class="gdoc-text">
        Each protocol page documents a formally verified subsystem of Agent H.A.L.O.
        Pages include a <strong>high-level overview</strong> (for everyone),
        <strong>technical details</strong> (Lean proofs, category theory, architecture diagrams),
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
        <div class="gdoc-hero-subtitle">Nothing \u2192 Oscillation \u2192 Re-entry \u2192 Nucleus</div>
        <div class="gdoc-hero-sep"></div>
        <div class="gdoc-hero-stat-row">
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">4</div>
            <div class="gdoc-hero-stat-lbl">Entropy Sources</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">64 B</div>
            <div class="gdoc-hero-stat-lbl">Nucleus Seed</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">\u221E</div>
            <div class="gdoc-hero-stat-lbl">Seal Chain</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">\u2295</div>
            <div class="gdoc-hero-stat-lbl">Coproduct</div>
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
        Genesis is the <strong>birth ceremony</strong> for your agent &mdash; the generative act
        that creates identity from nothing. Before the ceremony, the agent has no distinguishing
        mark: it is a <strong>void</strong>, pure potential without form. Genesis follows the
        eigenform pattern:
      </p>
      <div class="gdoc-pipeline" style="margin:12px 0 16px">
        <div class="gdoc-pipeline-box" style="text-align:center;letter-spacing:1px">
          <strong>\u2205 Nothing</strong> &nbsp;\u2192&nbsp;
          <strong>\u223F Oscillation</strong> &nbsp;\u2192&nbsp;
          <strong>\u21BA Re-entry</strong> &nbsp;\u2192&nbsp;
          <strong>\u2609 R Nucleus</strong>
        </div>
      </div>
      <p class="gdoc-text">
        Independent entropy sources around the world create <em>oscillatory perturbations</em>
        (quantum vacuum, distributed randomness, OS hardware). The XOR fold is the
        <em>re-entrant combination</em> &mdash; each source enters the fold and the fold's output
        is determined by all sources together. The SHA-256 commitment is the <em>nucleus</em>:
        a fixed point that cannot be unwound, the agent's permanent, immutable identity.
        This happens <strong>once</strong> and never again.
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
            The genesis seed is committed into a monotone seal chain. Every future database
            operation builds on this root. Tampering with the seed would invalidate the
            entire chain &mdash; like corrupting the genesis block of a blockchain.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--amber">
          <div class="gdoc-card-icon-lg">\u26A1</div>
          <div class="gdoc-card-head">Security</div>
          <div class="gdoc-card-body">
            The seed is sealed to encrypted-at-rest storage (PQ-wallet-derived key),
            while the public commitment hash is stored in immutable ledgers.
            Multiple independent sources ensure no single point of compromise.
            Post-quantum signatures bind every ledger entry to the agent's wallet.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-icon-lg">\u2200</div>
          <div class="gdoc-card-head">Category Theory</div>
          <div class="gdoc-card-body">
            The formal model uses category theory &mdash; the universal language of
            mathematical structure. Entropy sources are objects in a product category,
            XOR combination is a coproduct, and the seal chain is a diagram in the
            category of hash commitments. This means our proofs compose with any
            future mathematical system.
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
          <div class="gdoc-pipeline-label">Oscillation &mdash; Gather</div>
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
          <div class="gdoc-pipeline-label">Re-entry &mdash; Combine</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--accent">
          <strong>Categorical coproduct</strong>: XOR fold in canonical order:
          <code>curby \u2295 nist \u2295 drand \u2295 os</code><br>
          Each source enters the fold; the output depends on all sources together.
          Even if one source is compromised, the others contribute genuine randomness.
          In category theory, this is the coproduct in the category of byte vectors
          with XOR as the combining morphism.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">4</div>
          <div class="gdoc-pipeline-label">Nucleus &mdash; Commit</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--green">
          SHA-256 hash becomes the <strong>nucleus</strong> &mdash; a fixed point that cannot be
          unwound. Written to the <strong>identity ledger</strong> (permanent birth record)
          and anchored into the <strong>monotone seal chain</strong> (database integrity root).<br>
          Raw entropy is sealed to encrypted local storage and the structured hash
          commitment is bound into immutable ledgers and seal roots.
          The nucleus persists as the generative seed for future operations.
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Safety Net</div>
      <div class="gdoc-callout">
        <div class="gdoc-callout-icon">\u26A0</div>
        <div class="gdoc-callout-body">
          The gate requires <strong>at least 2 sources total</strong> and
          <strong>at least 1 remote source</strong> (CURBy/NIST/drand).
          If a beacon is temporarily unavailable, the remaining sources can still satisfy policy.
          The ceremony shows exactly what failed and offers a clear retry.
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
      <div class="gdoc-section-title">Generative Ontology: Nothing \u2192 Nucleus</div>
      <p class="gdoc-text">
        Genesis follows the Meinongian noneist generative pattern: identity emerges from
        <em>nothing</em> through a sequence of constructive acts. This is not metaphor &mdash;
        it is the literal mathematical structure of the protocol.
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">\u2205 Nothing (Void)</div>
          <div class="gdoc-card-body">
            Before Genesis, the agent has no identity. This is the initial object in the
            category &mdash; the empty state from which all structure must be constructed.
            In Lean: <code>IdentityState.default</code> with all fields <code>none</code>.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">\u223F Oscillation</div>
          <div class="gdoc-card-body">
            Four independent entropy sources create perturbations: quantum vacuum (CURBy),
            national beacon (NIST), distributed threshold (drand), hardware CSPRNG (OS).
            Each is an object in the product category <code>ByteVec64\u00B4</code>.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">\u21BA Re-entry (XOR Coproduct)</div>
          <div class="gdoc-card-body">
            The XOR fold is modeled as the compositional re-entry step: each source enters
            the fold, and the fold output depends on all sources together.
            The Lean combiner module proves determinism and algebraic laws used by the runtime.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">\u2609 R Nucleus (Fixed Point)</div>
          <div class="gdoc-card-body">
            The noneist module models the nucleus operator <code>R</code> as an idempotent closure
            on ceremony phases. Once committed,
            the nucleus cannot be unwound. It becomes the root of the monotone seal chain
            and the generative seed for all future operations.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Category Theory Foundation</div>
      <p class="gdoc-text">
        The formal model uses category theory as its foundational language. This is not
        decoration &mdash; it is the reason the system can incorporate any mathematical
        object and any future extension without structural changes.
      </p>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>Core/Nucleus.lean</code>
          <span class="gdoc-module-tag">Category of State Transitions</span>
        </div>
        <div class="gdoc-module-body">
          <p>Defines <code>NucleusSystem</code>: a category whose objects are states and whose
          morphisms are deltas (transitions). Every state evolution is a morphism in this category.
          The <code>step</code> function is composition. Identity morphisms are empty deltas.</p>
          <p>This is the abstract interface that <em>all</em> H.A.L.O. subsystems implement:
          identity management, wallet operations, and the genesis ceremony itself are all
          instances of the same categorical pattern.</p>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Core/Authorization.lean</code>
          <span class="gdoc-module-tag">Authorized Morphisms</span>
        </div>
        <div class="gdoc-module-body">
          <p><code>AuthorizationPolicy</code> is a predicate on morphisms: not every delta is
          permitted. An <code>AuthorizedDelta</code> bundles a morphism with a constructive
          proof that the policy permits it. This is a <em>typed morphism</em> in the category &mdash;
          you cannot apply a transition without proving authorization.</p>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>Core/Certificates.lean</code> + <code>Core/Ledger.lean</code>
          <span class="gdoc-module-tag">Commit Chain as Diagram</span>
        </div>
        <div class="gdoc-module-body">
          <p>A <code>CommitCertificate</code> is a verified morphism: it carries the previous state,
          the delta, the authorization proof, and a constructive witness that <code>next = apply(prev, delta)</code>.
          The ledger is a <em>chain complex</em> &mdash; a sequence of certificates where each entry
          chains to the previous via hash. <code>verifyLedger</code> validates the entire sequence.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>verifyCommitCertificate_sound</code><br>
              <span class="gdoc-thm-desc">Every constructed certificate is valid. Soundness by construction.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>verifyLedger_cons</code><br>
              <span class="gdoc-thm-desc">Ledger verification is inductive: valid head + valid tail = valid chain.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--amber">
        <div class="gdoc-module-header">
          <code>Core/Invariants.lean</code>
          <span class="gdoc-module-tag">Invariant Preservation</span>
        </div>
        <div class="gdoc-module-body">
          <p><code>PreservedBy</code> states that a state invariant is preserved across all morphisms.
          <code>replay</code> composes a sequence of deltas. The key theorem proves that invariant
          preservation is compositional: if each step preserves the invariant, replay preserves it.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>replay_preserves</code><br>
              <span class="gdoc-thm-desc">If <code>apply</code> preserves an invariant for every delta, then replaying any list of deltas preserves it. Inductive proof over the morphism sequence.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Sheaf/MaterializationFunctor.lean</code>
          <span class="gdoc-module-tag">Functorial Projection</span>
        </div>
        <div class="gdoc-module-body">
          <p>A <code>MaterializationFunctor</code> maps internal states to external key-value
          projections. The <code>naturality</code> law says: if two states are transport-equivalent,
          they produce identical projections. This is a natural transformation &mdash;
          the functor preserves the transport relation across the projection boundary.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>materialize_transport_eq</code><br>
              <span class="gdoc-thm-desc">Transport-equivalent states materialize identically. The naturality square commutes.</span></div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Entropy Harvest Protocol (Rust Runtime)</div>
      <p class="gdoc-text">
        The Rust runtime in <code>genesis_entropy.rs</code> implements the harvest ceremony.
        Four modules define the vocabulary, normalization, combination, and gating:
      </p>

      <div class="gdoc-arch">
        <div class="gdoc-arch-top">
          <div class="gdoc-arch-box gdoc-arch-box--root">
            <div class="gdoc-arch-box-title">EntropySourceId</div>
            <div class="gdoc-arch-box-sub">4 sources, canonical order, tier-ranked</div>
            <div class="gdoc-arch-box-items">
              <span>Curby (tier 2) | Nist (tier 3) | Drand (tier 4) | Os (tier 5)</span>
              <span>SourceSample = id + [u8; 64] + metadata</span>
              <span>ENTROPY_WIDTH = 64 bytes, SOURCE_MIN_SUCCESS = 2</span>
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
            <div class="gdoc-arch-box-title">Normalization</div>
            <div class="gdoc-arch-box-sub">Width enforcement</div>
            <div class="gdoc-arch-box-items">
              <span>curby: 64\u219264 (direct or SHA-512)</span>
              <span>nist: 64\u219264 (direct)</span>
              <span>os: 64\u219264 (OsRng)</span>
              <span>drand: 32\u2192SHA-512\u219264</span>
            </div>
          </div>
          <div class="gdoc-arch-box gdoc-arch-box--green">
            <div class="gdoc-arch-box-title">XOR Coproduct</div>
            <div class="gdoc-arch-box-sub">Categorical combination</div>
            <div class="gdoc-arch-box-items">
              <span>fold(\u2295, [0; 64])</span>
              <span>Canonical order by source id</span>
              <span>Commutative + associative</span>
            </div>
          </div>
          <div class="gdoc-arch-box gdoc-arch-box--amber">
            <div class="gdoc-arch-box-title">Policy Gate</div>
            <div class="gdoc-arch-box-sub">Unlock predicate</div>
            <div class="gdoc-arch-box-items">
              <span>remote_successes > 0</span>
              <span>total_successes \u2265 2</span>
              <span>SHA-256 commitment on pass</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Identity Formalization (Proved Theorems)</div>
      <p class="gdoc-text">
        The identity subsystem that receives the genesis seed is fully formalized in Lean 4.
        These theorems have been verified by the Lean kernel &mdash; no <code>sorry</code> or <code>admit</code>.
      </p>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>Identity/State.lean</code> + <code>Identity/Delta.lean</code>
          <span class="gdoc-module-tag">State Machine</span>
        </div>
        <div class="gdoc-module-body">
          <p>Defines the identity state (<code>IdentityState</code>: profile, anonymous mode,
          security tier, device, network) and the transition language (<code>IdentityDelta</code>:
          profileSet, anonymousModeSet, securityTierSet, deviceSet, networkSet).
          Deterministic transition function <code>applyDelta</code>.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>applyDelta_profile_locks_name</code><br>
              <span class="gdoc-thm-desc">Setting a profile name always locks it. No unlock path through the transition function.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>applyDelta_anonymous_clears_network</code><br>
              <span class="gdoc-thm-desc">Enabling anonymous mode clears network identity. Privacy is enforced by the transition, not by convention.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>networkConfigured_empty_false</code><br>
              <span class="gdoc-thm-desc">An empty network identity is never considered "configured". Prevents false positives.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Identity/Policy.lean</code> + <code>Identity/Certificate.lean</code>
          <span class="gdoc-module-tag">Authorization &amp; Verification</span>
        </div>
        <div class="gdoc-module-body">
          <p>Authorization policy requires: explicit authorization, non-empty actor ID, and
          delta-local constraints. Certificates bundle prev-state, delta, auth, and next-state
          with constructive proofs. Ledger verification is inductive over the certificate chain.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>identityPolicy_rejects_unauthorized</code><br>
              <span class="gdoc-thm-desc">No unauthorized actor can apply any delta. Proved by contradiction.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>identityPolicy_requires_actor</code><br>
              <span class="gdoc-thm-desc">Empty actor string is always rejected, even if <code>authorized = true</code>.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>verifyIdentityLedger_cons</code><br>
              <span class="gdoc-thm-desc">Ledger validity is inductive: valid head certificate + valid tail = valid ledger.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--amber">
        <div class="gdoc-module-header">
          <code>Identity/Materialization.lean</code>
          <span class="gdoc-module-tag">Functorial Projection</span>
        </div>
        <div class="gdoc-module-body">
          <p>The identity state materializes to POD (Provable Observable Data) key-value pairs
          via a <code>MaterializationFunctor</code>. The naturality law ensures that
          transport-equivalent states produce identical projections &mdash; internal bookkeeping
          changes are invisible to external observers.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>identityMaterialization_transport_eq</code><br>
              <span class="gdoc-thm-desc">Transport-equivalent identity states materialize identically. The naturality square commutes.</span></div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Genesis Entropy Lean Modules (Implemented)</div>
      <p class="gdoc-text">
        The Genesis entropy formal layer is implemented under
        <code>lean/NucleusDB/Genesis/Entropy/</code> and imported through
        <code>NucleusDB.Genesis</code>.
      </p>
      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>State.lean</code> + <code>Sources.lean</code> + <code>Combiner.lean</code> + <code>Gate.lean</code>
          <span class="gdoc-module-tag">Protocol Kernel</span>
        </div>
        <div class="gdoc-module-body">
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>sample_width_64</code> / <code>source_count_eq_four</code><br>
              <span class="gdoc-thm-desc">Fixed-width and source-cardinality invariants for the harvest domain.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>normalize_drand_deterministic</code><br>
              <span class="gdoc-thm-desc">Normalization from 32-byte drand input to 64-byte model output is deterministic.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>xorVec64_comm</code> / <code>combineXor_deterministic</code><br>
              <span class="gdoc-thm-desc">Combiner algebra and deterministic fold behavior for canonical XOR aggregation.</span></div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>policyPass_implies_minSources</code> / <code>gateUnlock_equiv_policy</code><br>
              <span class="gdoc-thm-desc">Unlock policy obligations are explicit and machine-checked.</span></div>
            </div>
          </div>
        </div>
      </div>
      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Category.lean</code> + <code>Noneist.lean</code>
          <span class="gdoc-module-tag">Category + Ontology Bridge</span>
        </div>
        <div class="gdoc-module-body">
          <p>
            <code>Category.lean</code> defines a harvest-state category with monotone
            evidence morphisms and functorial projection. <code>Noneist.lean</code>
            formalizes the phase progression
            <code>void \u2192 oscillation \u2192 reEntry \u2192 nucleus</code>
            and idempotent closure <code>R</code>.
          </p>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Monotone Seal Chain (Tamper Evidence)</div>
      <p class="gdoc-text">
        The genesis seed anchors a <strong>monotone seal chain</strong> in NucleusDB. This is the
        categorical diagram that makes tampering computationally infeasible &mdash; equivalent to
        blockchain integrity but with formal proof.
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Monotone Extension</div>
          <div class="gdoc-card-body">
            Every commit proves the new state is a <strong>monotone extension</strong> of the previous:
            all previously committed key-value pairs are preserved. Deletion would require
            inverting a SHA-256 hash &mdash; a 2<sup>128</sup> operation.
            <br><br>
            <code>seal\u2099 = SHA-256("NucleusDB.MonotoneSeal|" || seal\u2099\u208B\u2081 || kv_digest\u2099)</code>
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Genesis as Root</div>
          <div class="gdoc-card-body">
            The genesis seal is the <strong>initial object</strong> of the seal chain category.
            Every subsequent commit builds on it. Tampering with the genesis seed would
            invalidate every seal in the chain &mdash; complete corruption, detectable instantly.
            <br><br>
            This is exactly the blockchain property: the genesis block determines the
            identity of the entire chain.
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
            source list, policy outcome. Each entry chains to the previous via
            <code>prev_hash</code>. Permanent birth certificate.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-icon-lg">\u2630</div>
          <div class="gdoc-card-head">Runtime Trace</div>
          <div class="gdoc-card-body">
            NucleusDB-backed event log. Every attempt (success or failure) writes a
            <code>GenesisHarvest</code> trace event with timing, source counts, and error codes.
            The trace is stored in the NucleusDB append-only store, protected by the
            monotone seal chain.
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
WARNING: Reset is disabled by default.
Enable only with AGENTHALO_ENABLE_GENESIS_RESET=1.
Genesis reset. Next launch triggers ceremony.</pre>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">MCP Tool Surface</div>
      <p class="gdoc-text">
        For AI agent access (Claude, Codex, Gemini), the following surface is
        <strong>planned only for Track B</strong> and is
        <strong>not yet present in the current MCP tool registry</strong>:
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
            Requests Genesis reset. Requires admin authorization and explicit
            runtime enablement via <code>AGENTHALO_ENABLE_GENESIS_RESET=1</code>.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Formal Verification Bridge</div>
      <p class="gdoc-text">
        The Lean formal model serves as the specification that the Rust runtime
        and agent tools must conform to. The categorical structure ensures
        the bridge is structure-preserving (functorial):
      </p>

      <div class="gdoc-bridge">
        <div class="gdoc-bridge-col">
          <div class="gdoc-bridge-heading">Lean Category-Theoretic Model \u2192 Rust Runtime</div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Core/Nucleus.lean<br><span>NucleusSystem (State, Delta, apply)</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">genesis_entropy.rs<br><span>HarvestOutcome + finalize_harvest()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Core/Authorization.lean<br><span>AuthorizationPolicy</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">identity_ledger.rs<br><span>append_genesis_event()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Core/Certificates.lean<br><span>CommitCertificate</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">identity_ledger.rs<br><span>compute_entry_hash() + verify_chain()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Identity/Delta.lean<br><span>IdentityDelta + applyDelta</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">identity.rs<br><span>IdentityConfig + save()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Sheaf/MaterializationFunctor.lean<br><span>naturality law</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">immutable.rs<br><span>monotone seal chain</span></div>
          </div>
        </div>
      </div>

      <div class="gdoc-callout" style="margin-top:16px">
        <div class="gdoc-callout-icon">\u2693</div>
        <div class="gdoc-callout-body">
          Track B adds a <strong>CAB (Certified Artifact Bundle)</strong> \u2014 a provenance
          package binding proved Lean theorems to the deployed binary, enabling tamper-evident
          verification of the entire Genesis pipeline. The categorical structure ensures
          the proof obligation transfers cleanly from Lean types to Rust types.
        </div>
      </div>
    </div>
  `;
}
