/* ═══════════════════════════════════════════════════════════════
   Proof Game — Interactive Theorem Proving via Multiway Proof Trees
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
  var NODE_W = 150, NODE_H = 44, NODE_R = 6;
  var LEVEL_H = 100, SIB_GAP = 24;
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
      startTime: Date.now(),
      victoryTime: null,
    };
    return session;
  }

  function mkId() { return 'n' + (nodeSeq++); }

  function getGoalDef(node) {
    return session.theorem.goals[node.goalKey] || null;
  }

  function applyTactic(nodeId, tacticStr) {
    var node = session.nodes[nodeId];
    if (!node || node.status === 'solved' || node.status === 'inactive') return null;
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
    var oy = (cam.y * cam.zoom * dpr) % gridStep;
    for (var gx = ox; gx < W; gx += gridStep) { ctx.beginPath(); ctx.moveTo(gx, 0); ctx.lineTo(gx, H); ctx.stroke(); }
    for (var gy = oy; gy < H; gy += gridStep) { ctx.beginPath(); ctx.moveTo(0, gy); ctx.lineTo(W, gy); ctx.stroke(); }
    ctx.restore();

    if (!session) return;

    // Transform: center root at top-center
    ctx.save();
    ctx.translate(W / 2, 60 * dpr);
    ctx.scale(cam.zoom * dpr, cam.zoom * dpr);
    ctx.translate(cam.x, cam.y);

    // Draw edges first
    session.edges.forEach(function (e) {
      if (!e.to) return; // solve-in-place edge
      var from = session.nodes[e.from], to = session.nodes[e.to];
      if (!from || !to) return;
      drawEdge(from, to, e.tactic, e.status, to.animT);
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

  function drawEdge(from, to, tactic, status, t) {
    var alpha = Math.max(0.1, t);
    var isFailed = status === 'failed';
    ctx.save();
    ctx.globalAlpha = alpha * (isFailed ? 0.4 : 0.7);
    ctx.strokeStyle = isFailed ? '#ff3030' : 'rgba(53,255,62,0.35)';
    ctx.lineWidth = isFailed ? 1 : 1.5;
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

    // Edge label
    if (tactic) {
      var labelX = (from.x + to.x) / 2;
      var labelY = midY - 4;
      ctx.font = EDGE_LABEL_SIZE + 'px SF Mono, monospace';
      ctx.fillStyle = isFailed ? 'rgba(255,48,48,0.6)' : 'rgba(120,255,116,0.5)';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'bottom';
      var truncTac = tactic.length > 24 ? tactic.slice(0, 22) + '..' : tactic;
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

    // Goal text (truncated)
    var goalDef = node.goalKey ? (session.theorem.goals[node.goalKey] || null) : null;
    var label = '';
    if (node.status === 'failed') {
      label = node.errorMsg ? node.errorMsg.slice(0, 28) : 'Failed';
    } else if (goalDef) {
      // Show just the goal part (after ⊢)
      var disp = goalDef.display;
      var turnstile = disp.indexOf('⊢');
      label = turnstile >= 0 ? disp.slice(turnstile) : disp;
      if (label.length > 28) label = label.slice(0, 26) + '..';
    }
    ctx.font = '11px SF Mono, monospace';
    ctx.fillStyle = st.text;
    ctx.textAlign = 'left';
    ctx.textBaseline = 'middle';
    ctx.fillText(label, -hw + 8, 0);

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
    var wy = (cy - 60 * dpr) / (cam.zoom * dpr) - cam.y;
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
      if (goalDef && node.status === 'open') {
        (goalDef.suggested || []).forEach(function (tac) {
          var div = document.createElement('div');
          div.className = 'pg-tactic-item';
          var baseTac = tac.split(' ')[0];
          div.innerHTML = '<span class="pg-tactic-arrow">▸</span>' +
            '<span class="pg-tactic-name">' + esc(tac) + '</span>' +
            '<span class="pg-tactic-desc">' + esc(TACTIC_DESC[baseTac] || '') + '</span>';
          div.addEventListener('click', function () { doApplyTactic(tac); });
          tacList.appendChild(div);
        });
      } else if (node.status === 'solved') {
        tacList.innerHTML = '<div style="color:#39ff14;padding:8px;font-size:12px">Goal solved ✓</div>';
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

    setText('pg-stat-goals', open.length + ' remaining');
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

    // Timer
    var elapsed = Math.floor((Date.now() - session.startTime) / 1000);
    var mins = Math.floor(elapsed / 60);
    var secs = elapsed % 60;
    setText('pg-stat-time', mins + ':' + (secs < 10 ? '0' : '') + secs);

    // Theorem display
    setText('pg-theorem-display', session.theorem.statement);

    // Buttons
    var verifyBtn = document.getElementById('pg-verify-btn');
    var exportBtn = document.getElementById('pg-export-btn');
    var undoBtn = document.getElementById('pg-undo-btn');
    if (verifyBtn) verifyBtn.disabled = !isVictory();
    if (exportBtn) exportBtn.disabled = false;
    if (undoBtn) undoBtn.disabled = !session.selectedId || session.selectedId === session.rootId;

    // Status dot
    var dot = document.querySelector('.pg-status-dot');
    if (dot) {
      dot.className = 'pg-status-dot ' + (isVictory() ? 'victory' : 'simulated');
      var statusText = dot.parentElement && dot.parentElement.lastChild;
      if (statusText && statusText.nodeType === 3) {
        statusText.textContent = isVictory() ? ' Victory!' : ' Simulation';
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
    if (!node || node.status !== 'open') return;

    var result = applyTactic(session.selectedId, tacticStr);
    if (!result) return;

    computeLayout();
    centerOnNode(session.selectedId);
    needsRender = true;
    updateContextPanel();

    if (isVictory() && !session.victoryTime) {
      session.victoryTime = Date.now();
      showVictory();
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
    if (!session || !isVictory()) return;
    alert('Proof verified (simulation mode).\n\nAll goals solved. In production, the Lean compiler would verify the complete tactic script.\n\nScript:\n' + buildProofScript());
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
    var graph = document.getElementById('pg-graph');
    if (!graph) return;
    // Remove existing overlay
    var existing = graph.querySelector('.pg-victory-overlay');
    if (existing) existing.remove();

    var isSorry = hasSorryInProof();
    var overlay = document.createElement('div');
    overlay.className = 'pg-victory-overlay';
    overlay.innerHTML = (isSorry
      ? '<div class="pg-victory-text" style="color:#ffaa00;text-shadow:0 0 20px #ffaa00">Incomplete</div>' +
        '<div class="pg-victory-sub" style="color:#ffcc44">Proof uses sorry — connect a Lean server for genuine verification</div>'
      : '<div class="pg-victory-text">QED</div>' +
        '<div class="pg-victory-sub">All goals solved — proof complete</div>') +
      '<div class="pg-victory-btns">' +
      '<button class="pg-btn primary" id="pg-v-verify">Verify ✓</button>' +
      '<button class="pg-btn" id="pg-v-export">Export .lean</button>' +
      '<button class="pg-btn" id="pg-v-dismiss">Continue</button>' +
      '</div>';
    graph.appendChild(overlay);

    overlay.querySelector('#pg-v-verify').addEventListener('click', doVerify);
    overlay.querySelector('#pg-v-export').addEventListener('click', doExport);
    overlay.querySelector('#pg-v-dismiss').addEventListener('click', function () { overlay.remove(); });
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
  }

  function hideLibrary() {
    var overlay = document.querySelector('.pg-modal-overlay');
    if (overlay) overlay.remove();
  }

  function loadTheorem(thm) {
    newSession(thm);
    computeLayout();
    cam = { x: 0, y: 0, zoom: 1 };
    needsRender = true;

    // Hide welcome
    var welcome = document.getElementById('pg-welcome');
    if (welcome) welcome.style.display = 'none';

    selectNode(session.rootId);
  }

  // ═══════════════════════════════════════════════════════════════
  // §10  Helpers
  // ═══════════════════════════════════════════════════════════════

  function esc(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
  function setText(id, val) { var el = document.getElementById(id); if (el) el.textContent = val; }

  // Timer update
  var timerInterval = null;
  function startTimer() {
    if (timerInterval) clearInterval(timerInterval);
    timerInterval = setInterval(function () {
      if (session && !session.victoryTime) updateStats();
    }, 1000);
  }

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
      '<link rel="stylesheet" href="proof-game.css">' +
      '<div class="pg-page">' +
      '  <div class="pg-topbar">' +
      '    <span class="pg-title">Proof <span class="pg-title-accent">Game</span></span>' +
      '    <button class="pg-btn" id="pg-load-btn">Load ▼</button>' +
      '    <button class="pg-btn" id="pg-new-btn">New</button>' +
      '    <span class="pg-theorem-name" id="pg-theorem-display"></span>' +
      '    <span style="flex:1"></span>' +
      '    <button class="pg-btn primary" id="pg-verify-btn" disabled>Verify ✓</button>' +
      '    <button class="pg-btn" id="pg-export-btn" disabled>Export</button>' +
      '    <span class="pg-status"><span class="pg-status-dot simulated"></span> Simulation</span>' +
      '  </div>' +
      '  <div class="pg-main">' +
      '    <div class="pg-graph" id="pg-graph">' +
      '      <canvas id="pg-canvas"></canvas>' +
      '      <div class="pg-welcome" id="pg-welcome">' +
      '        <div class="pg-welcome-title">Proof Game</div>' +
      '        <div class="pg-welcome-sub">Navigate proof trees by selecting tactics. ' +
      '          Explore branching paths. Solve all goals to complete the proof.</div>' +
      '        <button class="pg-btn primary" id="pg-welcome-load">Browse Library</button>' +
      '        <div class="pg-welcome-mode">Simulation mode — pre-computed proof trees for ' + LIBRARY.length + ' theorems</div>' +
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
      '    <span class="pg-stat-sep">|</span>' +
      '    <span class="pg-stat">Time: <span class="pg-stat-value" id="pg-stat-time">0:00</span></span>' +
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

    // Hint / Auto-solve stubs
    document.getElementById('pg-hint-btn').addEventListener('click', function () {
      if (!session || !session.selectedId) return;
      var node = session.nodes[session.selectedId];
      var goalDef = getGoalDef(node);
      if (!goalDef) return;
      var thm = session.theorem;
      var hint = thm.hint || 'No hint available.';
      // Show first suggested tactic as hint
      var suggestion = goalDef.suggested && goalDef.suggested[0] ? goalDef.suggested[0] : null;
      alert('Hint: ' + hint + (suggestion ? '\n\nSuggested tactic: ' + suggestion : ''));
    });

    document.getElementById('pg-autosolve-btn').addEventListener('click', function () {
      if (!session || !session.selectedId) return;
      var node = session.nodes[session.selectedId];
      if (!node || node.status !== 'open') return;
      var goalDef = getGoalDef(node);
      if (!goalDef || !goalDef.suggested || !goalDef.suggested.length) {
        alert('Auto-solve: No solution found in simulation.');
        return;
      }
      // Auto-apply first suggested tactic
      doApplyTactic(goalDef.suggested[0]);
    });

    // Init canvas + timer
    requestAnimationFrame(function () {
      initCanvas();
      startTimer();
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
