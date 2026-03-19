/* AgentHALO — Orchestration Workflow Canvas
 *
 * Workflow JSON schema (stored/loaded):
 * {
 *   workflow_id: string,
 *   name: string,
 *   version: number,
 *   created_at: number,   // unix seconds
 *   updated_at: number,
 *   litegraph: object,    // raw graph.serialize() output
 *   halo_meta: {
 *     description: string,
 *     max_iterations: number,
 *     role_definitions: { [nodeId]: { role_name, agent_type, default_model, skill_ref, prompt_template } }
 *   }
 * }
 */
'use strict';

(function () {
  /* ── Agent type colors ───────────────────────────── */
  const AGENT_COLORS = {
    claude:  '#7c3aed',
    gemini:  '#0ea5e9',
    codex:   '#22c55e',
    shell:   '#f59e0b',
    api:     '#6b7280',
    local:   '#6b7280',
  };

  const AGENT_TYPES = ['claude', 'gemini', 'codex', 'shell', 'api', 'local'];
  const CONDITION_TYPES = ['json_path', 'regex', 'contains', 'llm_judge', 'lean_typecheck', 'exit_code'];
  const TRANSFORM_TYPES = ['identity', 'json_extract', 'prefix', 'suffix', 'assistant_answer'];

  /* Models per agent type — used in property editor dropdown */
  const MODELS_BY_TYPE = {
    claude:  ['claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-5-20251001'],
    gemini:  ['gemini-2.5-pro', 'gemini-2.5-flash', 'gemini-2.0-flash'],
    codex:   ['codex-5.4-high', 'codex-5.4-medium', 'codex-5.4-low', 'codex-mini-latest'],
    shell:   ['local'],
    api:     ['openai/gpt-4o', 'openai/o3', 'openai/o4-mini', 'deepseek/deepseek-r1'],
    local:   ['local'],
  };
  const ALL_MODELS = Object.values(MODELS_BY_TYPE).flat();

  /* ── State ───────────────────────────────────────── */
  let __graph = null;
  let __canvas = null;
  let __canvasEl = null;
  let __resizeObs = null;
  let __currentWorkflowId = null;
  let __sidePanelCollapsed = false;

  /* ── Custom Node: Agent ──────────────────────────── */
  function HaloAgentNode() {
    this.addInput('task_input', 'string');
    this.addOutput('task_output', 'string');
    this.properties = {
      role_name: 'Agent',
      agent_type: 'claude',
      model: '',
      skill_ref: '',
      prompt_template: '',
      timeout_secs: 600,
    };
    this.size = [240, 110];
  }

  HaloAgentNode.title = 'Agent';
  HaloAgentNode.desc = 'Dispatches a task to a bound HALO agent';

  HaloAgentNode.prototype.onDrawForeground = function (ctx) {
    var c = AGENT_COLORS[this.properties.agent_type] || AGENT_COLORS.api;
    // Color bar at top
    ctx.fillStyle = c;
    ctx.fillRect(0, -LiteGraph.NODE_TITLE_HEIGHT, this.size[0], 4);

    // Role name
    ctx.font = 'bold 14px Space Grotesk, Arial, sans-serif';
    ctx.fillStyle = '#fff';
    ctx.textAlign = 'center';
    ctx.fillText(this.properties.role_name, this.size[0] * 0.5, 28);

    // Agent type + model
    ctx.font = '11px JetBrains Mono, monospace';
    ctx.fillStyle = '#aaa';
    var label = this.properties.agent_type;
    if (this.properties.model) label += ' / ' + this.properties.model;
    ctx.fillText(label, this.size[0] * 0.5, 46);

    // Skill ref
    if (this.properties.skill_ref) {
      ctx.font = '10px JetBrains Mono, monospace';
      ctx.fillStyle = '#666';
      ctx.fillText('skill: ' + this.properties.skill_ref, this.size[0] * 0.5, 62);
    }
  };

  HaloAgentNode.prototype.onPropertyChanged = function (name, value) {
    if (name === 'agent_type') {
      var models = MODELS_BY_TYPE[value] || ALL_MODELS;
      // Update the model widget dropdown values
      if (this.widgets) {
        for (var i = 0; i < this.widgets.length; i++) {
          var w = this.widgets[i];
          if (w.name === 'model' && w.options) {
            w.options.values = models;
            // Reset model if current value is not in the new list
            if (models.indexOf(this.properties.model) < 0) {
              this.properties.model = models[0] || '';
              w.value = this.properties.model;
            }
          }
        }
      }
      this.setDirtyCanvas(true);
    }
  };

  HaloAgentNode.prototype.getExtraMenuOptions = function () {
    var node = this;
    var skillOptions = [{ content: '(none)', callback: function () { node.properties.skill_ref = ''; node.setDirtyCanvas(true); } }];
    // Fetch skills asynchronously — fallback to static list if API unavailable
    try {
      var xhr = new XMLHttpRequest();
      xhr.open('GET', '/api/skills', false); // synchronous for menu
      xhr.send();
      if (xhr.status === 200) {
        var data = JSON.parse(xhr.responseText);
        var skills = Array.isArray(data.skills) ? data.skills : (Array.isArray(data) ? data : []);
        skills.forEach(function (s) {
          skillOptions.push({ content: s.skill_id || s.name, callback: function () {
            node.properties.skill_ref = s.skill_id || s.name || '';
            node.setDirtyCanvas(true);
          }});
        });
      }
    } catch (_e) { /* API unavailable — only "(none)" option shown */ }
    return [
      { content: 'Set Agent Type', has_submenu: true, callback: function () {},
        submenu: { options: AGENT_TYPES.map(function (t) {
          return { content: t, callback: function () {
            node.properties.agent_type = t;
            if (node.onPropertyChanged) node.onPropertyChanged('agent_type', t);
            node.setDirtyCanvas(true);
          }};
        })}
      },
      { content: 'Set Skill', has_submenu: true, callback: function () {},
        submenu: { options: skillOptions }
      }
    ];
  };

  /* ── Custom Node: Decision ───────────────────────── */
  function HaloDecisionNode() {
    this.addInput('input', 'string');
    this.addOutput('pass', 'string');
    this.addOutput('fail', 'string');
    this.properties = {
      condition_type: 'contains',
      condition_value: 'findings: 0',
      max_iterations: 10,
    };
    this.size = [200, 120];
    this.shape = LiteGraph.BOX_SHAPE;
  }

  HaloDecisionNode.title = 'Decision';
  HaloDecisionNode.desc = 'Conditional routing — pass or fail based on output analysis';

  HaloDecisionNode.prototype.onDrawForeground = function (ctx) {
    // Diamond overlay
    var w = this.size[0], h = this.size[1];
    ctx.save();
    ctx.beginPath();
    ctx.moveTo(w * 0.5, 4);
    ctx.lineTo(w - 8, h * 0.5);
    ctx.lineTo(w * 0.5, h - 4);
    ctx.lineTo(8, h * 0.5);
    ctx.closePath();
    ctx.strokeStyle = '#f59e0b';
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.restore();

    // Condition label
    ctx.font = 'bold 12px Space Grotesk, Arial, sans-serif';
    ctx.fillStyle = '#f59e0b';
    ctx.textAlign = 'center';
    ctx.fillText(this.properties.condition_type, w * 0.5, h * 0.5 - 8);

    ctx.font = '10px JetBrains Mono, monospace';
    ctx.fillStyle = '#aaa';
    var val = this.properties.condition_value;
    if (val.length > 24) val = val.slice(0, 22) + '..';
    ctx.fillText('"' + val + '"', w * 0.5, h * 0.5 + 8);

    // Max iterations badge
    ctx.font = '9px JetBrains Mono, monospace';
    ctx.fillStyle = '#666';
    ctx.fillText('max: ' + this.properties.max_iterations, w * 0.5, h * 0.5 + 24);
  };

  HaloDecisionNode.prototype.getExtraMenuOptions = function () {
    var node = this;
    return [
      { content: 'Set Condition Type', has_submenu: true, callback: function () {},
        submenu: { options: CONDITION_TYPES.map(function (t) {
          return { content: t, callback: function () { node.properties.condition_type = t; node.setDirtyCanvas(true); } };
        })}
      }
    ];
  };

  /* ── Custom Node: Transform ──────────────────────── */
  function HaloTransformNode() {
    this.addInput('input', 'string');
    this.addOutput('output', 'string');
    this.properties = {
      transform_type: 'identity',
      transform_value: '',
    };
    this.size = [180, 60];
  }

  HaloTransformNode.title = 'Transform';
  HaloTransformNode.desc = 'Transforms data between nodes (extract, prefix, etc.)';

  HaloTransformNode.prototype.onDrawForeground = function (ctx) {
    ctx.font = '11px JetBrains Mono, monospace';
    ctx.fillStyle = '#aaa';
    ctx.textAlign = 'center';
    var label = this.properties.transform_type;
    if (this.properties.transform_value) label += ': ' + this.properties.transform_value.slice(0, 16);
    ctx.fillText(label, this.size[0] * 0.5, 34);
  };

  HaloTransformNode.prototype.getExtraMenuOptions = function () {
    var node = this;
    return [
      { content: 'Set Transform Type', has_submenu: true, callback: function () {},
        submenu: { options: TRANSFORM_TYPES.map(function (t) {
          return { content: t, callback: function () { node.properties.transform_type = t; node.setDirtyCanvas(true); } };
        })}
      }
    ];
  };

  /* ── Custom Node: Phase ──────────────────────────── */
  function HaloPhaseNode() {
    this.addInput('phase_start', 'string');
    this.addOutput('phase_complete', 'string');
    this.properties = {
      phase_name: 'Phase 1',
      phase_number: 1,
      description: '',
    };
    this.size = [220, 80];
  }

  HaloPhaseNode.title = 'Phase';
  HaloPhaseNode.desc = 'Groups a workflow section into a named phase';

  HaloPhaseNode.prototype.onDrawForeground = function (ctx) {
    // Phase number badge
    ctx.fillStyle = '#3b82f6';
    ctx.fillRect(0, -LiteGraph.NODE_TITLE_HEIGHT, this.size[0], 3);

    ctx.font = 'bold 13px Space Grotesk, Arial, sans-serif';
    ctx.fillStyle = '#fff';
    ctx.textAlign = 'center';
    ctx.fillText(this.properties.phase_name, this.size[0] * 0.5, 30);

    if (this.properties.description) {
      ctx.font = '10px JetBrains Mono, monospace';
      ctx.fillStyle = '#888';
      var d = this.properties.description;
      if (d.length > 30) d = d.slice(0, 28) + '..';
      ctx.fillText(d, this.size[0] * 0.5, 48);
    }
  };

  /* ── Custom Node: Tool ──────────────────────────── */
  function HaloToolNode() {
    this.addInput('input', 'string');
    this.addOutput('output', 'string');
    this.properties = {
      tool_name: '',
      tool_category: 'atp',
      args_template: '',
      timeout_secs: 120,
    };
    this.size = [220, 90];
  }

  HaloToolNode.title = 'Tool';
  HaloToolNode.desc = 'Invokes an MCP tool from the heyting-local-tools registry';

  HaloToolNode.prototype.onDrawForeground = function (ctx) {
    ctx.fillStyle = '#6366f1';
    ctx.fillRect(0, -LiteGraph.NODE_TITLE_HEIGHT, this.size[0], 3);

    ctx.font = 'bold 13px Space Grotesk, Arial, sans-serif';
    ctx.fillStyle = '#fff';
    ctx.textAlign = 'center';
    ctx.fillText(this.properties.tool_name || 'Tool', this.size[0] * 0.5, 28);

    ctx.font = '10px JetBrains Mono, monospace';
    ctx.fillStyle = '#888';
    ctx.fillText('cat: ' + this.properties.tool_category, this.size[0] * 0.5, 46);
  };

  /* ── Custom Node: Skill ─────────────────────────── */
  function HaloSkillNode() {
    this.addInput('input', 'string');
    this.addOutput('output', 'string');
    this.properties = {
      skill_name: '',
      skill_args: '',
      timeout_secs: 300,
    };
    this.size = [220, 90];
  }

  HaloSkillNode.title = 'Skill';
  HaloSkillNode.desc = 'Invokes a registered agent skill (e.g. formal-proof, adversarial-audit)';

  HaloSkillNode.prototype.onDrawForeground = function (ctx) {
    ctx.fillStyle = '#8b5cf6';
    ctx.fillRect(0, -LiteGraph.NODE_TITLE_HEIGHT, this.size[0], 3);

    ctx.font = 'bold 13px Space Grotesk, Arial, sans-serif';
    ctx.fillStyle = '#fff';
    ctx.textAlign = 'center';
    ctx.fillText(this.properties.skill_name || 'Skill', this.size[0] * 0.5, 28);

    if (this.properties.skill_args) {
      ctx.font = '10px JetBrains Mono, monospace';
      ctx.fillStyle = '#888';
      var a = this.properties.skill_args;
      if (a.length > 30) a = a.slice(0, 28) + '..';
      ctx.fillText(a, this.size[0] * 0.5, 46);
    }
  };

  /* ── Custom Node: Lean Verifier Gate ────────────── */
  function HaloLeanVerifierNode() {
    this.addInput('lean_code', 'string');
    this.addOutput('pass', 'string');
    this.addOutput('fail', 'string');
    this.properties = {
      check_mode: 'typecheck',  // typecheck | no_sorry | build
      target_module: '',
      max_time_secs: 120,
    };
    this.size = [220, 120];
    this.shape = LiteGraph.BOX_SHAPE;
  }

  HaloLeanVerifierNode.title = 'Lean Verifier';
  HaloLeanVerifierNode.desc = 'Gate: typecheck, no-sorry guard, or full build. Pass/fail routing.';

  HaloLeanVerifierNode.prototype.onDrawForeground = function (ctx) {
    var w = this.size[0], h = this.size[1];
    // Shield icon outline
    ctx.save();
    ctx.beginPath();
    ctx.moveTo(w * 0.5, 6);
    ctx.lineTo(w - 16, h * 0.35);
    ctx.lineTo(w - 16, h * 0.65);
    ctx.lineTo(w * 0.5, h - 6);
    ctx.lineTo(16, h * 0.65);
    ctx.lineTo(16, h * 0.35);
    ctx.closePath();
    ctx.strokeStyle = '#10b981';
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.restore();

    ctx.font = 'bold 12px Space Grotesk, Arial, sans-serif';
    ctx.fillStyle = '#10b981';
    ctx.textAlign = 'center';
    ctx.fillText(this.properties.check_mode, w * 0.5, h * 0.5 - 6);

    if (this.properties.target_module) {
      ctx.font = '10px JetBrains Mono, monospace';
      ctx.fillStyle = '#888';
      var mod = this.properties.target_module;
      if (mod.length > 24) mod = mod.slice(0, 22) + '..';
      ctx.fillText(mod, w * 0.5, h * 0.5 + 10);
    }
  };

  /* ── Register all node types ─────────────────────── */
  function registerNodeTypes() {
    if (typeof LiteGraph === 'undefined') return;
    LiteGraph.registerNodeType('halo/agent', HaloAgentNode);
    LiteGraph.registerNodeType('halo/decision', HaloDecisionNode);
    LiteGraph.registerNodeType('halo/transform', HaloTransformNode);
    LiteGraph.registerNodeType('halo/phase', HaloPhaseNode);
    LiteGraph.registerNodeType('halo/tool', HaloToolNode);
    LiteGraph.registerNodeType('halo/skill', HaloSkillNode);
    LiteGraph.registerNodeType('halo/lean_verifier', HaloLeanVerifierNode);
  }

  /* ── Built-in workflow templates ─────────────────── */
  function getTemplates() {
    return [
      {
        name: 'Proof + Hostile Audit Loop',
        description: 'Prover completes a phase, auditor reviews. Loops until zero findings.',
        build: function (graph) {
          var prover = LiteGraph.createNode('halo/agent');
          prover.pos = [100, 200];
          prover.properties.role_name = 'Prover';
          prover.properties.agent_type = 'codex';
          prover.properties.model = 'codex-5.4-high';
          prover.properties.skill_ref = 'formal-proof';
          prover.properties.prompt_template = 'Continue proving the current phase. Address all audit findings from the previous iteration.';
          graph.add(prover);

          var transform = LiteGraph.createNode('halo/transform');
          transform.pos = [400, 200];
          transform.properties.transform_type = 'assistant_answer';
          graph.add(transform);

          var auditor = LiteGraph.createNode('halo/agent');
          auditor.pos = [650, 200];
          auditor.properties.role_name = 'Auditor';
          auditor.properties.agent_type = 'claude';
          auditor.properties.model = 'claude-opus-4-6';
          auditor.properties.skill_ref = 'adversarial-audit';
          auditor.properties.prompt_template = 'Perform a hostile audit of the proof work. List all findings. If no findings, respond with exactly: findings: 0';
          graph.add(auditor);

          var decision = LiteGraph.createNode('halo/decision');
          decision.pos = [950, 200];
          decision.properties.condition_type = 'contains';
          decision.properties.condition_value = 'findings: 0';
          decision.properties.max_iterations = 5;
          graph.add(decision);

          prover.connect(0, transform, 0);
          transform.connect(0, auditor, 0);
          auditor.connect(0, decision, 0);
          // "fail" loops back to prover (conceptually — litegraph will show the edge)
          decision.connect(1, prover, 0);
        }
      },
      {
        name: 'Translation Verification',
        description: 'Translate between proof assistants, then verify compilation.',
        build: function (graph) {
          var translator = LiteGraph.createNode('halo/agent');
          translator.pos = [100, 200];
          translator.properties.role_name = 'Translator';
          translator.properties.agent_type = 'claude';
          translator.properties.skill_ref = 'meta-translation';
          translator.properties.prompt_template = 'Translate the given Lean code to Coq. Fix any compilation errors from the previous attempt.';
          graph.add(translator);

          var verifier = LiteGraph.createNode('halo/agent');
          verifier.pos = [450, 200];
          verifier.properties.role_name = 'Verifier';
          verifier.properties.agent_type = 'codex';
          verifier.properties.prompt_template = 'Compile and verify the translated Coq code. Report any errors. If clean, respond with: compilation: success';
          graph.add(verifier);

          var decision = LiteGraph.createNode('halo/decision');
          decision.pos = [780, 200];
          decision.properties.condition_type = 'contains';
          decision.properties.condition_value = 'compilation: success';
          decision.properties.max_iterations = 3;
          graph.add(decision);

          translator.connect(0, verifier, 0);
          verifier.connect(0, decision, 0);
          decision.connect(1, translator, 0);
        }
      },
      {
        name: 'Paper Formalization Pipeline',
        description: 'Extract obligations from paper, plan formalization, prove, then audit.',
        build: function (graph) {
          var p1 = LiteGraph.createNode('halo/phase');
          p1.pos = [60, 80];
          p1.properties.phase_name = 'Phase 1: Extract';
          p1.properties.description = 'Extract proof obligations from paper';
          graph.add(p1);

          var extractor = LiteGraph.createNode('halo/agent');
          extractor.pos = [60, 220];
          extractor.properties.role_name = 'Extractor';
          extractor.properties.agent_type = 'claude';
          extractor.properties.skill_ref = 'paper-ingest';
          extractor.properties.prompt_template = 'Extract all proof obligations and formalizable statements from the paper.';
          graph.add(extractor);

          var planner = LiteGraph.createNode('halo/agent');
          planner.pos = [380, 220];
          planner.properties.role_name = 'Planner';
          planner.properties.agent_type = 'claude';
          planner.properties.skill_ref = 'proof-strategy-polya';
          planner.properties.prompt_template = 'Create a formalization plan with phased execution for the extracted obligations.';
          graph.add(planner);

          var prover = LiteGraph.createNode('halo/agent');
          prover.pos = [700, 220];
          prover.properties.role_name = 'Prover';
          prover.properties.agent_type = 'claude';
          prover.properties.model = 'claude-opus-4-6';
          prover.properties.skill_ref = 'formal-proof';
          prover.properties.prompt_template = 'Formalize the planned obligations in Lean 4.';
          graph.add(prover);

          var auditor = LiteGraph.createNode('halo/agent');
          auditor.pos = [1020, 220];
          auditor.properties.role_name = 'Auditor';
          auditor.properties.agent_type = 'gemini';
          auditor.properties.skill_ref = 'adversarial-audit';
          auditor.properties.prompt_template = 'Audit the formalization for sorry, vacuity, and mathematical correctness. findings: 0 if clean.';
          graph.add(auditor);

          p1.connect(0, extractor, 0);
          extractor.connect(0, planner, 0);
          planner.connect(0, prover, 0);
          prover.connect(0, auditor, 0);
        }
      }
    ];
  }

  /* ── Workflow persistence (API with localStorage fallback) ── */
  function listWorkflows() {
    return fetch('/api/workflows').then(function (r) {
      if (!r.ok) throw new Error('API unavailable');
      return r.json();
    }).then(function (data) {
      return data.workflows || [];
    }).catch(function () {
      // Fallback: localStorage
      try {
        var raw = localStorage.getItem('halo_workflows');
        return raw ? JSON.parse(raw) : [];
      } catch (_e) { return []; }
    });
  }

  function saveWorkflow(wf) {
    return fetch('/api/workflows' + (wf.workflow_id ? '/' + wf.workflow_id : ''), {
      method: wf.workflow_id ? 'PUT' : 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(wf),
    }).then(function (r) {
      if (!r.ok) throw new Error('API save failed');
      return r.json();
    }).catch(function () {
      // Fallback: localStorage
      var list = [];
      try { list = JSON.parse(localStorage.getItem('halo_workflows') || '[]'); } catch (_e) {}
      if (!wf.workflow_id) {
        wf.workflow_id = 'wf-' + Date.now() + '-' + Math.random().toString(36).slice(2, 8);
        wf.created_at = Math.floor(Date.now() / 1000);
      }
      wf.updated_at = Math.floor(Date.now() / 1000);
      var idx = list.findIndex(function (w) { return w.workflow_id === wf.workflow_id; });
      if (idx >= 0) list[idx] = wf; else list.push(wf);
      localStorage.setItem('halo_workflows', JSON.stringify(list));
      return wf;
    });
  }

  function deleteWorkflow(id) {
    return fetch('/api/workflows/' + id, { method: 'DELETE' }).then(function (r) {
      if (!r.ok) throw new Error('API delete failed');
    }).catch(function () {
      try {
        var list = JSON.parse(localStorage.getItem('halo_workflows') || '[]');
        list = list.filter(function (w) { return w.workflow_id !== id; });
        localStorage.setItem('halo_workflows', JSON.stringify(list));
      } catch (_e) {}
    });
  }

  function loadWorkflowById(id) {
    return fetch('/api/workflows/' + id).then(function (r) {
      if (!r.ok) throw new Error('API load failed');
      return r.json();
    }).catch(function () {
      try {
        var list = JSON.parse(localStorage.getItem('halo_workflows') || '[]');
        return list.find(function (w) { return w.workflow_id === id; }) || null;
      } catch (_e) { return null; }
    });
  }

  /* ── Build current workflow object from graph state ── */
  function buildWorkflowObject(name) {
    if (!__graph) return null;
    var serialized = __graph.serialize();
    var roles = {};
    var nodes = __graph._nodes || [];
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.type === 'halo/agent') {
        roles[String(n.id)] = {
          role_name: n.properties.role_name || 'Agent',
          agent_type: n.properties.agent_type || 'claude',
          default_model: n.properties.model || '',
          skill_ref: n.properties.skill_ref || '',
          prompt_template: n.properties.prompt_template || '',
        };
      }
    }
    return {
      workflow_id: __currentWorkflowId || '',
      name: name || 'Untitled Workflow',
      version: 1,
      created_at: Math.floor(Date.now() / 1000),
      updated_at: Math.floor(Date.now() / 1000),
      litegraph: serialized,
      halo_meta: {
        description: '',
        max_iterations: 10,
        role_definitions: roles,
      },
    };
  }

  /* ── Render the orchestration page ───────────────── */
  function renderOrchestrationPage() {
    var content = document.getElementById('content');
    if (!content) return;

    // Cleanup previous canvas
    destroyCanvas();

    content.innerHTML =
      '<div class="orch-container">' +
        '<div class="orch-toolbar" id="orch-toolbar">' +
          '<input class="orch-name-input" id="orch-wf-name" type="text" placeholder="Workflow name..." value="Untitled Workflow">' +
          '<div class="orch-separator"></div>' +
          '<select class="orch-select" id="orch-wf-select"><option value="">Load workflow...</option></select>' +
          '<button class="orch-btn" id="orch-btn-load" title="Load selected workflow">Load</button>' +
          '<div class="orch-separator"></div>' +
          '<button class="orch-btn primary" id="orch-btn-save" title="Save current workflow">Save</button>' +
          '<button class="orch-btn" id="orch-btn-save-as" title="Save as new workflow">Save As</button>' +
          '<button class="orch-btn danger" id="orch-btn-delete" title="Delete current workflow">Delete</button>' +
          '<div class="orch-separator"></div>' +
          '<button class="orch-btn" id="orch-btn-clear" title="Clear canvas">Clear</button>' +
          '<button class="orch-btn" id="orch-btn-export" title="Export workflow JSON">Export</button>' +
          '<button class="orch-btn" id="orch-btn-import" title="Import workflow JSON">Import</button>' +
          '<div class="orch-spacer"></div>' +
          '<button class="orch-btn" id="orch-btn-side-toggle" title="Toggle side panel">Panel</button>' +
        '</div>' +
        '<div class="orch-main">' +
          '<div class="orch-canvas-wrap" id="orch-canvas-wrap">' +
            '<canvas id="orch-graph-canvas"></canvas>' +
          '</div>' +
          '<aside class="orch-side-panel" id="orch-side-panel">' +
            '<div class="orch-side-header">' +
              '<span>Templates &amp; Info</span>' +
              '<button class="orch-side-toggle" id="orch-side-close" title="Close panel">&times;</button>' +
            '</div>' +
            '<div class="orch-side-section">' +
              '<div class="orch-side-section-title">Node Palette</div>' +
              '<div class="orch-node-palette" id="orch-node-palette"></div>' +
            '</div>' +
            '<div class="orch-side-section">' +
              '<div class="orch-side-section-title">Workflow Templates</div>' +
              '<div id="orch-templates-list"></div>' +
            '</div>' +
            '<div class="orch-side-section">' +
              '<div class="orch-side-section-title">Node Info</div>' +
              '<div class="orch-node-info" id="orch-node-info">' +
                '<div style="color:var(--text-dim);font-size:12px;">Select a node to see details</div>' +
              '</div>' +
            '</div>' +
            '<div class="orch-side-section">' +
              '<div class="orch-side-section-title">Execution History</div>' +
              '<div id="orch-history-list" class="orch-history-list">' +
                '<div style="color:var(--text-dim);font-size:11px;">Loading...</div>' +
              '</div>' +
            '</div>' +
          '</aside>' +
        '</div>' +
        '<div class="orch-status-bar" id="orch-status-bar">' +
          '<div class="status-item"><span class="status-dot ok"></span> Canvas ready</div>' +
          '<div class="status-item" id="orch-node-count">Nodes: 0</div>' +
          '<div class="status-item" id="orch-link-count">Links: 0</div>' +
          '<div class="status-item" id="orch-wf-id"></div>' +
        '</div>' +
      '</div>';

    registerNodeTypes();
    initCanvas();
    bindToolbar();
    renderNodePalette();
    renderTemplates();
    refreshWorkflowSelect();
    refreshHistory();
  }

  /* ── Initialize litegraph canvas ─────────────────── */
  function initCanvas() {
    __graph = new LGraph();
    __canvasEl = document.getElementById('orch-graph-canvas');
    var wrap = document.getElementById('orch-canvas-wrap');
    if (!__canvasEl || !wrap) return;

    // Size canvas buffer AND display to container dimensions
    var rect = wrap.getBoundingClientRect();
    var initW = Math.round(rect.width) || 800;
    var initH = Math.round(rect.height) || 600;
    __canvasEl.width = initW;
    __canvasEl.height = initH;
    __canvasEl.style.width = initW + 'px';
    __canvasEl.style.height = initH + 'px';

    __canvas = new LGraphCanvas(__canvasEl, __graph);
    __canvas.background_image = null;
    __canvas.render_shadows = false;
    __canvas.clear_background = true;
    __canvas.default_link_color = '#4a5568';
    __canvas.highquality_render = true;

    // Dark background
    __canvas.background_color = '#0a0a14';
    __canvas.clear_background_color = '#0a0a14';

    __graph.start();

    // Resize observer — keep canvas buffer dimensions in sync with container.
    // CSS must NOT set width/height on the canvas, or LiteGraph coordinate
    // calculations (link hit-testing, node dragging) will be off.
    __resizeObs = new ResizeObserver(function () {
      if (!__canvasEl || !wrap || !__canvas) return;
      var r = wrap.getBoundingClientRect();
      var w = Math.round(r.width) || 800;
      var h = Math.round(r.height) || 600;
      if (__canvasEl.width !== w || __canvasEl.height !== h) {
        __canvasEl.width = w;
        __canvasEl.height = h;
        __canvasEl.style.width = w + 'px';
        __canvasEl.style.height = h + 'px';
        __canvas.resize();
      }
    });
    __resizeObs.observe(wrap);

    // Track node selection → update info panel
    __canvas.onNodeSelected = function (node) {
      renderNodeInfo(node);
    };

    __canvas.onNodeDeselected = function () {
      renderNodeInfo(null);
    };

    // Update status bar periodically
    setInterval(updateStatusBar, 1000);
  }

  function destroyCanvas() {
    if (__resizeObs) { __resizeObs.disconnect(); __resizeObs = null; }
    if (__graph) { __graph.stop(); __graph = null; }
    __canvas = null;
    __canvasEl = null;
    __currentWorkflowId = null;
  }

  /* ── Toolbar bindings ────────────────────────────── */
  function bindToolbar() {
    on('orch-btn-save', 'click', handleSave);
    on('orch-btn-save-as', 'click', handleSaveAs);
    on('orch-btn-load', 'click', handleLoad);
    on('orch-btn-delete', 'click', handleDelete);
    on('orch-btn-clear', 'click', handleClear);
    on('orch-btn-export', 'click', handleExport);
    on('orch-btn-import', 'click', handleImport);
    on('orch-btn-side-toggle', 'click', toggleSidePanel);
    on('orch-side-close', 'click', toggleSidePanel);
  }

  function on(id, evt, fn) {
    var el = document.getElementById(id);
    if (el) el.addEventListener(evt, fn);
  }

  function handleSave() {
    var nameEl = document.getElementById('orch-wf-name');
    var name = nameEl ? nameEl.value.trim() : 'Untitled Workflow';
    if (!name) name = 'Untitled Workflow';
    var wf = buildWorkflowObject(name);
    if (!wf) return;
    saveWorkflow(wf).then(function (saved) {
      __currentWorkflowId = saved.workflow_id;
      refreshWorkflowSelect();
      updateStatusBar();
      showToast('Workflow saved');
    });
  }

  function handleSaveAs() {
    var name = prompt('New workflow name:', 'Copy of ' + (document.getElementById('orch-wf-name')?.value || 'Workflow'));
    if (!name) return;
    __currentWorkflowId = null; // Force new ID
    var wf = buildWorkflowObject(name);
    if (!wf) return;
    var nameEl = document.getElementById('orch-wf-name');
    if (nameEl) nameEl.value = name;
    saveWorkflow(wf).then(function (saved) {
      __currentWorkflowId = saved.workflow_id;
      refreshWorkflowSelect();
      updateStatusBar();
      showToast('Workflow saved as "' + name + '"');
    });
  }

  function handleLoad() {
    var sel = document.getElementById('orch-wf-select');
    var id = sel ? sel.value : '';
    if (!id) return;
    loadWorkflowById(id).then(function (wf) {
      if (!wf || !wf.litegraph) { showToast('Workflow not found'); return; }
      __graph.configure(wf.litegraph);
      __currentWorkflowId = wf.workflow_id;
      var nameEl = document.getElementById('orch-wf-name');
      if (nameEl) nameEl.value = wf.name || 'Untitled';
      updateStatusBar();
      showToast('Loaded: ' + wf.name);
    });
  }

  function handleDelete() {
    if (!__currentWorkflowId) { showToast('No workflow selected'); return; }
    if (!confirm('Delete this workflow permanently?')) return;
    deleteWorkflow(__currentWorkflowId).then(function () {
      showToast('Deleted');
      __currentWorkflowId = null;
      __graph.clear();
      var nameEl = document.getElementById('orch-wf-name');
      if (nameEl) nameEl.value = 'Untitled Workflow';
      refreshWorkflowSelect();
      updateStatusBar();
    });
  }

  function handleClear() {
    if (__graph._nodes && __graph._nodes.length > 0) {
      if (!confirm('Clear all nodes from the canvas?')) return;
    }
    __graph.clear();
    __currentWorkflowId = null;
    var nameEl = document.getElementById('orch-wf-name');
    if (nameEl) nameEl.value = 'Untitled Workflow';
    updateStatusBar();
  }

  function handleExport() {
    var wf = buildWorkflowObject(document.getElementById('orch-wf-name')?.value || 'Exported');
    if (!wf) return;
    var json = JSON.stringify(wf, null, 2);
    var blob = new Blob([json], { type: 'application/json' });
    var url = URL.createObjectURL(blob);
    var a = document.createElement('a');
    a.href = url;
    a.download = (wf.name || 'workflow').replace(/[^a-zA-Z0-9_-]/g, '_') + '.json';
    a.click();
    URL.revokeObjectURL(url);
    showToast('Exported');
  }

  function handleImport() {
    var input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json';
    input.onchange = function () {
      var file = input.files && input.files[0];
      if (!file) return;
      var reader = new FileReader();
      reader.onload = function () {
        try {
          var wf = JSON.parse(reader.result);
          if (!wf.litegraph) { showToast('Invalid workflow file'); return; }
          __graph.configure(wf.litegraph);
          __currentWorkflowId = wf.workflow_id || null;
          var nameEl = document.getElementById('orch-wf-name');
          if (nameEl) nameEl.value = wf.name || 'Imported';
          updateStatusBar();
          showToast('Imported: ' + (wf.name || 'workflow'));
        } catch (e) {
          showToast('Failed to parse: ' + e.message);
        }
      };
      reader.readAsText(file);
    };
    input.click();
  }

  function toggleSidePanel() {
    var panel = document.getElementById('orch-side-panel');
    if (!panel) return;
    __sidePanelCollapsed = !__sidePanelCollapsed;
    panel.classList.toggle('collapsed', __sidePanelCollapsed);
    // Trigger canvas resize
    setTimeout(function () {
      if (__canvas && __canvasEl) {
        var wrap = document.getElementById('orch-canvas-wrap');
        if (wrap) {
          var r = wrap.getBoundingClientRect();
          __canvasEl.width = r.width;
          __canvasEl.height = r.height;
          __canvas.resize();
        }
      }
    }, 50);
  }

  /* ── Refresh workflow dropdown ───────────────────── */
  function refreshWorkflowSelect() {
    listWorkflows().then(function (list) {
      var sel = document.getElementById('orch-wf-select');
      if (!sel) return;
      var html = '<option value="">Load workflow...</option>';
      for (var i = 0; i < list.length; i++) {
        var w = list[i];
        var selected = w.workflow_id === __currentWorkflowId ? ' selected' : '';
        html += '<option value="' + escHtml(w.workflow_id) + '"' + selected + '>' + escHtml(w.name) + '</option>';
      }
      sel.innerHTML = html;
    });
  }

  /* ── Node palette (drag-and-drop) ────────────────── */
  var NODE_PALETTE = [
    { type: 'halo/agent',         label: 'Agent',         color: '#7c3aed', desc: 'Dispatch task to an agent' },
    { type: 'halo/decision',      label: 'Decision',      color: '#f59e0b', desc: 'Conditional pass/fail routing' },
    { type: 'halo/transform',     label: 'Transform',     color: '#6b7280', desc: 'Transform output between nodes' },
    { type: 'halo/phase',         label: 'Phase',         color: '#3b82f6', desc: 'Group nodes into a named phase' },
    { type: 'halo/tool',          label: 'Tool',          color: '#6366f1', desc: 'Invoke an MCP tool' },
    { type: 'halo/skill',         label: 'Skill',         color: '#8b5cf6', desc: 'Invoke an agent skill' },
    { type: 'halo/lean_verifier', label: 'Lean Verifier', color: '#10b981', desc: 'Gate: typecheck / no-sorry / build' },
  ];

  function renderNodePalette() {
    var container = document.getElementById('orch-node-palette');
    if (!container) return;
    var html = '';
    for (var i = 0; i < NODE_PALETTE.length; i++) {
      var p = NODE_PALETTE[i];
      html += '<div class="orch-palette-item" draggable="true" data-node-type="' + escHtml(p.type) + '">' +
        '<span class="palette-color-dot" style="background:' + p.color + '"></span>' +
        '<span class="palette-label">' + escHtml(p.label) + '</span>' +
        '<span class="palette-desc">' + escHtml(p.desc) + '</span>' +
      '</div>';
    }
    container.innerHTML = html;

    // Bind drag start
    container.querySelectorAll('.orch-palette-item').forEach(function (item) {
      item.addEventListener('dragstart', function (ev) {
        ev.dataTransfer.setData('application/x-halo-node-type', item.dataset.nodeType);
        ev.dataTransfer.effectAllowed = 'copy';
      });
    });

    // Click to add at center of viewport
    container.querySelectorAll('.orch-palette-item').forEach(function (item) {
      item.addEventListener('click', function () {
        if (!__graph || !__canvas) return;
        var nodeType = item.dataset.nodeType;
        var node = LiteGraph.createNode(nodeType);
        if (!node) return;
        // Place at center of current canvas viewport
        var cx = (__canvas.visible_area ? (__canvas.visible_area[0] + __canvas.visible_area[2]) * 0.5 : 400);
        var cy = (__canvas.visible_area ? (__canvas.visible_area[1] + __canvas.visible_area[3]) * 0.5 : 300);
        node.pos = [cx, cy];
        __graph.add(node);
        __canvas.selectNode(node);
        renderNodeInfo(node);
        updateStatusBar();
      });
    });

    // Bind drop target on canvas
    var wrap = document.getElementById('orch-canvas-wrap');
    if (wrap) {
      wrap.addEventListener('dragover', function (ev) {
        if (ev.dataTransfer.types.indexOf('application/x-halo-node-type') >= 0) {
          ev.preventDefault();
          ev.dataTransfer.dropEffect = 'copy';
        }
      });
      wrap.addEventListener('drop', function (ev) {
        var nodeType = ev.dataTransfer.getData('application/x-halo-node-type');
        if (!nodeType || !__graph || !__canvas) return;
        ev.preventDefault();
        var node = LiteGraph.createNode(nodeType);
        if (!node) return;
        // Convert screen coordinates to canvas coordinates
        var rect = wrap.getBoundingClientRect();
        var canvasX = (ev.clientX - rect.left) / __canvas.ds.scale + __canvas.ds.offset[0];
        var canvasY = (ev.clientY - rect.top) / __canvas.ds.scale + __canvas.ds.offset[1];
        node.pos = [canvasX, canvasY];
        __graph.add(node);
        __canvas.selectNode(node);
        renderNodeInfo(node);
        updateStatusBar();
      });
    }
  }

  /* ── Render template cards ──────────────────────── */
  function renderTemplates() {
    var container = document.getElementById('orch-templates-list');
    if (!container) return;
    var templates = getTemplates();
    var html = '';
    for (var i = 0; i < templates.length; i++) {
      var t = templates[i];
      html += '<div class="orch-template-card" data-template-idx="' + i + '">' +
        '<div class="template-name">' + escHtml(t.name) + '</div>' +
        '<div class="template-desc">' + escHtml(t.description) + '</div>' +
        '<div class="template-nodes">' +
          '<span class="template-node-pill agent">Agent</span>' +
          '<span class="template-node-pill decision">Decision</span>' +
        '</div>' +
      '</div>';
    }
    container.innerHTML = html;

    // Bind click handlers
    container.querySelectorAll('.orch-template-card').forEach(function (card) {
      card.addEventListener('click', function () {
        var idx = parseInt(card.dataset.templateIdx, 10);
        var tpl = templates[idx];
        if (!tpl || !__graph) return;
        if (__graph._nodes && __graph._nodes.length > 0) {
          if (!confirm('Load template "' + tpl.name + '"? This will clear the current canvas.')) return;
        }
        __graph.clear();
        tpl.build(__graph);
        __currentWorkflowId = null;
        var nameEl = document.getElementById('orch-wf-name');
        if (nameEl) nameEl.value = tpl.name;
        updateStatusBar();
        showToast('Template loaded: ' + tpl.name);
      });
    });
  }

  /* ── Node info panel ─────────────────────────────── */
  function renderNodeInfo(node) {
    var container = document.getElementById('orch-node-info');
    if (!container) return;
    if (!node) {
      container.innerHTML = '<div style="color:var(--text-dim);font-size:12px;">Select a node to see details</div>';
      return;
    }
    var html = '';
    html += '<div class="info-label">Type</div><div class="info-value">' + escHtml(node.type || '?') + '</div>';
    html += '<div class="info-label">Title</div><div class="info-value">' + escHtml(node.title || '?') + '</div>';

    // Render editable properties
    var props = node.properties || {};
    var keys = Object.keys(props);
    for (var i = 0; i < keys.length; i++) {
      var key = keys[i];
      var val = props[key];
      html += '<div class="info-label">' + escHtml(key) + '</div>';
      if (key === 'agent_type') {
        html += '<div class="info-value"><select data-prop="' + escHtml(key) + '" class="orch-prop-edit">';
        for (var j = 0; j < AGENT_TYPES.length; j++) {
          var sel = AGENT_TYPES[j] === val ? ' selected' : '';
          html += '<option value="' + AGENT_TYPES[j] + '"' + sel + '>' + AGENT_TYPES[j] + '</option>';
        }
        html += '</select></div>';
      } else if (key === 'model') {
        // Model dropdown — scoped to agent_type if available, plus all models
        var agentType = (props.agent_type || '').toLowerCase();
        var scopedModels = MODELS_BY_TYPE[agentType] || [];
        var otherModels = ALL_MODELS.filter(function (m) { return scopedModels.indexOf(m) === -1; });
        html += '<div class="info-value"><select data-prop="' + escHtml(key) + '" class="orch-prop-edit">';
        html += '<option value=""' + (!val ? ' selected' : '') + '>(auto)</option>';
        if (scopedModels.length > 0) {
          html += '<optgroup label="' + escHtml(agentType) + ' models">';
          for (var mi = 0; mi < scopedModels.length; mi++) {
            var msel = scopedModels[mi] === val ? ' selected' : '';
            html += '<option value="' + scopedModels[mi] + '"' + msel + '>' + scopedModels[mi] + '</option>';
          }
          html += '</optgroup>';
        }
        if (otherModels.length > 0) {
          html += '<optgroup label="Other models">';
          for (var oi = 0; oi < otherModels.length; oi++) {
            var osel = otherModels[oi] === val ? ' selected' : '';
            html += '<option value="' + otherModels[oi] + '"' + osel + '>' + otherModels[oi] + '</option>';
          }
          html += '</optgroup>';
        }
        // If current value is custom / not in list, add it
        if (val && ALL_MODELS.indexOf(val) === -1) {
          html += '<option value="' + escHtml(val) + '" selected>' + escHtml(val) + '</option>';
        }
        html += '</select></div>';
      } else if (key === 'check_mode') {
        var CHECK_MODES = ['typecheck', 'no_sorry', 'build'];
        html += '<div class="info-value"><select data-prop="' + escHtml(key) + '" class="orch-prop-edit">';
        for (var ci = 0; ci < CHECK_MODES.length; ci++) {
          var csel = CHECK_MODES[ci] === val ? ' selected' : '';
          html += '<option value="' + CHECK_MODES[ci] + '"' + csel + '>' + CHECK_MODES[ci] + '</option>';
        }
        html += '</select></div>';
      } else if (key === 'condition_type') {
        html += '<div class="info-value"><select data-prop="' + escHtml(key) + '" class="orch-prop-edit">';
        for (var k = 0; k < CONDITION_TYPES.length; k++) {
          var sel2 = CONDITION_TYPES[k] === val ? ' selected' : '';
          html += '<option value="' + CONDITION_TYPES[k] + '"' + sel2 + '>' + CONDITION_TYPES[k] + '</option>';
        }
        html += '</select></div>';
      } else if (key === 'transform_type') {
        html += '<div class="info-value"><select data-prop="' + escHtml(key) + '" class="orch-prop-edit">';
        for (var m = 0; m < TRANSFORM_TYPES.length; m++) {
          var sel3 = TRANSFORM_TYPES[m] === val ? ' selected' : '';
          html += '<option value="' + TRANSFORM_TYPES[m] + '"' + sel3 + '>' + TRANSFORM_TYPES[m] + '</option>';
        }
        html += '</select></div>';
      } else if (key === 'prompt_template' || key === 'description') {
        html += '<div class="info-value"><textarea data-prop="' + escHtml(key) + '" class="orch-prop-edit">' + escHtml(String(val)) + '</textarea></div>';
      } else if (typeof val === 'number') {
        html += '<div class="info-value"><input type="number" data-prop="' + escHtml(key) + '" class="orch-prop-edit" value="' + val + '"></div>';
      } else {
        html += '<div class="info-value"><input type="text" data-prop="' + escHtml(key) + '" class="orch-prop-edit" value="' + escHtml(String(val)) + '"></div>';
      }
    }

    container.innerHTML = html;

    // Bind property edits
    container.querySelectorAll('.orch-prop-edit').forEach(function (el) {
      var prop = el.dataset.prop;
      el.addEventListener('change', function () {
        if (!node.properties) return;
        var newVal = el.value;
        if (typeof node.properties[prop] === 'number') newVal = Number(newVal) || 0;
        node.properties[prop] = newVal;
        node.setDirtyCanvas(true);
        // When agent_type changes, re-render to update model dropdown options
        if (prop === 'agent_type') renderNodeInfo(node);
      });
    });
  }

  /* ── Status bar ──────────────────────────────────── */
  function updateStatusBar() {
    var nodeCount = document.getElementById('orch-node-count');
    var linkCount = document.getElementById('orch-link-count');
    var wfId = document.getElementById('orch-wf-id');
    if (!__graph) return;
    if (nodeCount) nodeCount.textContent = 'Nodes: ' + (__graph._nodes ? __graph._nodes.length : 0);
    if (linkCount) linkCount.textContent = 'Links: ' + (__graph.links ? Object.keys(__graph.links).length : 0);
    if (wfId) wfId.textContent = __currentWorkflowId ? 'ID: ' + __currentWorkflowId : '';
  }

  /* ── Utils ───────────────────────────────────────── */
  function escHtml(s) {
    var div = document.createElement('div');
    div.textContent = s;
    return div.innerHTML;
  }

  function showToast(msg) {
    var existing = document.getElementById('orch-toast');
    if (existing) existing.remove();
    var el = document.createElement('div');
    el.id = 'orch-toast';
    el.textContent = msg;
    el.style.cssText = 'position:fixed;bottom:20px;left:50%;transform:translateX(-50%);' +
      'background:#1a1a2e;border:1px solid #2a2a3e;color:#e0e0e0;padding:8px 20px;' +
      'border-radius:8px;font-family:JetBrains Mono,monospace;font-size:12px;z-index:9999;' +
      'transition:opacity 0.3s;';
    document.body.appendChild(el);
    setTimeout(function () { el.style.opacity = '0'; }, 2000);
    setTimeout(function () { el.remove(); }, 2500);
  }

  /* ── Execution History ──────────────────────────── */
  function listWorkflowInstances() {
    return fetch('/api/workflows/instances').then(function (r) {
      if (!r.ok) throw new Error('API unavailable');
      return r.json();
    }).then(function (data) {
      return data.instances || [];
    }).catch(function () {
      return [];
    });
  }

  function refreshHistory() {
    var container = document.getElementById('orch-history-list');
    if (!container) return;
    listWorkflowInstances().then(function (instances) {
      if (instances.length === 0) {
        container.innerHTML = '<div style="color:var(--text-dim);font-size:11px;">No workflow runs yet</div>';
        return;
      }
      var html = '';
      for (var i = 0; i < Math.min(instances.length, 20); i++) {
        var inst = instances[i];
        var status = inst.status || 'unknown';
        var statusClass = status === 'completed' ? 'hist-ok' : status === 'failed' || status === 'max_iterations_exceeded' ? 'hist-fail' : 'hist-pending';
        var startDate = inst.started_at ? new Date(inst.started_at * 1000).toLocaleString() : '?';
        var duration = '';
        if (inst.started_at && inst.completed_at) {
          var secs = inst.completed_at - inst.started_at;
          duration = secs < 60 ? secs + 's' : Math.floor(secs / 60) + 'm ' + (secs % 60) + 's';
        }
        var evCount = Array.isArray(inst.events) ? inst.events.length : 0;
        html += '<div class="orch-history-entry" data-instance-id="' + escHtml(inst.instance_id || '') + '">' +
          '<div class="hist-header">' +
            '<span class="hist-status ' + statusClass + '">' + escHtml(status) + '</span>' +
            '<span class="hist-wf">' + escHtml(inst.workflow_id || '').slice(0, 20) + '</span>' +
          '</div>' +
          '<div class="hist-meta">' + escHtml(startDate) + (duration ? ' · ' + escHtml(duration) : '') + ' · ' + evCount + ' events</div>' +
        '</div>';
      }
      container.innerHTML = html;

      // Bind expand on click
      container.querySelectorAll('.orch-history-entry').forEach(function (entry) {
        entry.addEventListener('click', function () {
          var expanded = entry.querySelector('.hist-events');
          if (expanded) { expanded.remove(); return; }
          var instanceId = entry.dataset.instanceId;
          var inst = instances.find(function (ins) { return ins.instance_id === instanceId; });
          if (!inst || !Array.isArray(inst.events) || inst.events.length === 0) return;
          var evHtml = '<div class="hist-events">';
          inst.events.forEach(function (ev) {
            evHtml += '<div class="hist-event">' +
              '<span class="hist-event-type">' + escHtml(typeof ev.event_type === 'string' ? ev.event_type : JSON.stringify(ev.event_type)) + '</span>' +
              '<span class="hist-event-msg">' + escHtml(ev.message || '') + '</span>' +
            '</div>';
          });
          evHtml += '</div>';
          entry.insertAdjacentHTML('beforeend', evHtml);
        });
      });
    });
  }

  /* ── Expose to global page router ────────────────── */
  window.renderOrchestrationPage = renderOrchestrationPage;

  // Cleanup on page navigation
  var _origHashChange = window.onhashchange;
  window.addEventListener('hashchange', function () {
    if (location.hash.indexOf('#/orchestration') !== 0) {
      destroyCanvas();
    }
  });
})();
