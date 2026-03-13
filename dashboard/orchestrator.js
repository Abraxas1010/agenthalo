'use strict';

(function() {
  function esc(v) {
    const s = String(v == null ? '' : v);
    return s
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }

  async function apiGet(path) {
    if (typeof window.api === 'function') return window.api(path);
    const res = await fetch('/api' + path);
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.error || ('HTTP ' + res.status));
    return body;
  }

  async function apiPost(path, payload) {
    if (typeof window.apiPost === 'function') return window.apiPost(path, payload);
    const res = await fetch('/api' + path, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload || {}),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(body.error || ('HTTP ' + res.status));
    return body;
  }

  const state = {
    container: null,
    interval: null,
    selectedAgent: '',
    selectedTask: '',
  };

  function statusBadge(status) {
    const s = String(status || '').toLowerCase();
    if (s === 'idle' || s === 'complete' || s === 'completed') return '<span class="badge badge-ok">idle</span>';
    if (s === 'running' || s === 'busy' || s === 'pending') return '<span class="badge badge-info">' + esc(s) + '</span>';
    if (s === 'failed' || s === 'timeout' || s === 'stopped') return '<span class="badge badge-warn">' + esc(s) + '</span>';
    return '<span class="badge">' + esc(s || 'unknown') + '</span>';
  }

  async function refresh() {
    if (!state.container) return;
    const msg = state.container.querySelector('#orch-msg');
    try {
      const [agentsRes, tasksRes, graphRes] = await Promise.all([
        apiGet('/orchestrator/agents'),
        apiGet('/orchestrator/tasks'),
        apiGet('/orchestrator/graph'),
      ]);
      const agents = Array.isArray(agentsRes.agents) ? agentsRes.agents : [];
      const tasks = Array.isArray(tasksRes.tasks) ? tasksRes.tasks : [];
      const graph = graphRes.graph || { nodes: {}, edges: [] };
      renderAgents(agents);
      renderTasks(tasks);
      renderGraph(graph);
      if (msg) {
        msg.textContent = '';
        msg.classList.remove('err');
      }
    } catch (e) {
      if (msg) {
        msg.textContent = String(e && e.message || e || 'refresh failed');
        msg.classList.add('err');
      }
    }
  }

  function renderAgents(agents) {
    const host = state.container.querySelector('#orch-agents');
    if (!host) return;
    host.innerHTML = (agents || []).map((a) => `
      <tr data-agent-id="${esc(a.agent_id)}">
        <td><code>${esc(a.agent_name || a.agent_id)}</code></td>
        <td>${esc(a.agent_type || '-')}</td>
        <td>${esc(a.container_id || '-')}</td>
        <td>${esc(a.lock_state || '-')}</td>
        <td>${statusBadge(a.status)}</td>
        <td>${Number(a.tasks_completed || 0)}</td>
        <td>${Number(a.total_cost_usd || 0).toFixed(4)}</td>
        <td class="orch-actions-cell">
          <button class="btn btn-sm" data-action="pick-agent" data-agent-id="${esc(a.agent_id)}">Use</button>
          <button class="btn btn-sm" data-action="stop-agent" data-agent-id="${esc(a.agent_id)}">Stop</button>
        </td>
      </tr>`).join('');

    host.querySelectorAll('button[data-action="pick-agent"]').forEach((btn) => {
      btn.addEventListener('click', () => {
        state.selectedAgent = btn.dataset.agentId || '';
        const input = state.container.querySelector('#orch-task-agent-id');
        if (input) input.value = state.selectedAgent;
      });
    });
    host.querySelectorAll('button[data-action="stop-agent"]').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const agentId = btn.dataset.agentId || '';
        if (!agentId) return;
        try {
          await apiPost('/orchestrator/stop', { agent_id: agentId, force: true });
          await refresh();
        } catch (e) {
          const msg = state.container.querySelector('#orch-msg');
          if (msg) {
            msg.textContent = String(e && e.message || e || 'stop failed');
            msg.classList.add('err');
          }
        }
      });
    });
  }

  function renderTasks(tasks) {
    const host = state.container.querySelector('#orch-tasks');
    if (!host) return;
    host.innerHTML = (tasks || []).map((t) => `
      <tr>
        <td><code>${esc(t.task_id)}</code></td>
        <td><code>${esc(t.agent_id)}</code></td>
        <td>${statusBadge(t.status)}</td>
        <td>${esc((t.result || t.error || '').slice(0, 120))}</td>
      </tr>`).join('');
  }

  function renderGraph(graph) {
    const host = state.container.querySelector('#orch-graph');
    if (!host) return;
    const nodes = graph && graph.nodes ? Object.keys(graph.nodes).length : 0;
    const edges = graph && Array.isArray(graph.edges) ? graph.edges.length : 0;
    host.textContent = `nodes=${nodes} edges=${edges}`;
  }

  function bindLaunchForm() {
    const form = state.container.querySelector('#orch-launch-form');
    if (!form) return;
    form.addEventListener('submit', async (ev) => {
      ev.preventDefault();
      const fd = new FormData(form);
      const payload = {
        agent: String(fd.get('agent') || 'codex'),
        agent_name: String(fd.get('agent_name') || 'agent'),
        working_dir: String(fd.get('working_dir') || ''),
        timeout_secs: Number(fd.get('timeout_secs') || 600),
        trace: !!fd.get('trace'),
        capabilities: String(fd.get('capabilities') || 'memory_read,memory_write')
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
      };
      if (!payload.working_dir) delete payload.working_dir;
      const msg = state.container.querySelector('#orch-msg');
      try {
        const out = await apiPost('/orchestrator/launch', payload);
        state.selectedAgent = out.agent_id || '';
        const input = state.container.querySelector('#orch-task-agent-id');
        if (input) input.value = state.selectedAgent;
        if (msg) {
          msg.textContent = 'Agent launched: ' + (out.agent_id || 'ok');
          msg.classList.remove('err');
        }
        await refresh();
      } catch (e) {
        if (msg) {
          msg.textContent = String(e && e.message || e || 'launch failed');
          msg.classList.add('err');
        }
      }
    });
  }

  function bindTaskForm() {
    const form = state.container.querySelector('#orch-task-form');
    if (!form) return;
    form.addEventListener('submit', async (ev) => {
      ev.preventDefault();
      const fd = new FormData(form);
      const payload = {
        agent_id: String(fd.get('agent_id') || ''),
        task: String(fd.get('task') || ''),
        timeout_secs: Number(fd.get('timeout_secs') || 300),
        delay_secs: Number(fd.get('delay_secs') || 0),
        wait: !!fd.get('wait'),
      };
      const msg = state.container.querySelector('#orch-msg');
      try {
        const route = payload.delay_secs > 0 ? '/orchestrator/schedule' : '/orchestrator/task';
        if (payload.delay_secs <= 0) delete payload.delay_secs;
        const out = await apiPost(route, payload);
        state.selectedTask = out && out.task && out.task.task_id ? out.task.task_id : '';
        if (msg) {
          msg.textContent = route === '/orchestrator/schedule'
            ? 'Task scheduled: ' + (state.selectedTask || 'ok')
            : 'Task submitted: ' + (state.selectedTask || 'ok');
          msg.classList.remove('err');
        }
        await refresh();
      } catch (e) {
        if (msg) {
          msg.textContent = String(e && e.message || e || 'task failed');
          msg.classList.add('err');
        }
      }
    });
  }

  function bindPipeForm() {
    const form = state.container.querySelector('#orch-pipe-form');
    if (!form) return;
    form.addEventListener('submit', async (ev) => {
      ev.preventDefault();
      const fd = new FormData(form);
      const payload = {
        source_task_id: String(fd.get('source_task_id') || state.selectedTask || ''),
        target_agent_id: String(fd.get('target_agent_id') || state.selectedAgent || ''),
        transform: String(fd.get('transform') || 'identity'),
        task_prefix: String(fd.get('task_prefix') || ''),
      };
      const msg = state.container.querySelector('#orch-msg');
      try {
        await apiPost('/orchestrator/pipe', payload);
        if (msg) {
          msg.textContent = 'Pipe created';
          msg.classList.remove('err');
        }
        await refresh();
      } catch (e) {
        if (msg) {
          msg.textContent = String(e && e.message || e || 'pipe failed');
          msg.classList.add('err');
        }
      }
    });
  }

  function startPolling() {
    stopPolling();
    state.interval = setInterval(() => {
      refresh().catch(() => {});
    }, 1500);
  }

  function stopPolling() {
    if (state.interval) {
      clearInterval(state.interval);
      state.interval = null;
    }
  }

  function render(container) {
    state.container = container;
    container.innerHTML = `
      <section class="card orchestrator-grid">
        <div class="orchestrator-col">
          <h3>Launch Agent</h3>
          <form id="orch-launch-form" class="orchestrator-form">
            <label>Agent
              <select name="agent" class="input">
                <option value="codex">codex</option>
                <option value="claude">claude</option>
                <option value="gemini">gemini</option>
                <option value="shell">shell</option>
              </select>
            </label>
            <label>Name <input class="input" name="agent_name" value="worker"></label>
            <label>Working Dir <input class="input" name="working_dir" placeholder="/data/workspace"></label>
            <label>Timeout (s) <input class="input" name="timeout_secs" type="number" min="5" value="600"></label>
            <label>Capabilities <input class="input" name="capabilities" value="memory_read,memory_write"></label>
            <label><input name="trace" type="checkbox" checked> Enable Trace</label>
            <button class="btn btn-primary" type="submit">Launch</button>
          </form>
        </div>
        <div class="orchestrator-col">
          <h3>Send Task</h3>
          <form id="orch-task-form" class="orchestrator-form">
            <label>Agent ID <input id="orch-task-agent-id" class="input" name="agent_id" placeholder="orch-..."></label>
            <label>Task <textarea class="input" name="task" rows="5" placeholder="Review src/main.rs"></textarea></label>
            <label>Timeout (s) <input class="input" name="timeout_secs" type="number" min="1" value="300"></label>
            <label>Delay (s) <input class="input" name="delay_secs" type="number" min="0" value="0"></label>
            <label><input name="wait" type="checkbox" checked> Wait for completion</label>
            <button class="btn btn-primary" type="submit">Run Task</button>
          </form>
          <h3 style="margin-top:14px">Pipe Tasks</h3>
          <form id="orch-pipe-form" class="orchestrator-form">
            <label>Source Task <input class="input" name="source_task_id" placeholder="task-..."></label>
            <label>Target Agent <input class="input" name="target_agent_id" placeholder="orch-..."></label>
            <label>Transform <input class="input" name="transform" value="identity"></label>
            <label>Task Prefix <input class="input" name="task_prefix" placeholder="Implement these fixes:\n\n"></label>
            <button class="btn" type="submit">Create Pipe</button>
          </form>
        </div>
      </section>
      <section class="card">
        <h3>Agents</h3>
        <table class="table"><thead><tr>
          <th>Name</th><th>Type</th><th>Container</th><th>Lock</th><th>Status</th><th>Tasks</th><th>Cost</th><th>Actions</th>
        </tr></thead><tbody id="orch-agents"></tbody></table>
      </section>
      <section class="card">
        <h3>Tasks</h3>
        <table class="table"><thead><tr>
          <th>Task</th><th>Agent</th><th>Status</th><th>Result/Error</th>
        </tr></thead><tbody id="orch-tasks"></tbody></table>
      </section>
      <section class="card">
        <h3>Graph</h3>
        <div id="orch-graph" class="mono">nodes=0 edges=0</div>
      </section>
      <div id="orch-msg" class="networking-msg"></div>
    `;
    bindLaunchForm();
    bindTaskForm();
    bindPipeForm();
    try {
      const pending = JSON.parse(localStorage.getItem('orchestrator_pending_launch') || 'null');
      if (pending && pending.agent_id) {
        state.selectedAgent = pending.agent_id;
        const input = state.container.querySelector('#orch-task-agent-id');
        if (input) input.value = state.selectedAgent;
      }
      localStorage.removeItem('orchestrator_pending_launch');
    } catch (_e) {
      localStorage.removeItem('orchestrator_pending_launch');
    }
    refresh().catch(() => {});
    startPolling();
  }

  function cleanup() {
    stopPolling();
    state.container = null;
  }

  window.OrchestratorPage = { render, cleanup };
})();
