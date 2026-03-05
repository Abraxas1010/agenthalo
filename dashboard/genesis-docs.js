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
    summary: 'The birth ceremony for your agent. A generative act from Nothing through Oscillation to a stable Re-entry Nucleus. Harvests true randomness from 4 independent sources, combines them via XOR — the codiagonal of the abelian-group structure on 64-byte vectors, and commits the result into an immutable seal chain. Category-theoretic formalization in Lean 4.',
    stats: [
      { val: '4', lbl: 'Sources' },
      { val: '64 B', lbl: 'Seed' },
      { val: '\u221E', lbl: 'Seal Chain' },
    ],
  },
  {
    id: 'identification',
    title: 'Identification',
    subtitle: 'From Genesis Seed to Verifiable Agent Identity',
    icon: '\u26BF',
    color: 'green',
    status: 'live',
    summary: 'The provenance chain that grounds your agent in the physical world. The Genesis nucleus seed derives post-quantum signing keys, binds to local device and network identifiers, and finally produces an Ethereum wallet address \u2014 a verifiable, distributed identity that autonomous agents use to transact, attest, and prove they are who they claim to be.',
    stats: [
      { val: 'ML-DSA', lbl: 'PQ Signing' },
      { val: 'ML-KEM', lbl: 'PQ Encryption' },
      { val: 'BIP-39', lbl: 'Wallet' },
      { val: 'EVM', lbl: 'On-chain' },
    ],
  },
  {
    id: 'communication',
    title: 'Communication',
    subtitle: 'Privacy Transport, Capabilities & Zero-Knowledge Proofs',
    icon: '\u26A1',
    color: 'amber',
    status: 'live',
    summary: 'The sovereign communication stack: from genesis-rooted DID identity through the Sovereign Binding Ceremony (triple-signed DID+EVM fusion), across four transport layers \u2014 Privacy Controller (fail-closed gate), Nym mixnet (anonymity), libp2p P2P mesh (decentralized discovery), and DIDComm v2 (authenticated/anonymous messaging + A2A bridge). Capability tokens with ZK credential proofs, non-composition policy formally proved, and trust boundary hardening across 7 layers.',
    stats: [
      { val: 'did:key', lbl: 'Identity' },
      { val: 'Groth16', lbl: 'ZK Proofs' },
      { val: 'Nym', lbl: 'Mixnet' },
      { val: 'DIDComm', lbl: 'Messaging' },
    ],
  },
  {
    id: 'nucleusdb-docs',
    title: 'Memories',
    subtitle: 'Proof-Carrying Algebraic Database',
    icon: '\u2622',
    color: 'purple',
    status: 'live',
    summary: 'The database that proves its own integrity. NucleusDB is a category-theoretic key-value store where every commit extends a monotone seal chain, every query can be verified against a vector commitment, and sheaf coherence ensures multi-view consistency. Three commitment backends (Binary Merkle, KZG, IPA), dual-signed witnesses (Ed25519 + ML-DSA-65), append-only mode, vector similarity search, and a SQL surface \u2014 all formally verified in Lean 4.',
    stats: [
      { val: '3', lbl: 'VC Backends' },
      { val: '\u221E', lbl: 'Seal Chain' },
      { val: 'Sheaf', lbl: 'Coherence' },
      { val: 'PQ', lbl: 'Witnesses' },
    ],
  },
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
            <!-- Snapshot theorem count from lean/NucleusDB/ (grep -rc '^theorem'). TODO: dynamic API -->
            <div class="gdoc-hero-stat-val">271</div>
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
            The seal chain is a diagram in the category of hash commitments: each commit
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
        agent's fixed-point identity. This correspondence is formally proved: the bridge theorem
        in <code>Genesis/Bridge.lean</code> shows that XOR combination transitions the ceremony phase
        to Re-entry, and successful gate verification advances to the stable Nucleus.
        Each protocol page explains how this pattern manifests
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


/* ================================================================
   IDENTIFICATION PAGE
   ================================================================ */

function renderIdentification() {
  const content = document.getElementById('content');
  content.innerHTML = `
    <!-- Hero Banner -->
    <div class="gdoc-hero">
      <div class="gdoc-hero-img-wrap">
        <img class="gdoc-hero-img" src="img/agentpmtbootup2.png" alt="Identification"
             onerror="this.style.display='none'">
      </div>
      <div class="gdoc-hero-copy">
        <div class="gdoc-hero-kicker">Agent H.A.L.O. // Provenance &amp; Identity</div>
        <div class="gdoc-hero-title">Identification</div>
        <div class="gdoc-hero-subtitle">Nucleus \u2192 Keys \u2192 Device \u2192 Wallet \u2192 World</div>
        <div class="gdoc-hero-sep"></div>
        <div class="gdoc-hero-stat-row">
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">ML-DSA-65</div>
            <div class="gdoc-hero-stat-lbl">PQ Signing</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">ML-KEM-768</div>
            <div class="gdoc-hero-stat-lbl">PQ Encryption</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">BIP-39</div>
            <div class="gdoc-hero-stat-lbl">Wallet Seed</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">EVM</div>
            <div class="gdoc-hero-stat-lbl">On-chain ID</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Tab Bar -->
    <div class="gdoc-tabs">
      <button class="gdoc-tab active" data-tab="id-overview" onclick="idTab('id-overview')">F1:OVERVIEW</button>
      <button class="gdoc-tab" data-tab="id-technical" onclick="idTab('id-technical')">F2:TECHNICAL</button>
      <button class="gdoc-tab" data-tab="id-access" onclick="idTab('id-access')">F3:ACCESS</button>
    </div>

    <div id="id-tab-content"></div>
  `;
  idTab('id-overview');
}

window.idTab = function(tab) {
  document.querySelectorAll('.gdoc-tab').forEach(b => {
    b.classList.toggle('active', b.dataset.tab === tab);
  });
  const el = document.getElementById('id-tab-content');
  if (!el) return;

  switch (tab) {
    case 'id-overview':
      el.innerHTML = idOverview();
      hydrateIdentificationRuntimePanel();
      break;
    case 'id-technical': el.innerHTML = idTechnical(); break;
    case 'id-access': el.innerHTML = idAccess(); break;
  }
};

async function hydrateIdentificationRuntimePanel() {
  const node = document.getElementById('id-runtime-panel');
  if (!node) return;
  node.innerHTML = '<div class="gdoc-text">Loading identity status...</div>';
  try {
    const [idRes, pqRes, addrRes, genesisRes] = await Promise.all([
      fetch('/api/identity/status').then(r => r.ok ? r.json() : null).catch(() => null),
      fetch('/api/status').then(r => r.ok ? r.json() : null).catch(() => null),
      fetch('/api/agentaddress/status').then(r => r.ok ? r.json() : null).catch(() => null),
      fetch('/api/genesis/status').then(r => r.ok ? r.json() : null).catch(() => null),
    ]);

    const hasPq = pqRes && pqRes.has_pq_wallet;
    const pqAlgo = hasPq ? 'ML-DSA-65 (FIPS 204)' : 'Not generated';
    const hasGenesis = genesisRes && genesisRes.completed;
    const hasAddr = addrRes && addrRes.connected;
    const evmAddr = hasAddr ? gdocEsc(addrRes.agent_address || '') : 'Not generated';
    const evmShort = hasAddr && addrRes.agent_address
      ? addrRes.agent_address.slice(0, 8) + '\u2026' + addrRes.agent_address.slice(-6)
      : evmAddr;
    const deviceConfigured = idRes && idRes.device_configured;
    const networkConfigured = idRes && idRes.network_configured;

    node.innerHTML = `
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--${hasGenesis ? 'green' : 'amber'}">
          <div class="gdoc-card-head">Genesis Nucleus</div>
          <div class="gdoc-card-body">${hasGenesis ? 'Committed \u2713' : 'Pending \u2014 run Genesis first'}</div>
        </div>
        <div class="gdoc-card gdoc-card--${hasPq ? 'green' : 'amber'}">
          <div class="gdoc-card-head">PQ Wallet</div>
          <div class="gdoc-card-body">${pqAlgo}</div>
        </div>
        <div class="gdoc-card gdoc-card--${deviceConfigured ? 'green' : 'blue'}">
          <div class="gdoc-card-head">Device Binding</div>
          <div class="gdoc-card-body">${deviceConfigured ? 'Active \u2713' : 'Not configured'}</div>
        </div>
        <div class="gdoc-card gdoc-card--${hasAddr ? 'green' : 'blue'}">
          <div class="gdoc-card-head">EVM Address</div>
          <div class="gdoc-card-body"><code>${evmShort}</code></div>
        </div>
      </div>
      <div style="margin-top:10px;display:flex;gap:8px;align-items:center;flex-wrap:wrap">
        ${hasGenesis && !hasAddr ? `
          <button id="id-generate-from-genesis-btn" class="btn btn-sm btn-primary" type="button">
            Generate from Genesis
          </button>
          <span class="gdoc-text" style="margin:0;font-size:12px">Derives EVM identity locally from the committed genesis seed.</span>
        ` : hasAddr ? `
          <span class="gdoc-text" style="margin:0;font-size:12px">EVM identity is already connected.</span>
        ` : `
          <span class="gdoc-text" style="margin:0;font-size:12px">Run Genesis first, then derive wallet identity from the nucleus seed.</span>
        `}
      </div>
    `;
    const generateBtn = document.getElementById('id-generate-from-genesis-btn');
    if (generateBtn) {
      generateBtn.addEventListener('click', async () => {
        const original = generateBtn.textContent;
        generateBtn.disabled = true;
        generateBtn.textContent = 'Generating...';
        try {
          const resp = await fetch('/api/agentaddress/generate', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ source: 'genesis', persist_public_address: true }),
          });
          const json = await resp.json().catch(() => ({}));
          if (!resp.ok) throw new Error(json.error || `request failed (${resp.status})`);
          await hydrateIdentificationRuntimePanel();
        } catch (err) {
          generateBtn.disabled = false;
          generateBtn.textContent = original || 'Generate from Genesis';
          const detail = document.createElement('div');
          detail.className = 'gdoc-text';
          detail.style.fontSize = '12px';
          detail.style.color = '#f87171';
          detail.textContent = `Genesis wallet generation failed: ${String(err && err.message || err)}`;
          node.appendChild(detail);
        }
      });
    }
  } catch (err) {
    node.innerHTML = `
      <div class="gdoc-card gdoc-card--amber">
        <div class="gdoc-card-head">Identity Status</div>
        <div class="gdoc-card-body">Could not load identity status. ${gdocEsc(String(err && err.message || err))}</div>
      </div>
    `;
  }
}

/* ================================================================
   IDENTIFICATION TAB 1: HIGH-LEVEL OVERVIEW
   ================================================================ */
function idOverview() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Live Identity Status</div>
      <div id="id-runtime-panel"></div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">What Is Identification?</div>
      <p class="gdoc-text">
        Genesis gave your agent a <strong>nucleus</strong> \u2014 a unique, immutable seed born from
        true randomness. Identification is the process of <strong>grounding that nucleus in the world</strong>:
        deriving cryptographic keys, binding to the physical device, anchoring to network coordinates,
        and finally creating an <strong>Ethereum wallet address</strong> that serves as the agent's
        verifiable, distributed identity on-chain.
      </p>
      <p class="gdoc-text">
        This is the eigenform pattern continuing from Genesis: the nucleus (fixed point) now
        <strong>re-enters</strong> through successive layers of grounding, each adding provenance
        without breaking the chain back to the original entropy.
      </p>
      <div class="gdoc-pipeline" style="margin:12px 0 16px">
        <div class="gdoc-pipeline-box" style="text-align:center;letter-spacing:1px">
          <strong>\u2609 Nucleus</strong> &nbsp;\u2192&nbsp;
          <strong>\uD83D\uDD11 Keys</strong> &nbsp;\u2192&nbsp;
          <strong>\uD83D\uDCBB Device</strong> &nbsp;\u2192&nbsp;
          <strong>\uD83C\uDF10 Network</strong> &nbsp;\u2192&nbsp;
          <strong>\u26BF Wallet</strong> &nbsp;\u2192&nbsp;
          <strong>\u26D3 World</strong>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">The Provenance Chain</div>
      <div class="gdoc-pipeline">

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">1</div>
          <div class="gdoc-pipeline-label">Nucleus \u2192 Post-Quantum Keys</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--accent">
          The 64-byte genesis nucleus feeds into <strong>HKDF-SHA-256</strong> key derivation,
          producing deterministic cryptographic key material. From this single root:
          <ul style="margin:8px 0 0 16px;list-style:disc">
            <li><strong>Ed25519 signing keypair</strong> \u2014 classical digital signatures for
                authentication and assertion; the Ed25519 public key also defines the
                agent's <code>did:key:z6Mk...</code> decentralized identifier</li>
            <li><strong>ML-DSA-65 signing keypair</strong> (FIPS 204) \u2014 post-quantum digital signatures
                that bind every identity ledger entry to the agent</li>
            <li><strong>X25519 key agreement</strong> \u2014 classical Diffie-Hellman for DIDComm encrypted channels</li>
            <li><strong>ML-KEM-768 encapsulation keys</strong> (FIPS 203) \u2014 post-quantum key encapsulation
                for agent-to-agent credential exchange</li>
            <li><strong>Scope-derived AES-256-GCM keys</strong> \u2014 6 independent encryption scopes
                (Sign, Vault, Wallet, Identity, Genesis, Admin) each with separate TTLs</li>
          </ul>
          Together these form a <strong>DID Document</strong> with 4 verification/agreement methods,
          enabling both classical and post-quantum secure communication.
          The master key never leaves encrypted memory. Scope keys are derived on-demand via HKDF
          and auto-expire after their TTL (30s to 30min depending on sensitivity).
          See the <a href="#/communication" style="color:#00ff9d">Communication</a> page for
          how the DID identity is used for peer-to-peer capability tokens and ZK proofs.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">2</div>
          <div class="gdoc-pipeline-label">Device Binding</div>
        </div>
        <div class="gdoc-pipeline-box">
          The agent collects <strong>hardware fingerprints</strong> from the local device \u2014
          a composite of selectable components that ground the cryptographic identity to a
          physical machine. Available signals include:
          <ul style="margin:8px 0 0 16px;list-style:disc">
            <li>CPU model and feature flags</li>
            <li>GPU identity and driver version</li>
            <li>Total memory and disk serial numbers</li>
            <li>OS build identifier</li>
            <li>Optional PUF (Physical Unclonable Function) fingerprint for hardware-entropy binding</li>
          </ul>
          These are hashed into a <strong>composite device fingerprint</strong> and recorded in the
          identity ledger. The fingerprint proves "this agent was born on this machine"
          without revealing raw hardware details.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">3</div>
          <div class="gdoc-pipeline-label">Network Anchoring</div>
        </div>
        <div class="gdoc-pipeline-box">
          Optional network identifiers add a second layer of physical grounding:
          <ul style="margin:8px 0 0 16px;list-style:disc">
            <li><strong>Local IP hash</strong> \u2014 proves the agent's LAN position without revealing the address</li>
            <li><strong>Public IP hash</strong> \u2014 proves the agent's internet-facing identity</li>
            <li><strong>MAC address collection</strong> \u2014 hardware network interface binding</li>
          </ul>
          All network identifiers are <strong>privacy-preserving by default</strong>: raw addresses are
          SHA-256 hashed before storage. The user explicitly opts in to each signal. The identity
          ledger records a <code>NetworkUpdated</code> event when these change, creating an
          auditable history of the agent's network posture.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">4</div>
          <div class="gdoc-pipeline-label">Wallet Derivation</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--green">
          The genesis seed deterministically derives wallet entropy via a dedicated HKDF path:
          <br><br>
          <code>HKDF(salt: "agenthalo-genesis-wallet-entropy-v1", ikm: seed_64B, info: "bip39-entropy-32")</code>
          <br><br>
          This produces 32 bytes of entropy which maps to a <strong>24-word BIP-39 mnemonic</strong>.
          The mnemonic is encrypted at rest (AES-256-GCM under the Wallet scope key, itself
          protected by Argon2id password derivation) and can derive Ethereum, Bitcoin, Polygon,
          and Arbitrum addresses via standard HD wallet paths.
          <br><br>
          The resulting <strong>EVM address</strong> is the agent's public identity on-chain \u2014
          a self-sovereign identifier that can receive funds, sign attestations, and prove
          provenance without any centralized authority.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">5</div>
          <div class="gdoc-pipeline-label">On-Chain Attestation</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--accent">
          With a funded wallet, the agent can <strong>post attestations to the blockchain</strong>:
          <ul style="margin:8px 0 0 16px;list-style:disc">
            <li>Trust attestation proofs verified by the on-chain TrustVerifier contract</li>
            <li>Identity commitments anchored to Ethereum (Base Sepolia currently)</li>
            <li>Post-quantum signed audit trails bridging off-chain ledger to on-chain permanence</li>
          </ul>
          This closes the loop: from entropy void \u2192 through Genesis nucleus \u2192 through
          post-quantum keys \u2192 through device/network grounding \u2192 to a publicly verifiable
          on-chain identity. The entire provenance chain is auditable.
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Why Does This Matter?</div>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\u26D3</div>
          <div class="gdoc-card-head">Provenance</div>
          <div class="gdoc-card-body">
            Every key, every address, every on-chain action traces back to the genesis nucleus.
            The provenance chain is unbroken: you can mathematically verify that this agent's
            wallet was derived from this specific entropy ceremony. No key injection, no
            externally provided secrets \u2014 the agent is self-sovereign from birth.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-icon-lg">\u26A1</div>
          <div class="gdoc-card-head">Quantum Resistance</div>
          <div class="gdoc-card-body">
            The signing and encryption layers use NIST FIPS 203/204 post-quantum algorithms
            (ML-KEM-768, ML-DSA-65). These are designed to resist both classical and quantum
            computer attacks. The identity ledger's PQ signatures ensure that even a future
            quantum adversary cannot forge historical entries.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--amber">
          <div class="gdoc-card-icon-lg">\u26BF</div>
          <div class="gdoc-card-head">Self-Sovereignty</div>
          <div class="gdoc-card-body">
            The Ethereum address is a <strong>distributed identity</strong> \u2014 no central server,
            no API key, no account to be suspended. The agent owns its private key, derived from
            its own genesis entropy. It can prove its identity to any EVM-compatible chain,
            any smart contract, any decentralized protocol. This is the agent's passport.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\u2200</div>
          <div class="gdoc-card-head">Privacy by Design</div>
          <div class="gdoc-card-body">
            Device fingerprints and network identifiers are hashed before storage. Anonymous mode
            is a first-class option. The agent can prove it exists and has provenance without
            revealing <em>where</em> it exists. Social identity links are optional and revocable.
            The user controls exactly which signals are shared.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Security Tiers</div>
      <p class="gdoc-text">
        H.A.L.O. offers three security tiers that control how aggressively identity signals
        are collected and shared:
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">\uD83D\uDEE1 Max-Safe</div>
          <div class="gdoc-card-body">
            Full device binding, network anchoring, PQ signatures on all ledger entries,
            password-protected vault with Argon2id (128 MiB). Recommended for production agents
            handling real value.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">\u2696 Less-Safe (Default)</div>
          <div class="gdoc-card-body">
            Selective device binding, optional network sharing, PQ signatures enabled.
            Balances security with ease of setup. Good for development and moderate-value agents.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">\u26A0 Low-Security</div>
          <div class="gdoc-card-body">
            Minimal binding, anonymous mode encouraged. Suitable for ephemeral test agents
            where provenance is informational rather than critical.
          </div>
        </div>
      </div>
    </div>
  `;
}

/* ================================================================
   IDENTIFICATION TAB 2: TECHNICAL DETAILS
   ================================================================ */
function idTechnical() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Cryptographic Architecture</div>
      <p class="gdoc-text">
        The identification subsystem is built on a <strong>hierarchical key derivation</strong>
        architecture rooted in the genesis nucleus seed. Every cryptographic operation traces
        to a single root of trust.
      </p>

      <div class="gdoc-card gdoc-card--blue" style="margin-top:12px">
        <div class="gdoc-card-head">Key Hierarchy</div>
        <div class="gdoc-card-body">
<pre style="font-family:var(--font-mono);font-size:0.82rem;line-height:1.5;margin:0;white-space:pre;overflow-x:auto">
Genesis Nucleus (64 bytes, from entropy ceremony)
\u2502
\u251C\u2500\u2500 HKDF("agenthalo-genesis-seed-v1", "seed-wrap")
\u2502   \u2514\u2500\u2500 AES-256-GCM wrap key (encrypts genesis seed at rest)
\u2502
\u251C\u2500\u2500 ML-DSA-65 KeyGen (FIPS 204)
\u2502   \u251C\u2500\u2500 Signing Key (sk) \u2014 encrypted in PQ wallet file
\u2502   \u2514\u2500\u2500 Verifying Key (vk) \u2014 public, in wallet + ledger entries
\u2502
\u251C\u2500\u2500 ML-KEM-768 KeyGen (FIPS 203)
\u2502   \u251C\u2500\u2500 Encapsulation Key (ek) \u2014 public, shared with agent credentials
\u2502   \u2514\u2500\u2500 Decapsulation Key (dk) \u2014 encrypted, per-agent scope access
\u2502
\u251C\u2500\u2500 Argon2id(password, salt) \u2192 Master Key (32 bytes)
\u2502   \u2502   Params: 128 MiB memory, 4 iterations, parallelism 1
\u2502   \u2502
\u2502   \u251C\u2500\u2500 HKDF("agenthalo-scope-v2", "scope:sign")   \u2192 Sign key    (TTL: 300s)
\u2502   \u251C\u2500\u2500 HKDF("agenthalo-scope-v2", "scope:vault")  \u2192 Vault key   (TTL: 300s)
\u2502   \u251C\u2500\u2500 HKDF("agenthalo-scope-v2", "scope:wallet") \u2192 Wallet key  (TTL: 120s)
\u2502   \u251C\u2500\u2500 HKDF("agenthalo-scope-v2", "scope:identity") \u2192 ID key   (TTL: 1800s)
\u2502   \u251C\u2500\u2500 HKDF("agenthalo-scope-v2", "scope:genesis") \u2192 Genesis key (TTL: 30s)
\u2502   \u2514\u2500\u2500 HKDF("agenthalo-scope-v2", "scope:admin")  \u2192 Admin key   (TTL: 60s)
\u2502
\u2514\u2500\u2500 HKDF("agenthalo-genesis-wallet-entropy-v1", "bip39-entropy-32")
    \u2514\u2500\u2500 32 bytes \u2192 BIP-39 Mnemonic (24 words) \u2192 HD Wallet
        \u251C\u2500\u2500 Ethereum address (m/44'/60'/0'/0/0)
        \u251C\u2500\u2500 Bitcoin address  (m/44'/0'/0'/0/0)
        \u251C\u2500\u2500 Polygon address  (m/44'/60'/0'/0/0, chain 137)
        \u2514\u2500\u2500 Arbitrum address (m/44'/60'/0'/0/0, chain 42161)
</pre>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Post-Quantum Algorithms</div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">ML-DSA-65 (FIPS 204)</div>
          <div class="gdoc-card-body">
            <strong>Module-Lattice Digital Signature Algorithm</strong>. Security level 3
            (equivalent to AES-192). Used for all identity ledger signatures, trust attestations,
            and session authentication. Based on the hardness of the Module Learning-With-Errors
            (MLWE) problem. Signature size: ~3.3 KB. Public key: ~1.95 KB.
            <br><br>
            <em>Rust crate:</em> <code>ml-dsa</code> (RustCrypto)
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">ML-KEM-768 (FIPS 203)</div>
          <div class="gdoc-card-body">
            <strong>Module-Lattice Key Encapsulation Mechanism</strong>. Security level 3
            (equivalent to AES-192). Used for agent credential encapsulation: when a new agent
            is authorized, each scope key is wrapped via KEM \u2192 shared-secret \u2192 AES-GCM.
            The encapsulated key can only be decrypted by the agent's decapsulation key.
            <br><br>
            <em>Rust crate:</em> <code>ml-kem</code> (RustCrypto)
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Encrypted Master Cache (Option B)</div>
      <p class="gdoc-text">
        The master key derived from the user's password via Argon2id is <strong>never held in
        plaintext in RAM</strong> after initial derivation. Instead, it follows the
        <strong>Option B Encrypted Master Cache</strong> pattern:
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">1. Derive</div>
          <div class="gdoc-card-body">
            <code>Argon2id(password, salt, 128MiB, 4 iter)</code> \u2192 32-byte master key.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">2. Encrypt</div>
          <div class="gdoc-card-body">
            Generate ephemeral 32-byte session key + 12-byte nonce.
            <code>AES-256-GCM(session_key, nonce, master_key)</code>.
            Zeroize plaintext master key immediately.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">3. Use</div>
          <div class="gdoc-card-body">
            When a scope key is needed: decrypt master from cache, derive scope via HKDF,
            zeroize master again. Scope keys auto-expire (TTL-based).
            Session key struct implements <code>ZeroizeOnDrop</code>.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Identity Ledger</div>
      <p class="gdoc-text">
        The identity ledger is a <strong>hash-chained, append-only log</strong> with post-quantum
        signatures on every entry. It records the complete lifecycle of the agent's identity:
      </p>
      <div class="gdoc-card gdoc-card--blue" style="margin-top:10px">
        <div class="gdoc-card-head">Ledger Event Types</div>
        <div class="gdoc-card-body">
          <table style="width:100%;border-collapse:collapse;font-size:0.85rem">
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>GenesisEntropyHarvested</code></td>
              <td style="padding:4px 8px">Birth certificate \u2014 entropy hash + source list</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>WalletCreated</code></td>
              <td style="padding:4px 8px">PQ wallet generated, public key recorded</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>WalletImported</code></td>
              <td style="padding:4px 8px">External wallet or EVM address connected</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>DeviceUpdated</code></td>
              <td style="padding:4px 8px">Hardware fingerprint changed/collected</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>NetworkUpdated</code></td>
              <td style="padding:4px 8px">IP/MAC hashes changed</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>SafetyTierApplied</code></td>
              <td style="padding:4px 8px">Security tier changed (max-safe/less-safe/low)</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>SocialTokenConnected</code></td>
              <td style="padding:4px 8px">OAuth provider linked (GitHub, Google, etc.)</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><code>SuperSecureUpdated</code></td>
              <td style="padding:4px 8px">Passkey, security key, or TOTP changed</td>
            </tr>
            <tr>
              <td style="padding:4px 8px"><code>AnonymousModeUpdated</code></td>
              <td style="padding:4px 8px">Anonymous mode toggled</td>
            </tr>
          </table>
        </div>
      </div>
      <p class="gdoc-text" style="margin-top:12px">
        Each entry includes: sequence number, timestamp, event kind, payload, prev_hash, entry_hash,
        and a <strong>PQ signature envelope</strong> (ML-DSA-65 signature over the entry hash).
        The chain is tamper-evident: modifying any entry invalidates all subsequent hashes and signatures.
      </p>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">CURBy Quantum Randomness &amp; TWINE</div>
      <p class="gdoc-text">
        The CURBy quantum entropy source (University of Colorado) participates in the
        <strong>TWINE (Trust-Worthy INformation Exchange) network</strong> \u2014 a distributed
        protocol for publishing verifiable randomness pulses. Each CURBy pulse includes a
        <code>twine_hash</code> field: a content-addressed hash that anchors the pulse into
        the TWINE DAG.
      </p>
      <div class="gdoc-card gdoc-card--green" style="margin-top:10px">
        <div class="gdoc-card-head">TWINE Integration Point</div>
        <div class="gdoc-card-body">
          When the Genesis ceremony harvests CURBy entropy, the agent records the TWINE hash
          alongside the pulse ID in the entropy source metadata:
          <br><br>
          <code>{ "pulse_id": 7523, "twine_hash": "bafk...", "timestamp": "..." }</code>
          <br><br>
          This means the agent's birth entropy is <strong>anchored to a publicly verifiable
          TWINE DAG node</strong>. Any third party can verify: (1) the CURBy pulse existed,
          (2) it was published at the claimed time, (3) the entropy bytes match.
          Future versions may register the agent's genesis commitment directly into the
          TWINE network as a first-class identity anchor.
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Wallet Derivation Path</div>
      <div class="gdoc-card gdoc-card--blue" style="margin-top:10px">
        <div class="gdoc-card-head">Genesis Seed \u2192 BIP-39 \u2192 EVM Address</div>
        <div class="gdoc-card-body">
<pre style="font-family:var(--font-mono);font-size:0.82rem;line-height:1.5;margin:0;white-space:pre">
genesis_seed (64 bytes, from entropy ceremony)
    \u2502
    \u2514\u2500 HKDF-SHA-256
         salt:  "agenthalo-genesis-wallet-entropy-v1"
         info:  "bip39-entropy-32"
         \u2502
         \u2514\u2500 32 bytes raw entropy
              \u2502
              \u2514\u2500 BIP-39 encoding \u2192 24-word mnemonic phrase
                   \u2502
                   \u2514\u2500 BIP-32/BIP-44 HD derivation
                        m/44'/60'/0'/0/0 \u2192 Ethereum private key
                        \u2502
                        \u2514\u2500 secp256k1 \u2192 public key \u2192 Keccak-256 \u2192 EVM address
</pre>
          <p style="margin-top:8px;font-size:0.85rem">
            The mnemonic is <strong>encrypted at rest</strong> under the Wallet scope key
            (AES-256-GCM, key derived from Argon2id master via HKDF). Decryption requires
            the user's password and an active Wallet scope session (120s TTL).
          </p>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Scope-Based Access Control</div>
      <p class="gdoc-text">
        Every sensitive API endpoint requires an active scope key. The scope system enforces
        least-privilege access with time-limited keys:
      </p>
      <div class="gdoc-card gdoc-card--amber" style="margin-top:10px">
        <div class="gdoc-card-head">Scope TTLs and Protected Operations</div>
        <div class="gdoc-card-body">
          <table style="width:100%;border-collapse:collapse;font-size:0.85rem">
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px;width:100px"><strong>Sign</strong></td>
              <td style="padding:4px 8px;width:60px">300s</td>
              <td style="padding:4px 8px">PQ signature creation, ledger writes, session attestation</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><strong>Vault</strong></td>
              <td style="padding:4px 8px">300s</td>
              <td style="padding:4px 8px">API key storage, secret retrieval</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><strong>Wallet</strong></td>
              <td style="padding:4px 8px">120s</td>
              <td style="padding:4px 8px">Mnemonic access, address generation, WDK seed operations</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><strong>Identity</strong></td>
              <td style="padding:4px 8px">1800s</td>
              <td style="padding:4px 8px">Device/network config, profile updates, social links</td>
            </tr>
            <tr style="border-bottom:1px solid rgba(0,255,157,0.15)">
              <td style="padding:4px 8px"><strong>Genesis</strong></td>
              <td style="padding:4px 8px">30s</td>
              <td style="padding:4px 8px">Genesis seed access (extremely sensitive, shortest TTL)</td>
            </tr>
            <tr>
              <td style="padding:4px 8px"><strong>Admin</strong></td>
              <td style="padding:4px 8px">60s</td>
              <td style="padding:4px 8px">Reserved for future administrative operations</td>
            </tr>
          </table>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">On-Chain Contract Architecture</div>
      <p class="gdoc-text">
        The TrustVerifier smart contract on Base Sepolia accepts attestation proofs and records
        verification results on-chain. The agent's EVM address is the <code>msg.sender</code>
        for all transactions, tying every on-chain action to the genesis-derived identity.
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">verifyAndRecord()</div>
          <div class="gdoc-card-body">
            Posts a proof + public inputs to the on-chain verifier. If verification passes,
            the attestation digest is recorded. The agent's address is stored as the attester.
            Gas estimation with preflight cap (500K gas max).
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">verifyAndRecordAnonymous()</div>
          <div class="gdoc-card-body">
            Same verification, but the proof is recorded without linking to the sender's
            identity. Uses the same contract but a separate storage mapping.
            For agents operating in anonymous mode.
          </div>
        </div>
      </div>
    </div>
  `;
}

/* ================================================================
   IDENTIFICATION TAB 3: AGENT ACCESS
   ================================================================ */
function idAccess() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">CLI Commands</div>
      <p class="gdoc-text">
        The <code>agenthalo</code> CLI provides identity management commands:
      </p>

      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Create password and unlock vault</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo crypto create-password
Enter password: ********
Confirm: ********
Password created. Master key derived (Argon2id, 128 MiB).
Session unlocked. Scope keys available.</pre>
      </div>

      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Check identity status</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo identity status
Genesis:     COMPLETE (4 sources, pulse #7523)
PQ Wallet:   ML-DSA-65, key_id=sha256:a3f7...
Device:      Bound (CPU+GPU+OS, entropy: 48 bits)
Network:     Local IP hashed, public IP hashed
EVM Address: 0x7a3B...9f2E (Base Sepolia)
Ledger:      14 entries, chain valid</pre>
      </div>

      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Generate agent wallet from genesis seed</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo wallet generate
Deriving wallet entropy from genesis seed...
BIP-39 mnemonic: 24 words (encrypted to vault)
EVM address: 0x7a3B...9f2E
Chains: ethereum, bitcoin, polygon, arbitrum
Ledger: WalletCreated event written (seq 3)</pre>
      </div>

      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Authorize agent credential (ML-KEM-768)</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo agents authorize --label "claude-prod" --scopes sign,vault
Agent authorized:
  ID:     agent_1709312400_a3f7
  Label:  claude-prod
  Scopes: sign, vault
  Algorithm: ML-KEM-768 (FIPS 203)
  Credential file written to .halo/agent_credentials/</pre>
      </div>

      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Post attestation on-chain</div>
        <pre class="gdoc-pre gdoc-code">$ agenthalo onchain attest --session 42
Estimating gas... 287,341 (cap: 500,000)
Submitting to Base Sepolia...
TX: 0x4f2e...c831
Block: #12847291
Gas used: 274,892
Attestation recorded on-chain.</pre>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">API Endpoints</div>
      <p class="gdoc-text">
        The dashboard API exposes identity operations for programmatic access:
      </p>
      <div class="gdoc-tool-list">
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">GET /api/identity/status</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--read">Read-only</div>
          </div>
          <div class="gdoc-tool-desc">
            Returns complete identity state: genesis status, PQ wallet, device/network
            configuration, security tier, social links, and EVM address.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">POST /api/crypto/create-password</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--guard">Guarded</div>
          </div>
          <div class="gdoc-tool-desc">
            Creates the vault password, derives master key via Argon2id, triggers v1\u2192v2
            migration if legacy files exist, and auto-unlocks the session.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">POST /api/crypto/unlock</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--guard">Guarded</div>
          </div>
          <div class="gdoc-tool-desc">
            Unlocks the session with password or agent credentials (ML-KEM-768).
            Supports dual-path: human password OR agent_id + agent_sk + scopes.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">POST /api/agents/authorize</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--admin">Requires Sign scope</div>
          </div>
          <div class="gdoc-tool-desc">
            Authorizes a new agent credential. Generates ML-KEM-768 keypair, encapsulates
            scope keys, writes credential file. Returns agent_id + agent_sk (display once).
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">POST /api/agentaddress/generate</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--guard">Guarded</div>
          </div>
          <div class="gdoc-tool-desc">
            Generates an EVM address from the genesis-derived wallet seed.
            Records WalletImported event in the identity ledger.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">GET /api/crypto/status</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--read">Read-only</div>
          </div>
          <div class="gdoc-tool-desc">
            Returns current crypto session state: lock status, migration status,
            active scopes, failed attempt count, and throttle timer.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Formal Verification Bridge</div>
      <p class="gdoc-text">
        The identity subsystem maps to Lean 4 formal models, extending the Genesis
        categorical framework:
      </p>

      <div class="gdoc-bridge">
        <div class="gdoc-bridge-col">
          <div class="gdoc-bridge-heading">Lean Category-Theoretic Model \u2192 Rust Runtime</div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Identity/Delta.lean<br><span>IdentityDelta + applyDelta</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">identity.rs<br><span>IdentityConfig + save()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Core/Certificates.lean<br><span>CommitCertificate</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">identity_ledger.rs<br><span>LedgerSignatureRef + verify_chain()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Core/Authorization.lean<br><span>ScopePolicy</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">crypto_scope.rs<br><span>CryptoScope + ScopeKey (TTL)</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Sheaf/MaterializationFunctor.lean<br><span>naturality</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">encrypted_file.rs<br><span>EncryptedFileV2 (scope-keyed)</span></div>
          </div>
        </div>
      </div>
    </div>
  `;
}

/* ─────────────────────────────────────────────────────────────────────────────
   COMMUNICATION PAGE
   ───────────────────────────────────────────────────────────────────────────── */

function renderCommunication() {
  const content = document.getElementById('content');
  content.innerHTML = `
    <!-- Hero Banner -->
    <div class="gdoc-hero">
      <div class="gdoc-hero-img-wrap">
        <img class="gdoc-hero-img" src="img/agentpmtbootup.png" alt="Communication"
             onerror="this.style.display='none'">
      </div>
      <div class="gdoc-hero-copy">
        <div class="gdoc-hero-kicker">Agent H.A.L.O. // Secure Communication</div>
        <div class="gdoc-hero-title">Communication</div>
        <div class="gdoc-hero-subtitle">Privacy Transport, Capabilities &amp; Zero-Knowledge Proofs</div>
        <div class="gdoc-hero-sep"></div>
        <div class="gdoc-hero-stat-row">
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">did:key</div>
            <div class="gdoc-hero-stat-lbl">Identity</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">Groth16</div>
            <div class="gdoc-hero-stat-lbl">ZK Proofs</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">Nym</div>
            <div class="gdoc-hero-stat-lbl">Mixnet</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">DIDComm</div>
            <div class="gdoc-hero-stat-lbl">Messaging</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Tab Bar -->
    <div class="gdoc-tabs">
      <button class="gdoc-tab active" data-tab="comm-overview" onclick="commTab('comm-overview')">F1:OVERVIEW</button>
      <button class="gdoc-tab" data-tab="comm-technical" onclick="commTab('comm-technical')">F2:TECHNICAL</button>
      <button class="gdoc-tab" data-tab="comm-access" onclick="commTab('comm-access')">F3:ACCESS</button>
    </div>

    <div id="comm-tab-content"></div>
  `;
  commTab('comm-overview');
}

window.commTab = function(tab) {
  document.querySelectorAll('.gdoc-tab').forEach(b => {
    b.classList.toggle('active', b.dataset.tab === tab);
  });
  const el = document.getElementById('comm-tab-content');
  if (!el) return;

  switch (tab) {
    case 'comm-overview':
      el.innerHTML = commOverview();
      hydrateCommRuntimePanel();
      break;
    case 'comm-technical': el.innerHTML = commTechnical(); break;
    case 'comm-access': el.innerHTML = commAccess(); break;
  }
};

/* commTab is now defined above as a window method, matching Genesis/Identification pattern */

function hydrateCommRuntimePanel() {
  const panel = document.getElementById('comm-runtime-panel');
  if (!panel) return;
  panel.innerHTML = '<div style="color:var(--text-dim);font-size:0.78rem;padding:8px 0">Querying transport status\u2026</div>';
  Promise.all([
    fetch('/api/status').then(r => r.ok ? r.json() : null).catch(() => null),
    fetch('/api/genesis/status').then(r => r.ok ? r.json() : null).catch(() => null)
  ]).then(([status, genesis]) => {
    const nym = status?.nym || {};
    const did = genesis?.did_uri || null;
    const nymMode = nym.mode || null;
    const nymHealthy = nym.healthy === true;
    const failClosed = nym.fail_closed !== false;
    const proxy = nym.socks5_proxy || null;

    const didDisplay = did
      ? `<span style="font-size:0.72rem;word-break:break-all">${gdocEsc(did)}</span>`
      : '<span style="color:var(--text-dim)">Not generated yet &mdash; complete <a href="#/genesis" style="color:var(--accent)">Genesis</a> first</span>';

    const nymDisplay = nymMode
      ? `${gdocEsc(nymMode)} ${nymHealthy ? '<span style="color:var(--green)">\u2713 healthy</span>' : '<span style="color:var(--accent)">\u2717 not connected</span>'}`
      : '<span style="color:var(--text-dim)">Not configured &mdash; set <code>SOCKS5_PROXY</code> or <code>NYM_BINARY</code></span>';

    const proxyDisplay = proxy
      ? `<code style="color:var(--green)">${gdocEsc(proxy)}</code>`
      : '<span style="color:var(--text-dim)">No proxy configured</span>';

    panel.innerHTML = `
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">DID Identity</div>
          <div class="gdoc-card-body">${didDisplay}</div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Nym Mixnet Transport</div>
          <div class="gdoc-card-body">${nymDisplay}</div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">Fail-Closed Policy</div>
          <div class="gdoc-card-body">${failClosed
            ? '<span style="color:var(--green)">\u2713 ENFORCED</span> &mdash; external traffic blocked without proxy'
            : '<span style="color:var(--accent)">\u26A0 OPEN (degraded)</span> &mdash; direct fallback active'}</div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">SOCKS5 Proxy</div>
          <div class="gdoc-card-body">${proxyDisplay}</div>
        </div>
      </div>
    `;
  });
}

function commOverview() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Live Transport Status</div>
      <div id="comm-runtime-panel"></div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">The Sovereign Communication Stack</div>
      <p class="gdoc-text">
        After <a href="#/genesis" style="color:#00ff9d">Genesis</a> creates the seed and
        <a href="#/identification" style="color:#00ff9d">Identification</a> derives keys,
        the <strong>Sovereign Binding Ceremony</strong> fuses DID + EVM into a
        durable, dual-signed attestation persisted in the identity ledger.
        From that point, nothing leaves this node unsigned, unclassified, or unrouted.
        The identity provably flows from genesis to communication \u2014 the Lean
        proofs establish that the sovereign binding is a <strong>natural transformation</strong>
        (category-theoretic guarantee that identity is preserved, not lost or confused).
      </p>
      <div class="gdoc-pipeline" style="margin:12px 0 16px">
        <div class="gdoc-pipeline-box" style="text-align:center;letter-spacing:1px">
          <strong>Genesis Seed</strong> &nbsp;\u2192&nbsp;
          <strong>DID Identity</strong> &nbsp;\u2192&nbsp;
          <strong>Sovereign Binding</strong> &nbsp;\u2192&nbsp;
          <strong>Identity Ledger</strong>
          <br>
          &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;\u2193<br>
          <strong>DIDComm</strong> (authcrypt carries sender DID) &nbsp;\u2192&nbsp;
          <strong>A2A Bridge</strong> (agent card references DID + EVM)
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Transport Layers</div>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--amber">
          <div class="gdoc-card-icon-lg">\u26D4</div>
          <div class="gdoc-card-head">Layer 0 \u2014 Privacy Controller (The Gate)</div>
          <div class="gdoc-card-body">
            Every outbound request \u2014 HTTP, DIDComm, or on-chain \u2014 passes through the
            <strong>Privacy Controller</strong> first. It classifies destination URLs into three tiers:
            <strong>None</strong> (local + public infrastructure like entropy beacons),
            <strong>P2P</strong> (mesh peers), <strong>Maximum</strong> (external \u2014 SOCKS5/Nym required).
            <strong>Fail-closed by default</strong>: if no proxy is available, external requests are blocked,
            not downgraded. Sensitive DIDComm message types (task-send, credential-offer, etc.) are
            <em>always</em> Maximum regardless of destination.
            <br><br><code>classify_url() \u2192 None | P2P | Maximum \u2192 direct | proxy | blocked</code>
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\uD83C\uDF10</div>
          <div class="gdoc-card-head">Layer 1 \u2014 Nym Mixnet Transport</div>
          <div class="gdoc-card-body">
            Two complementary modes: <strong>SOCKS5 proxy</strong> (wraps HTTP through Nym client,
            auto-detected via env vars, health-checked with 300ms timeout) and
            <strong>Native nym-sdk</strong> (direct Rust integration, SURB-based anonymous replies,
            configurable cover traffic, inbound message reception).
            All on-chain <code>cast</code> commands get SOCKS5 proxy env vars injected automatically.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-icon-lg">\u2637</div>
          <div class="gdoc-card-head">Layer 2 \u2014 P2P Mesh (libp2p)</div>
          <div class="gdoc-card-body">
            Full peer-to-peer mesh with 7 sub-protocols: identify, Kademlia DHT (distributed agent
            directory), gossipsub (topic-based pub/sub), relay client (NAT traversal), DCUTR
            (hole punching), mDNS (local discovery), autonat. The mesh keypair is derived from the
            <strong>same Ed25519 key as the DID</strong> \u2014 PeerId = DID = same genesis seed.
            Agent announcements are dual-signed and published to Kademlia with TTL-based expiry.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-icon-lg">\uD83D\uDD11</div>
          <div class="gdoc-card-head">Layer 3 \u2014 DIDComm + A2A Bridge</div>
          <div class="gdoc-card-body">
            DIDComm v2 messaging with <strong>authcrypt</strong> (authenticated, sender-identified)
            and <strong>anoncrypt</strong> (anonymous sender). Authcrypt carries the sovereign identity
            in every envelope via SenderEnrichment (EVM address + binding proof hash).
            7 built-in message types (ping, ack, agent-card, task lifecycle, credential exchange, error).
            The A2A Bridge wraps DIDComm in a Google A2A-compatible JSON-RPC surface
            (<code>GET /.well-known/agent.json</code>, <code>tasks/send|get|cancel</code>).
            <br><br><code>did:key:z6Mk\u2026 \u2192 Ed25519 + ML-DSA-65 + X25519 + ML-KEM-768</code>
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\uD83D\uDEE1\uFE0F</div>
          <div class="gdoc-card-head">Layer 4 \u2014 Capability Tokens + ZK Proofs</div>
          <div class="gdoc-card-body">
            <strong>Capability tokens</strong>: time-bounded, dual-signed grants (Ed25519 + ML-DSA-65)
            specifying resource patterns and access modes (Read, Write, Append, Control).
            GrantStore keyed by PUF fingerprints \u2014 hardware-bound authorization.
            <strong>ZK credential proofs</strong>: Groth16/BN254 circuit with 9 public inputs proves
            you hold a valid, unexpired, non-revoked capability without revealing which one or
            who granted it. Agents can also prove mesh membership via anonymous ZK proofs.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Sovereign Binding Ceremony</div>
      <p class="gdoc-text">
        The critical step that fuses DID identity and EVM wallet into a single provable binding.
        Triggered automatically at the end of Genesis harvest and idempotent on retry.
      </p>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Signed Attestation</div>
          <div class="gdoc-card-body">
            A canonical document containing <code>{DID, EVM address, entropy hash, timestamps}</code>,
            dual-signed with Ed25519 + ML-DSA-65 and content-addressed via SHA-256.
            Proves: "This DID and this EVM address were derived from the same genesis."
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Binding Proof</div>
          <div class="gdoc-card-body">
            A separate canonical document, <strong>triple-signed</strong> with Ed25519 + ML-DSA-65 + secp256k1.
            The secp256k1 signature proves the agent controls the EVM private key.
            Creates a cryptographic triangle: DID keys sign it (DID ownership), EVM key signs it
            (EVM ownership), content is hashed (tamper evidence).
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">DID Document Mutation</div>
          <div class="gdoc-card-body">
            The DID document gets an <code>alsoKnownAs</code> entry:
            <code>did:pkh:eip155:1:&lt;evm_address&gt;</code> (W3C standard).
            The mutated document with <code>alsoKnownAs</code> is serialized to JSON
            and persisted in the identity ledger as a durable record.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Formal Guarantee</div>
          <div class="gdoc-card-body">
            <code>SovereignBinding.lean</code>: The binding is a natural transformation from the
            agent identity presheaf to the communication presheaf. Proved:
            <code>sovereign_binding_natural</code> (same seed \u2192 same identity),
            <code>binding_proof_verifiable</code> (DID + EVM share common genesis),
            <code>preserves_did</code>, <code>preserves_evm</code>.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">DIDComm Composition Policy</div>
      <p class="gdoc-text">
        H.A.L.O. enforces a <strong>strict non-composition policy</strong> for DIDComm message packaging.
        Messages are packaged as either <code>authcrypt</code> (authenticated encryption) or
        <code>anoncrypt</code> (anonymous encryption) \u2014 <strong>never nested</strong>.
        This is formally verified as a closed sum: the <code>EnvelopeKind</code> type has exactly two
        constructors, and exhaustiveness is proved at the type level.
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Why Non-Composition?</div>
          <div class="gdoc-card-body">
            IOG's analysis (<a href="https://eprint.iacr.org/2024/1361" style="color:#00ff9d" target="_blank">eprint 2024/1361</a>)
            shows that composing <code>anoncrypt(authcrypt(...))</code> with AES-256-GCM creates a
            key-commitment vulnerability. AES-CBC-HMAC would be needed for safe composition.
            Instead, H.A.L.O. avoids the problem entirely: <strong>Nym mixnet provides
            transport-layer anonymity</strong>, eliminating the need to wrap authcrypt inside anoncrypt.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Formal Guarantee</div>
          <div class="gdoc-card-body">
            <code>CompositionPolicy.lean</code> proves:
            <div class="gdoc-theorem-list" style="margin-top:8px">
              <div class="gdoc-theorem">
                <span class="gdoc-thm-badge">\u2713</span>
                <div><code>no_composition_possible</code><br>
                <span class="gdoc-thm-desc">EnvelopeKind is a closed sum \u2014 no third constructor can exist.</span></div>
              </div>
              <div class="gdoc-theorem">
                <span class="gdoc-thm-badge">\u2713</span>
                <div><code>authcrypt_plus_nym_achieves_both</code><br>
                <span class="gdoc-thm-desc">Authcrypt + Nym transport achieves both sender authentication and sender anonymity.</span></div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Trust Boundary Hardening</div>
      <p class="gdoc-text">
        The communication stack's security boundaries are formally verified at 7 trust boundary layers.
        A hostile audit identified 12 potential gaps; all have been closed with proved Lean theorems.
      </p>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Proof Gate</div>
          <div class="gdoc-card-body">
            <code>ProofGateSpec.lean</code> + <code>ProofGateRefinement.lean</code> \u2014
            Formal spec and refinement for the proof verification gate.
            Theorems prove that the gate rejects invalid proofs and that the runtime
            <code>verify_proof()</code> refines the Lean spec.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Certificate Integrity</div>
          <div class="gdoc-card-body">
            <code>CertificateIntegrity.lean</code> \u2014
            Proves certificate construction is sound, hash chain integrity is preserved,
            and tampering is detectable. Every certificate chains to the previous
            via hash, and verification is inductive.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">Identity Ledger Chain</div>
          <div class="gdoc-card-body">
            <code>LedgerSpec.lean</code> + <code>LedgerChain.lean</code> + <code>LedgerRefinement.lean</code> \u2014
            Three-layer formal model: abstract spec, chain properties (monotonicity, immutability),
            and Rust runtime refinement. The ledger is append-only by proof, not by convention.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Sender Enrichment</div>
          <div class="gdoc-card-body">
            <code>EnrichmentSpec.lean</code> \u2014
            Proves that message enrichment (adding metadata for the recipient) preserves
            the original message payload and does not introduce forgery vectors.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">DIDComm Refinement</div>
          <div class="gdoc-card-body">
            <code>DIDCommRefinement.lean</code> \u2014
            Bidirectional correspondence between Lean spec and Rust runtime:
            <code>rust_unpack_rejects_composition</code> (no composed envelopes accepted)
            and <code>rust_unpack_accepts_iff_valid_kind</code> (accepts iff valid).
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Why Does This Matter?</div>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\u26BF</div>
          <div class="gdoc-card-head">Sovereign Identity</div>
          <div class="gdoc-card-body">
            No certificate authority, no OAuth provider, no platform lock-in.
            Your DID is derived from your own entropy + hardware. You own it.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-icon-lg">\u2699</div>
          <div class="gdoc-card-head">Hardware-Bound Authorization</div>
          <div class="gdoc-card-body">
            Capability grants are tied to PUF fingerprints. Stolen credentials
            don't work on different hardware.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--amber">
          <div class="gdoc-card-icon-lg">\uD83D\uDD2E</div>
          <div class="gdoc-card-head">Selective Disclosure</div>
          <div class="gdoc-card-body">
            ZK proofs let you prove authorization without revealing identity,
            capability details, or the grantor. Privacy by construction.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\uD83C\uDF10</div>
          <div class="gdoc-card-head">Network Privacy</div>
          <div class="gdoc-card-body">
            Nym mixnet routing prevents traffic analysis. The fail-closed default
            means even a misconfigured node can't accidentally leak metadata.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">On-Chain Integration</div>
      <p class="gdoc-text">
        The communication stack bridges to EVM chains for durable attestation.
        Session proofs (Merkle root over event digests) are posted on-chain via
        <code>verifyAndRecord()</code> or anonymously via <code>verifyAndRecordAnonymous()</code>.
        All on-chain calls route through the same Nym privacy transport \u2014
        <code>cast</code> commands get SOCKS5 proxy environment variables injected automatically.
      </p>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Payment Channel Network (PCN)</div>
      <p class="gdoc-text">
        For micropayment and resource-metering use cases, the PCN adapter generates
        <strong>compliance witnesses</strong> that prove channel conservation
        (<code>balance_left + balance_right == capacity</code>) and replay protection.
        These witnesses feed into the attestation circuit for on-chain settlement.
      </p>
    </div>
  `;
}

function commTechnical() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">DID Derivation Tree</div>
      <p class="gdoc-text">
        The Genesis seed is the single root from which all cryptographic material is derived.
        HKDF with domain-separated info strings ensures each key pair is independent.
      </p>
      <pre class="gdoc-pre gdoc-code">Genesis Seed (64 bytes, AES-256-GCM at rest)
\u2502
\u251C\u2500 HKDF("agenthalo-p2p-identity-v1") \u2500\u2500\u2500\u2500\u2192 Ed25519 secret (32 bytes)
\u2502    \u251C\u2500 Public key \u2192 multicodec 0xed01 + base58btc \u2192 did:key:z6Mk\u2026
\u2502    \u2514\u2500 ML-DSA-65 keypair (post-quantum signing)
\u2502
\u251C\u2500 HKDF("agenthalo-did-agreement-v1") \u2500\u2500\u2500\u2500\u2192 X25519 secret (32 bytes)
\u2502    \u2514\u2500 ML-KEM-768 seed (post-quantum key exchange)
\u2502
\u2514\u2500 HKDF("agenthalo-wallet-entropy-v1") \u2500\u2500\u2500\u2192 32 bytes \u2192 BIP-39 mnemonic (24 words)
     \u2514\u2500 EVM address (Alloy signer)</pre>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Dual Signature Protocol</div>
      <p class="gdoc-text">
        Every signed payload carries two signatures. Both must verify for the message to be accepted.
        This provides <strong>hybrid classical + post-quantum security</strong>.
      </p>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Classical: Ed25519</div>
          <div class="gdoc-card-body">
            <code>Ed25519.sign(M)</code> &rarr; <strong>sig_ed</strong> (64 bytes)<br>
            Fast, compact, battle-tested. Provides immediate security against classical adversaries.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Post-Quantum: ML-DSA-65</div>
          <div class="gdoc-card-body">
            <code>ML-DSA-65.sign(M)</code> &rarr; <strong>sig_pq</strong> (3309 bytes)<br>
            NIST FIPS 204. Provides protection against future quantum computers.
          </div>
        </div>
      </div>
      <pre class="gdoc-pre gdoc-code" style="margin-top:10px">dual_verify(doc, M, sig_ed, sig_pq):
  \u2713 Ed25519.verify(doc.ed25519_pk, M, sig_ed)  AND
  \u2713 ML-DSA-65.verify(doc.mldsa65_pk, M, sig_pq)
  \u2192 both must pass</pre>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Capability Token Structure</div>
      <p class="gdoc-text">
        <strong>GrantStore</strong> materializes accepted capabilities keyed by PUF fingerprint.
        <code>accept_capability()</code> verifies both signatures before storing.
      </p>
      <table class="gdoc-table">
        <thead><tr><th>Field</th><th>Type</th><th>Description</th></tr></thead>
        <tbody>
          <tr><td><code>grantor_did</code></td><td>String</td><td>DID URI of the granting agent</td></tr>
          <tr><td><code>grantee_did</code></td><td>String</td><td>DID URI of the receiving agent</td></tr>
          <tr><td><code>resource_patterns</code></td><td>Vec&lt;String&gt;</td><td>Glob patterns for accessible resources</td></tr>
          <tr><td><code>modes</code></td><td>Vec&lt;AccessMode&gt;</td><td>Read, Write, Append, Control</td></tr>
          <tr><td><code>not_before / not_after</code></td><td>u64</td><td>Unix timestamps bounding validity</td></tr>
          <tr><td><code>agent_class</code></td><td>AgentClass</td><td>Public | Authenticated | Verified{min_tier} | Specific{did}</td></tr>
          <tr><td><code>ed_signature</code></td><td>[u8; 64]</td><td>Ed25519 signature from grantor</td></tr>
          <tr><td><code>pq_signature</code></td><td>Vec&lt;u8&gt;</td><td>ML-DSA-65 signature from grantor</td></tr>
        </tbody>
      </table>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">ZK Circuit Architecture</div>
      <p class="gdoc-text">
        The ZK credential circuit proves authorization without revealing identity,
        capability details, or the grantor. Groth16 over BN254 with 9 public inputs.
      </p>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Public Inputs (9)</div>
          <div class="gdoc-card-body">
            <code>identity_commit</code> &mdash; Pedersen(did_secret)<br>
            <code>resource_commit</code> &mdash; Pedersen(resource_pattern)<br>
            <code>action_mask</code> &mdash; bitfield: R=1 W=2 A=4 C=8<br>
            <code>epoch</code> &mdash; current time epoch<br>
            <code>revocation_root</code> &mdash; Merkle root of revocation set
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Witness (Private)</div>
          <div class="gdoc-card-body">
            <code>did_secret</code>, <code>grant_blob</code>, <code>grantor_secret</code><br>
            <code>resource_preimage</code>, <code>action_preimage</code><br>
            <code>revocation_merkle_path</code> + siblings<br>
            Time bounds, nonce (16+ fields total)
          </div>
        </div>
      </div>
      <div class="gdoc-module gdoc-module--green" style="margin-top:12px">
        <div class="gdoc-module-header">
          <code>ZK Credential Proof</code>
          <span class="gdoc-module-tag">Groth16 / BN254</span>
        </div>
        <div class="gdoc-module-body">
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div>I hold a valid capability token</div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div>The token covers this resource + action</div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div>The token is not expired or revoked</div>
            </div>
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge" style="color:var(--red)">\u2717</span>
              <div>Does NOT reveal: which token, who granted it, grantee identity</div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Privacy Transport Architecture</div>
      <p class="gdoc-text">
        Every outbound request passes through the Privacy Controller. Classification is deterministic
        and fail-closed &mdash; if no proxy is configured, external traffic is blocked, not downgraded.
      </p>
      <pre class="gdoc-pre gdoc-code">Outbound HTTP Request
\u2502
\u251C\u2500 classify_url(url)
\u2502   \u251C\u2500 is_peer(host)?               \u2192 PrivacyLevel::P2P     \u2192 SOCKS5 if available
\u2502   \u251C\u2500 is_local(host)?              \u2192 PrivacyLevel::None    \u2192 direct
\u2502   \u251C\u2500 is_public_infrastructure(h)? \u2192 PrivacyLevel::None    \u2192 direct
\u2502   \u2502   (random.colorado.edu, beacon.nist.gov, api.drand.sh, *.drand.sh)
\u2502   \u2514\u2500 otherwise                    \u2192 PrivacyLevel::Maximum \u2192 SOCKS5 required
\u2502
\u251C\u2500 resolve_socks5_proxy()
\u2502   \u251C\u2500 $SOCKS5_PROXY  \u2192 normalize \u2192 socks5://host:port
\u2502   \u251C\u2500 $ALL_PROXY     \u2192 if socks5:// scheme
\u2502   \u251C\u2500 $NYM_BINARY    \u2192 socks5://127.0.0.1:1080
\u2502   \u2514\u2500 none found     \u2192 None
\u2502
\u2514\u2500 ensure_route_allowed(url)
    \u251C\u2500 proxy found    \u2192 Ok(Some(proxy_uri))
    \u251C\u2500 fail-closed    \u2192 Err("outbound blocked: ... requires mixnet")
    \u2514\u2500 fail-open      \u2192 Ok(None) \u2192 direct fallback (degraded)</pre>

      <div class="gdoc-module gdoc-module--amber" style="margin-top:12px">
        <div class="gdoc-module-header">
          <code>DIDComm-Aware Routing</code>
          <span class="gdoc-module-tag">Message-Type Override</span>
        </div>
        <div class="gdoc-module-body">
          <p>Sensitive message types are <em>always</em> routed at <code>Maximum</code> privacy
          regardless of destination classification:
          <code>task-send</code>, <code>task-status</code>, <code>task-artifact</code>,
          <code>task-cancel</code>, <code>credential-offer</code>, <code>credential-request</code>,
          <code>credential-issue</code>. Pings and acks are exempt.</p>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--blue" style="margin-top:12px">
        <div class="gdoc-module-header">
          <code>apply_proxy_env_for_cast()</code>
          <span class="gdoc-module-tag">Cast RPC Proxy Injection</span>
        </div>
        <div class="gdoc-module-body">
          <p>On-chain transactions via Foundry <code>cast</code> inherit the same routing.
          Extracts <code>--rpc-url</code> from args, classifies it, and injects
          <code>ALL_PROXY</code>, <code>HTTPS_PROXY</code>, <code>SOCKS5_PROXY</code>
          into the child process environment.</p>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Attestation &amp; On-Chain Settlement</div>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Attestation Circuit (Groth16 / BN254)</div>
          <div class="gdoc-card-body">
            <strong>5 public inputs:</strong> Merkle root over session events, SHA-256 of session digest, event count.<br><br>
            <code>verifyAndRecord(proof, pubInputs)</code> &rarr; identified attestation<br>
            <code>verifyAndRecordAnonymous(proof, pubInputs)</code> &rarr; anonymous attestation
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">PCN Compliance Witness</div>
          <div class="gdoc-card-body">
            For micropayment channels, a compliance witness proves channel conservation
            and replay protection.<br><br>
            <code>\u2200 ch \u2208 channels: ch.balance_left + ch.balance_right == ch.capacity</code><br>
            Witnesses feed into the attestation circuit for on-chain settlement.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Formalization Coverage</div>
      <p class="gdoc-text">
        End-to-end formal verification status across all 14 subsystem layers.
        "Proved" means Lean 4 theorems exist with no <code>sorry</code>.
      </p>
      <table class="gdoc-table">
        <thead><tr><th>Layer</th><th>Rust</th><th>Lean</th><th>Gap</th></tr></thead>
        <tbody>
          <tr><td>Genesis entropy</td><td>Complete</td><td>Proved (T5, entropy mixing)</td><td>AES-GCM at rest not spec'd</td></tr>
          <tr><td>DID derivation</td><td>Complete (4 key pairs)</td><td>Proved (T5, T7, T13)</td><td>Crypto primitives axiomatized</td></tr>
          <tr><td>EVM wallet</td><td>Complete (BIP-39/44)</td><td>Partial (address derivation)</td><td>BIP standard not spec'd</td></tr>
          <tr><td>Sovereign binding</td><td>Complete + idempotent</td><td>Proved (naturality, verifiability)</td><td>Hash chain not spec'd</td></tr>
          <tr><td>DIDComm authcrypt</td><td>Complete + enrichment</td><td>Proved (T22, T23, refinement)</td><td>AEAD/ECDH axiomatized</td></tr>
          <tr><td>DIDComm anoncrypt</td><td>Complete</td><td>Proved (anoncrypt spec)</td><td>Same</td></tr>
          <tr><td>Composition policy</td><td>API hardened</td><td>Proved (closed sum, non-composition)</td><td>&mdash;</td></tr>
          <tr><td>DIDComm handler</td><td>Complete (7 msg types)</td><td>Partial (handler spec)</td><td>State machine not spec'd</td></tr>
          <tr><td>A2A bridge</td><td>Complete (JSON-RPC)</td><td>Partial (mesh spec)</td><td>Card doesn't carry binding yet</td></tr>
          <tr><td>ZK credentials</td><td>Complete (full roundtrip)</td><td>Proved (T17-T20, T24)</td><td>Circuit axiomatized</td></tr>
          <tr><td>Privacy controller</td><td>Complete (3-tier + infra)</td><td>Proved (T6, fail-closed)</td><td>URL parsing not spec'd</td></tr>
          <tr><td>Identity ledger</td><td>Complete (hash-chained)</td><td>Proved (LedgerSpec + Chain + Refinement)</td><td>&mdash;</td></tr>
          <tr><td>Proof gate</td><td>Complete</td><td>Proved (ProofGateSpec + Refinement)</td><td>&mdash;</td></tr>
          <tr><td>Certificate integrity</td><td>Complete</td><td>Proved (CertificateIntegrity)</td><td>&mdash;</td></tr>
        </tbody>
      </table>
    </div>
  `;
}

function commAccess() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">CLI Commands</div>
      <div class="gdoc-tool-list">
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo nym status</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--read">read</span>
          </div>
          <div class="gdoc-tool-desc">Show Nym transport mode, proxy URI, health, fail-closed state</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo privacy classify &lt;url&gt;</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--read">read</span>
          </div>
          <div class="gdoc-tool-desc">Show privacy level (None/P2P/Maximum) for a URL</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo onchain attest [--anonymous]</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--admin">write</span>
          </div>
          <div class="gdoc-tool-desc">Generate attestation proof and post to chain. Add <code>--anonymous</code> to post without revealing DID identity.</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo zk prove-capability --resource &lt;pat&gt;</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--guard">guard</span>
          </div>
          <div class="gdoc-tool-desc">Generate ZK credential proof for a capability</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo capability grant --to &lt;did&gt; --resource &lt;pat&gt;</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--admin">write</span>
          </div>
          <div class="gdoc-tool-desc">Issue a dual-signed capability token to another agent</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">agenthalo capability list</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--read">read</span>
          </div>
          <div class="gdoc-tool-desc">List active capabilities in GrantStore</div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">MCP Tools</div>
      <div class="gdoc-tool-list">
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">nym_status</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--read">read</span>
          </div>
          <div class="gdoc-tool-desc">Query Nym transport status (mode, health, proxy, fail-closed)</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">privacy_classify</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--read">read</span>
          </div>
          <div class="gdoc-tool-desc">Classify a URL's privacy level and routing decision</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">onchain_attest</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--admin">write</span>
          </div>
          <div class="gdoc-tool-desc">Generate and post an attestation proof</div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">zk_credential_prove</div>
            <span class="gdoc-tool-badge gdoc-tool-badge--guard">guard</span>
          </div>
          <div class="gdoc-tool-desc">Generate a ZK credential proof for anonymous authorization</div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Environment Variables</div>
      <table class="gdoc-table">
        <thead><tr><th>Variable</th><th>Default</th><th>Description</th></tr></thead>
        <tbody>
          <tr><td><code>SOCKS5_PROXY</code></td><td>\u2014</td><td>SOCKS5 proxy address (e.g. <code>127.0.0.1:1080</code>)</td></tr>
          <tr><td><code>ALL_PROXY</code></td><td>\u2014</td><td>Fallback proxy (must be socks5:// scheme)</td></tr>
          <tr><td><code>NYM_BINARY</code></td><td>\u2014</td><td>Path to Nym client binary (auto-configures local mode)</td></tr>
          <tr><td><code>NYM_CONFIG_DIR</code></td><td>\u2014</td><td>Nym config directory (auto-configures local mode)</td></tr>
          <tr><td><code>NYM_FAIL_OPEN</code></td><td><code>false</code></td><td>Set <code>true</code> to allow direct fallback (degrades privacy)</td></tr>
          <tr><td><code>NYM_FAIL_CLOSED</code></td><td><code>true</code></td><td>Legacy: set <code>false</code> for fail-open (prefer NYM_FAIL_OPEN)</td></tr>
        </tbody>
      </table>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Formal Verification Bridge</div>
      <p class="gdoc-text">
        The Lean categorical model in <code>lean/NucleusDB/</code> mirrors the Rust runtime.
        Key correspondences for the communication stack:
      </p>
      <div class="gdoc-bridge">
        <div class="gdoc-bridge-col">
          <div class="gdoc-bridge-heading">Lean Category-Theoretic Model \u2192 Rust Runtime</div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/Identity/GenesisDerivation.lean<br><span>deterministic derivation (T5)</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">did.rs<br><span>did_from_genesis_seed()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/Identity/DIDDocumentSpec.lean<br><span>well-formedness (T7)</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">did.rs<br><span>DIDDocument struct</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/Identity/SovereignBinding.lean<br><span>natural transformation</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">sovereign_binding.rs<br><span>perform_binding_ceremony()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/DIDComm/EnvelopeSpec.lean<br><span>roundtrip (T22, T23)</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">didcomm.rs<br><span>pack_authcrypt() + unpack()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Security/DIDCommRefinement.lean<br><span>bidirectional \u2194</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">didcomm.rs<br><span>rejects_composition + accepts_iff_valid</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/Protocol/CompositionPolicy.lean<br><span>closed-sum non-composition</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">didcomm.rs<br><span>pack_anoncrypt (crate-scoped)</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Security/ProofGateSpec.lean + Refinement<br><span>gate accepts/rejects</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">trust/attestation.rs<br><span>verify_proof()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Security/CertificateIntegrity.lean<br><span>hash chain integrity</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">identity_ledger.rs<br><span>verify_chain()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Identity/LedgerSpec + Chain + Refinement<br><span>append-only by proof</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">identity_ledger.rs<br><span>append_entry()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/Privacy/RouterSpec.lean<br><span>total classifier (T6)</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">privacy_controller.rs<br><span>classify_url()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/Privacy/FailClosedSpec.lean<br><span>fail-closed guarantee</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">nym.rs<br><span>ensure_route_allowed()</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/ZK/CredentialSpec.lean<br><span>T17, T18</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">zk_credential.rs<br><span>ZkCredentialCircuit</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/Protocol/EnrichmentSpec.lean<br><span>sender enrichment</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">didcomm.rs<br><span>SenderEnrichment</span></div>
          </div>
          <div class="gdoc-bridge-row">
            <div class="gdoc-bridge-item">Comms/AccessControl/CapabilityToken.lean<br><span>token type</span></div>
            <div class="gdoc-bridge-arrow">\u2192</div>
            <div class="gdoc-bridge-item">capability.rs<br><span>CapabilityToken struct</span></div>
          </div>
        </div>
      </div>

    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Source Files</div>
      <table class="gdoc-table">
        <thead><tr><th>File</th><th>Role</th></tr></thead>
        <tbody>
          <tr><td><code>src/halo/did.rs</code></td><td>DID derivation, dual signing, DID Document</td></tr>
          <tr><td><code>src/halo/sovereign_binding.rs</code></td><td>Sovereign binding ceremony (attestation + binding proof + DID mutation)</td></tr>
          <tr><td><code>src/halo/didcomm.rs</code></td><td>DIDComm v2 authcrypt/anoncrypt, SenderEnrichment, non-composition enforcement</td></tr>
          <tr><td><code>src/halo/privacy_controller.rs</code></td><td>URL classification, privacy levels, public infrastructure exemption</td></tr>
          <tr><td><code>src/halo/nym.rs</code></td><td>Nym status, proxy resolution, fail-closed enforcement</td></tr>
          <tr><td><code>src/halo/nym_native.rs</code></td><td>Native nym-sdk integration, SURB replies, cover traffic</td></tr>
          <tr><td><code>src/halo/http_client.rs</code></td><td>Privacy-aware HTTP agent factory</td></tr>
          <tr><td><code>src/halo/p2p_node.rs</code></td><td>libp2p mesh node (7 sub-protocols, DID-derived keypair)</td></tr>
          <tr><td><code>src/halo/p2p_discovery.rs</code></td><td>Agent discovery, dual-signed announcements, Kademlia DHT</td></tr>
          <tr><td><code>src/pod/capability.rs</code></td><td>Capability tokens, AccessMode, AgentClass</td></tr>
          <tr><td><code>src/pod/did_acl_bridge.rs</code></td><td>DID-to-ACL bridge, GrantStore (PUF-keyed)</td></tr>
          <tr><td><code>src/halo/zk_credential.rs</code></td><td>ZK credential circuit (Groth16/BN254)</td></tr>
          <tr><td><code>src/halo/a2a_bridge.rs</code></td><td>Google A2A-compatible JSON-RPC endpoint over DIDComm</td></tr>
          <tr><td><code>src/halo/circuit.rs</code></td><td>Attestation circuit (session Merkle proof)</td></tr>
          <tr><td><code>src/halo/onchain.rs</code></td><td>On-chain attestation posting + query</td></tr>
          <tr><td><code>src/trust/onchain.rs</code></td><td>Trust bridge (PUF attestation + cast proxy)</td></tr>
          <tr><td><code>src/pcn/adapter.rs</code></td><td>PCN compliance witness + conservation</td></tr>
          <tr><td><code>src/halo/identity_ledger.rs</code></td><td>Hash-chained, append-only identity ledger with PQ signatures</td></tr>
        </tbody>
      </table>
    </div>
  `;
}


/* ================================================================
   NUCLEUSDB DOCUMENTATION PAGE
   ================================================================ */

function renderNucleusDBDocs() {
  const content = document.getElementById('content');
  content.innerHTML = `
    <!-- Hero Banner -->
    <div class="gdoc-hero">
      <div class="gdoc-hero-img-wrap">
        <img class="gdoc-hero-img" src="img/nucleus_db_hero.png" alt="NucleusDB"
             onerror="this.style.display='none'">
      </div>
      <div class="gdoc-hero-copy">
        <div class="gdoc-hero-kicker">Agent H.A.L.O. // Data Layer</div>
        <div class="gdoc-hero-title">Memories</div>
        <div class="gdoc-hero-subtitle" style="font-size:0.85rem;opacity:0.7;margin-top:-2px">Powered by NucleusDB</div>
        <div class="gdoc-hero-subtitle">Proof-Carrying Algebraic Database</div>
        <div class="gdoc-hero-sep"></div>
        <div class="gdoc-hero-stat-row">
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">3</div>
            <div class="gdoc-hero-stat-lbl">VC Backends</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">\u221E</div>
            <div class="gdoc-hero-stat-lbl">Seal Chain</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">Sheaf</div>
            <div class="gdoc-hero-stat-lbl">Coherence</div>
          </div>
          <div class="gdoc-hero-stat">
            <div class="gdoc-hero-stat-val">PQ</div>
            <div class="gdoc-hero-stat-lbl">Witnesses</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Tab Bar -->
    <div class="gdoc-tabs">
      <button class="gdoc-tab active" data-tab="ndb-overview" onclick="ndbDocTab('ndb-overview')">F1:OVERVIEW</button>
      <button class="gdoc-tab" data-tab="ndb-technical" onclick="ndbDocTab('ndb-technical')">F2:TECHNICAL</button>
      <button class="gdoc-tab" data-tab="ndb-access" onclick="ndbDocTab('ndb-access')">F3:ACCESS</button>
    </div>

    <div id="ndb-doc-content"></div>
  `;
  ndbDocTab('ndb-overview');
}

window.ndbDocTab = function(tab) {
  document.querySelectorAll('.gdoc-tab').forEach(b => {
    b.classList.toggle('active', b.dataset.tab === tab);
  });
  const el = document.getElementById('ndb-doc-content');
  if (!el) return;

  switch (tab) {
    case 'ndb-overview':
      el.innerHTML = ndbDocOverview();
      hydrateNdbRuntimePanel();
      break;
    case 'ndb-technical': el.innerHTML = ndbDocTechnical(); break;
    case 'ndb-access': el.innerHTML = ndbDocAccess(); break;
  }
};

async function hydrateNdbRuntimePanel() {
  const node = document.getElementById('ndb-runtime-panel');
  if (!node) return;
  node.innerHTML = '<div class="gdoc-text">Loading database status...</div>';
  try {
    const [status, stats] = await Promise.all([
      fetch('/api/nucleusdb/status').then(r => r.ok ? r.json() : null).catch(() => null),
      fetch('/api/nucleusdb/stats').then(r => r.ok ? r.json() : null).catch(() => null),
    ]);
    const keyCount = stats?.key_count || 0;
    const commitCount = stats?.commit_count || 0;
    const dbSize = stats?.db_size_bytes || 0;
    const backend = status?.backend || 'binary_merkle';
    const backendNames = { binary_merkle: 'Binary Merkle', kzg: 'KZG', ipa: 'IPA' };
    const chainOk = status?.exists && commitCount > 0;
    const fmtBytes = (b) => b < 1024 ? b + ' B' : b < 1048576 ? (b/1024).toFixed(1) + ' KB' : (b/1048576).toFixed(1) + ' MB';

    node.innerHTML = `
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--${chainOk ? 'green' : 'amber'}">
          <div class="gdoc-card-head">Seal Chain</div>
          <div class="gdoc-card-body">${chainOk ? 'Healthy \u2713 (Seal #' + commitCount + ')' : status?.exists ? 'Empty \u2014 no commits yet' : 'No database'}</div>
        </div>
        <div class="gdoc-card gdoc-card--purple">
          <div class="gdoc-card-head">Backend</div>
          <div class="gdoc-card-body">${gdocEsc(backendNames[backend] || backend)}</div>
        </div>
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-head">Keys</div>
          <div class="gdoc-card-body">${keyCount.toLocaleString()} keys (${fmtBytes(dbSize)})</div>
        </div>
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-head">Commits</div>
          <div class="gdoc-card-body">${commitCount.toLocaleString()} state transitions</div>
        </div>
      </div>
    `;
  } catch (err) {
    node.innerHTML = `
      <div class="gdoc-card gdoc-card--amber">
        <div class="gdoc-card-head">Database Status</div>
        <div class="gdoc-card-body">Could not load NucleusDB status. ${gdocEsc(String(err && err.message || err))}</div>
      </div>
    `;
  }
}


/* ================================================================
   NUCLEUSDB TAB 1: HIGH-LEVEL OVERVIEW
   ================================================================ */
function ndbDocOverview() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Live Database Status</div>
      <div id="ndb-runtime-panel"></div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">What Is NucleusDB?</div>
      <p class="gdoc-text">
        NucleusDB is a <strong>proof-carrying algebraic database</strong> \u2014 a key-value store
        where every mutation generates cryptographic evidence of correctness and every query can
        be verified against a binding commitment. It is the persistent memory of your agent:
        the place where identity records, session traces, wallet state, configuration, and
        operational data live \u2014 all protected by the same formal framework that governs
        Genesis and Identification.
      </p>
      <p class="gdoc-text">
        Unlike conventional databases that trust their own storage layer, NucleusDB
        <strong>proves</strong> that its data is intact. Each commit extends a monotone seal chain
        (the same one anchored by your Genesis nucleus), each value is bound into a vector
        commitment, and a sheaf coherence layer ensures that different views of the same data
        agree. The formal model is verified in Lean 4 with zero <code>sorry</code>.
      </p>
      <div class="gdoc-pipeline" style="margin:12px 0 16px">
        <div class="gdoc-pipeline-box" style="text-align:center;letter-spacing:1px">
          <strong>State</strong> &nbsp;\u2192&nbsp;
          <strong>\u0394 Delta</strong> &nbsp;\u2192&nbsp;
          <strong>Commit</strong> &nbsp;\u2192&nbsp;
          <strong>Seal</strong> &nbsp;\u2192&nbsp;
          <strong>Witness</strong> &nbsp;\u2192&nbsp;
          <strong>Verify</strong>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Why Does This Matter?</div>
      <div class="gdoc-card-row">
        <div class="gdoc-card gdoc-card--purple">
          <div class="gdoc-card-icon-lg">\u2200</div>
          <div class="gdoc-card-head">Algebraic Structure</div>
          <div class="gdoc-card-body">
            NucleusDB is modeled as a <strong>category</strong>: objects are states (vectors of field
            elements), morphisms are deltas (write operations), and composition is sequential application.
            This means every database operation is a morphism in a mathematically precise sense \u2014
            identity morphisms are empty deltas, composition is associative, and invariant preservation
            is proved by induction over morphism sequences.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green">
          <div class="gdoc-card-icon-lg">\u26D3</div>
          <div class="gdoc-card-head">Tamper Evidence</div>
          <div class="gdoc-card-body">
            Every commit extends a <strong>monotone seal chain</strong>:
            <code>seal\u2099 = SHA-256("NucleusDB.MonotoneSeal|" || seal\u2099\u208B\u2081 || kv_digest\u2099)</code>.
            Monotone extension means every previously committed key-value pair is preserved.
            Deleting or modifying a historical record would require finding a SHA-256 preimage
            \u2014 a 2\u00B9\u00B2\u2078 operation. The genesis seal anchors the entire chain to your agent's
            birth ceremony.
          </div>
        </div>
      </div>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--blue">
          <div class="gdoc-card-icon-lg">\u26A1</div>
          <div class="gdoc-card-head">Vector Commitments</div>
          <div class="gdoc-card-body">
            Three pluggable commitment backends \u2014 <strong>Binary Merkle</strong> (hash-based,
            post-quantum safe), <strong>KZG</strong> (elliptic curve, constant-size proofs),
            and <strong>IPA</strong> (inner product argument, no trusted setup) \u2014 bind each
            state snapshot to a compact digest. Any individual key-value pair can be verified
            against the commitment without downloading the entire database.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--amber">
          <div class="gdoc-card-icon-lg">\u2630</div>
          <div class="gdoc-card-head">Sheaf Coherence</div>
          <div class="gdoc-card-body">
            Multiple subsystems (identity, wallet, sessions) each see a <strong>local section</strong>
            of the database through their own lens. The sheaf coherence layer verifies that these
            local views agree on shared keys \u2014 no contradictions, no silent overwrites. When
            views conflict, the coherence proof records the exact disagreement for resolution.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">How It Works</div>
      <div class="gdoc-pipeline">

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">1</div>
          <div class="gdoc-pipeline-label">State \u2014 The Current Snapshot</div>
        </div>
        <div class="gdoc-pipeline-box">
          The database state is a <strong>vector of field elements</strong> (<code>Vec&lt;u64&gt;</code>).
          Each position is a logical key (mapped through a <code>KeyMap</code> that translates
          string keys to vector indices). This flat vector representation enables efficient
          commitment computation and algebraic operations.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">2</div>
          <div class="gdoc-pipeline-label">Delta \u2014 The Transition</div>
        </div>
        <div class="gdoc-pipeline-box">
          A <strong>delta</strong> is a list of <code>(index, value)</code> writes. Applying a delta
          to a state produces a new state. In category-theoretic terms, deltas are morphisms:
          <code>apply(prev, delta) = next</code>. Composition of deltas is sequential application.
          The empty delta is the identity morphism.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">3</div>
          <div class="gdoc-pipeline-label">Commit \u2014 Bind to Commitment</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--accent">
          The new state vector is committed via the active <strong>vector commitment scheme</strong>
          (Binary Merkle, KZG, or IPA). The commitment is a compact digest that binds the entire
          state: any single value can be opened and verified against it. A <code>CommitCertificate</code>
          bundles the previous state, the delta, the authorization proof, and a constructive witness
          that <code>next = apply(prev, delta)</code>.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">4</div>
          <div class="gdoc-pipeline-label">Seal \u2014 Extend the Chain</div>
        </div>
        <div class="gdoc-pipeline-box gdoc-pipeline-box--green">
          The monotone seal chain extends: the new seal is computed from the previous seal and
          the key-value digest of the current state. <strong>Monotone extension</strong> is verified
          before sealing: all previously committed values must still be present and unchanged.
          Only new keys or zero-to-nonzero transitions are permitted.
          <br><br>
          In <strong>append-only mode</strong>, the database additionally rejects value changes on
          existing keys \u2014 data can only grow, never be modified. This is the strictest integrity
          guarantee: a pure append-only log.
        </div>

        <div class="gdoc-pipeline-arrow">\u25BC</div>

        <div class="gdoc-pipeline-stage">
          <div class="gdoc-pipeline-badge">5</div>
          <div class="gdoc-pipeline-label">Witness \u2014 Dual-Signed Attestation</div>
        </div>
        <div class="gdoc-pipeline-box">
          Each commit is signed by a <strong>witness quorum</strong> using dual signatures:
          <strong>Ed25519</strong> (classical) and <strong>ML-DSA-65</strong> (post-quantum, FIPS 204).
          The witness threshold is configurable (e.g., 2-of-3). Both signature algorithms must
          verify for a witness to count. This ensures tamper evidence even against future
          quantum adversaries.
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Data Types</div>
      <p class="gdoc-text">
        NucleusDB stores more than integers. The <strong>typed value layer</strong> maps rich
        data types onto the underlying algebraic vector:
      </p>
      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">Primitive Types</div>
          <div class="gdoc-card-body">
            <strong>Integer</strong> (u64), <strong>Float</strong> (f64 as bits),
            <strong>Boolean</strong> (0/1), <strong>String</strong> (blob-stored, hash-indexed),
            <strong>Bytes</strong> (arbitrary binary), <strong>JSON</strong> (structured documents).
          </div>
        </div>
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Vector Embeddings</div>
          <div class="gdoc-card-body">
            <strong>Vector</strong> type stores float arrays for semantic similarity search.
            The built-in <strong>vector index</strong> supports cosine, L2, and inner-product metrics.
            Search returns the top-k nearest neighbors by distance. Dimensions are enforced
            after the first insert.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--purple" style="flex:1">
          <div class="gdoc-card-head">SQL Surface</div>
          <div class="gdoc-card-body">
            A SQL query interface translates familiar <code>SELECT</code>, <code>INSERT</code>,
            <code>CREATE TABLE</code> syntax into NucleusDB operations. The SQL layer preserves
            all integrity guarantees \u2014 every SQL write goes through the same commit/seal/witness
            pipeline as direct API calls. Includes <code>SET MODE APPEND_ONLY</code> for locking
            down the database.
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Genesis Integration</div>
      <div class="gdoc-callout">
        <div class="gdoc-callout-icon">\u2693</div>
        <div class="gdoc-callout-body">
          The Genesis entropy ceremony anchors the NucleusDB seal chain. The genesis hash
          becomes the <strong>initial seal</strong> via <code>genesis_seal_with_anchor()</code>,
          binding every future database commit to the agent's birth identity. Tampering with
          the genesis seed would invalidate the entire database history \u2014 the chain of trust
          is unbroken from entropy void to the latest commit.
        </div>
      </div>
    </div>
  `;
}


/* ================================================================
   NUCLEUSDB TAB 2: TECHNICAL DETAILS
   ================================================================ */
function ndbDocTechnical() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Category-Theoretic Architecture</div>
      <p class="gdoc-text">
        NucleusDB is formalized as a category in the mathematical sense. The Lean 4 formalization
        mirrors the Rust implementation with machine-checked proofs of correctness.
      </p>

      <div class="gdoc-module gdoc-module--purple">
        <div class="gdoc-module-header">
          <code>Core/Nucleus.lean</code>
          <span class="gdoc-module-tag">Category of State Transitions</span>
        </div>
        <div class="gdoc-module-body">
          <p>Defines <code>NucleusSystem</code>: a category whose <strong>objects</strong> are states
          (<code>Vec u64</code>) and whose <strong>morphisms</strong> are deltas (write operations).
          The <code>apply</code> function is the action of morphisms on objects.
          Identity morphisms are empty deltas. Composition is sequential application.</p>
          <p>This is the foundational abstraction: every subsystem \u2014 identity, wallet, sessions,
          genesis \u2014 is an instance of this same categorical pattern. Properties proved once
          for <code>NucleusSystem</code> automatically hold for all instances.</p>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>Core/Authorization.lean</code>
          <span class="gdoc-module-tag">Authorized Morphisms</span>
        </div>
        <div class="gdoc-module-body">
          <p><code>AuthorizationPolicy</code> is a predicate on morphisms. An <code>AuthorizedDelta</code>
          bundles a delta with a constructive proof that the policy permits it. This is a
          <strong>typed morphism</strong>: you cannot apply a transition without proving authorization.
          The policy requires explicit authorization, non-empty actor ID, and delta-local constraints.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>authPolicy_rejects_unauthorized</code><br>
              <span class="gdoc-thm-desc">No unauthorized actor can apply any delta. Proved by contradiction.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Core/Certificates.lean</code> + <code>Core/Ledger.lean</code>
          <span class="gdoc-module-tag">Commit Chain as Diagram</span>
        </div>
        <div class="gdoc-module-body">
          <p>A <code>CommitCertificate</code> is a verified morphism: previous state, delta,
          authorization proof, and constructive witness that <code>next = apply(prev, delta)</code>.
          The ledger is a <strong>chain complex</strong> \u2014 a sequence of certificates where each
          entry chains to the previous via hash.</p>
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
          preservation is compositional.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>replay_preserves</code><br>
              <span class="gdoc-thm-desc">If <code>apply</code> preserves an invariant for every delta, then replaying
              any list of deltas preserves it. Inductive proof over the morphism sequence.</span></div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Monotone Seal Chain (Rust Runtime)</div>
      <p class="gdoc-text">
        The Rust implementation in <code>immutable.rs</code> implements the seal chain with
        monotone extension verification. The formal model in <code>Core/SealChain.lean</code>
        provides the specification.
      </p>

      <div class="gdoc-card gdoc-card--purple" style="margin-top:12px">
        <div class="gdoc-card-head">Seal Chain Protocol</div>
        <div class="gdoc-card-body">
<pre style="font-family:var(--font-mono);font-size:0.82rem;line-height:1.5;margin:0;white-space:pre;overflow-x:auto">
Seal Chain Structure
\u2502
\u251C\u2500\u2500 genesis_seal()
\u2502   \u2514\u2500\u2500 SHA-256("NucleusDB.MonotoneSeal|" || [0;32] || [0;32])
\u2502       Base case: empty database, empty digest
\u2502
\u251C\u2500\u2500 genesis_seal_with_anchor(anchor)
\u2502   \u2514\u2500\u2500 SHA-256("NucleusDB.MonotoneSeal|" || [0;32] || SHA-256(anchor))
\u2502       Genesis identity hash anchors the chain
\u2502
\u251C\u2500\u2500 next_seal(prev_seal, kv_digest)
\u2502   \u2514\u2500\u2500 SHA-256("NucleusDB.MonotoneSeal|" || prev_seal || kv_digest)
\u2502       Forward extension: new state digest linked to previous seal
\u2502
\u251C\u2500\u2500 verify_monotone_extension(old_state, new_state)
\u2502   \u251C\u2500\u2500 All nonzero values in old_state present and unchanged in new_state
\u2502   \u251C\u2500\u2500 Zero-to-nonzero transitions permitted (new keys)
\u2502   \u2514\u2500\u2500 Value changes and deletions REJECTED
\u2502
\u2514\u2500\u2500 verify_seal_chain(seals, states)
    \u2514\u2500\u2500 Inductive: each seal must equal next_seal(prev, kv_digest(state))
        Chain-wide integrity: one tampered entry invalidates all successors</pre>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Vector Commitment Backends</div>
      <p class="gdoc-text">
        NucleusDB implements a <strong>pluggable vector commitment</strong> (VC) layer via the
        <code>VC</code> trait: <code>commit</code>, <code>open</code>, <code>verify</code>,
        <code>digest</code>. Three backends are available:
      </p>

      <div class="gdoc-card-row" style="margin-top:10px">
        <div class="gdoc-card gdoc-card--green" style="flex:1">
          <div class="gdoc-card-head">Binary Merkle</div>
          <div class="gdoc-card-body">
            <strong>Hash-based</strong> (SHA-256). Proof size: O(log n). Post-quantum safe by
            construction \u2014 no elliptic curve assumptions. The default backend for production
            deployments. Domain-separated with <code>nucleusdb.vc.binary_merkle.v1</code>.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--blue" style="flex:1">
          <div class="gdoc-card-head">KZG</div>
          <div class="gdoc-card-body">
            <strong>Polynomial commitment</strong> (Kate-Zaverucha-Goldberg). Constant-size proofs
            regardless of vector length. Requires a trusted setup (structured reference string).
            Domain-separated with <code>nucleusdb.vc.kzg.v1</code>. Best for on-chain verification.
          </div>
        </div>
        <div class="gdoc-card gdoc-card--amber" style="flex:1">
          <div class="gdoc-card-head">IPA</div>
          <div class="gdoc-card-body">
            <strong>Inner Product Argument</strong>. Logarithmic proof size, no trusted setup.
            Domain-separated with <code>nucleusdb.vc.ipa.v1</code>. A middle ground: smaller proofs
            than Merkle, no setup ceremony like KZG.
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--purple" style="margin-top:16px">
        <div class="gdoc-module-header">
          <code>commitment/mod.rs</code>
          <span class="gdoc-module-tag">Commitment Policy</span>
        </div>
        <div class="gdoc-module-body">
          <p><code>CommitmentPolicy</code> validates that the selected VC scheme matches the
          security profile and that the polynomial degree bound is sufficient for the state vector
          length. Scheme-profile mismatches (e.g., KZG with a PQ-only profile) are rejected
          at configuration time, not at commit time.</p>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Sheaf Coherence</div>
      <p class="gdoc-text">
        NucleusDB uses <strong>sheaf theory</strong> to manage multi-view consistency.
        Different subsystems (identity, sessions, wallet) each see a <em>local section</em>
        of the global state through their own lens. The sheaf coherence verifier ensures
        these local views agree.
      </p>

      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Sheaf/Coherence.lean</code> + <code>sheaf/coherence.rs</code>
          <span class="gdoc-module-tag">Local Sections + Gluing</span>
        </div>
        <div class="gdoc-module-body">
          <p>A <code>LocalSection</code> is a lens-specific key-value view. <code>build_sheaf_coherence</code>
          checks all local sections for key conflicts \u2014 if two lenses assign different values to
          the same key, the proof records the conflict. <code>SheafCoherenceProof</code> carries a
          boolean coherence flag, a list of conflicting keys, and a digest for efficient comparison.</p>
          <p>The Lean formalization in <code>Sheaf/Coherence.lean</code> proves that coherent
          local sections produce a well-defined global section \u2014 the <strong>gluing axiom</strong>
          of sheaf theory.</p>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>Sheaf/MaterializationFunctor.lean</code>
          <span class="gdoc-module-tag">Naturality</span>
        </div>
        <div class="gdoc-module-body">
          <p>A <code>MaterializationFunctor</code> maps internal states to external key-value
          projections. The <code>naturality</code> law ensures transport-equivalent states
          produce identical projections. This is a natural transformation: internal bookkeeping
          changes are invisible to external observers.</p>
          <div class="gdoc-theorem-list">
            <div class="gdoc-theorem">
              <span class="gdoc-thm-badge">\u2713</span>
              <div><code>materialize_transport_eq</code><br>
              <span class="gdoc-thm-desc">Transport-equivalent states materialize identically. The naturality square commutes.</span></div>
            </div>
          </div>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--amber">
        <div class="gdoc-module-header">
          <code>Sheaf/ChainGluing.lean</code> + <code>Sheaf/ChainTransport.lean</code>
          <span class="gdoc-module-tag">Chain-Level Coherence</span>
        </div>
        <div class="gdoc-module-body">
          <p><code>ChainGluing</code> extends sheaf coherence to entire commit chains: local
          ledger fragments must glue consistently into a global ledger. <code>ChainTransport</code>
          defines structure-preserving maps between chains that preserve the seal relationship
          \u2014 a functor between chain categories.</p>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Dual-Signed Witnesses</div>
      <p class="gdoc-text">
        NucleusDB commits are attested by a configurable witness quorum. Each witness signs with
        <strong>both</strong> classical (Ed25519) and post-quantum (ML-DSA-65) algorithms.
      </p>

      <div class="gdoc-card gdoc-card--purple" style="margin-top:12px">
        <div class="gdoc-card-head">Witness Architecture</div>
        <div class="gdoc-card-body">
<pre style="font-family:var(--font-mono);font-size:0.82rem;line-height:1.5;margin:0;white-space:pre;overflow-x:auto">
WitnessConfig
\u2502
\u251C\u2500\u2500 threshold: usize          (e.g., 2-of-3 quorum)
\u251C\u2500\u2500 witnesses: Vec&lt;String&gt;    (named witness identities)
\u251C\u2500\u2500 signing_algorithm: Ed25519 | MlDsa65
\u2502
\u251C\u2500\u2500 Ed25519 keys             (classical, 256-bit)
\u2502   \u251C\u2500\u2500 signing_keys:   per-witness
\u2502   \u2514\u2500\u2500 verifying_keys: per-witness
\u2502
\u2514\u2500\u2500 ML-DSA-65 keys           (post-quantum, FIPS 204)
    \u251C\u2500\u2500 signing_keys:   per-witness
    \u2514\u2500\u2500 verifying_keys: per-witness

Attestation:
  commit_hash = SHA-512(state || delta || seal)
  sig_ed25519 = Ed25519.sign(sk, commit_hash)
  sig_mldsa65 = MlDsa65.sign(sk, commit_hash)
  BOTH must verify for a witness attestation to count.</pre>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Security Formalization</div>
      <p class="gdoc-text">
        The security model is formalized in Lean 4 under <code>lean/NucleusDB/Security/</code>:
      </p>
      <div class="gdoc-module gdoc-module--green">
        <div class="gdoc-module-header">
          <code>Security/Assumptions.lean</code> + <code>Security/Reductions.lean</code>
          <span class="gdoc-module-tag">Cryptographic Assumptions</span>
        </div>
        <div class="gdoc-module-body">
          <p>Formal statement of hash-function collision resistance, binding properties of
          vector commitments, and computational hardness assumptions. Reduction theorems
          prove that breaking NucleusDB's integrity requires breaking these underlying primitives.</p>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--blue">
        <div class="gdoc-module-header">
          <code>Security/CertificateIntegrity.lean</code>
          <span class="gdoc-module-tag">End-to-End Integrity</span>
        </div>
        <div class="gdoc-module-body">
          <p>Proves that the commit certificate chain provides end-to-end integrity: a valid
          certificate chain implies that the current state is the unique result of applying
          every authorized delta in sequence from the genesis state.</p>
        </div>
      </div>

      <div class="gdoc-module gdoc-module--amber">
        <div class="gdoc-module-header">
          <code>Security/Refinement.lean</code> + <code>Security/ProofGateSpec.lean</code>
          <span class="gdoc-module-tag">Refinement + Access Control</span>
        </div>
        <div class="gdoc-module-body">
          <p>The refinement module proves that the Rust runtime is a valid refinement of the
          Lean specification \u2014 every observable behavior of the implementation is permitted
          by the spec. The proof gate enforces that certain operations (e.g., seal verification,
          witness validation) must succeed before state transitions are accepted.</p>
        </div>
      </div>
    </div>
  `;
}


/* ================================================================
   NUCLEUSDB TAB 3: ACCESS
   ================================================================ */
function ndbDocAccess() {
  return `
    <div class="gdoc-section">
      <div class="gdoc-section-title">Dashboard Interface</div>
      <p class="gdoc-text">
        The <a href="#/nucleusdb" style="color:#c49bff">NucleusDB interactive page</a> provides
        a full management interface with live data browsing, SQL queries, vector search,
        seal verification, and access control management. Navigate there via the sidebar.
      </p>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">CLI Commands</div>
      <p class="gdoc-text">
        The <code>nucleusdb</code> CLI provides direct terminal access to the database:
      </p>
      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Interactive REPL</div>
        <pre class="gdoc-pre gdoc-code">$ nucleusdb repl
NucleusDB v0.3.0 \u2014 Proof-Carrying Algebraic Database
Type .help for commands, .quit to exit.

nucleusdb> SET key1 42
OK (commit #1, seal extended)

nucleusdb> GET key1
42

nucleusdb> VERIFY key1
\u2713 Key "key1" verified against Binary Merkle commitment.
  Value: 42
  Root:  a3f7c9...d42e
  Proof: [sibling hashes...]

nucleusdb> SET MODE APPEND_ONLY
OK \u2014 database locked to append-only mode.</pre>
      </div>
      <div class="gdoc-code-block">
        <div class="gdoc-code-title">SQL surface</div>
        <pre class="gdoc-pre gdoc-code">$ nucleusdb sql "SELECT * FROM kv WHERE key LIKE 'session.%'"
+------------------+--------+-------+
| key              | type   | value |
+------------------+--------+-------+
| session.count    | int    | 14    |
| session.last_ts  | int    | 1709  |
+------------------+--------+-------+

$ nucleusdb sql "INSERT INTO kv (key, type, value) VALUES ('memo', 'string', 'hello')"
OK (commit #15, seal extended)</pre>
      </div>
      <div class="gdoc-code-block">
        <div class="gdoc-code-title">Export and verify</div>
        <pre class="gdoc-pre gdoc-code">$ nucleusdb export --format json > backup.json
Exported 42 keys to JSON.

$ nucleusdb verify --full
Verifying seal chain... 15 seals OK.
Verifying monotone extension... OK.
Verifying vector commitments... 15/15 verified.
Verifying witness signatures... 15/15 dual-signed.
\u2713 Database integrity verified.</pre>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">HTTP API</div>
      <p class="gdoc-text">
        The NucleusDB server exposes a REST API (default port 8088, internal to the container):
      </p>
      <div class="gdoc-tool-list">
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">GET /nucleusdb/status</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--read">Read</div>
          </div>
          <div class="gdoc-tool-desc">
            Returns database existence, backend type, and chain health.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">GET /nucleusdb/stats</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--read">Read</div>
          </div>
          <div class="gdoc-tool-desc">
            Key count, commit count, database size, type distribution.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">POST /nucleusdb/edit</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--guard">Write</div>
          </div>
          <div class="gdoc-tool-desc">
            Create or update a key-value pair. Goes through full commit/seal/witness pipeline.
            Body: <code>{"key": "...", "type": "integer|string|json|vector", "value": ...}</code>
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">GET /nucleusdb/verify/:key</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--read">Verify</div>
          </div>
          <div class="gdoc-tool-desc">
            Verifies a single key against the vector commitment. Returns proof and verification status.
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">POST /nucleusdb/sql</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--guard">SQL</div>
          </div>
          <div class="gdoc-tool-desc">
            Execute a SQL query. Read queries return results; write queries go through the commit pipeline.
            Body: <code>{"query": "SELECT ..."}</code>
          </div>
        </div>
        <div class="gdoc-tool">
          <div class="gdoc-tool-header">
            <div class="gdoc-tool-name">POST /nucleusdb/vector-search</div>
            <div class="gdoc-tool-badge gdoc-tool-badge--read">Search</div>
          </div>
          <div class="gdoc-tool-desc">
            Similarity search over vector embeddings. Returns top-k nearest neighbors.
            Body: <code>{"query": [0.1, 0.2, ...], "k": 5, "metric": "cosine"}</code>
          </div>
        </div>
      </div>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Rust Source Map</div>
      <table class="gdoc-source-table">
        <thead>
          <tr><th>File</th><th>Role</th></tr>
        </thead>
        <tbody>
          <tr><td><code>src/state.rs</code></td><td>State + Delta: core algebraic types (Vec&lt;u64&gt; + writes)</td></tr>
          <tr><td><code>src/immutable.rs</code></td><td>Monotone seal chain: extension, sealing, verification</td></tr>
          <tr><td><code>src/vc/mod.rs</code></td><td>Vector commitment trait (commit/open/verify/digest)</td></tr>
          <tr><td><code>src/vc/binary_merkle.rs</code></td><td>Binary Merkle tree VC backend</td></tr>
          <tr><td><code>src/vc/kzg.rs</code></td><td>KZG polynomial commitment backend</td></tr>
          <tr><td><code>src/vc/ipa.rs</code></td><td>Inner Product Argument backend</td></tr>
          <tr><td><code>src/commitment/mod.rs</code></td><td>Commitment policy validation, scheme registry</td></tr>
          <tr><td><code>src/sheaf/coherence.rs</code></td><td>Sheaf coherence: local sections, conflict detection, gluing</td></tr>
          <tr><td><code>src/witness.rs</code></td><td>Dual-signed witnesses (Ed25519 + ML-DSA-65), quorum</td></tr>
          <tr><td><code>src/materialize.rs</code></td><td>State materialization (functorial projection)</td></tr>
          <tr><td><code>src/keymap.rs</code></td><td>String key \u2194 vector index mapping</td></tr>
          <tr><td><code>src/typed_value.rs</code></td><td>Rich type layer (int, float, string, json, vector, bytes)</td></tr>
          <tr><td><code>src/vector_index.rs</code></td><td>Vector similarity search (cosine, L2, inner product)</td></tr>
          <tr><td><code>src/sql/mod.rs</code></td><td>SQL parser and executor</td></tr>
          <tr><td><code>src/persistence.rs</code></td><td>Disk persistence (state snapshots, blob store)</td></tr>
          <tr><td><code>src/security.rs</code></td><td>Security profiles, VC profile selection</td></tr>
          <tr><td><code>src/audit.rs</code></td><td>Audit log, commit history</td></tr>
          <tr><td><code>src/api.rs</code></td><td>HTTP API routes (/nucleusdb/*)</td></tr>
        </tbody>
      </table>
    </div>

    <div class="gdoc-section">
      <div class="gdoc-section-title">Lean Formalization Map</div>
      <table class="gdoc-source-table">
        <thead>
          <tr><th>Module</th><th>Role</th></tr>
        </thead>
        <tbody>
          <tr><td><code>Core/Nucleus.lean</code></td><td>NucleusSystem category (objects, morphisms, composition)</td></tr>
          <tr><td><code>Core/Authorization.lean</code></td><td>Authorization policy, typed morphisms</td></tr>
          <tr><td><code>Core/Certificates.lean</code></td><td>CommitCertificate, constructive witness</td></tr>
          <tr><td><code>Core/Ledger.lean</code></td><td>Chain complex, inductive ledger verification</td></tr>
          <tr><td><code>Core/Invariants.lean</code></td><td>Invariant preservation, replay theorem</td></tr>
          <tr><td><code>Core/SealChain.lean</code></td><td>Seal chain formal model</td></tr>
          <tr><td><code>Core/NaturalTransformation.lean</code></td><td>Natural transformations between functors</td></tr>
          <tr><td><code>Sheaf/Coherence.lean</code></td><td>Sheaf gluing axiom, local section agreement</td></tr>
          <tr><td><code>Sheaf/MaterializationFunctor.lean</code></td><td>Functorial projection, naturality law</td></tr>
          <tr><td><code>Sheaf/ChainGluing.lean</code></td><td>Chain-level sheaf coherence</td></tr>
          <tr><td><code>Sheaf/ChainTransport.lean</code></td><td>Structure-preserving maps between chains</td></tr>
          <tr><td><code>Security/Assumptions.lean</code></td><td>Cryptographic hardness assumptions</td></tr>
          <tr><td><code>Security/Reductions.lean</code></td><td>Reduction theorems (integrity \u2192 primitives)</td></tr>
          <tr><td><code>Security/CertificateIntegrity.lean</code></td><td>End-to-end certificate chain integrity</td></tr>
          <tr><td><code>Security/Refinement.lean</code></td><td>Runtime-to-spec refinement proof</td></tr>
          <tr><td><code>Security/ProofGateSpec.lean</code></td><td>Mandatory proof gate for state transitions</td></tr>
        </tbody>
      </table>
    </div>
  `;
}
