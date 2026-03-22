/* ═══════════════════════════════════════════════════════════════
   Proof Builder — Interactive Theorem Proving via Multiway Proof Trees
   Part of Agent H.A.L.O. Dashboard

   Client-side simulation engine with pre-computed proof trees.
   API contract is identical to a real Lean server — swap-in transparent.
   Uses D3.js for tree layout + Canvas for rendering.
   ═══════════════════════════════════════════════════════════════ */
'use strict';
(function () {

  // ═══════════════════════════════════════════════════════════════
  // §1  Constants
  // ═══════════════════════════════════════════════════════════════
  var PG_VERSION = '2.1'; // cache-bust marker
  var NODE_W = 240, NODE_H = 50, NODE_R = 6;
  var LEVEL_H = 110, SIB_GAP = 30;
  var EDGE_LABEL_SIZE = 10;

  var STATUS_STYLE = {
    root:     { bg: '#0e1e0e', border: '#78ff74', text: '#78ff74', glow: 'rgba(120,255,116,0.15)' },
    open:     { bg: '#081828', border: '#00aaff', text: '#88ccff', glow: 'rgba(0,170,255,0.12)' },
    active:   { bg: '#082828', border: '#00ffff', text: '#aaffff', glow: 'rgba(0,255,255,0.20)' },
    solved:   { bg: '#082808', border: '#39ff14', text: '#78ff74', glow: 'rgba(57,255,20,0.15)' },
    sorry:    { bg: '#282808', border: '#ffaa00', text: '#ffcc44', glow: 'rgba(255,170,0,0.12)' },
    failed:   { bg: '#280808', border: '#ff3030', text: '#ff6666', glow: 'rgba(255,48,48,0.12)' },
    inactive: { bg: '#111',    border: '#333',    text: '#555',    glow: 'transparent' },
  };

  var TACTIC_DESC = {
    'intro':         'Introduce hypothesis',
    'exact':         'Exact proof term',
    'apply':         'Apply lemma / hyp',
    'simp':          'Simplify',
    'rfl':           'Reflexivity',
    'ring':          'Ring solver',
    'omega':         'Linear arithmetic',
    'constructor':   'Split conjunction',
    'cases':         'Case analysis',
    'induction':     'Induction',
    'assumption':    'Use hypothesis',
    'tauto':         'Propositional solver',
    'by_contra':     'By contradiction',
    'push_neg':      'Push negations',
    'trivial':       'Trivial',
    'linarith':      'Linear arith',
    'norm_num':      'Numeric normalization',
    'left':          'Choose left case',
    'right':         'Choose right case',
    'contradiction': 'Contradiction',
    'decide':        'Decidable',
    'ext':           'Extensionality',
    'funext':        'Function ext',
    'obtain':        'Destructure hyp',
  };

  // ═══════════════════════════════════════════════════════════════
  // §2  Theorem Library (Simulation)
  // ═══════════════════════════════════════════════════════════════
  //
  // Each theorem: { id, name, category, statement, difficulty, tags, hint,
  //   rootGoal: goalKey,
  //   goals: { goalKey: { display, hyps:[{n,t}], tactics:{tacStr: goalKeys[]|'error:msg'}, suggested:[] } }
  // }
  // tactics[tac] = [] means goal solved; = ['g1','g2'] means new subgoals; = 'error:...' means failure

  var LIBRARY = [
    // ── Tutorial ──────────────────────────────────────────
    {
      id: 'tut_true', name: 'True', category: 'Tutorial',
      statement: 'theorem true_is_true : True', difficulty: 1,
      tags: ['logic'], hint: 'The trivial tactic handles this.',
      rootGoal: 'r', goals: {
        r: { display: '⊢ True', hyps: [],
          tactics: { 'trivial': [], 'exact True.intro': [], 'decide': [] },
          suggested: ['trivial', 'exact True.intro'] }
      }
    },
    {
      id: 'tut_id', name: 'P → P', category: 'Tutorial',
      statement: 'theorem id_impl (P : Prop) : P → P', difficulty: 1,
      tags: ['logic', 'intro'], hint: 'Introduce the hypothesis, then use it.',
      rootGoal: 'r', goals: {
        r: { display: 'P : Prop ⊢ P → P', hyps: [],
          tactics: { 'intro h': ['g1'], 'tauto': [], 'simp': [] },
          suggested: ['intro h', 'tauto', 'simp'] },
        g1: { display: 'P : Prop, h : P ⊢ P', hyps: [{ n: 'h', t: 'P' }],
          tactics: { 'exact h': [], 'assumption': [] },
          suggested: ['exact h', 'assumption'] }
      }
    },
    {
      id: 'tut_refl', name: 'n = n', category: 'Tutorial',
      statement: 'theorem self_eq (n : Nat) : n = n', difficulty: 1,
      tags: ['equality'], hint: 'Reflexivity closes this immediately.',
      rootGoal: 'r', goals: {
        r: { display: 'n : Nat ⊢ n = n', hyps: [{ n: 'n', t: 'Nat' }],
          tactics: { 'rfl': [], 'simp': [], 'omega': [] },
          suggested: ['rfl', 'simp'] }
      }
    },
    {
      id: 'tut_const', name: 'P → Q → P', category: 'Tutorial',
      statement: 'theorem const_fn (P Q : Prop) : P → Q → P', difficulty: 1,
      tags: ['logic', 'intro'], hint: 'Introduce both hypotheses.',
      rootGoal: 'r', goals: {
        r: { display: 'P Q : Prop ⊢ P → Q → P', hyps: [],
          tactics: { 'intro hp': ['g1'], 'tauto': [] },
          suggested: ['intro hp', 'tauto'] },
        g1: { display: 'P Q : Prop, hp : P ⊢ Q → P', hyps: [{ n: 'hp', t: 'P' }],
          tactics: { 'intro _': ['g2'], 'tauto': [] },
          suggested: ['intro _', 'tauto'] },
        g2: { display: 'P Q : Prop, hp : P, _ : Q ⊢ P', hyps: [{ n: 'hp', t: 'P' }, { n: '_', t: 'Q' }],
          tactics: { 'exact hp': [], 'assumption': [] },
          suggested: ['exact hp', 'assumption'] }
      }
    },
    {
      id: 'tut_add_zero', name: 'n + 0 = n', category: 'Tutorial',
      statement: 'theorem add_zero (n : Nat) : n + 0 = n', difficulty: 1,
      tags: ['arithmetic'], hint: 'Simplification handles this definitionally.',
      rootGoal: 'r', goals: {
        r: { display: 'n : Nat ⊢ n + 0 = n', hyps: [{ n: 'n', t: 'Nat' }],
          tactics: { 'simp': [], 'omega': [], 'rfl': 'error:rfl failed — Nat.add is not definitionally equal here' },
          suggested: ['simp', 'omega'] }
      }
    },

    // ── Logic ─────────────────────────────────────────────
    {
      id: 'log_and_comm', name: 'P ∧ Q → Q ∧ P', category: 'Logic',
      statement: 'theorem and_comm_impl (P Q : Prop) : P ∧ Q → Q ∧ P', difficulty: 2,
      tags: ['logic', 'conjunction'], hint: 'Introduce, then split with constructor.',
      rootGoal: 'r', goals: {
        r: { display: 'P Q : Prop ⊢ P ∧ Q → Q ∧ P', hyps: [],
          tactics: { 'intro h': ['g1'], 'tauto': [], 'intro ⟨hp, hq⟩': ['g1b'] },
          suggested: ['intro h', 'intro ⟨hp, hq⟩', 'tauto'] },
        g1: { display: 'h : P ∧ Q ⊢ Q ∧ P', hyps: [{ n: 'h', t: 'P ∧ Q' }],
          tactics: { 'exact ⟨h.2, h.1⟩': [], 'constructor': ['g2', 'g3'], 'obtain ⟨hp, hq⟩ := h': ['g1b'] },
          suggested: ['exact ⟨h.2, h.1⟩', 'constructor', 'obtain ⟨hp, hq⟩ := h'] },
        g1b: { display: 'hp : P, hq : Q ⊢ Q ∧ P', hyps: [{ n: 'hp', t: 'P' }, { n: 'hq', t: 'Q' }],
          tactics: { 'exact ⟨hq, hp⟩': [], 'constructor': ['g2b', 'g3b'] },
          suggested: ['exact ⟨hq, hp⟩', 'constructor'] },
        g2: { display: 'h : P ∧ Q ⊢ Q', hyps: [{ n: 'h', t: 'P ∧ Q' }],
          tactics: { 'exact h.2': [], 'exact h.right': [] },
          suggested: ['exact h.2', 'exact h.right'] },
        g3: { display: 'h : P ∧ Q ⊢ P', hyps: [{ n: 'h', t: 'P ∧ Q' }],
          tactics: { 'exact h.1': [], 'exact h.left': [] },
          suggested: ['exact h.1', 'exact h.left'] },
        g2b: { display: 'hp : P, hq : Q ⊢ Q', hyps: [{ n: 'hp', t: 'P' }, { n: 'hq', t: 'Q' }],
          tactics: { 'exact hq': [], 'assumption': [] },
          suggested: ['exact hq', 'assumption'] },
        g3b: { display: 'hp : P, hq : Q ⊢ P', hyps: [{ n: 'hp', t: 'P' }, { n: 'hq', t: 'Q' }],
          tactics: { 'exact hp': [], 'assumption': [] },
          suggested: ['exact hp', 'assumption'] },
      }
    },
    {
      id: 'log_or_comm', name: 'P ∨ Q → Q ∨ P', category: 'Logic',
      statement: 'theorem or_comm_impl (P Q : Prop) : P ∨ Q → Q ∨ P', difficulty: 2,
      tags: ['logic', 'disjunction'], hint: 'Introduce, then case split.',
      rootGoal: 'r', goals: {
        r: { display: 'P Q : Prop ⊢ P ∨ Q → Q ∨ P', hyps: [],
          tactics: { 'intro h': ['g1'], 'tauto': [] },
          suggested: ['intro h', 'tauto'] },
        g1: { display: 'h : P ∨ Q ⊢ Q ∨ P', hyps: [{ n: 'h', t: 'P ∨ Q' }],
          tactics: { 'cases h with | inl hp => exact Or.inr hp | inr hq => exact Or.inl hq': [],
            'cases h': ['g2', 'g3'] },
          suggested: ['cases h'] },
        g2: { display: 'case inl\nhp : P ⊢ Q ∨ P', hyps: [{ n: 'hp', t: 'P' }],
          tactics: { 'right': ['g2a'], 'exact Or.inr hp': [] },
          suggested: ['right', 'exact Or.inr hp'] },
        g2a: { display: 'hp : P ⊢ P', hyps: [{ n: 'hp', t: 'P' }],
          tactics: { 'exact hp': [], 'assumption': [] },
          suggested: ['exact hp', 'assumption'] },
        g3: { display: 'case inr\nhq : Q ⊢ Q ∨ P', hyps: [{ n: 'hq', t: 'Q' }],
          tactics: { 'left': ['g3a'], 'exact Or.inl hq': [] },
          suggested: ['left', 'exact Or.inl hq'] },
        g3a: { display: 'hq : Q ⊢ Q', hyps: [{ n: 'hq', t: 'Q' }],
          tactics: { 'exact hq': [], 'assumption': [] },
          suggested: ['exact hq', 'assumption'] },
      }
    },
    {
      id: 'log_compose', name: '(P→Q)→(Q→R)→P→R', category: 'Logic',
      statement: 'theorem compose (P Q R : Prop) : (P → Q) → (Q → R) → P → R', difficulty: 2,
      tags: ['logic', 'composition'], hint: 'Introduce all three hypotheses, then apply.',
      rootGoal: 'r', goals: {
        r: { display: 'P Q R : Prop ⊢ (P → Q) → (Q → R) → P → R', hyps: [],
          tactics: { 'intro hpq': ['g1'], 'tauto': [] },
          suggested: ['intro hpq', 'tauto'] },
        g1: { display: 'hpq : P → Q ⊢ (Q → R) → P → R', hyps: [{ n: 'hpq', t: 'P → Q' }],
          tactics: { 'intro hqr': ['g2'] },
          suggested: ['intro hqr'] },
        g2: { display: 'hpq : P → Q, hqr : Q → R ⊢ P → R',
          hyps: [{ n: 'hpq', t: 'P → Q' }, { n: 'hqr', t: 'Q → R' }],
          tactics: { 'intro hp': ['g3'], 'exact fun hp => hqr (hpq hp)': [] },
          suggested: ['intro hp', 'exact fun hp => hqr (hpq hp)'] },
        g3: { display: 'hpq : P → Q, hqr : Q → R, hp : P ⊢ R',
          hyps: [{ n: 'hpq', t: 'P → Q' }, { n: 'hqr', t: 'Q → R' }, { n: 'hp', t: 'P' }],
          tactics: { 'exact hqr (hpq hp)': [], 'apply hqr': ['g4'] },
          suggested: ['exact hqr (hpq hp)', 'apply hqr'] },
        g4: { display: 'hpq : P → Q, hp : P ⊢ Q',
          hyps: [{ n: 'hpq', t: 'P → Q' }, { n: 'hp', t: 'P' }],
          tactics: { 'exact hpq hp': [], 'apply hpq': ['g5'] },
          suggested: ['exact hpq hp', 'apply hpq'] },
        g5: { display: 'hp : P ⊢ P', hyps: [{ n: 'hp', t: 'P' }],
          tactics: { 'exact hp': [], 'assumption': [] },
          suggested: ['exact hp'] },
      }
    },
    {
      id: 'log_dne_intro', name: 'P → ¬¬P', category: 'Logic',
      statement: 'theorem dne_intro (P : Prop) : P → ¬¬P', difficulty: 2,
      tags: ['logic', 'negation'], hint: 'Introduce P and ¬P, then derive contradiction.',
      rootGoal: 'r', goals: {
        r: { display: 'P : Prop ⊢ P → ¬¬P', hyps: [],
          tactics: { 'intro hp': ['g1'], 'tauto': [] },
          suggested: ['intro hp', 'tauto'] },
        g1: { display: 'hp : P ⊢ ¬¬P', hyps: [{ n: 'hp', t: 'P' }],
          tactics: { 'intro hnp': ['g2'], 'exact fun hnp => hnp hp': [] },
          suggested: ['intro hnp', 'exact fun hnp => hnp hp'] },
        g2: { display: 'hp : P, hnp : ¬P ⊢ False', hyps: [{ n: 'hp', t: 'P' }, { n: 'hnp', t: '¬P' }],
          tactics: { 'exact hnp hp': [], 'apply hnp': ['g3'], 'contradiction': [] },
          suggested: ['exact hnp hp', 'contradiction', 'apply hnp'] },
        g3: { display: 'hp : P ⊢ P', hyps: [{ n: 'hp', t: 'P' }],
          tactics: { 'exact hp': [], 'assumption': [] },
          suggested: ['exact hp'] },
      }
    },
    {
      id: 'log_iff_refl', name: 'P ↔ P', category: 'Logic',
      statement: 'theorem iff_refl_impl (P : Prop) : P ↔ P', difficulty: 2,
      tags: ['logic', 'iff'], hint: 'Use constructor to split into two directions.',
      rootGoal: 'r', goals: {
        r: { display: 'P : Prop ⊢ P ↔ P', hyps: [],
          tactics: { 'constructor': ['g1', 'g2'], 'exact Iff.rfl': [], 'tauto': [] },
          suggested: ['constructor', 'exact Iff.rfl', 'tauto'] },
        g1: { display: 'P : Prop ⊢ P → P', hyps: [],
          tactics: { 'intro h': ['g1a'], 'exact id': [], 'tauto': [] },
          suggested: ['intro h', 'exact id'] },
        g1a: { display: 'h : P ⊢ P', hyps: [{ n: 'h', t: 'P' }],
          tactics: { 'exact h': [], 'assumption': [] },
          suggested: ['exact h'] },
        g2: { display: 'P : Prop ⊢ P → P', hyps: [],
          tactics: { 'intro h': ['g2a'], 'exact id': [], 'tauto': [] },
          suggested: ['intro h', 'exact id'] },
        g2a: { display: 'h : P ⊢ P', hyps: [{ n: 'h', t: 'P' }],
          tactics: { 'exact h': [], 'assumption': [] },
          suggested: ['exact h'] },
      }
    },

    // ── Arithmetic ────────────────────────────────────────
    {
      id: 'arith_zero_add', name: '0 + n = n', category: 'Arithmetic',
      statement: 'theorem zero_add (n : Nat) : 0 + n = n', difficulty: 2,
      tags: ['arithmetic', 'induction'], hint: 'Try induction on n.',
      rootGoal: 'r', goals: {
        r: { display: 'n : Nat ⊢ 0 + n = n', hyps: [{ n: 'n', t: 'Nat' }],
          tactics: { 'induction n with | zero => simp | succ k ih => simp [ih]': [],
            'induction n': ['g_base', 'g_step'], 'simp': [], 'omega': [] },
          suggested: ['induction n', 'simp', 'omega'] },
        g_base: { display: 'case zero\n⊢ 0 + 0 = 0', hyps: [],
          tactics: { 'simp': [], 'rfl': 'error:rfl failed', 'norm_num': [] },
          suggested: ['simp', 'norm_num'] },
        g_step: { display: 'case succ\nk : Nat, ih : 0 + k = k\n⊢ 0 + (k + 1) = k + 1',
          hyps: [{ n: 'k', t: 'Nat' }, { n: 'ih', t: '0 + k = k' }],
          tactics: { 'simp [ih]': [], 'simp': [], 'omega': [] },
          suggested: ['simp [ih]', 'simp', 'omega'] },
      }
    },
    {
      id: 'arith_add_comm', name: 'n + m = m + n', category: 'Arithmetic',
      statement: 'theorem add_comm_nat (n m : Nat) : n + m = m + n', difficulty: 3,
      tags: ['arithmetic', 'commutativity'], hint: 'omega handles linear arithmetic.',
      rootGoal: 'r', goals: {
        r: { display: 'n m : Nat ⊢ n + m = m + n', hyps: [{ n: 'n', t: 'Nat' }, { n: 'm', t: 'Nat' }],
          tactics: { 'omega': [], 'ring': 'error:ring failed — Nat is not a ring',
            'induction n': ['g_base', 'g_step'] },
          suggested: ['omega', 'induction n'] },
        g_base: { display: 'case zero\nm : Nat\n⊢ 0 + m = m + 0', hyps: [{ n: 'm', t: 'Nat' }],
          tactics: { 'simp': [], 'omega': [] },
          suggested: ['simp', 'omega'] },
        g_step: { display: 'case succ\nk : Nat, ih : k + m = m + k\n⊢ k + 1 + m = m + (k + 1)',
          hyps: [{ n: 'k', t: 'Nat' }, { n: 'ih', t: 'k + m = m + k' }],
          tactics: { 'omega': [], 'simp [ih]': 'error:simp made no progress' },
          suggested: ['omega'] },
      }
    },
  ];

  // Build category groups
  function getCategories() {
    var cats = {};
    LIBRARY.forEach(function (t) {
      if (!cats[t.category]) cats[t.category] = [];
      cats[t.category].push(t);
    });
    return cats;
  }

  // ═══════════════════════════════════════════════════════════════
  // §2b  Server Mode (Pantograph)
  // ═══════════════════════════════════════════════════════════════

  var serverMode = false;   // true when Lean proof server is connected

  function checkServerStatus() {
    fetch('/api/explorer/status').then(function (r) { return r.json(); }).then(function (data) {
      serverMode = !!(data && data.lean_server);
      var dot = document.querySelector('.pg-status-dot');
      if (dot) {
        var statusText = dot.parentElement && dot.parentElement.lastChild;
        if (!session) {
          dot.className = 'pg-status-dot ' + (serverMode ? 'active' : 'simulated');
          if (statusText && statusText.nodeType === 3) {
            statusText.textContent = serverMode ? ' Connected' : ' Simulation';
          }
        }
      }
      var welcomeMode = document.querySelector('.pg-welcome-mode');
      if (welcomeMode) {
        welcomeMode.textContent = serverMode
          ? 'Server mode — connected to Lean proof server'
          : 'Simulation mode — pre-computed proof trees for ' + LIBRARY.length + ' theorems';
      }
    }).catch(function () { serverMode = false; });
  }

  // ═══════════════════════════════════════════════════════════════
  // §3  Simulation Engine
  // ═══════════════════════════════════════════════════════════════

  var session = null;   // current game session
  var nodeSeq = 0;

  function newSession(theorem) {
    nodeSeq = 0;
    var rootId = mkId();
    var nodes = {};
    nodes[rootId] = {
      id: rootId, goalKey: theorem.rootGoal, status: 'open',
      parentId: null, parentTactic: null,
      x: 0, y: 0, animT: 0,
    };
    session = {
      theorem: theorem,
      nodes: nodes,
      edges: [],          // { from, to, tactic, group, status:'applied'|'failed' }
      rootId: rootId,
      selectedId: rootId,
      solvedSet: {},       // nodeId -> true
      tacticsApplied: 0,
      branchesExplored: 0,
    };
    return session;
  }

  function mkId() { return 'n' + (nodeSeq++); }

  function getGoalDef(node) {
    // Server-mode nodes carry their own goal info
    if (node.serverGoal) return node.serverGoal;
    return session.theorem.goals[node.goalKey] || null;
  }

  function applyTactic(nodeId, tacticStr) {
    var node = session.nodes[nodeId];
    if (!node || node.status === 'inactive') return null;
    var goalDef = getGoalDef(node);
    if (!goalDef) return { error: 'No goal definition (simulation limit)' };

    var tacResult = goalDef.tactics[tacticStr];
    session.tacticsApplied++;

    // Unknown tactic
    if (tacResult === undefined) {
      session.branchesExplored++;
      var failId = mkId();
      session.nodes[failId] = {
        id: failId, goalKey: null, status: 'failed',
        parentId: nodeId, parentTactic: tacticStr,
        x: 0, y: 0, animT: 0,
        errorMsg: 'Tactic failed in simulation — not in pre-computed tree',
      };
      session.edges.push({ from: nodeId, to: failId, tactic: tacticStr, group: failId, status: 'failed' });
      return { error: 'Tactic not applicable (simulation)', nodeId: failId };
    }

    // Error string
    if (typeof tacResult === 'string' && tacResult.startsWith('error:')) {
      session.branchesExplored++;
      var fId = mkId();
      session.nodes[fId] = {
        id: fId, goalKey: null, status: 'failed',
        parentId: nodeId, parentTactic: tacticStr,
        x: 0, y: 0, animT: 0,
        errorMsg: tacResult.slice(6),
      };
      session.edges.push({ from: nodeId, to: fId, tactic: tacticStr, group: fId, status: 'failed' });
      return { error: tacResult.slice(6), nodeId: fId };
    }

    // Solved (empty goals array)
    if (Array.isArray(tacResult) && tacResult.length === 0) {
      node.status = 'solved';
      session.solvedSet[nodeId] = true;
      session.edges.push({ from: nodeId, to: null, tactic: tacticStr, group: nodeId, status: 'applied' });
      propagateSolved(node.parentId);
      return { solved: true, newGoals: [] };
    }

    // New subgoals
    session.branchesExplored++;
    var groupId = mkId();
    var newNodes = [];
    tacResult.forEach(function (goalKey) {
      var nid = mkId();
      session.nodes[nid] = {
        id: nid, goalKey: goalKey, status: 'open',
        parentId: nodeId, parentTactic: tacticStr,
        x: 0, y: 0, animT: 0,
      };
      session.edges.push({ from: nodeId, to: nid, tactic: tacticStr, group: groupId, status: 'applied' });
      newNodes.push(nid);
    });
    // Select first new goal
    session.selectedId = newNodes[0];
    return { solved: false, newGoals: newNodes };
  }

  function propagateSolved(nodeId) {
    if (!nodeId) return;
    var node = session.nodes[nodeId];
    if (!node || node.status === 'solved') return;
    // Check if ANY tactic group has all its children solved (Or-semantics
    // across strategies: one complete proof branch suffices).
    var childEdges = session.edges.filter(function (e) {
      return e.from === nodeId && e.status === 'applied' && e.to !== null;
    });
    if (childEdges.length === 0) return;
    // Group by tactic group
    var groups = {};
    childEdges.forEach(function (e) {
      if (!groups[e.group]) groups[e.group] = [];
      groups[e.group].push(e);
    });
    // Or-semantics: if ANY group has all children solved, parent is solved
    var anySolved = Object.keys(groups).some(function (g) {
      return groups[g].every(function (e) {
        var child = session.nodes[e.to];
        return child && child.status === 'solved';
      });
    });
    if (anySolved) {
      node.status = 'solved';
      session.solvedSet[nodeId] = true;
      propagateSolved(node.parentId);
    }
  }

  function getOpenGoals() {
    if (!session) return [];
    return Object.values(session.nodes).filter(function (n) {
      return n.status === 'open' && n.goalKey;
    });
  }

  function isVictory() {
    if (!session) return false;
    var root = session.nodes[session.rootId];
    return root && root.status === 'solved';
  }

  function buildProofScript() {
    if (!session) return '-- No proof in progress';
    var lines = [session.theorem.statement + ' := by'];
    appendScript(session.rootId, lines, '  ', {});
    return lines.join('\n');
  }

  function appendScript(nodeId, lines, indent, visited) {
    if (!nodeId || visited[nodeId]) return;
    visited[nodeId] = true;
    var node = session.nodes[nodeId];
    if (!node) return;
    // Find applied (non-failed) edges from this node
    var applied = session.edges.filter(function (e) {
      return e.from === nodeId && e.status === 'applied' && e.to !== null;
    });
    // Find solve-in-place edge (to === null)
    var solveEdge = session.edges.find(function (e) {
      return e.from === nodeId && e.status === 'applied' && e.to === null;
    });

    if (solveEdge) {
      lines.push(indent + solveEdge.tactic);
      return;
    }
    if (applied.length === 0) {
      if (node.status !== 'solved') lines.push(indent + 'sorry');
      return;
    }
    // Group by tactic+group
    var groups = {};
    applied.forEach(function (e) { if (!groups[e.group]) groups[e.group] = []; groups[e.group].push(e); });
    var groupKeys = Object.keys(groups);
    // Use the first non-failed group
    var grp = groups[groupKeys[0]];
    if (!grp || !grp.length) { lines.push(indent + 'sorry'); return; }
    lines.push(indent + grp[0].tactic);
    if (grp.length === 1) {
      appendScript(grp[0].to, lines, indent, visited);
    } else {
      grp.forEach(function (e) {
        var subLines = [];
        appendScript(e.to, subLines, indent + '  ', visited);
        if (subLines.length === 0) {
          var child = session.nodes[e.to];
          if (child && child.status === 'solved') {
            // Find what solved it
            var childSolve = session.edges.find(function (ce) {
              return ce.from === e.to && ce.status === 'applied';
            });
            lines.push(indent + '· ' + (childSolve ? childSolve.tactic : 'sorry'));
          } else {
            lines.push(indent + '· sorry');
          }
        } else {
          lines.push(indent + '· ' + subLines[0].trim());
          for (var i = 1; i < subLines.length; i++) {
            lines.push(indent + '  ' + subLines[i]);
          }
        }
      });
    }
  }

  // ═══════════════════════════════════════════════════════════════
  // §4  Tree Layout (d3.tree)
  // ═══════════════════════════════════════════════════════════════

  function computeLayout() {
    if (!session) return;
    var root = buildHierarchy(session.rootId, {});
    if (!root) return;
    var hier = d3.hierarchy(root, function (d) { return d.ch; });
    var layout = d3.tree().nodeSize([NODE_W + SIB_GAP, LEVEL_H]);
    layout(hier);
    hier.each(function (d) {
      if (d.data && d.data._nid) {
        var n = session.nodes[d.data._nid];
        if (n) { n.x = d.x; n.y = d.y; }
      }
    });
  }

  function buildHierarchy(nodeId, visited) {
    if (!nodeId || visited[nodeId]) return null;
    visited[nodeId] = true;
    var node = session.nodes[nodeId];
    if (!node) return null;
    var children = [];
    session.edges.forEach(function (e) {
      if (e.from === nodeId && e.to && !visited[e.to]) {
        var child = buildHierarchy(e.to, visited);
        if (child) children.push(child);
      }
    });
    return { _nid: nodeId, ch: children.length ? children : null };
  }

  // ═══════════════════════════════════════════════════════════════
  // §5  Canvas Rendering
  // ═══════════════════════════════════════════════════════════════

  var canvas, ctx;
  var cam = { x: 0, y: 0, zoom: 1 };
  var animFrame = 0;
  var needsRender = true;
  var hoveredId = null;

  function initCanvas() {
    canvas = document.getElementById('pg-canvas');
    if (!canvas) return;
    ctx = canvas.getContext('2d');
    resizeCanvas();
    setupCanvasEvents();
    renderLoop();
  }

  function resizeCanvas() {
    if (!canvas) return;
    var rect = canvas.parentElement.getBoundingClientRect();
    canvas.width = Math.floor(rect.width * (window.devicePixelRatio || 1));
    canvas.height = Math.floor(rect.height * (window.devicePixelRatio || 1));
    canvas.style.width = rect.width + 'px';
    canvas.style.height = rect.height + 'px';
    needsRender = true;
  }

  function renderLoop() {
    if (needsRender) { renderCanvas(); needsRender = false; }
    // Animate node appearances
    if (session) {
      var anyAnim = false;
      Object.values(session.nodes).forEach(function (n) {
        if (n.animT < 1) { n.animT = Math.min(1, n.animT + 0.08); anyAnim = true; }
      });
      if (anyAnim) needsRender = true;
    }
    animFrame = requestAnimationFrame(renderLoop);
  }

  function renderCanvas() {
    if (!ctx || !canvas) return;
    var W = canvas.width, H = canvas.height;
    var dpr = window.devicePixelRatio || 1;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = '#050804';
    ctx.fillRect(0, 0, W, H);

    // Draw subtle grid
    ctx.save();
    ctx.strokeStyle = 'rgba(53,255,62,0.03)';
    ctx.lineWidth = 1;
    var gridStep = 60 * cam.zoom * dpr;
    var ox = (W / 2 + cam.x * cam.zoom * dpr) % gridStep;
    var oy = (H / 3 + cam.y * cam.zoom * dpr) % gridStep;
    for (var gx = ox; gx < W; gx += gridStep) { ctx.beginPath(); ctx.moveTo(gx, 0); ctx.lineTo(gx, H); ctx.stroke(); }
    for (var gy = oy; gy < H; gy += gridStep) { ctx.beginPath(); ctx.moveTo(0, gy); ctx.lineTo(W, gy); ctx.stroke(); }
    ctx.restore();

    if (!session) return;

    // Transform: center root in upper third of canvas
    ctx.save();
    ctx.translate(W / 2, H / 3);
    ctx.scale(cam.zoom * dpr, cam.zoom * dpr);
    ctx.translate(cam.x, cam.y);

    // Draw edges — labels only shown on edges connected to selected/hovered node
    var sel = session.selectedId;
    var hov = hoveredId;
    session.edges.forEach(function (e) {
      if (!e.to) return; // solve-in-place edge
      var from = session.nodes[e.from], to = session.nodes[e.to];
      if (!from || !to) return;
      var showLabel = (e.from === sel || e.to === sel || e.from === hov || e.to === hov);
      drawEdge(from, to, e.tactic, e.status, to.animT, showLabel);
    });

    // Draw nodes
    var nodeList = Object.values(session.nodes);
    // Draw inactive/failed first, then open/solved, then selected
    nodeList.sort(function (a, b) {
      var order = { inactive: 0, failed: 1, open: 2, solved: 3 };
      var oa = a.id === session.selectedId ? 10 : (order[a.status] || 2);
      var ob = b.id === session.selectedId ? 10 : (order[b.status] || 2);
      return oa - ob;
    });
    nodeList.forEach(function (n) { drawNode(n); });

    ctx.restore();
  }

  function drawEdge(from, to, tactic, status, t, showLabel) {
    var alpha = Math.max(0.1, t);
    var isFailed = status === 'failed';
    var isHighlighted = showLabel;
    ctx.save();
    ctx.globalAlpha = alpha * (isFailed ? 0.4 : isHighlighted ? 0.9 : 0.5);
    ctx.strokeStyle = isFailed ? '#ff3030' : isHighlighted ? 'rgba(53,255,62,0.6)' : 'rgba(53,255,62,0.25)';
    ctx.lineWidth = isFailed ? 1 : (isHighlighted ? 2 : 1);
    if (isFailed) ctx.setLineDash([4, 4]);

    // Draw path: vertical from parent bottom, then vertical to child top
    var midY = from.y + LEVEL_H * 0.5;
    ctx.beginPath();
    ctx.moveTo(from.x, from.y + NODE_H / 2);
    ctx.lineTo(from.x, midY);
    ctx.lineTo(to.x, midY);
    ctx.lineTo(to.x, to.y - NODE_H / 2);
    ctx.stroke();
    ctx.setLineDash([]);

    // Edge label — only show on edges connected to selected/hovered node
    if (tactic && showLabel) {
      // Position label near the child node (avoids overlap at parent midpoint)
      var labelX = to.x;
      var labelY = to.y - NODE_H / 2 - 6;
      ctx.font = EDGE_LABEL_SIZE + 'px SF Mono, monospace';
      ctx.fillStyle = isFailed ? 'rgba(255,48,48,0.8)' : 'rgba(120,255,116,0.75)';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'bottom';
      var truncTac = tactic.length > 28 ? tactic.slice(0, 26) + '..' : tactic;
      ctx.fillText(truncTac, labelX, labelY);
    }
    ctx.restore();
  }

  function drawNode(node) {
    var t = Math.max(0.05, node.animT || 0);
    var isSelected = node.id === session.selectedId;
    var isHovered = node.id === hoveredId;
    var st = isSelected ? STATUS_STYLE.active :
             (STATUS_STYLE[node.status] || STATUS_STYLE.open);

    ctx.save();
    ctx.globalAlpha = t;
    ctx.translate(node.x, node.y);
    var s = 0.3 + 0.7 * t; // scale-in animation
    ctx.scale(s, s);

    var hw = NODE_W / 2, hh = NODE_H / 2;

    // Glow
    if (st.glow !== 'transparent') {
      ctx.shadowColor = st.glow;
      ctx.shadowBlur = isSelected ? 16 : (isHovered ? 12 : 8);
    }

    // Background
    roundRect(ctx, -hw, -hh, NODE_W, NODE_H, NODE_R);
    ctx.fillStyle = st.bg;
    ctx.fill();
    ctx.strokeStyle = st.border;
    ctx.lineWidth = isSelected ? 2.5 : (isHovered ? 2 : 1.2);
    ctx.stroke();
    ctx.shadowColor = 'transparent';
    ctx.shadowBlur = 0;

    // Clip text content to node bounds
    ctx.save();
    roundRect(ctx, -hw + 1, -hh + 1, NODE_W - 2, NODE_H - 2, NODE_R);
    ctx.clip();

    // Status icon
    var icon = node.status === 'solved' ? '✓' :
               node.status === 'failed' ? '✗' :
               node.status === 'sorry'  ? '!' :
               node.status === 'open'   ? '○' : '●';
    ctx.font = 'bold 11px sans-serif';
    ctx.fillStyle = st.border;
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    ctx.fillText(icon, hw - 6, 0);

    // Goal text (truncated to fit node width)
    var goalDef = getGoalDef(node);
    var label = '';
    if (node.status === 'failed') {
      label = node.errorMsg ? node.errorMsg.slice(0, 30) : 'Failed';
    } else if (goalDef) {
      var disp = goalDef.display;
      var turnstile = disp.indexOf('⊢');
      label = turnstile >= 0 ? disp.slice(turnstile) : disp;
      // Remove newlines for single-line display
      label = label.replace(/\n/g, ' ');
      if (label.length > 30) label = label.slice(0, 28) + '..';
    }
    ctx.font = '11px SF Mono, monospace';
    ctx.fillStyle = st.text;
    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';
    ctx.fillText(label, -hw + 8, 0);

    ctx.restore(); // remove clip
    ctx.restore();
  }

  function roundRect(c, x, y, w, h, r) {
    c.beginPath();
    c.moveTo(x + r, y);
    c.lineTo(x + w - r, y);
    c.quadraticCurveTo(x + w, y, x + w, y + r);
    c.lineTo(x + w, y + h - r);
    c.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
    c.lineTo(x + r, y + h);
    c.quadraticCurveTo(x, y + h, x, y + h - r);
    c.lineTo(x, y + r);
    c.quadraticCurveTo(x, y, x + r, y);
    c.closePath();
  }

  // ═══════════════════════════════════════════════════════════════
  // §6  Canvas Interaction (click, hover, zoom, pan)
  // ═══════════════════════════════════════════════════════════════

  var drag = null;
  var boundListeners = []; // track listeners for cleanup

  function addTrackedListener(target, event, handler, opts) {
    target.addEventListener(event, handler, opts);
    boundListeners.push({ target: target, event: event, handler: handler, opts: opts });
  }

  function screenToWorld(sx, sy) {
    var dpr = window.devicePixelRatio || 1;
    var rect = canvas.getBoundingClientRect();
    var cx = (sx - rect.left) * dpr;
    var cy = (sy - rect.top) * dpr;
    var W = canvas.width, H = canvas.height;
    var wx = (cx - W / 2) / (cam.zoom * dpr) - cam.x;
    var wy = (cy - H / 3) / (cam.zoom * dpr) - cam.y;
    return { x: wx, y: wy };
  }

  function hitTest(wx, wy) {
    if (!session) return null;
    var best = null, bestD = Infinity;
    Object.values(session.nodes).forEach(function (n) {
      var dx = wx - n.x, dy = wy - n.y;
      if (Math.abs(dx) < NODE_W / 2 + 4 && Math.abs(dy) < NODE_H / 2 + 4) {
        var d = dx * dx + dy * dy;
        if (d < bestD) { bestD = d; best = n.id; }
      }
    });
    return best;
  }

  function setupCanvasEvents() {
    if (!canvas) return;

    addTrackedListener(canvas, 'mousemove', function (e) {
      if (drag) {
        cam.x = drag.cx + (e.clientX - drag.sx) / cam.zoom;
        cam.y = drag.cy + (e.clientY - drag.sy) / cam.zoom;
        needsRender = true;
        return;
      }
      var w = screenToWorld(e.clientX, e.clientY);
      var hit = hitTest(w.x, w.y);
      if (hit !== hoveredId) { hoveredId = hit; needsRender = true; }
      canvas.style.cursor = hit ? 'pointer' : 'grab';
    });

    addTrackedListener(canvas, 'mousedown', function (e) {
      var w = screenToWorld(e.clientX, e.clientY);
      var hit = hitTest(w.x, w.y);
      if (hit) {
        selectNode(hit);
      } else {
        drag = { sx: e.clientX, sy: e.clientY, cx: cam.x, cy: cam.y };
        canvas.style.cursor = 'grabbing';
      }
    });

    addTrackedListener(window, 'mouseup', function () {
      if (drag) { drag = null; if (canvas) canvas.style.cursor = 'grab'; }
    });

    addTrackedListener(canvas, 'wheel', function (e) {
      e.preventDefault();
      var factor = e.deltaY < 0 ? 1.12 : 0.89;
      cam.zoom = Math.max(0.2, Math.min(4, cam.zoom * factor));
      needsRender = true;
    }, { passive: false });

    // Keyboard shortcuts — scoped to proof game page via pg-page check
    addTrackedListener(document, 'keydown', function (e) {
      if (!session) return;
      if (!document.querySelector('.pg-page')) return;
      if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
      if (e.key === 'Enter') {
        var input = document.getElementById('pg-tactic-input');
        if (input && input.value.trim()) { doApplyTactic(input.value.trim()); input.value = ''; }
      }
      if (e.key === 'Backspace' || (e.key === 'z' && (e.ctrlKey || e.metaKey))) {
        e.preventDefault();
        doUndo();
      }
    });
  }

  // ═══════════════════════════════════════════════════════════════
  // §7  Context Panel
  // ═══════════════════════════════════════════════════════════════

  function updateContextPanel() {
    if (!session) return;
    var node = session.nodes[session.selectedId];
    if (!node) return;
    var goalDef = getGoalDef(node);

    // Goal display
    var goalEl = document.getElementById('pg-goal-display');
    if (goalEl) {
      if (goalDef) {
        // Try KaTeX for simple math
        var display = goalDef.display;
        goalEl.textContent = display;
        // Attempt KaTeX rendering for the turnstile part
        if (window.katex) {
          try {
            var parts = display.split('⊢');
            if (parts.length === 2) {
              var hypsStr = parts[0].trim();
              var goalStr = parts[1].trim();
              goalEl.innerHTML = '';
              if (hypsStr) {
                var hSpan = document.createElement('div');
                hSpan.style.cssText = 'color:#4ca43a;font-size:11px;margin-bottom:4px';
                hSpan.textContent = hypsStr;
                goalEl.appendChild(hSpan);
              }
              var gDiv = document.createElement('div');
              gDiv.textContent = '⊢ ' + goalStr;
              goalEl.appendChild(gDiv);
            }
          } catch (_) { /* fall back to text */ }
        }
      } else if (node.status === 'failed') {
        goalEl.textContent = node.errorMsg || 'Tactic failed';
        goalEl.style.color = '#ff6666';
      } else {
        goalEl.textContent = 'No goal state available';
      }
    }

    // Hypotheses
    var hypList = document.getElementById('pg-hyp-list');
    if (hypList && goalDef) {
      hypList.innerHTML = '';
      goalDef.hyps.forEach(function (h) {
        var li = document.createElement('li');
        li.className = 'pg-hyp-item';
        li.innerHTML = '<span class="pg-hyp-name">' + esc(h.n) + '</span>' +
          '<span class="pg-hyp-colon">:</span>' +
          '<span class="pg-hyp-type">' + esc(h.t) + '</span>';
        li.title = 'Click to insert into tactic input';
        li.addEventListener('click', function () {
          var input = document.getElementById('pg-tactic-input');
          if (input) { input.value = 'exact ' + h.n; input.focus(); }
        });
        hypList.appendChild(li);
      });
    }

    // Available tactics
    var tacList = document.getElementById('pg-tactic-list');
    if (tacList) {
      tacList.innerHTML = '';
      if (goalDef && (node.status === 'open' || node.status === 'solved')) {
        if (node.status === 'solved') {
          var solvedDiv = document.createElement('div');
          solvedDiv.style.cssText = 'color:#39ff14;padding:4px 8px 8px;font-size:11px';
          solvedDiv.textContent = '✓ Solved — alternative tactics:';
          tacList.appendChild(solvedDiv);
        }
        // Server mode: show prompt to type tactics + suggest button
        if (serverMode && node.stateId !== undefined && node.status === 'open') {
          var serverHint = document.createElement('div');
          serverHint.style.cssText = 'color:#00ff41;padding:6px 8px;font-size:11px;border:1px solid rgba(0,255,65,0.1);border-radius:4px;margin-bottom:6px';
          serverHint.textContent = 'Server mode — type any Lean tactic below';
          tacList.appendChild(serverHint);
          // Add suggest button
          var suggestBtn = document.createElement('button');
          suggestBtn.className = 'pg-btn';
          suggestBtn.textContent = 'Suggest Tactics';
          suggestBtn.style.cssText = 'margin-bottom:8px;font-size:11px';
          suggestBtn.addEventListener('click', function () {
            fetchSuggestions(node, tacList);
          });
          tacList.appendChild(suggestBtn);
        }
        (goalDef.suggested || []).forEach(function (tac) {
          var div = document.createElement('div');
          div.className = 'pg-tactic-item';
          var baseTac = tac.split(' ')[0];
          var tacResultType = goalDef.tactics[tac];
          var badge = '';
          if (Array.isArray(tacResultType) && tacResultType.length === 0) {
            badge = '<span style="color:#39ff14;font-size:9px;margin-left:4px">QED</span>';
          } else if (Array.isArray(tacResultType) && tacResultType.length > 0) {
            badge = '<span style="color:#00aaff;font-size:9px;margin-left:4px">' + tacResultType.length + ' goals</span>';
          }
          div.innerHTML = '<span class="pg-tactic-arrow">▸</span>' +
            '<span class="pg-tactic-name">' + esc(tac) + badge + '</span>' +
            '<span class="pg-tactic-desc">' + esc(TACTIC_DESC[baseTac] || '') + '</span>';
          div.addEventListener('click', function () { doApplyTactic(tac); });
          tacList.appendChild(div);
        });
      } else if (node.status === 'failed') {
        tacList.innerHTML = '<div style="color:#ff3030;padding:8px;font-size:12px">Tactic failed — try backtracking</div>';
      }
    }

    // Proof script
    var scriptEl = document.getElementById('pg-script');
    if (scriptEl) { scriptEl.textContent = buildProofScript(); }

    // Stats
    updateStats();
  }

  function updateStats() {
    if (!session) return;
    var open = getOpenGoals();
    var total = Object.values(session.nodes).filter(function (n) { return n.goalKey; }).length;
    var solved = Object.keys(session.solvedSet).length;

    var totalNodes = Object.keys(session.nodes).length;
    setText('pg-stat-goals', totalNodes + ' nodes (' + open.length + ' open)');
    setText('pg-stat-tactics', '' + session.tacticsApplied);
    setText('pg-stat-branches', '' + session.branchesExplored);

    // Depth
    var maxDepth = 0;
    Object.values(session.nodes).forEach(function (n) {
      var d = 0, cur = n;
      while (cur.parentId) { d++; cur = session.nodes[cur.parentId]; if (!cur) break; }
      if (d > maxDepth) maxDepth = d;
    });
    setText('pg-stat-depth', '' + maxDepth);

    // Theorem display
    setText('pg-theorem-display', session.theorem.statement);

    // Buttons
    var verifyBtn = document.getElementById('pg-verify-btn');
    var exportBtn = document.getElementById('pg-export-btn');
    var undoBtn = document.getElementById('pg-undo-btn');
    if (verifyBtn) verifyBtn.disabled = false;
    if (exportBtn) exportBtn.disabled = false;
    if (undoBtn) undoBtn.disabled = !session.selectedId || session.selectedId === session.rootId;

    // Status dot
    var dot = document.querySelector('.pg-status-dot');
    if (dot) {
      var complete = isVictory();
      var sorry = complete && hasSorryInProof();
      var dotClass = complete && !sorry ? 'active' : serverMode ? 'active' : 'simulated';
      dot.className = 'pg-status-dot ' + dotClass;
      var statusText = dot.parentElement && dot.parentElement.lastChild;
      if (statusText && statusText.nodeType === 3) {
        statusText.textContent = complete && !sorry ? ' Complete' :
          sorry ? ' Incomplete (sorry)' :
          serverMode ? ' Connected' : ' Simulation';
      }
    }
  }

  // ═══════════════════════════════════════════════════════════════
  // §8  Actions
  // ═══════════════════════════════════════════════════════════════

  function selectNode(nodeId) {
    if (!session || !session.nodes[nodeId]) return;
    session.selectedId = nodeId;
    needsRender = true;
    updateContextPanel();
  }

  function doApplyTactic(tacticStr) {
    if (!session || !session.selectedId) return;
    var node = session.nodes[session.selectedId];
    if (!node || node.status === 'failed' || node.status === 'inactive') return;

    // Server mode: delegate to Pantograph
    if (serverMode && node.stateId !== undefined) {
      doApplyTacticServer(tacticStr);
      return;
    }

    var result = applyTactic(session.selectedId, tacticStr);
    if (!result) return;

    computeLayout();
    centerOnNode(session.selectedId);
    needsRender = true;
    updateContextPanel();

    if (isVictory()) {
      showVictory();
    }
  }

  function doApplyTacticServer(tacticStr) {
    var node = session.nodes[session.selectedId];
    if (!node || node.stateId === undefined) return;

    // Show loading state
    var tacList = document.getElementById('pg-tactic-list');
    if (tacList) tacList.innerHTML = '<div style="color:#4ca43a;padding:8px;font-size:12px">Applying tactic…</div>';

    fetch('/api/explorer/tactic', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        state_id: node.stateId,
        goal_id: node.goalId || 0,
        tactic: tacticStr,
      }),
    })
    .then(function (r) { return r.json(); })
    .then(function (data) {
      if (!data.ok) {
        showTacticError(data.error || 'Tactic failed');
        return;
      }
      session.tacticsApplied++;
      if (data.solved) {
        node.status = 'solved';
        session.solvedSet[node.id] = true;
        session.edges.push({ from: node.id, to: null, tactic: tacticStr, group: node.id, status: 'applied' });
        propagateSolved(node.parentId);
      } else {
        session.branchesExplored++;
        var groupId = mkId();
        var firstChild = null;
        (data.goals || []).forEach(function (goal) {
          var nid = mkId();
          var hyps = (goal.vars || []).map(function (v) { return { n: v.name, t: v.type || v['type'] }; });
          var display = hyps.map(function (h) { return h.n + ' : ' + h.t; }).join(', ');
          if (display) display += ' ';
          display += '⊢ ' + (goal.target || '?');
          session.nodes[nid] = {
            id: nid,
            goalKey: null,
            stateId: data.stateId,
            goalId: goal.goalId,
            serverGoal: {
              display: display,
              hyps: hyps,
              tactics: {},
              suggested: [],
            },
            status: 'open',
            parentId: node.id,
            parentTactic: tacticStr,
            x: 0, y: 0, animT: 0,
          };
          session.edges.push({
            from: node.id, to: nid, tactic: tacticStr,
            group: groupId, status: 'applied',
          });
          if (!firstChild) firstChild = nid;
        });
        if (firstChild) session.selectedId = firstChild;
      }
      computeLayout();
      var zoom = fitTreeZoom();
      cam = { x: 0, y: 0, zoom: zoom };
      needsRender = true;
      updateContextPanel();
      if (isVictory()) showVictory();
    })
    .catch(function (err) {
      console.warn('[ProofBuilder] Server tactic failed, falling back:', err);
      serverMode = false;
      updateStats();
    });
  }

  function showTacticError(msg) {
    var tacList = document.getElementById('pg-tactic-list');
    if (tacList) {
      var errDiv = document.createElement('div');
      errDiv.style.cssText = 'color:#ff6666;padding:8px;font-size:12px;border:1px solid rgba(255,48,48,0.2);border-radius:4px;margin-bottom:6px';
      errDiv.textContent = msg;
      tacList.insertBefore(errDiv, tacList.firstChild);
      setTimeout(function () { if (errDiv.parentNode) errDiv.remove(); }, 5000);
    }
  }

  function doUndo() {
    if (!session || !session.selectedId) return;
    var node = session.nodes[session.selectedId];
    if (!node || !node.parentId) return;
    selectNode(node.parentId);
    centerOnNode(node.parentId);
  }

  function centerOnNode(nodeId) {
    var node = session.nodes[nodeId];
    if (!node) return;
    cam.x = -node.x;
    cam.y = -node.y + 30;
  }

  function doVerify() {
    if (!session) return;
    var goalEl = document.getElementById('pg-goal-display');
    if (!goalEl) return;

    if (!isVictory()) {
      var open = getOpenGoals();
      showInlineResult(goalEl, 'incomplete',
        'Proof incomplete — ' + open.length + ' open goal' + (open.length === 1 ? '' : 's') + ' remaining.',
        'Solve all open goals before verifying.');
      return;
    }
    var isSorry = hasSorryInProof();
    var script = buildProofScript();
    if (isSorry) {
      showInlineResult(goalEl, 'warning',
        'Proof uses sorry (incomplete)',
        'Connect a Lean proof server for genuine verification.');
    } else if (serverMode) {
      showInlineResult(goalEl, 'success',
        'Proof verified by Lean proof server',
        'All goals solved and verified.');
    } else {
      showInlineResult(goalEl, 'success',
        'Proof verified (simulation)',
        'All goals solved. Connect a Lean server for genuine verification.');
    }
    // Show script in the script panel
    var scriptEl = document.getElementById('pg-script');
    if (scriptEl) scriptEl.textContent = script;
  }

  function showInlineResult(container, type, title, detail) {
    var colors = {
      success: { bg: 'rgba(57,255,20,0.08)', border: '#39ff14', text: '#78ff74', icon: '✓' },
      warning: { bg: 'rgba(255,170,0,0.08)', border: '#ffaa00', text: '#ffcc44', icon: '!' },
      incomplete: { bg: 'rgba(0,170,255,0.08)', border: '#00aaff', text: '#88ccff', icon: '○' },
    };
    var c = colors[type] || colors.incomplete;
    container.innerHTML = '';
    container.style.cssText = 'padding:12px;background:' + c.bg + ';border:1px solid ' + c.border + ';border-radius:4px';
    var h = document.createElement('div');
    h.style.cssText = 'font-size:14px;font-weight:700;color:' + c.text + ';margin-bottom:6px';
    h.textContent = c.icon + ' ' + title;
    container.appendChild(h);
    if (detail) {
      var d = document.createElement('div');
      d.style.cssText = 'font-size:12px;color:' + c.text + ';opacity:0.8';
      d.textContent = detail;
      container.appendChild(d);
    }
  }

  function doExport() {
    if (!session) return;
    var script = buildProofScript();
    var blob = new Blob([script], { type: 'text/plain' });
    var a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = (session.theorem.id || 'proof') + '.lean';
    a.click();
    URL.revokeObjectURL(a.href);
  }

  function hasSorryInProof() {
    if (!session) return false;
    var script = buildProofScript();
    return script.indexOf('sorry') >= 0;
  }

  function showVictory() {
    // Update status dot and stats — no overlay, just inline feedback
    updateStats();
    updateContextPanel();
  }

  // ═══════════════════════════════════════════════════════════════
  // §9  Library Modal
  // ═══════════════════════════════════════════════════════════════

  function showLibrary() {
    // Remove existing modal
    var existing = document.querySelector('.pg-modal-overlay');
    if (existing) existing.remove();

    var overlay = document.createElement('div');
    overlay.className = 'pg-modal-overlay';
    var modal = document.createElement('div');
    modal.className = 'pg-modal';

    var html = '<div class="pg-modal-title">Theorem Library</div>' +
      '<button class="pg-modal-close" id="pg-modal-close">&times;</button>';

    var cats = getCategories();
    Object.keys(cats).forEach(function (cat) {
      html += '<div class="pg-category-title">' + esc(cat) + '</div>';
      html += '<div class="pg-theorem-grid">';
      cats[cat].forEach(function (thm) {
        var stars = '';
        for (var i = 1; i <= 5; i++) {
          stars += '<span class="pg-star' + (i > thm.difficulty ? ' dim' : '') + '">★</span>';
        }
        html += '<div class="pg-theorem-card" data-tid="' + thm.id + '">' +
          '<div class="pg-tc-name">' + esc(thm.name) + '</div>' +
          '<div class="pg-tc-stmt">' + esc(thm.statement) + '</div>' +
          '<div class="pg-tc-meta">' +
          '<span>' + stars + '</span>' +
          thm.tags.map(function (t) { return '<span class="pg-tag">' + esc(t) + '</span>'; }).join('') +
          '</div></div>';
      });
      html += '</div>';
    });

    // New theorem section
    html += '<div class="pg-new-section">' +
      '<div class="pg-section-title">Custom Theorem</div>' +
      '<div class="pg-new-row">' +
      '<input class="pg-new-input" id="pg-new-stmt" placeholder="theorem my_thm : P → P" />' +
      '<button class="pg-btn primary" id="pg-new-go">Start</button>' +
      '</div>' +
      '<div class="pg-new-hint">Custom theorems use simulation. Only pre-computed proof trees are interactive.</div>' +
      '</div>';

    // Import proof tree section
    html += '<div class="pg-new-section">' +
      '<div class="pg-section-title">Import Proof Sketch</div>' +
      '<div class="pg-new-hint" style="margin-bottom:8px">Load a Heyting proof tree JSON to visualize strategy trees from pre-project plans.</div>' +
      '<div class="pg-new-row">' +
      '<input type="file" accept=".json" id="pg-import-file" style="display:none" />' +
      '<button class="pg-btn" id="pg-import-btn">Choose File…</button>' +
      '<button class="pg-btn" id="pg-import-paste-btn">Paste JSON</button>' +
      '</div>' +
      '<textarea class="pg-new-input" id="pg-import-textarea" placeholder="Paste proof tree JSON here…" ' +
      'style="display:none;min-height:100px;resize:vertical;margin-top:8px;font-size:11px"></textarea>' +
      '<button class="pg-btn primary" id="pg-import-paste-go" style="display:none;margin-top:6px">Load Proof Tree</button>' +
      '</div>';

    // Import from Lean Database section
    html += '<div class="pg-new-section">' +
      '<div class="pg-section-title">Import from Lean Database</div>' +
      '<div class="pg-new-hint" style="margin-bottom:8px">Browse theorems from the connected Lean project.</div>' +
      '<div id="pg-lean-browser">' +
      '<button class="pg-btn" id="pg-lean-browse-btn">Browse Lean Theorems</button>' +
      '<div id="pg-lean-results" style="margin-top:8px"></div>' +
      '</div>' +
      '</div>';

    modal.innerHTML = html;
    overlay.appendChild(modal);
    document.body.appendChild(overlay);

    // Events
    overlay.addEventListener('click', function (e) {
      if (e.target === overlay) hideLibrary();
    });
    modal.querySelector('#pg-modal-close').addEventListener('click', hideLibrary);

    modal.querySelectorAll('.pg-theorem-card').forEach(function (card) {
      card.addEventListener('click', function () {
        var tid = card.getAttribute('data-tid');
        var thm = LIBRARY.find(function (t) { return t.id === tid; });
        if (thm) { loadTheorem(thm); hideLibrary(); }
      });
    });

    modal.querySelector('#pg-new-go').addEventListener('click', function () {
      var stmt = (document.getElementById('pg-new-stmt') || {}).value || '';
      if (!stmt.trim()) return;
      // Try to match against library
      var match = LIBRARY.find(function (t) {
        return t.statement.toLowerCase().includes(stmt.toLowerCase().replace('theorem ', '').trim());
      });
      if (match) {
        loadTheorem(match);
        hideLibrary();
      } else {
        // Create a minimal simulation with just sorry
        var custom = {
          id: 'custom_' + Date.now(), name: 'Custom', category: 'Custom',
          statement: stmt.trim(), difficulty: 0, tags: ['custom'],
          hint: 'Custom theorems require a Lean server for interactive proving.',
          rootGoal: 'r', goals: {
            r: {
              display: '⊢ ' + stmt.replace(/theorem\s+\w+[^:]*:\s*/, '').trim(),
              hyps: [], tactics: { 'sorry': [] }, suggested: ['sorry']
            }
          }
        };
        loadTheorem(custom);
        hideLibrary();
      }
    });

    // ── Import proof tree from file ──
    var importFileInput = modal.querySelector('#pg-import-file');
    modal.querySelector('#pg-import-btn').addEventListener('click', function () {
      importFileInput.click();
    });
    importFileInput.addEventListener('change', function () {
      var file = importFileInput.files[0];
      if (!file) return;
      var reader = new FileReader();
      reader.onload = function (ev) {
        try {
          var tree = JSON.parse(ev.target.result);
          var thm = convertProofTreeToTheorem(tree);
          if (thm) { loadTheorem(thm); hideLibrary(); }
        } catch (e) {
          alert('Failed to parse proof tree JSON: ' + e.message);
        }
      };
      reader.readAsText(file);
    });

    // ── Import proof tree from paste ──
    var pasteArea = modal.querySelector('#pg-import-textarea');
    var pasteGoBtn = modal.querySelector('#pg-import-paste-go');
    modal.querySelector('#pg-import-paste-btn').addEventListener('click', function () {
      var showing = pasteArea.style.display !== 'none';
      pasteArea.style.display = showing ? 'none' : 'block';
      pasteGoBtn.style.display = showing ? 'none' : 'block';
      if (!showing) pasteArea.focus();
    });
    pasteGoBtn.addEventListener('click', function () {
      var text = pasteArea.value.trim();
      if (!text) return;
      try {
        var tree = JSON.parse(text);
        var thm = convertProofTreeToTheorem(tree);
        if (thm) { loadTheorem(thm); hideLibrary(); }
      } catch (e) {
        alert('Failed to parse proof tree JSON: ' + e.message);
      }
    });

    // ── Import from Lean Database ──
    modal.querySelector('#pg-lean-browse-btn').addEventListener('click', function () {
      loadLeanTheorems(modal.querySelector('#pg-lean-results'));
    });
  }

  function hideLibrary() {
    var overlay = document.querySelector('.pg-modal-overlay');
    if (overlay) overlay.remove();
  }

  function loadTheorem(thm) {
    buildFullTree(thm);
    computeLayout();
    var zoom = fitTreeZoom();
    cam = { x: 0, y: 0, zoom: zoom };
    needsRender = true;
    var welcome = document.getElementById('pg-welcome');
    if (welcome) welcome.style.display = 'none';
    selectNode(session.rootId);
  }

  // Build the full proof tree directly from the theorem's goals graph.
  // Walks the graph recursively, preferring branching tactics (those that
  // produce subgoals) over immediate solves, so the deepest tree is shown.
  // This replaces the broken autoExpandTree which had a duplicate-expansion bug.
  function buildFullTree(theorem) {
    nodeSeq = 0;
    var nodes = {};
    var edges = [];
    var solvedSet = {};
    var rootId = null;

    function expand(goalKey, parentId, parentTac, parentGroup, visited) {
      if (!goalKey) return null;
      var goalDef = theorem.goals[goalKey];
      if (!goalDef) return null;
      // Cycle protection — copy visited so sibling branches don't collide
      if (visited[goalKey]) return null;
      visited = Object.assign ? Object.assign({}, visited) : JSON.parse(JSON.stringify(visited));
      visited[goalKey] = true;

      var nid = mkId();
      nodes[nid] = {
        id: nid, goalKey: goalKey, status: 'open',
        parentId: parentId, parentTactic: parentTac,
        x: 0, y: 0, animT: 1,
      };

      if (parentId && parentTac) {
        edges.push({ from: parentId, to: nid, tactic: parentTac, group: parentGroup, status: 'applied' });
      }

      // Expand ALL branching tactics to show the full multiway tree.
      // For proof-tree imports: children are parallel sub-tasks (all shown).
      // For library theorems: alternative tactics create branching paths.
      var suggested = goalDef.suggested || Object.keys(goalDef.tactics);
      var expanded = false, solveTac = null;
      for (var i = 0; i < suggested.length; i++) {
        var tac = suggested[i];
        var tacResult = goalDef.tactics[tac];
        if (typeof tacResult === 'string') continue; // error
        if (!Array.isArray(tacResult)) continue;
        if (tacResult.length === 0) {
          if (!solveTac) solveTac = tac;
        } else {
          // Branching tactic — expand it (creates child nodes for each subgoal)
          var groupId = 'g' + (nodeSeq++);
          for (var j = 0; j < tacResult.length; j++) {
            expand(tacResult[j], nid, tac, groupId, visited);
          }
          expanded = true;
        }
      }

      if (!expanded && solveTac) {
        // Leaf node solved by this tactic (no branching tactics available)
        edges.push({ from: nid, to: null, tactic: solveTac, group: nid, status: 'applied' });
        nodes[nid].status = 'solved';
        solvedSet[nid] = true;
      }
      // else if !expanded && !solveTac: stays 'open' (sorry / needs server)

      return nid;
    }

    rootId = expand(theorem.rootGoal, null, null, null, {});
    if (!rootId) {
      // Fallback to minimal session
      newSession(theorem);
      return;
    }

    // Propagate solved upward: if all children in a tactic group are solved, parent is solved
    var propChanged = true;
    var maxProp = 200;
    while (propChanged && maxProp-- > 0) {
      propChanged = false;
      Object.values(nodes).forEach(function (node) {
        if (node.status === 'solved') return;
        var childEdges = edges.filter(function (e) {
          return e.from === node.id && e.status === 'applied' && e.to !== null;
        });
        if (childEdges.length === 0) return;
        var groups = {};
        childEdges.forEach(function (e) {
          if (!groups[e.group]) groups[e.group] = [];
          groups[e.group].push(e);
        });
        var anySolved = Object.keys(groups).some(function (g) {
          return groups[g].every(function (e) {
            var ch = nodes[e.to];
            return ch && ch.status === 'solved';
          });
        });
        if (anySolved) {
          node.status = 'solved';
          solvedSet[node.id] = true;
          propChanged = true;
        }
      });
    }

    session = {
      theorem: theorem,
      nodes: nodes,
      edges: edges,
      rootId: rootId,
      selectedId: rootId,
      solvedSet: solvedSet,
      tacticsApplied: edges.filter(function (e) { return e.status === 'applied'; }).length,
      branchesExplored: Object.keys(nodes).length - 1,
    };
  }

  // Calculate zoom to fit all nodes in the canvas
  function fitTreeZoom() {
    if (!session || !canvas) return 1;
    var nodes = Object.values(session.nodes);
    if (nodes.length <= 1) return 1;
    var minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    nodes.forEach(function (n) {
      if (n.x - NODE_W / 2 < minX) minX = n.x - NODE_W / 2;
      if (n.x + NODE_W / 2 > maxX) maxX = n.x + NODE_W / 2;
      if (n.y - NODE_H / 2 < minY) minY = n.y - NODE_H / 2;
      if (n.y + NODE_H / 2 > maxY) maxY = n.y + NODE_H / 2;
    });
    var treeW = maxX - minX + 80;  // padding
    var treeH = maxY - minY + 80;
    var dpr = window.devicePixelRatio || 1;
    var canvasW = canvas.width / dpr;
    var canvasH = canvas.height / dpr;
    var zoomX = canvasW / treeW;
    var zoomY = (canvasH * 0.8) / treeH; // leave margin
    return Math.max(0.2, Math.min(1.5, Math.min(zoomX, zoomY)));
  }

  // ═══════════════════════════════════════════════════════════════
  // §9b  Import Converters
  // ═══════════════════════════════════════════════════════════════

  // Convert a Heyting proof tree JSON into a Proof Builder theorem.
  // Heyting trees have: conjecture_id, statement, root, nodes: { id: { node_type, content, outcome, children, ... } }
  function convertProofTreeToTheorem(tree) {
    if (!tree || !tree.nodes || !tree.root) {
      alert('Invalid proof tree: missing nodes or root');
      return null;
    }
    var treeNodes = tree.nodes;
    var rootNode = treeNodes[tree.root];
    if (!rootNode) { alert('Root node not found: ' + tree.root); return null; }

    var goalSeq = 0;
    var goals = {};

    function makeGoalKey() { return 'g' + (goalSeq++); }

    function outcomeToStatus(outcome) {
      if (outcome === 'accepted' || outcome === 'proved') return 'solved';
      if (outcome === 'blocked' || outcome === 'failed' || outcome === 'pruned') return 'failed';
      return 'open';
    }

    // Recursively build goals from tree nodes
    function convertNode(nodeId) {
      var n = treeNodes[nodeId];
      if (!n) return null;
      var gk = makeGoalKey();
      var children = n.children || [];
      var display = n.content || n.goal_before || nodeId;
      // Truncate long content for display
      if (display.length > 120) display = display.slice(0, 117) + '...';

      var tactics = {};
      var suggested = [];

      if (children.length === 0) {
        // Leaf node: offer sorry to close it
        var status = outcomeToStatus(n.outcome);
        if (status === 'solved') {
          tactics['QED'] = [];
          suggested.push('QED');
        } else {
          tactics['sorry'] = [];
          suggested.push('sorry');
        }
      } else {
        // Non-leaf: each child becomes a tactic that leads to subgoals
        children.forEach(function (childId) {
          var child = treeNodes[childId];
          if (!child) return;
          var tacName = child.content || child.node_type || childId;
          if (tacName.length > 60) tacName = tacName.slice(0, 57) + '...';
          var childGoals = (child.children || []).length > 0 ? [] : null;

          if (childGoals === null) {
            // This child is a leaf — tactic leads directly to a new goal
            var childGk = convertNode(childId);
            if (childGk) {
              tactics[tacName] = [childGk];
              suggested.push(tacName);
            }
          } else {
            // This child has further children — tactic leads to multiple subgoals
            var subGoalKeys = [];
            (child.children || []).forEach(function (grandchildId) {
              var sgk = convertNode(grandchildId);
              if (sgk) subGoalKeys.push(sgk);
            });
            if (subGoalKeys.length > 0) {
              tactics[tacName] = subGoalKeys;
            } else {
              // No grandchildren resolved — just make it a leaf
              var leafGk = convertNode(childId);
              if (leafGk) tactics[tacName] = [leafGk];
            }
            suggested.push(tacName);
          }
        });
      }

      goals[gk] = {
        display: display,
        hyps: [],
        tactics: tactics,
        suggested: suggested,
      };
      return gk;
    }

    var rootGoalKey = convertNode(tree.root);
    if (!rootGoalKey) { alert('Failed to convert proof tree'); return null; }

    // Determine difficulty from tree depth
    var maxDepth = 0;
    Object.values(treeNodes).forEach(function (n) { if ((n.depth || 0) > maxDepth) maxDepth = n.depth; });
    var difficulty = Math.min(5, Math.max(1, Math.ceil(maxDepth / 2)));

    return {
      id: 'import_' + (tree.conjecture_id || Date.now()),
      name: tree.conjecture_id || 'Imported Proof',
      category: 'Imported',
      statement: tree.statement || rootNode.content || 'Imported proof tree',
      difficulty: difficulty,
      tags: ['imported', rootNode.node_type || 'proof'],
      hint: 'Imported from Heyting proof tree. Navigate the strategy tree to explore proof approaches.',
      rootGoal: rootGoalKey,
      goals: goals,
    };
  }

  // Fetch theorems from the Lean Database and render as selectable cards
  function loadLeanTheorems(container) {
    if (!container) return;
    container.innerHTML = '<div style="color:#4ca43a;font-size:12px">Loading Lean project…</div>';

    fetch('/api/lean/scan').then(function (resp) {
      if (!resp.ok) throw new Error('HTTP ' + resp.status);
      return resp.json();
    }).then(function (res) {
      if (!res || !res.ok) {
        container.innerHTML = '<div style="color:#ff6666;font-size:12px">' +
          (res && res.message ? esc(res.message) : 'Lean scan failed — is a project configured?') + '</div>';
        return;
      }
      // Collect all .lean files recursively
      var files = [];
      function collectFiles(node, prefix) {
        if (!node) return;
        if (node.type === 'file') {
          files.push({ name: node.name, path: node.path || (prefix ? prefix + '/' + node.name : node.name), size: node.size || 0 });
        }
        if (node.type === 'dir' && node.children) {
          var dirPath = prefix ? prefix + '/' + node.name : node.name;
          node.children.forEach(function (c) { collectFiles(c, dirPath); });
        }
        // Root level
        if (node.children && !node.type) {
          node.children.forEach(function (c) { collectFiles(c, ''); });
        }
      }
      collectFiles(res.tree, '');

      if (files.length === 0) {
        container.innerHTML = '<div style="color:#ffaa00;font-size:12px">No Lean files found</div>';
        return;
      }

      // Render file list with search
      var html = '<input class="pg-new-input" id="pg-lean-search" placeholder="Filter files…" style="margin-bottom:8px;font-size:11px" />' +
        '<div id="pg-lean-file-list" style="max-height:200px;overflow-y:auto">';
      files.slice(0, 100).forEach(function (f) {
        html += '<div class="pg-tactic-item pg-lean-file-card" data-lean-path="' + esc(f.path) + '" ' +
          'style="padding:4px 8px;font-size:11px;cursor:pointer">' +
          '<span style="color:#78ff74">' + esc(f.name.replace('.lean', '')) + '</span>' +
          '<span style="color:#3a6030;margin-left:8px;font-size:10px">' + esc(f.path) + '</span>' +
          '</div>';
      });
      if (files.length > 100) {
        html += '<div style="color:#4ca43a;font-size:10px;padding:4px 8px">+' + (files.length - 100) + ' more — use filter</div>';
      }
      html += '</div>';
      container.innerHTML = html;

      // Search filter
      var searchInput = container.querySelector('#pg-lean-search');
      if (searchInput) {
        searchInput.addEventListener('input', function () {
          var q = searchInput.value.toLowerCase();
          container.querySelectorAll('.pg-lean-file-card').forEach(function (card) {
            var path = (card.getAttribute('data-lean-path') || '').toLowerCase();
            card.style.display = (!q || path.indexOf(q) >= 0) ? '' : 'none';
          });
        });
      }

      // Click to load file and extract theorems
      container.querySelectorAll('.pg-lean-file-card').forEach(function (card) {
        card.addEventListener('click', function () {
          var path = card.getAttribute('data-lean-path');
          loadLeanFileTheorems(path, container);
        });
      });
    }).catch(function (e) {
      container.innerHTML = '<div style="color:#ff6666;font-size:12px">Error: ' + esc(String(e)) + '</div>';
    });
  }

  // Load a specific Lean file and extract theorem declarations
  function loadLeanFileTheorems(path, container) {
    container.innerHTML = '<div style="color:#4ca43a;font-size:12px">Loading ' + esc(path) + '…</div>';

    fetch('/api/lean/file?path=' + encodeURIComponent(path)).then(function (resp) {
      if (!resp.ok) throw new Error('HTTP ' + resp.status);
      return resp.json();
    }).then(function (res) {
      var content = (res && res.content) || '';
      if (!content) {
        container.innerHTML = '<div style="color:#ff6666;font-size:12px">Empty file</div>';
        return;
      }

      // Extract all declarations (theorem, lemma, def, structure, inductive, etc.)
      var decls = [];
      var lines = content.split('\n');
      var declPattern = /^(?:private\s+|protected\s+|noncomputable\s+|@\[.*?\]\s*)*(theorem|lemma|def|structure|inductive|class|instance|abbrev|opaque|axiom)\s+(\S+)\s*(.*)/;
      for (var i = 0; i < lines.length; i++) {
        var line = lines[i].trim();
        var m = line.match(declPattern);
        if (m) {
          // Collect the full statement up to `:= by` or `:=` or `where`
          var stmt = line;
          var j = i + 1;
          while (j < lines.length && j < i + 8 && !/:=/.test(stmt) && !/\bwhere\b/.test(stmt)) {
            stmt += ' ' + lines[j].trim();
            j++;
          }
          // Clean up: remove everything after `:=` or `where`
          stmt = stmt.replace(/:=[\s\S]*$/, '').replace(/\bwhere[\s\S]*$/, '').trim();
          decls.push({ kind: m[1], name: m[2], statement: stmt, line: i + 1 });
        }
      }

      if (decls.length === 0) {
        container.innerHTML = '<div style="color:#ffaa00;font-size:12px">No declarations found in ' + esc(path) +
          '</div><button class="pg-btn" id="pg-lean-back" style="margin-top:6px">← Back</button>';
        container.querySelector('#pg-lean-back').addEventListener('click', function () { loadLeanTheorems(container); });
        return;
      }

      var html = '<button class="pg-btn" id="pg-lean-back" style="margin-bottom:8px">← Back</button>' +
        '<div style="color:#4ca43a;font-size:11px;margin-bottom:6px">' + decls.length + ' declarations in ' + esc(path) + '</div>' +
        '<div style="max-height:240px;overflow-y:auto">';
      decls.forEach(function (d) {
        html += '<div class="pg-theorem-card pg-lean-decl-card" data-lean-stmt="' + esc(d.statement) + '" data-lean-name="' + esc(d.name) + '">' +
          '<div class="pg-tc-name">' + esc(d.name) + '</div>' +
          '<div class="pg-tc-stmt">' + esc(d.statement) + '</div>' +
          '<div class="pg-tc-meta"><span class="pg-tag">' + esc(d.kind) + '</span><span class="pg-tag">line ' + d.line + '</span></div>' +
          '</div>';
      });
      html += '</div>';
      container.innerHTML = html;

      container.querySelector('#pg-lean-back').addEventListener('click', function () { loadLeanTheorems(container); });
      // Store file content for proof body extraction
      var fileContent = content;

      container.querySelectorAll('.pg-lean-decl-card').forEach(function (card) {
        card.addEventListener('click', function () {
          var stmt = card.getAttribute('data-lean-stmt');
          var name = card.getAttribute('data-lean-name');
          // Try to match against built-in library first
          var match = LIBRARY.find(function (t) {
            return t.statement.toLowerCase().includes(name.toLowerCase());
          });
          if (match) {
            loadTheorem(match);
            hideLibrary();
          } else if (serverMode) {
            // Server mode: use Pantograph to start a real proof session
            hideLibrary();
            loadLeanTheoremServerMode(stmt, name, fileContent);
          } else {
            // Simulation fallback: try to extract proof body for structural tree
            var tactics = extractProofBody(fileContent, name);
            if (tactics && tactics.length > 0) {
              var thm = buildTheoremFromTactics(stmt, name, tactics);
              loadTheorem(thm);
            } else {
              loadLeanTheoremSimulation(stmt, name);
            }
            hideLibrary();
          }
        });
      });
    }).catch(function (e) {
      container.innerHTML = '<div style="color:#ff6666;font-size:12px">Error: ' + esc(String(e)) + '</div>';
    });
  }

  // ═══════════════════════════════════════════════════════════════
  // §9c  Server Mode: Suggest Tactics + Lean DB Proof Extraction
  // ═══════════════════════════════════════════════════════════════

  function fetchSuggestions(node, tacList) {
    if (!node || node.stateId === undefined) return;
    var btn = tacList.querySelector('.pg-btn');
    if (btn) btn.textContent = 'Searching…';

    fetch('/api/explorer/suggest', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        state_id: node.stateId,
        goal_id: node.goalId || 0,
      }),
    })
    .then(function (r) { return r.json(); })
    .then(function (data) {
      if (btn) btn.textContent = 'Suggest Tactics';
      if (!data.ok || !data.tactics || !data.tactics.length) {
        if (btn) btn.textContent = 'No suggestions found';
        return;
      }
      // Add suggested tactics to the list
      data.tactics.forEach(function (s) {
        var div = document.createElement('div');
        div.className = 'pg-tactic-item';
        var badge = s.solves
          ? '<span style="color:#39ff14;font-size:9px;margin-left:4px">QED</span>'
          : '<span style="color:#00aaff;font-size:9px;margin-left:4px">' + s.goals_after + ' goals</span>';
        div.innerHTML = '<span class="pg-tactic-arrow">▸</span>' +
          '<span class="pg-tactic-name">' + esc(s.tactic) + badge + '</span>' +
          '<span class="pg-tactic-desc">' + esc(TACTIC_DESC[s.tactic] || '') + '</span>';
        div.addEventListener('click', function () { doApplyTactic(s.tactic); });
        tacList.appendChild(div);
      });
      // Update the serverGoal suggested list
      if (node.serverGoal) {
        node.serverGoal.suggested = data.tactics.map(function (s) { return s.tactic; });
      }
    })
    .catch(function () {
      if (btn) btn.textContent = 'Suggest Tactics';
    });
  }

  // Extract proof body (tactic lines) from Lean file content for a declaration
  function extractProofBody(fileContent, declName) {
    var lines = fileContent.split('\n');
    var startIdx = -1;
    for (var i = 0; i < lines.length; i++) {
      if (lines[i].match(new RegExp('(theorem|lemma)\\s+' + declName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '\\b'))) {
        startIdx = i;
        break;
      }
    }
    if (startIdx < 0) return null;

    // Find := by
    var bodyStart = -1;
    for (var j = startIdx; j < Math.min(startIdx + 10, lines.length); j++) {
      if (lines[j].indexOf(':= by') >= 0) {
        bodyStart = j;
        break;
      }
    }
    if (bodyStart < 0) return null;

    // Collect tactic lines until next top-level declaration or dedent
    var tactics = [];
    var baseIndent = lines[bodyStart].search(/\S/);
    for (var k = bodyStart + 1; k < lines.length; k++) {
      var line = lines[k];
      var trimmed = line.trim();
      if (!trimmed || trimmed.indexOf('--') === 0) continue;
      var indent = line.search(/\S/);
      if (indent >= 0 && indent <= baseIndent && /^(theorem|lemma|def|structure|inductive|class|end|namespace|section)/.test(trimmed)) break;
      tactics.push(trimmed);
    }
    return tactics.length > 0 ? tactics : null;
  }

  // Server-mode Lean DB import: start goal, then replay tactics to build tree
  function loadLeanTheoremServerMode(stmt, name, fileContent) {
    // Extract the type expression from the statement
    var typeExpr = stmt.replace(/^(theorem|lemma)\s+\S+\s*/, '').trim();
    // Remove everything after the last : to get the type
    var colonIdx = typeExpr.lastIndexOf(':');
    if (colonIdx > 0) typeExpr = typeExpr.slice(colonIdx + 1).trim();

    // Start a proof goal via Pantograph
    fetch('/api/explorer/load', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ expr: typeExpr }),
    })
    .then(function (r) { return r.json(); })
    .then(function (data) {
      if (!data.ok) {
        // Fall back to simulation mode
        loadLeanTheoremSimulation(stmt, name);
        return;
      }

      // Create root session from server response
      nodeSeq = 0;
      var rootId = mkId();
      var nodes = {};
      var goals = data.goals || [];
      var rootGoal = goals[0] || { target: typeExpr, vars: [], goalId: 0 };
      var hyps = (rootGoal.vars || []).map(function (v) { return { n: v.name, t: v.type || v['type'] }; });
      var display = hyps.map(function (h) { return h.n + ' : ' + h.t; }).join(', ');
      if (display) display += ' ';
      display += '⊢ ' + (rootGoal.target || '?');

      nodes[rootId] = {
        id: rootId, goalKey: null, status: 'open',
        stateId: data.stateId,
        goalId: rootGoal.goalId || 0,
        serverGoal: {
          display: display,
          hyps: hyps,
          tactics: {},
          suggested: [],
        },
        parentId: null, parentTactic: null,
        x: 0, y: 0, animT: 1,
      };

      session = {
        theorem: {
          id: 'lean_' + Date.now(), name: name, category: 'Lean Import',
          statement: stmt, difficulty: 0, tags: ['lean', 'server'],
          hint: 'Server mode — apply real Lean tactics.',
          rootGoal: '_server_', goals: {},
        },
        nodes: nodes,
        edges: [],
        rootId: rootId,
        selectedId: rootId,
        solvedSet: {},
        tacticsApplied: 0,
        branchesExplored: 0,
      };

      // If we have the proof body, auto-apply tactics to build tree
      var tactics = fileContent ? extractProofBody(fileContent, name) : null;
      if (tactics && tactics.length > 0) {
        replayTacticsSequentially(rootId, tactics, 0);
      } else {
        // Just show the root goal — user applies tactics interactively
        computeLayout();
        cam = { x: 0, y: 0, zoom: fitTreeZoom() };
        needsRender = true;
        var welcome = document.getElementById('pg-welcome');
        if (welcome) welcome.style.display = 'none';
        selectNode(rootId);
      }
    })
    .catch(function () {
      loadLeanTheoremSimulation(stmt, name);
    });
  }

  // Replay tactics one at a time to build the proof tree from the server
  function replayTacticsSequentially(nodeId, tactics, idx) {
    if (idx >= tactics.length || !session) {
      computeLayout();
      cam = { x: 0, y: 0, zoom: fitTreeZoom() };
      needsRender = true;
      var welcome = document.getElementById('pg-welcome');
      if (welcome) welcome.style.display = 'none';
      selectNode(session.rootId);
      return;
    }

    var node = session.nodes[nodeId];
    if (!node || node.stateId === undefined) {
      // Can't continue replay — show what we have
      computeLayout();
      cam = { x: 0, y: 0, zoom: fitTreeZoom() };
      needsRender = true;
      var welcome2 = document.getElementById('pg-welcome');
      if (welcome2) welcome2.style.display = 'none';
      selectNode(session.rootId);
      return;
    }

    var tactic = tactics[idx];
    fetch('/api/explorer/tactic', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        state_id: node.stateId,
        goal_id: node.goalId || 0,
        tactic: tactic,
      }),
    })
    .then(function (r) { return r.json(); })
    .then(function (data) {
      if (!data.ok) {
        // Tactic failed — stop replaying, show what we have
        replayTacticsSequentially(null, [], tactics.length);
        return;
      }

      session.tacticsApplied++;
      if (data.solved) {
        node.status = 'solved';
        session.solvedSet[node.id] = true;
        session.edges.push({ from: node.id, to: null, tactic: tactic, group: node.id, status: 'applied' });
        propagateSolved(node.parentId);
        // Continue with next open node
        var nextOpen = findNextOpenServerNode();
        if (nextOpen) {
          replayTacticsSequentially(nextOpen, tactics, idx + 1);
        } else {
          replayTacticsSequentially(null, [], tactics.length);
        }
      } else {
        var groupId = mkId();
        var childIds = [];
        (data.goals || []).forEach(function (goal) {
          var nid = mkId();
          var hyps = (goal.vars || []).map(function (v) { return { n: v.name, t: v.type || v['type'] }; });
          var display = hyps.map(function (h) { return h.n + ' : ' + h.t; }).join(', ');
          if (display) display += ' ';
          display += '⊢ ' + (goal.target || '?');
          session.nodes[nid] = {
            id: nid, goalKey: null, status: 'open',
            stateId: data.stateId, goalId: goal.goalId,
            serverGoal: { display: display, hyps: hyps, tactics: {}, suggested: [] },
            parentId: node.id, parentTactic: tactic,
            x: 0, y: 0, animT: 1,
          };
          session.edges.push({ from: node.id, to: nid, tactic: tactic, group: groupId, status: 'applied' });
          childIds.push(nid);
        });
        session.branchesExplored++;
        // Continue replaying on first child
        if (childIds.length > 0) {
          replayTacticsSequentially(childIds[0], tactics, idx + 1);
        } else {
          replayTacticsSequentially(null, [], tactics.length);
        }
      }
    })
    .catch(function () {
      replayTacticsSequentially(null, [], tactics.length);
    });
  }

  function findNextOpenServerNode() {
    if (!session) return null;
    var nodes = Object.values(session.nodes);
    for (var i = 0; i < nodes.length; i++) {
      if (nodes[i].status === 'open' && nodes[i].stateId !== undefined) return nodes[i].id;
    }
    return null;
  }

  // Build a theorem from extracted tactic lines (static analysis, no server needed).
  // Parses indentation + focus dots (·) to reconstruct branching.
  function buildTheoremFromTactics(stmt, name, tactics) {
    var goalSeq = 0;
    function gk() { return 'tg' + (goalSeq++); }

    // Parse tactic structure: indentation levels and focus dots indicate branching
    function parseTacticTree(tacLines, start, baseIndent) {
      var children = [];
      var i = start;
      while (i < tacLines.length) {
        var line = tacLines[i];
        var indent = line.length - line.replace(/^\s+/, '').length;
        var trimmed = line.replace(/^\s+/, '');

        // If indent <= baseIndent and not a focus dot continuation, we've exited this block
        if (i > start && indent <= baseIndent && !trimmed.match(/^[·•<;]/)) break;

        // Focus dot (·) indicates a branch
        if (trimmed.match(/^[·•]\s*/)) {
          var branchTac = trimmed.replace(/^[·•]\s*/, '');
          var branchChildren = [];
          // Collect sub-tactics under this focus dot
          var j = i + 1;
          while (j < tacLines.length) {
            var subLine = tacLines[j];
            var subIndent = subLine.length - subLine.replace(/^\s+/, '').length;
            var subTrimmed = subLine.replace(/^\s+/, '');
            if (subIndent <= indent && !subTrimmed.match(/^[·•<;]/)) break;
            if (subTrimmed.match(/^[·•]\s*/) && subIndent <= indent) break;
            branchChildren.push(subTrimmed);
            j++;
          }
          children.push({ tactic: branchTac, subTactics: branchChildren });
          i = j;
        } else {
          children.push({ tactic: trimmed, subTactics: [] });
          i++;
        }
      }
      return children;
    }

    var tree = parseTacticTree(tactics, 0, -1);

    // Convert parsed tree to theorem goals structure
    var goals = {};

    function buildGoalTree(parsed, parentDisplay) {
      var key = gk();
      var tacticMap = {};
      var suggested = [];

      if (parsed.length === 0) {
        // Leaf node — tactic closes the goal
        goals[key] = {
          display: parentDisplay || '⊢ (goal)',
          hyps: [],
          tactics: { 'QED': [] },
          suggested: ['QED'],
        };
        return key;
      }

      // Check if this level has focus dots (branching) or sequential tactics
      var hasBranching = false;
      var branchChildren = [];
      var seqTactics = [];

      for (var i = 0; i < parsed.length; i++) {
        var p = parsed[i];
        if (p.subTactics && p.subTactics.length > 0) {
          hasBranching = true;
        }
      }

      if (hasBranching || parsed.length > 1) {
        // First sequential tactic leads to branching
        var firstTac = parsed[0].tactic;
        var subGoalKeys = [];
        for (var j = 0; j < parsed.length; j++) {
          var childKey = gk();
          var childDisplay = '⊢ subgoal ' + (j + 1) + ' after ' + firstTac;
          var childTactics = {};
          var childSuggested = [];

          if (parsed[j].subTactics && parsed[j].subTactics.length > 0) {
            // This branch has sub-tactics
            var subParsed = parseTacticTree(parsed[j].subTactics, 0, -1);
            if (subParsed.length > 0) {
              var subKey = buildGoalTree(subParsed, childDisplay);
              // The branch tactic leads to the sub-tree
              childTactics[parsed[j].tactic] = [subKey];
              childSuggested.push(parsed[j].tactic);
            } else {
              childTactics[parsed[j].tactic] = [];
              childSuggested.push(parsed[j].tactic);
            }
          } else {
            // Leaf branch
            childTactics[parsed[j].tactic] = [];
            childSuggested.push(parsed[j].tactic);
          }

          goals[childKey] = {
            display: childDisplay,
            hyps: [],
            tactics: childTactics,
            suggested: childSuggested,
          };
          subGoalKeys.push(childKey);
        }

        if (subGoalKeys.length > 1) {
          tacticMap[firstTac] = subGoalKeys;
          suggested.push(firstTac);
        } else if (subGoalKeys.length === 1) {
          tacticMap[firstTac] = subGoalKeys;
          suggested.push(firstTac);
        }
      } else {
        // Single sequential tactic
        var tac = parsed[0].tactic;
        tacticMap[tac] = [];
        suggested.push(tac);
      }

      goals[key] = {
        display: parentDisplay || '⊢ (goal)',
        hyps: [],
        tactics: tacticMap,
        suggested: suggested,
      };
      return key;
    }

    // Extract the type from the statement for root display
    var typeExpr = stmt.replace(/^(theorem|lemma)\s+\S+\s*/, '').trim();
    var colonIdx = typeExpr.lastIndexOf(':');
    if (colonIdx > 0) typeExpr = typeExpr.slice(colonIdx + 1).trim();

    var rootKey = buildGoalTree(tree, '⊢ ' + typeExpr);

    return {
      id: 'lean_' + Date.now(),
      name: name,
      category: 'Lean Import',
      statement: stmt,
      difficulty: Math.min(5, Math.max(1, Math.ceil(tactics.length / 3))),
      tags: ['lean', 'imported', 'structural'],
      hint: 'Proof structure extracted from Lean source. Connect a Lean server for full goal states.',
      rootGoal: rootKey,
      goals: goals,
    };
  }

  // Fallback: create a sorry-based theorem for simulation mode
  function loadLeanTheoremSimulation(stmt, name) {
    var goalDisplay = stmt.replace(/^(theorem|lemma)\s+\S+\s*/, '').trim();
    var colonIdx = goalDisplay.lastIndexOf(':');
    if (colonIdx > 0) goalDisplay = goalDisplay.slice(colonIdx + 1).trim();
    var custom = {
      id: 'lean_' + Date.now(), name: name, category: 'Lean Import',
      statement: stmt, difficulty: 0, tags: ['lean', 'imported'],
      hint: 'Imported from Lean project. Connect a Lean proof server for interactive tactics.',
      rootGoal: 'r',
      goals: { r: { display: '⊢ ' + goalDisplay, hyps: [], tactics: { 'sorry': [] }, suggested: ['sorry'] } }
    };
    loadTheorem(custom);
  }

  // ═══════════════════════════════════════════════════════════════
  // §10  Helpers
  // ═══════════════════════════════════════════════════════════════

  function esc(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
  function setText(id, val) { var el = document.getElementById(id); if (el) el.textContent = val; }

  var timerInterval = null; // kept for cleanup compatibility

  // Cleanup: remove all listeners, stop timers, cancel animation frame
  function cleanup() {
    if (animFrame) { cancelAnimationFrame(animFrame); animFrame = 0; }
    if (timerInterval) { clearInterval(timerInterval); timerInterval = null; }
    boundListeners.forEach(function (l) {
      l.target.removeEventListener(l.event, l.handler, l.opts);
    });
    boundListeners = [];
    canvas = null;
    ctx = null;
    drag = null;
  }

  // ═══════════════════════════════════════════════════════════════
  // §11  Page Render
  // ═══════════════════════════════════════════════════════════════

  window.renderProofGamePage = function () {
    var content = document.getElementById('content');
    if (!content) return;
    // Stop previous animation loop
    if (animFrame) { cancelAnimationFrame(animFrame); animFrame = 0; }
    if (timerInterval) { clearInterval(timerInterval); timerInterval = null; }

    content.innerHTML =
      '<link rel="stylesheet" href="proof-game.css?v=2">' +
      '<div class="pg-page">' +
      '  <div class="pg-topbar">' +
      '    <span class="pg-title">Proof <span class="pg-title-accent">Builder</span></span>' +
      '    <button class="pg-btn" id="pg-load-btn">Load ▼</button>' +
      '    <button class="pg-btn" id="pg-new-btn">New</button>' +
      '    <span class="pg-theorem-name" id="pg-theorem-display"></span>' +
      '    <span style="flex:1"></span>' +
      '    <button class="pg-btn primary" id="pg-verify-btn">Verify ✓</button>' +
      '    <button class="pg-btn" id="pg-export-btn" disabled>Export</button>' +
      '    <span class="pg-status"><span class="pg-status-dot simulated"></span> Simulation</span>' +
      '  </div>' +
      '  <div class="pg-main">' +
      '    <div class="pg-graph" id="pg-graph">' +
      '      <canvas id="pg-canvas"></canvas>' +
      '      <div class="pg-welcome" id="pg-welcome">' +
      '        <div class="pg-welcome-title">Proof Builder</div>' +
      '        <div class="pg-welcome-sub">Navigate proof trees by selecting tactics. ' +
      '          Explore branching paths. Solve all goals to complete the proof.</div>' +
      '        <button class="pg-btn primary" id="pg-welcome-load">Browse Library</button>' +
      '        <div class="pg-welcome-mode">' + (serverMode ? 'Server mode' : 'Simulation mode — ' + LIBRARY.length + ' theorems') + ' v' + PG_VERSION + '</div>' +
      '      </div>' +
      '    </div>' +
      '    <div class="pg-context" id="pg-context">' +
      '      <div class="pg-section">' +
      '        <div class="pg-section-title">Current Goal</div>' +
      '        <div class="pg-goal-display" id="pg-goal-display">Select a node to view its goal state</div>' +
      '      </div>' +
      '      <div class="pg-section">' +
      '        <div class="pg-section-title">Hypotheses</div>' +
      '        <ul class="pg-hyp-list" id="pg-hyp-list"></ul>' +
      '      </div>' +
      '      <div class="pg-section">' +
      '        <div class="pg-section-title">Available Tactics</div>' +
      '        <div class="pg-tactic-list" id="pg-tactic-list"></div>' +
      '        <div class="pg-tactic-input-row">' +
      '          <input class="pg-tactic-input" id="pg-tactic-input" placeholder="Type tactic..." />' +
      '          <button class="pg-btn" id="pg-tactic-apply">Apply</button>' +
      '        </div>' +
      '      </div>' +
      '      <div class="pg-section" style="flex:1">' +
      '        <div class="pg-section-title">Proof Script</div>' +
      '        <div class="pg-script" id="pg-script">-- No proof in progress</div>' +
      '      </div>' +
      '      <div class="pg-section">' +
      '        <div class="pg-section-title">AI Assist</div>' +
      '        <div class="pg-hint-btns">' +
      '          <button class="pg-btn" id="pg-hint-btn" title="Spawns HALO agent for tactic suggestion">Get Hint</button>' +
      '          <button class="pg-btn" id="pg-autosolve-btn" title="Spawns proof search agent">Auto-solve</button>' +
      '        </div>' +
      '      </div>' +
      '    </div>' +
      '  </div>' +
      '  <div class="pg-bottombar">' +
      '    <span class="pg-stat">Goals: <span class="pg-stat-value" id="pg-stat-goals">—</span></span>' +
      '    <span class="pg-stat-sep">|</span>' +
      '    <span class="pg-stat">Tactics: <span class="pg-stat-value" id="pg-stat-tactics">0</span></span>' +
      '    <span class="pg-stat-sep">|</span>' +
      '    <span class="pg-stat">Branches: <span class="pg-stat-value" id="pg-stat-branches">0</span></span>' +
      '    <span class="pg-stat-sep">|</span>' +
      '    <span class="pg-stat">Depth: <span class="pg-stat-value" id="pg-stat-depth">0</span></span>' +
      '    <span style="flex:1"></span>' +
      '    <button class="pg-btn" id="pg-undo-btn" disabled>Undo ↩</button>' +
      '  </div>' +
      '</div>';

    // Wire buttons
    document.getElementById('pg-load-btn').addEventListener('click', showLibrary);
    document.getElementById('pg-new-btn').addEventListener('click', showLibrary);
    document.getElementById('pg-welcome-load').addEventListener('click', showLibrary);
    document.getElementById('pg-verify-btn').addEventListener('click', doVerify);
    document.getElementById('pg-export-btn').addEventListener('click', doExport);
    document.getElementById('pg-undo-btn').addEventListener('click', doUndo);

    document.getElementById('pg-tactic-apply').addEventListener('click', function () {
      var input = document.getElementById('pg-tactic-input');
      if (input && input.value.trim()) { doApplyTactic(input.value.trim()); input.value = ''; }
    });
    document.getElementById('pg-tactic-input').addEventListener('keydown', function (e) {
      if (e.key === 'Enter') {
        e.preventDefault();
        var input = document.getElementById('pg-tactic-input');
        if (input && input.value.trim()) { doApplyTactic(input.value.trim()); input.value = ''; }
      }
    });

    // Hint / Auto-solve
    document.getElementById('pg-hint-btn').addEventListener('click', function () {
      if (!session || !session.selectedId) return;
      var node = session.nodes[session.selectedId];
      if (!node) return;

      if (serverMode && node.stateId !== undefined) {
        // Server mode: fetch suggestions
        var tacList = document.getElementById('pg-tactic-list');
        if (tacList) fetchSuggestions(node, tacList);
        return;
      }

      var goalDef = getGoalDef(node);
      if (!goalDef) return;
      var thm = session.theorem;
      var hint = thm.hint || 'No hint available.';
      var suggestion = goalDef.suggested && goalDef.suggested[0] ? goalDef.suggested[0] : null;
      var goalEl = document.getElementById('pg-goal-display');
      if (goalEl) {
        showInlineResult(goalEl, 'warning', 'Hint: ' + hint,
          suggestion ? 'Suggested tactic: ' + suggestion : null);
      }
    });

    document.getElementById('pg-autosolve-btn').addEventListener('click', function () {
      if (!session || !session.selectedId) return;
      var node = session.nodes[session.selectedId];
      if (!node || node.status !== 'open') return;

      if (serverMode && node.stateId !== undefined) {
        // Server mode: try auto-solve via backend
        var btn = document.getElementById('pg-autosolve-btn');
        if (btn) btn.textContent = 'Solving…';
        fetch('/api/explorer/autosolve', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ state_id: node.stateId, goal_id: node.goalId || 0 }),
        })
        .then(function (r) { return r.json(); })
        .then(function (data) {
          if (btn) btn.textContent = 'Auto-solve';
          if (data.ok && data.solved && data.tactic) {
            doApplyTactic(data.tactic);
          } else {
            showTacticError(data.message || 'No automatic solution found');
          }
        })
        .catch(function () { if (btn) btn.textContent = 'Auto-solve'; });
        return;
      }

      var goalDef = getGoalDef(node);
      if (!goalDef || !goalDef.suggested || !goalDef.suggested.length) {
        var goalEl2 = document.getElementById('pg-goal-display');
        if (goalEl2) showInlineResult(goalEl2, 'warning', 'Auto-solve', 'No solution found in simulation.');
        return;
      }
      doApplyTactic(goalDef.suggested[0]);
    });

    // Check server status on page load
    checkServerStatus();

    // Init canvas
    requestAnimationFrame(function () {
      initCanvas();
      // Restore session if exists
      if (session) {
        var welcome = document.getElementById('pg-welcome');
        if (welcome) welcome.style.display = 'none';
        computeLayout();
        updateContextPanel();
      }
    });

    // Resize handler (tracked for cleanup)
    addTrackedListener(window, 'resize', resizeCanvas);
  };

  // Teardown — called by SPA router when navigating away
  window.teardownProofGamePage = function () {
    cleanup();
  };

})();
