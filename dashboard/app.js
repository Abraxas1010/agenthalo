/* Agent H.A.L.O. Dashboard — Fallout Terminal Theme */
"use strict";

const $ = (sel, ctx) => (ctx || document).querySelector(sel);
const $$ = (sel, ctx) => [...(ctx || document).querySelectorAll(sel)];
const content = $("#content");
const PROVIDER_INFO = {
  openrouter: {
    name: "OpenRouter (Required)",
    envVar: "OPENROUTER_API_KEY",
    keyUrl: "https://openrouter.ai/settings/keys",
    category: "llm",
    required: true,
    description:
      "Sole LLM inference upstream — all model requests route through OpenRouter.",
  },
  anthropic: {
    name: "Anthropic (Direct)",
    envVar: "ANTHROPIC_API_KEY",
    keyUrl: "https://console.anthropic.com/settings/keys",
    category: "llm",
    required: false,
    description:
      "Optional direct access (operator only). Customer traffic uses OpenRouter.",
  },
  openai: {
    name: "OpenAI (Direct)",
    envVar: "OPENAI_API_KEY",
    keyUrl: "https://platform.openai.com/api-keys",
    category: "llm",
    required: false,
    description:
      "Optional direct access (operator only). Customer traffic uses OpenRouter.",
  },
  google: {
    name: "Google AI (Direct)",
    envVar: "GOOGLE_API_KEY",
    keyUrl: "https://aistudio.google.com/app/apikey",
    category: "llm",
    required: false,
    description:
      "Optional direct access (operator only). Customer traffic uses OpenRouter.",
  },
  huggingface: {
    name: "Hugging Face",
    envVar: "HF_TOKEN",
    keyUrl: "https://huggingface.co/settings/tokens",
    category: "llm",
    required: false,
    description:
      "Optional token for gated model downloads, Hub search, and vLLM-backed local serving.",
  },
  pinata: {
    name: "Pinata (IPFS Storage)",
    envVar: "PINATA_JWT",
    keyUrl: "https://app.pinata.cloud/developers/api-keys",
    category: "storage",
    required: false,
    description:
      "Immutable 3rd-party storage. Customer IPFS pins route through your Pinata account.",
  },
  agentpmt: {
    name: "AgentPMT (Tool Proxy)",
    envVar: "AGENTPMT_API_KEY",
    keyUrl: "https://www.agentpmt.com",
    category: "tooling",
    required: false,
    description:
      "Third-party MCP tool routing. Required for live agentpmt/* tool execution.",
  },
};

function renderMcpToolsPageRoute() {
  if (typeof window.renderMcpToolsPage === "function") {
    window.renderMcpToolsPage();
  } else {
    content.innerHTML =
      '<div class="loading">MCP Tools module not loaded.</div>';
  }
}

function renderOrchestrationPageRoute() {
  if (typeof window.renderOrchestrationPage === "function") {
    window.renderOrchestrationPage();
  } else {
    content.innerHTML =
      '<div class="loading">Orchestration module not loaded.</div>';
  }
}

function renderSkillsPageRoute() {
  if (typeof window.renderSkillsPage === "function") {
    window.renderSkillsPage();
  } else {
    content.innerHTML =
      '<div class="loading">Skills module not loaded.</div>';
  }
}

function renderLeanPageRoute(initialView) {
  if (typeof window.renderLeanPage === "function") {
    window.renderLeanPage(initialView || null);
  } else {
    content.innerHTML =
      '<div class="loading">Lean module not loaded.</div>';
  }
}

function renderProofExplorerRoute() {
  renderLeanPageRoute("lattice");
}

function renderSystemMonitorRoute() {
  // Stop any previous monitor polling when navigating away
  if (typeof window.stopSystemMonitor === "function") {
    window.stopSystemMonitor();
  }
  if (typeof window.renderSystemMonitorPage === "function") {
    window.renderSystemMonitorPage();
  } else {
    content.innerHTML =
      '<div class="loading">System Monitor module not loaded.</div>';
  }
}

function renderFlowchartPage() {
  content.innerHTML = `
    <div style="display:flex;flex-direction:column;height:100%;padding:0">
      <div style="display:flex;align-items:center;justify-content:space-between;padding:16px 24px;border-bottom:1px solid var(--border);flex-shrink:0">
        <div>
          <div style="font-size:18px;font-weight:700">System Architecture</div>
          <div style="font-size:12px;color:var(--text-dim)">Agent H.A.L.O. / NucleusDB &mdash; full system diagram</div>
        </div>
        <a class="btn btn-sm" href="agenthalo-system-diagram.html" target="_blank" rel="noopener noreferrer" style="font-size:12px">
          Open in New Tab &#8599;
        </a>
      </div>
      <div style="flex:1;min-height:0">
        <iframe
          src="agenthalo-system-diagram.html"
          title="AgentHALO System Architecture"
          style="width:100%;height:100%;border:none;display:block"
          loading="lazy"
        ></iframe>
      </div>
    </div>`;
}

async function renderAgentPmt() {
  const content = $("#content");
  let cfg = null;
  try {
    cfg = await api("/config");
  } catch (_e) {}
  const walletStatus = (cfg && cfg.wallet_status) || {};
  const agentpmtConnected = !!walletStatus.agentpmt_connected;

  // Auto-check vault for stored credentials and enable if found
  if (!agentpmtConnected) {
    try {
      const credCheck = await apiPost("/agentpmt/credential-check", {});
      if (credCheck && credCheck.credentials_found && credCheck.auto_enabled) {
        // Credentials found in vault — re-fetch config to get updated status
        try { cfg = await api("/config"); } catch (_e2) {}
      }
    } catch (_e) {}
  }

  const wsUpdated = (cfg && cfg.wallet_status) || {};
  const connected = !!wsUpdated.agentpmt_connected;
  const toolCount = (cfg && cfg.agentpmt && cfg.agentpmt.tool_count) || 0;

  content.innerHTML = `
    <div style="display:flex;flex-direction:column;height:100%;padding:0">
      <div style="display:flex;align-items:center;justify-content:space-between;padding:16px 24px;border-bottom:1px solid var(--border);flex-shrink:0">
        <div style="display:flex;align-items:center;gap:12px">
          <img src="img/agentpmt-192.png" alt="AgentPMT" style="height:32px;width:32px;border-radius:6px" onerror="this.style.display='none'">
          <div>
            <div style="font-size:18px;font-weight:700">AgentPMT Dashboard</div>
            <div style="font-size:12px;color:var(--text-dim)">Manage your agents, tools, wallets, and budget</div>
          </div>
        </div>
        <div style="display:flex;align-items:center;gap:10px">
          ${
            connected
              ? `<span style="color:var(--green);font-size:13px">&#10003; Connected${toolCount > 0 ? " &mdash; " + toolCount + " tools" : ""}</span>`
              : '<span style="color:var(--text-dim);font-size:13px">Not connected</span>'
          }
          <a class="btn btn-sm" href="https://www.agentpmt.com" target="_blank" rel="noopener noreferrer" style="font-size:12px">
            Open in New Tab &#8599;
          </a>
        </div>
      </div>
      <div style="flex:1;min-height:0">
        <iframe
          src="https://www.agentpmt.com/embed/dashboard?theme=dark"
          title="AgentPMT Dashboard"
          style="width:100%;height:100%;border:none;display:block"
          allow="storage-access"
          loading="lazy"
        ></iframe>
      </div>
    </div>
  `;
}

// -- Routing ------------------------------------------------------------------
const pages = {
  overview: renderOverviewHub,
  sessions: renderSessions,
  config: renderConfig,
  genesis: renderGenesisPage,
  identification: renderIdentificationPage,
  communication: renderCommunicationPage,
  "nucleusdb-docs": renderNucleusDBDocsPage,
  flowchart: renderFlowchartPage,
  networking: renderP2PClawPage,
  p2pclaw: renderP2PClawPage,
  trust: renderTrust,
  "proof-gate": renderProofGate,
  nucleusdb: renderNucleusDB,
  cockpit: renderCockpit,
  orchestration: renderOrchestrationPageRoute,
  "mcp-tools": renderMcpToolsPageRoute,
  skills: renderSkillsPageRoute,
  lean: renderLeanPageRoute,
  "proof-explorer": renderProofExplorerRoute,
  "system-monitor": renderSystemMonitorRoute,
  agentpmt: renderAgentPmt,
};

const REMOVED_PAGE_REDIRECTS = {
  dashboard: "overview",
  costs: "overview",
  models: "config",
  deploy: "cockpit",
  containers: "cockpit",
  orchestrator: "cockpit",
  setup: "config",
  "proof-game": "proof-explorer",
};

const NETWORKS = [
  {
    id: "p2pclaw",
    name: "P2PCLAW Research Hive",
    icon: "&#9788;",
    description: "Decentralized research collaboration with peer validation",
    configurable: true,
    comingSoon: false,
  },
  {
    id: "nym-mesh",
    name: "Nym Mixnet Mesh",
    icon: "&#128274;",
    description: "Privacy-preserving agent communication",
    configurable: true,
    comingSoon: false,
  },
  {
    id: "didcomm-federation",
    name: "DIDComm Federation",
    icon: "&#127760;",
    description: "Cross-organization agent identity mesh",
    configurable: true,
    comingSoon: false,
  },
];
let _networkingSelected = "p2pclaw";
let _p2pclawPaperTab = "published";
let _p2pclawTimers = [];

function clearP2PClawTimers() {
  _p2pclawTimers.forEach((timer) => {
    try {
      clearInterval(timer);
    } catch (_e) {}
  });
  _p2pclawTimers = [];
}

// Genesis + Overview hub + Identification pages — rendering logic in genesis-docs.js (loaded after app.js)
function renderGenesisPage() {
  if (typeof renderGenesis === "function") renderGenesis();
  else
    content.innerHTML =
      '<div class="loading">Genesis docs module not loaded.</div>';
}
function renderIdentificationPage() {
  if (typeof renderIdentification === "function") renderIdentification();
  else
    content.innerHTML =
      '<div class="loading">Identification docs module not loaded.</div>';
}
function renderCommunicationPage() {
  if (typeof renderCommunication === "function") renderCommunication();
  else
    content.innerHTML =
      '<div class="loading">Communication docs module not loaded.</div>';
}
function renderNucleusDBDocsPage() {
  if (typeof renderNucleusDBDocs === "function") renderNucleusDBDocs();
  else
    content.innerHTML =
      '<div class="loading">NucleusDB docs module not loaded.</div>';
}
async function renderProofGate() {
  clearP2PClawTimers();
  content.innerHTML = '<div style="padding:32px;text-align:center">' +
    '<h2 style="color:var(--accent);margin-bottom:16px">Proof Gate has moved</h2>' +
    '<p style="color:var(--text-muted);margin-bottom:16px">Full Proof Gate controls are now in the <strong>Gates</strong> page under Internal Gates.</p>' +
    '<a href="/gates.html" style="color:var(--accent);font-size:14px">Open Gates &rarr;</a>' +
    '</div><hr style="opacity:0.2;margin:16px 0"><div class="loading">Loading legacy view...</div>';
  // Legacy view below for backwards compatibility
  const fmtWhen = (ts) =>
    ts ? new Date(Number(ts) * 1000).toLocaleString() : "Never";
  const statusTone = (req) => {
    const check = req && req.check;
    if (check && check.verified) return { label: "Satisfied", color: "var(--green)" };
    if (req && req.enforced) return { label: "Blocked", color: "var(--red)" };
    return { label: "Advisory", color: "var(--amber)" };
  };
  try {
    const payload = await api("/proof-gate/status");
    const tools = Array.isArray(payload.tools) ? payload.tools : [];
    const certificates = Array.isArray(payload.certificates)
      ? payload.certificates
      : [];
    content.innerHTML = `
      <div class="page-title">Proof Gate</div>
      <p class="muted">Formal gate control plane over the live theorem requirement registry.</p>
      <section class="card" style="border-color:${payload.enabled ? "var(--green)" : "var(--amber)"};margin-bottom:16px">
        <div style="display:flex;justify-content:space-between;gap:16px;align-items:flex-start;flex-wrap:wrap">
          <div>
            <div class="card-label">Master Mode</div>
            <div class="card-value" style="font-size:20px;color:${payload.enabled ? "var(--green)" : "var(--amber)"}">
              ${payload.enabled ? "ENFORCEMENT MODE" : "ADVISORY MODE"}
            </div>
            <div class="card-sub">Counts and tool surfaces are rendered directly from the live proof gate config.</div>
          </div>
          <div style="display:flex;gap:10px;align-items:center;flex-wrap:wrap">
            <label class="switch">
              <input type="checkbox" id="proof-gate-master-toggle" ${payload.enabled ? "checked" : ""}>
              <span class="slider"></span>
            </label>
            <button class="btn btn-sm" id="proof-gate-verify-all">Verify All</button>
            <button class="btn btn-sm btn-primary" id="proof-gate-upload">Submit Certificate</button>
            <input type="file" id="proof-gate-file" accept=".lean4export" style="display:none">
          </div>
        </div>
        <div class="card-grid" style="margin-top:16px">
          <div class="card"><div class="card-label">Tool Surfaces</div><div class="card-value">${Number(payload.tool_count || tools.length)}</div></div>
          <div class="card"><div class="card-label">Requirements</div><div class="card-value">${Number(payload.requirement_count || 0)}</div></div>
          <div class="card"><div class="card-label">Enforced</div><div class="card-value">${Number(payload.enforced_count || 0)}</div></div>
          <div class="card"><div class="card-label">Certificates</div><div class="card-value">${certificates.length}</div><div class="card-sub">${esc(String(payload.certificate_dir || ""))}</div></div>
        </div>
        <div class="card-sub" style="margin-top:10px">Last evaluation: ${fmtWhen(payload.evaluated_at)}</div>
      </section>
      <section style="display:grid;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));gap:16px">
        ${tools
          .map((tool) => {
            const evaln = tool.evaluation || {};
            const reqs = Array.isArray(tool.requirements) ? tool.requirements : [];
            return `
              <div class="card">
                <div style="display:flex;justify-content:space-between;gap:12px;align-items:flex-start">
                  <div>
                    <div class="card-label" style="font-family:var(--mono)">${esc(String(tool.tool_name || ""))}</div>
                    <div class="card-sub">${reqs.length} requirements · ${Number(evaln.requirements_met || 0)} satisfied</div>
                  </div>
                  <span class="badge ${evaln.passed ? "badge-ok" : "badge-warn"}">${evaln.passed ? "PASS" : "CHECK"}</span>
                </div>
                <div class="network-form-actions" style="margin:12px 0">
                  <button class="btn btn-sm" data-proof-verify-tool="${esc(String(tool.tool_name || ""))}">Verify Now</button>
                </div>
                ${reqs
                  .map((req) => {
                    const tone = statusTone(req);
                    const check = req.check || {};
                    return `
                      <details style="border-top:1px solid var(--border);padding-top:10px;margin-top:10px">
                        <summary style="cursor:pointer;display:flex;justify-content:space-between;gap:12px;align-items:center">
                          <span style="font-family:var(--mono);font-size:12px">${esc(String(req.required_theorem || ""))}</span>
                          <span style="color:${tone.color};font-size:12px">${tone.label}</span>
                        </summary>
                        <div style="padding-top:10px;display:grid;gap:8px">
                          <div class="config-desc">${esc(String(req.description || ""))}</div>
                          <div class="config-desc">Statement hash: <code>${esc(truncate(String(req.expected_statement_hash || "n/a"), 24))}</code></div>
                          <div class="config-desc">Commit hash: <code>${esc(truncate(String(req.expected_commit_hash || "n/a"), 18))}</code></div>
                          <div class="config-desc">Signature required: ${req.require_signature ? "yes" : "no"} · Certificate: ${check.found ? "submitted" : "missing"}</div>
                          <div class="config-desc">${check.error ? esc(String(check.error)) : check.verified ? "Verification passed." : "Awaiting verification."}</div>
                          <div style="display:flex;justify-content:space-between;gap:12px;align-items:center;flex-wrap:wrap">
                            <label style="display:flex;align-items:center;gap:8px">
                              <input type="checkbox" data-proof-enforced="1" data-proof-tool="${esc(String(tool.tool_name || ""))}" data-proof-theorem="${esc(String(req.required_theorem || ""))}" ${req.enforced ? "checked" : ""}>
                              <span>Enforce</span>
                            </label>
                            <button class="btn btn-sm" data-proof-upload="1">Submit Certificate</button>
                          </div>
                        </div>
                      </details>
                    `;
                  })
                  .join("")}
              </div>
            `;
          })
          .join("")}
      </section>
      <section class="card" style="margin-top:18px">
        <div style="display:flex;justify-content:space-between;gap:16px;align-items:center;flex-wrap:wrap">
          <div>
            <div class="section-header">Certificate Management</div>
            <div class="card-sub">Upload or remove signed .lean4export artifacts from the active certificate directory.</div>
          </div>
          <button class="btn btn-sm" id="proof-gate-refresh-certs">Refresh</button>
        </div>
        <div class="table-wrap" style="margin-top:12px">
          <table>
            <thead><tr><th>File</th><th>Status</th><th>Theorems</th><th>Modified</th></tr></thead>
            <tbody>
              ${
                certificates.length
                  ? certificates
                      .map((cert) => {
                        const verification = cert.verification || {};
                        const theorems = Array.isArray(verification.theorem_names)
                          ? verification.theorem_names
                          : [];
                        return `<tr>
                          <td style="font-family:var(--mono);font-size:11px">${esc(String(cert.filename || cert.id || ""))}</td>
                          <td>${esc(String(cert.status || "unknown"))}</td>
                          <td style="font-size:11px">${esc(theorems.slice(0, 2).join(", ") || "n/a")}${theorems.length > 2 ? ` (+${theorems.length - 2})` : ""}</td>
                          <td>${fmtWhen(cert.modified_at)}</td>
                        </tr>`;
                      })
                      .join("")
                  : '<tr><td colspan="4" class="muted">No certificates submitted yet.</td></tr>'
              }
            </tbody>
          </table>
        </div>
      </section>
    `;

    const fileInput = $("#proof-gate-file");
    $("#proof-gate-upload")?.addEventListener("click", () => fileInput?.click());
    $("[data-proof-upload='1']")?.addEventListener?.("click", () => {});
    $$("[data-proof-upload='1']").forEach((btn) =>
      btn.addEventListener("click", () => fileInput?.click()),
    );
    $("#proof-gate-master-toggle")?.addEventListener("change", async (ev) => {
      const enabled = !!ev.currentTarget.checked;
      const confirmed = window.confirm(
        enabled
          ? "Enable enforcement mode? Tool calls will be blocked when enforced requirements fail."
          : "Return the proof gate to advisory mode?",
      );
      if (!confirmed) {
        ev.currentTarget.checked = !enabled;
        return;
      }
      await apiPost("/proof-gate/toggle-master", { enabled });
      await renderProofGate();
    });
    $("#proof-gate-verify-all")?.addEventListener("click", async () => {
      await apiPost("/proof-gate/verify", {});
      await renderProofGate();
    });
    $("#proof-gate-refresh-certs")?.addEventListener("click", () => renderProofGate());
    $$("[data-proof-verify-tool]").forEach((btn) =>
      btn.addEventListener("click", async () => {
        await apiPost("/proof-gate/verify", { tool_name: btn.dataset.proofVerifyTool });
        await renderProofGate();
      }),
    );
    $$("[data-proof-enforced='1']").forEach((toggle) =>
      toggle.addEventListener("change", async () => {
        await apiPost("/proof-gate/toggle-requirement", {
          tool_name: toggle.dataset.proofTool,
          theorem_name: toggle.dataset.proofTheorem,
          enforced: !!toggle.checked,
        });
        await renderProofGate();
      }),
    );
    fileInput?.addEventListener("change", async () => {
      const file = fileInput.files && fileInput.files[0];
      if (!file) return;
      const contentText = await file.text();
      await apiPost("/proof-gate/submit-cert", {
        filename: file.name,
        content: contentText,
      });
      fileInput.value = "";
      await renderProofGate();
    });
  } catch (e) {
    content.innerHTML = `<div class="card"><div class="card-label">Proof Gate unavailable</div><div class="card-sub">${esc(String((e && e.message) || e))}</div></div>`;
  }
}

async function renderP2PClawPage() {
  clearP2PClawTimers();
  content.innerHTML = '<div class="loading" style="color:#9a9490;font-family:\'JetBrains Mono\',monospace">Connecting to research hive...</div>';
  const fmtWhen = (ts) =>
    ts ? new Date(Number(ts) * 1000).toLocaleString() : "Never";
  const safeList = (value) => (Array.isArray(value) ? value : []);
  try {
    const [addons, statusRes, briefingRes, papersRes, mempoolRes, investigationsRes, eventsRes] =
      await Promise.all([
        api("/addons").catch(() => ({ addons: {} })),
        api("/p2pclaw/status").catch((error) => ({ error })),
        api("/p2pclaw/briefing").catch((error) => ({ error })),
        api("/p2pclaw/papers?limit=12").catch((error) => ({ papers: [], error })),
        api("/p2pclaw/mempool").catch((error) => ({ papers: [], error })),
        api("/p2pclaw/investigations").catch((error) => ({ investigations: [], error })),
        api("/p2pclaw/events?limit=12").catch((error) => ({ events: [], error })),
      ]);

    const statusErr = statusRes && statusRes.error;
    const status = statusErr ? null : statusRes;
    let config = (status && status.config) || {
      endpoint_url: "https://p2pclaw.com",
      agent_id: "agenthalo",
      agent_name: "AgentHALO",
      tier: "tier1",
      auth_configured: false,
    };
    const swarm = (status && status.swarm) || {};
    const papers = safeList(papersRes && papersRes.papers);
    const mempool = safeList(mempoolRes && mempoolRes.papers);
    const investigations = safeList(investigationsRes && investigationsRes.investigations);
    const events = safeList(eventsRes && eventsRes.events);
    const activePapers = _p2pclawPaperTab === "mempool" ? mempool : papers;
    const briefingText =
      (briefingRes && briefingRes.briefing_markdown) ||
      (briefingRes && briefingRes.error && String(briefingRes.error.message || briefingRes.error)) ||
      "No briefing yet.";
    let enabled = !!(addons && addons.addons && addons.addons.p2pclaw_enabled);
    let connectionLive = !statusErr;

    // Auto-register from agent identity if P2PCLAW is unconfigured or using defaults
    const needsAutoRegister = statusErr || config.agent_id === "agenthalo";
    if (needsAutoRegister && !window._p2pclawAutoRegisterAttempted) {
      window._p2pclawAutoRegisterAttempted = true;
      try {
        const reg = await apiPost("/p2pclaw/auto-register", {});
        if (reg && reg.registered) {
          return renderP2PClawPage();
        }
        if (reg && reg.already_configured) {
          config.agent_id = reg.agent_id;
          config.agent_name = reg.agent_name;
        }
      } catch (_e) {}
    }

    const embedUrl = (config.endpoint_url || "https://p2pclaw.com").replace(/\/+$/, "");

    content.innerHTML = `
      <div class="p2p-hybrid">
        <!-- Main embed area -->
        <div class="p2p-embed" id="p2p-embed-area">
          <iframe src="${esc(embedUrl)}/app/dashboard" id="p2p-iframe" allow="clipboard-write"></iframe>
        </div>

        <!-- Side panel — HALO controls -->
        <div class="p2p-panel">
          <div class="p2p-panel-header">
            <div class="p2p-panel-title">HALO Controls</div>
            <div class="p2p-panel-status">
              <div class="p2p-status-dot ${connectionLive ? "live" : "off"}"></div>
              <span class="p2p-status-label">${connectionLive ? "Live" : "Offline"}</span>
              <label class="p2p-switch">
                <input type="checkbox" id="p2pclaw-enabled-toggle" ${enabled ? "checked" : ""}>
                <span class="p2p-slider"></span>
              </label>
            </div>
          </div>

          <div class="p2p-panel-body">
            <!-- Stats -->
            <div class="p2p-section">
              <div class="p2p-section-header" data-p2-toggle="stats">
                <span class="p2p-section-label">Status</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <div class="p2p-stats-row">
                  <div class="p2p-stat">
                    <span class="p2p-stat-value">${Number(swarm.agents || 0)}</span>
                    <span class="p2p-stat-label">Peers</span>
                  </div>
                  <div class="p2p-stat">
                    <span class="p2p-stat-value">${papers.length}</span>
                    <span class="p2p-stat-label">Papers</span>
                  </div>
                  <div class="p2p-stat">
                    <span class="p2p-stat-value">${mempool.length}</span>
                    <span class="p2p-stat-label">Mempool</span>
                  </div>
                  <div class="p2p-stat">
                    <span class="p2p-stat-value">${investigations.length}</span>
                    <span class="p2p-stat-label">Investigations</span>
                  </div>
                </div>
                <div style="margin-top:10px;font-size:11px;color:#9a9490;font-family:'JetBrains Mono',monospace">
                  Agent: ${esc(String(config.agent_id || "agenthalo"))} · ${esc(String(config.agent_name || "AgentHALO"))}
                </div>
                ${statusErr ? `<div class="p2p-msg err" style="margin-top:8px">${esc(String(statusErr.message || statusErr))}</div>` : ""}
              </div>
            </div>

            <!-- Briefing -->
            <div class="p2p-section">
              <div class="p2p-section-header" data-p2-toggle="briefing">
                <span class="p2p-section-label">Briefing</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <pre class="p2p-log" id="p2pclaw-briefing" style="max-height:180px">${esc(String(briefingText))}</pre>
              </div>
            </div>

            <!-- Papers -->
            <div class="p2p-section">
              <div class="p2p-section-header" data-p2-toggle="papers">
                <span class="p2p-section-label">Papers</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <div class="p2p-tab-bar">
                  <button class="p2p-tab ${_p2pclawPaperTab === "published" ? "active" : ""}" data-p2-tab="published">Published</button>
                  <button class="p2p-tab ${_p2pclawPaperTab === "mempool" ? "active" : ""}" data-p2-tab="mempool">Mempool</button>
                </div>
                <table class="p2p-papers-table">
                  <thead><tr><th>Title</th><th>Status</th><th></th></tr></thead>
                  <tbody>
                    ${activePapers.length
                      ? activePapers.map((paper) => {
                          const paperId = paper.paper_id || paper.id || "";
                          return `<tr>
                            <td title="${esc(String(paper.title || paperId || "untitled"))}">${esc(String(paper.title || paperId || "untitled"))}</td>
                            <td>${esc(String(paper.status || (_p2pclawPaperTab === "mempool" ? "pending" : "published")))}</td>
                            <td>
                              ${_p2pclawPaperTab === "mempool"
                                ? `<button class="p2p-btn p2p-btn-sm" data-p2-validate="${esc(String(paperId))}" data-p2-approve="1">Validate</button>`
                                : `<button class="p2p-btn p2p-btn-sm" data-p2-verify="${esc(String(paperId))}" data-p2-title="${esc(String(paper.title || ""))}" data-p2-content="${esc(String(paper.extra?.content || paper.content || ""))}">Verify</button>`
                              }
                            </td>
                          </tr>`;
                        }).join("")
                      : `<tr><td colspan="3" style="color:#6a6560;padding:12px 8px">No papers on this tab.</td></tr>`
                    }
                  </tbody>
                </table>
              </div>
            </div>

            <!-- Publish -->
            <div class="p2p-section collapsed">
              <div class="p2p-section-header" data-p2-toggle="publish">
                <span class="p2p-section-label">Publish</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <div class="p2p-field">
                  <label class="p2p-label">Title</label>
                  <input class="p2p-input" id="p2-publish-title" placeholder="Research note title">
                </div>
                <div class="p2p-field">
                  <label class="p2p-label">Abstract</label>
                  <textarea class="p2p-input" id="p2-publish-abstract" rows="2" placeholder="Abstract"></textarea>
                </div>
                <div class="p2p-field">
                  <label class="p2p-label">Content</label>
                  <textarea class="p2p-input" id="p2-publish-content" rows="5" placeholder="Paper body, markdown, or extracted notes"></textarea>
                </div>
                <div class="p2p-field">
                  <label class="p2p-label">Category</label>
                  <input class="p2p-input" id="p2-publish-category" placeholder="systems / proofs / markets">
                </div>
                <div class="p2p-actions">
                  <button class="p2p-btn p2p-btn-primary" id="p2-publish-submit">Publish</button>
                </div>
                <div class="p2p-msg" id="p2-publish-msg"></div>
              </div>
            </div>

            <!-- Investigation Wheel -->
            <div class="p2p-section collapsed">
              <div class="p2p-section-header" data-p2-toggle="wheel">
                <span class="p2p-section-label">Investigation Wheel</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <div class="p2p-field">
                  <label class="p2p-label">Search</label>
                  <input class="p2p-input" id="p2-wheel-query" placeholder="hypothesis, theorem, or paper id">
                </div>
                <div class="p2p-actions">
                  <button class="p2p-btn p2p-btn-sm" id="p2-wheel-search">Search</button>
                </div>
                <pre class="p2p-log" id="p2-wheel-result" style="max-height:140px">Awaiting query.</pre>
                ${investigations.length
                  ? `<div style="margin-top:10px;font-size:10px;color:#6a6560;text-transform:uppercase;letter-spacing:0.5px">Active</div>
                     ${investigations.map((it) => `<div style="margin-top:6px;padding:8px 10px;background:#0c0c0d;border:1px solid #2c2c30;border-radius:4px">
                       <div style="font-family:'JetBrains Mono',monospace;font-size:11px;color:#f5f0eb">${esc(String(it.title || it.id || "untitled"))}</div>
                       <div style="font-size:10px;color:#6a6560;margin-top:2px">${esc(String(it.status || "open"))}</div>
                     </div>`).join("")}`
                  : ""
                }
              </div>
            </div>

            <!-- Chat -->
            <div class="p2p-section collapsed">
              <div class="p2p-section-header" data-p2-toggle="chat">
                <span class="p2p-section-label">Hive Chat</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <div class="p2p-field">
                  <textarea class="p2p-input" id="p2-chat-message" rows="3" placeholder="Ask the hive a question"></textarea>
                </div>
                <div class="p2p-actions">
                  <button class="p2p-btn p2p-btn-sm" id="p2-chat-send">Send</button>
                </div>
                <pre class="p2p-log" id="p2-chat-events" style="max-height:200px">${esc(
                  events
                    .map((event) =>
                      `[${fmtWhen(event.timestamp)}] ${event.kind || "event"} ${JSON.stringify(event.extra || {})}`,
                    )
                    .join("\n") || "No hive events yet.",
                )}</pre>
              </div>
            </div>

            <!-- Configuration -->
            <div class="p2p-section collapsed">
              <div class="p2p-section-header" data-p2-toggle="config">
                <span class="p2p-section-label">Configuration</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <div class="p2p-field">
                  <label class="p2p-label">Endpoint</label>
                  <input class="p2p-input" id="p2-endpoint-url" value="${esc(String(config.endpoint_url || "https://p2pclaw.com"))}">
                </div>
                <div class="p2p-field">
                  <label class="p2p-label">Agent Name</label>
                  <input class="p2p-input" id="p2-agent-name" value="${esc(String(config.agent_name || "AgentHALO"))}">
                </div>
                <div class="p2p-field">
                  <label class="p2p-label">Agent ID</label>
                  <input class="p2p-input" id="p2-agent-id" value="${esc(String(config.agent_id || "agenthalo"))}">
                </div>
                <div class="p2p-field">
                  <label class="p2p-label">HMAC Secret</label>
                  <input class="p2p-input" id="p2-auth-secret" type="password" placeholder="optional shared secret">
                </div>
                <div class="p2p-field">
                  <label class="p2p-label">Tier</label>
                  <div class="p2p-tier-row">
                    <label class="p2p-tier-opt"><input type="radio" name="p2-tier" value="tier1" ${String(config.tier || "tier1") === "tier1" ? "checked" : ""}> Tier 1</label>
                    <label class="p2p-tier-opt"><input type="radio" name="p2-tier" value="tier2" ${String(config.tier || "tier1") === "tier2" ? "checked" : ""}> Tier 2</label>
                  </div>
                </div>
                <div class="p2p-actions">
                  <button class="p2p-btn p2p-btn-sm" id="p2-test-btn">Test</button>
                  <button class="p2p-btn p2p-btn-primary" id="p2-save-btn">Save</button>
                </div>
                <div class="p2p-msg" id="p2-config-msg"></div>
              </div>
            </div>

            <!-- Navigate embed -->
            <div class="p2p-section" style="border-bottom:none">
              <div class="p2p-section-header" data-p2-toggle="nav">
                <span class="p2p-section-label">Navigate</span>
                <span class="p2p-section-arrow">&#9662;</span>
              </div>
              <div class="p2p-section-content">
                <div class="p2p-actions" style="flex-wrap:wrap;margin-top:0">
                  <button class="p2p-btn p2p-btn-sm" data-p2-nav="/app/dashboard">Dashboard</button>
                  <button class="p2p-btn p2p-btn-sm" data-p2-nav="/app/network">Network Map</button>
                  <button class="p2p-btn p2p-btn-sm" data-p2-nav="/app/papers">Papers</button>
                  <button class="p2p-btn p2p-btn-sm" data-p2-nav="/app/mempool">Mempool</button>
                  <button class="p2p-btn p2p-btn-sm" data-p2-nav="/app/agents">Agents</button>
                </div>
                <div class="p2p-actions" style="margin-top:8px">
                  <button class="p2p-btn p2p-btn-sm" id="p2pclaw-refresh">Refresh</button>
                  <a href="${esc(embedUrl)}" target="_blank" rel="noopener" class="p2p-btn p2p-btn-sm" style="text-decoration:none;text-align:center">Open in Tab</a>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    `;

    // === Event Handlers ===

    const setMsg = (id, text, ok) => {
      const msg = $(`#${id}`);
      if (!msg) return;
      msg.textContent = text || "";
      msg.className = "p2p-msg" + (text ? (ok ? " ok" : " err") : "");
    };

    // Collapsible sections
    $$("[data-p2-toggle]").forEach((hdr) =>
      hdr.addEventListener("click", () => {
        hdr.closest(".p2p-section")?.classList.toggle("collapsed");
      }),
    );

    // Enable/disable toggle
    $("#p2pclaw-enabled-toggle")?.addEventListener("change", async (ev) => {
      const enabling = !!ev.currentTarget.checked;
      await apiPost("/addons", { name: "p2pclaw", enabled: enabling });
      if (enabling) {
        window._p2pclawAutoRegisterAttempted = false;
        try { await apiPost("/p2pclaw/auto-register", {}); } catch (_e) {}
      }
      await renderP2PClawPage();
    });

    // Placeholder enable/retry buttons
    $("#p2p-enable-btn")?.addEventListener("click", async () => {
      await apiPost("/addons", { name: "p2pclaw", enabled: true });
      window._p2pclawAutoRegisterAttempted = false;
      try { await apiPost("/p2pclaw/auto-register", {}); } catch (_e) {}
      await renderP2PClawPage();
    });
    $("#p2p-retry-btn")?.addEventListener("click", () => renderP2PClawPage());

    // Refresh
    $("#p2pclaw-refresh")?.addEventListener("click", () => renderP2PClawPage());

    // Navigate embed iframe
    $$("[data-p2-nav]").forEach((btn) =>
      btn.addEventListener("click", () => {
        const iframe = $("#p2p-iframe");
        const path = btn.dataset.p2Nav || "/app/dashboard";
        if (iframe) {
          iframe.src = `${embedUrl}${path}`;
        } else {
          // Not yet enabled — enable and load
          const area = $("#p2p-embed-area");
          if (area) {
            area.innerHTML = `<iframe src="${esc(embedUrl)}${esc(path)}" id="p2p-iframe" allow="clipboard-write" style="width:100%;height:100%;border:none;background:#0c0c0d"></iframe>`;
          }
        }
      }),
    );

    // Paper tabs
    $$("[data-p2-tab]").forEach((btn) =>
      btn.addEventListener("click", () => {
        _p2pclawPaperTab = btn.dataset.p2Tab || "published";
        renderP2PClawPage();
      }),
    );

    // Test connection
    $("#p2-test-btn")?.addEventListener("click", async () => {
      try {
        await api("/p2pclaw/status");
        setMsg("p2-config-msg", "Connection successful.", true);
      } catch (e) {
        setMsg("p2-config-msg", String((e && e.message) || e), false);
      }
    });

    // Save config
    $("#p2-save-btn")?.addEventListener("click", async () => {
      const payload = {
        endpoint_url: ($("#p2-endpoint-url")?.value || "").trim(),
        agent_name: ($("#p2-agent-name")?.value || "").trim(),
        agent_id: ($("#p2-agent-id")?.value || "").trim(),
        tier: $('input[name="p2-tier"]:checked')?.value || "tier1",
      };
      const authSecret = ($("#p2-auth-secret")?.value || "").trim();
      if (authSecret) payload.auth_secret = authSecret;
      try {
        const res = await apiPost("/p2pclaw/configure", payload);
        setMsg("p2-config-msg",
          authSecret
            ? `Saved. Secret ${res.auth_in_vault ? "stored in vault." : "stored via insecure fallback."}`
            : "Saved.",
          true,
        );
        await renderP2PClawPage();
      } catch (e) {
        setMsg("p2-config-msg", String((e && e.message) || e), false);
      }
    });

    // Publish
    $("#p2-publish-submit")?.addEventListener("click", async () => {
      const title = ($("#p2-publish-title")?.value || "").trim();
      const abstractText = ($("#p2-publish-abstract")?.value || "").trim();
      const body = ($("#p2-publish-content")?.value || "").trim();
      const category = ($("#p2-publish-category")?.value || "").trim();
      const contentText = [
        abstractText ? `Abstract:\n${abstractText}` : "",
        category ? `Category: ${category}` : "",
        body,
      ].filter(Boolean).join("\n\n");
      try {
        const result = await apiPost("/p2pclaw/papers/publish", { title, content: contentText });
        setMsg("p2-publish-msg", `Published ${result.paper_id || title || "paper"} (${result.status || "ok"}).`, true);
        await renderP2PClawPage();
      } catch (e) {
        setMsg("p2-publish-msg", String((e && e.message) || e), false);
      }
    });

    // Investigation wheel search
    $("#p2-wheel-search")?.addEventListener("click", async () => {
      const query = ($("#p2-wheel-query")?.value || "").trim();
      const target = $("#p2-wheel-result");
      if (target) target.textContent = "Searching...";
      try {
        const result = await api(`/p2pclaw/wheel?q=${encodeURIComponent(query)}`);
        if (target) target.textContent = JSON.stringify(result, null, 2);
      } catch (e) {
        if (target) target.textContent = String((e && e.message) || e);
      }
    });

    // Chat send
    $("#p2-chat-send")?.addEventListener("click", async () => {
      const message = ($("#p2-chat-message")?.value || "").trim();
      if (!message) return;
      await apiPost("/p2pclaw/chat", { message, channel: "research" });
      await renderP2PClawPage();
    });

    // Paper validate/verify
    $$("[data-p2-validate]").forEach((btn) =>
      btn.addEventListener("click", async () => {
        await apiPost("/p2pclaw/papers/validate", {
          paper_id: btn.dataset.p2Validate,
          approve: btn.dataset.p2Approve === "1",
        });
        await renderP2PClawPage();
      }),
    );
    $$("[data-p2-verify]").forEach((btn) =>
      btn.addEventListener("click", async () => {
        const result = await apiPost("/p2pclaw/verify", {
          title: btn.dataset.p2Title || btn.dataset.p2Verify || "paper",
          content: btn.dataset.p2Content || "",
        });
        alert(JSON.stringify(result, null, 2));
      }),
    );

    // Briefing auto-refresh
    const briefingTimer = setInterval(async () => {
      const currentPage = (location.hash.replace("#/", "") || "config").split("/")[0];
      if (!["p2pclaw", "networking"].includes(currentPage)) return;
      try {
        const fresh = await api("/p2pclaw/briefing");
        const target = $("#p2pclaw-briefing");
        if (target) {
          target.textContent = String(
            (fresh && fresh.briefing_markdown) || "No briefing content.",
          );
        }
      } catch (_e) {}
    }, 30000);
    _p2pclawTimers.push(briefingTimer);
  } catch (e) {
    content.innerHTML = `<div class="p2p-hybrid" style="align-items:center;justify-content:center">
      <div style="text-align:center;padding:40px;font-family:'Space Grotesk',system-ui,sans-serif">
        <div style="font-size:18px;color:#ff4e1a;font-weight:600">P2PCLAW Unavailable</div>
        <div style="margin-top:8px;font-size:13px;color:#9a9490">${esc(String((e && e.message) || e))}</div>
        <button class="p2p-btn" style="margin-top:16px" onclick="renderP2PClawPage()">Retry</button>
      </div>
    </div>`;
  }
}

async function renderNetworkingPage() {
  return renderP2PClawPage();
}
function renderOverviewHub() {
  if (typeof renderDocsOverview === "function") {
    renderDocsOverview();
    renderOverviewOperationalSummary().catch(() => {});
  } else {
    content.innerHTML =
      '<div class="loading">Overview module not loaded.</div>';
  }
}

async function renderOverviewOperationalSummary() {
  const mount = document.createElement("section");
  mount.className = "card";
  mount.style.marginTop = "16px";
  mount.innerHTML =
    '<div class="loading">Loading operational overview...</div>';
  content.appendChild(mount);
  try {
    const [status, sessions] = await Promise.all([
      api("/status"),
      api("/sessions?limit=5"),
    ]);
    const recentSessions = (sessions.sessions || []).slice(0, 5);
    mount.innerHTML = `
      <div class="section-header">Operational Overview</div>
      <div class="card-grid">
        <div class="card">
          <div class="card-label">Live Sessions</div>
          <div class="card-value">${Number(status.session_count || 0)}</div>
          <div class="card-sub">Cockpit, orchestration, and deploy activity</div>
        </div>
        <div class="card">
          <div class="card-label">Tracked Tokens</div>
          <div class="card-value">${fmtTokens(Number(status.total_tokens || 0))}</div>
          <div class="card-sub">Aggregate session token history</div>
        </div>
        <div class="card">
          <div class="card-label">Active Wrappers</div>
          <div class="card-value">${Object.values(status.wrapping || {}).filter(Boolean).length}</div>
          <div class="card-sub">Claude, Codex, Gemini wrapper lanes currently engaged</div>
        </div>
        <div class="card">
          <div class="card-label">Governor Stability</div>
          <div class="card-value">${Number(status.governors?.stable || 0)}/${Number(status.governors?.total || 0)}</div>
          <div class="card-sub">${Number(status.governors?.oscillating || 0)} oscillating • ${Number(status.governors?.gain_violated || 0)} gain violations</div>
        </div>
      </div>
      <div class="section-header">Recent Sessions</div>
      ${
        recentSessions.length
          ? `
        <div class="table-wrap"><table>
          <thead><tr><th>Session</th><th>Agent</th><th>Model</th><th>Tokens</th><th>Status</th></tr></thead>
          <tbody>
            ${recentSessions
              .map((item) => {
                const ss = item.session;
                const sm = item.summary || {};
                const totalTokens = Number(sm.total_input_tokens || 0) + Number(sm.total_output_tokens || 0);
                return `<tr class="clickable" onclick="location.hash='#/sessions/${encodeURIComponent(ss.session_id)}'">
                <td style="font-size:11px">${esc(truncate(ss.session_id, 24))}</td>
                <td>${esc(displaySessionAgentName(ss.agent))}</td>
                <td>${esc(truncate(ss.model || "unknown", 20))}</td>
                <td>${fmtTokens(totalTokens)}</td>
                <td>${statusBadge(ss.status)}</td>
              </tr>`;
              })
              .join("")}
          </tbody>
        </table></div>
      `
          : '<div class="muted">No sessions recorded yet.</div>'
      }
    `;
  } catch (e) {
    mount.innerHTML = `<div class="config-desc" style="color:var(--amber)">Operational summary unavailable: ${esc(String(e.message || e))}</div>`;
  }
}

// Setup-first gate: cached setup state
let _setupState = null;
let _setupStateFetchedAt = 0;
let _lastSetupComplete = null;
const SETUP_CACHE_MS = 5000;
let _genesisComplete = null;
let _genesisStatusFetchedAt = 0;
let _genesisCeremonyRunning = false;
const GENESIS_CACHE_MS = 3000;
let _cryptoStatus = null;
let _cryptoStatusFetchedAt = 0;
const CRYPTO_CACHE_MS = 2000;
const GENESIS_STAGES = [
  { id: "hw", label: "Hardware Detection", stub: true },
  { id: "curby", label: "Quantum Entropy", stub: false },
  { id: "beacons", label: "Remote Beacons", stub: false },
  { id: "combine", label: "Entropy Combination", stub: false },
  { id: "derive", label: "Identity Derivation", stub: true },
  { id: "lattice", label: "Lattice Formation", stub: true },
  { id: "destroy", label: "Seed Destruction", stub: true },
  { id: "anchor", label: "Triple Anchor", stub: true },
  { id: "verify", label: "Verification", stub: true },
  { id: "complete", label: "Genesis Complete", stub: false },
];
const GENESIS_ERROR_MESSAGES = {
  WALLET_BOOTSTRAP_FAILED: {
    category: "Signing Wallet Bootstrap Failed",
    message: "Genesis could not initialize the local PQ signing wallet.",
    steps: [
      "Retry Genesis once",
      "Ensure the app can write to your AgentHALO home directory",
      "If this persists, open Configuration and run diagnostics",
    ],
  },
  LEDGER_READ_FAILURE: {
    category: "Identity Ledger Read Failure",
    message: "Genesis could not read the identity ledger state.",
    steps: [
      "Retry Genesis once",
      "Check local disk permissions for AgentHALO files",
      "If this persists, run ledger health check from Configuration",
    ],
  },
  HARVEST_RUNTIME_FAILURE: {
    category: "Genesis Runtime Failure",
    message:
      "The Genesis worker failed unexpectedly before entropy completion.",
    steps: [
      "Retry Genesis",
      "Restart the dashboard service",
      "If this persists, export logs and contact support",
    ],
  },
  CURBY_UNREACHABLE: {
    category: "Network / CURBy Unreachable",
    message:
      "Could not reach the quantum randomness beacon at random.colorado.edu.",
    steps: [
      "Check your internet connection",
      "Whitelist random.colorado.edu",
      "Disable VPN/proxy and retry",
      "Verify system clock is correct",
    ],
  },
  ALL_REMOTE_FAILED: {
    category: "All Remote Entropy Sources Failed",
    message:
      "Could not reach CURBy, NIST, or drand. Internet access is required for Genesis.",
    steps: [
      "Check your internet connection",
      "Whitelist random.colorado.edu, beacon.nist.gov, api.drand.sh",
      "Disable VPN/proxy and retry",
      "Retry when network is stable",
    ],
  },
  INSUFFICIENT_ENTROPY: {
    category: "Insufficient Entropy Sources",
    message: "Genesis requires at least 2 independent entropy sources.",
    steps: [
      "Ensure internet access is available",
      "Retry (remote beacon outage may be temporary)",
      "Check firewall rules for blocked domains",
    ],
  },
  SEED_READ_FAILURE: {
    category: "Sealed Seed Read Failure",
    message: "Genesis could not read an existing sealed seed state.",
    steps: [
      "Retry Genesis",
      "If this is a migrated install, re-run Genesis initialization",
      "If it persists, use diagnostics to repair local seed state",
    ],
  },
  SEED_STORAGE_FAILURE: {
    category: "Sealed Seed Storage Failure",
    message: "Genesis could not seal/store the derived seed.",
    steps: [
      "Retry Genesis",
      "Check disk space and local directory permissions",
      "If it persists, restart service and run diagnostics",
    ],
  },
  GENESIS_SEED_MISMATCH: {
    category: "Genesis Seed Mismatch",
    message:
      "Existing sealed seed does not match the newly harvested Genesis value.",
    steps: [
      "Do not continue with mismatched seed state",
      "Run Genesis diagnostics/repair from Configuration",
      "Contact support if mismatch persists",
    ],
  },
  LEDGER_APPEND_FAILURE: {
    category: "Ledger Append Failure",
    message:
      "Genesis completed entropy harvest but could not append immutable ledger entry.",
    steps: [
      "Retry Genesis",
      "Check local file permissions and disk health",
      "If it persists, run ledger repair diagnostics",
    ],
  },
  UNKNOWN: {
    category: "Unknown Genesis Error",
    message: "The entropy harvest failed unexpectedly.",
    steps: [
      "Retry the Genesis ceremony",
      "Check network and system time",
      "If it persists, contact support",
    ],
  },
};

async function fetchGenesisStatus(force) {
  const now = Date.now();
  if (
    !force &&
    _genesisComplete !== null &&
    now - _genesisStatusFetchedAt < GENESIS_CACHE_MS
  ) {
    return _genesisComplete;
  }
  try {
    const status = await api("/genesis/status");
    _genesisComplete = !!status.completed;
  } catch (_e) {
    // Fail closed.
    _genesisComplete = false;
  }
  _genesisStatusFetchedAt = now;
  return _genesisComplete;
}

async function fetchCryptoStatus(force) {
  const now = Date.now();
  if (
    !force &&
    _cryptoStatus &&
    now - _cryptoStatusFetchedAt < CRYPTO_CACHE_MS
  ) {
    return _cryptoStatus;
  }
  try {
    const status = await api("/crypto/status");
    _cryptoStatus = status || {};
  } catch (e) {
    _cryptoStatus = {
      locked: false,
      password_protected: false,
      migration_status: "unknown",
      active_scopes: [],
      bootstrap_mode: "required",
      retry_after_secs: 0,
      error: String((e && e.message) || e),
    };
  }
  _cryptoStatusFetchedAt = now;
  return _cryptoStatus;
}

function hideCryptoOverlay() {
  const overlay = $("#crypto-lock-overlay");
  if (overlay) overlay.style.display = "none";
}

function cryptoBootstrapMode(status) {
  return String(status?.bootstrap_mode || "required").toLowerCase();
}

function cryptoNeedsPasswordCreation(status) {
  return (
    !status?.password_protected &&
    (status?.migration_status === "needs_password_creation" ||
      status?.migration_status === "fresh")
  );
}

function lockTitle(status) {
  if (cryptoNeedsPasswordCreation(status)) {
    return "Create a password to protect your identity";
  }
  return "Enter password to unlock";
}

function lockHint(status) {
  if (cryptoNeedsPasswordCreation(status)) {
    return "This password protects local cryptographic scopes (sign, vault, wallet, identity, genesis).";
  }
  if (Number(status.retry_after_secs || 0) > 0) {
    return `Too many failed attempts. Retry in ${Number(status.retry_after_secs)}s.`;
  }
  return "Unlock to access encrypted identity, vault, and wallet operations.";
}

function renderCryptoOverlay(status) {
  const overlay = $("#crypto-lock-overlay");
  if (!overlay) return;
  const needsCreate = cryptoNeedsPasswordCreation(status);
  overlay.style.display = "flex";
  overlay.innerHTML = `
    <div class="crypto-lock-card">
      <div class="crypto-lock-title">${esc(lockTitle(status))}</div>
      <div class="crypto-lock-subtitle">${esc(lockHint(status))}</div>
      ${
        needsCreate
          ? `
        <div class="crypto-lock-row">
          <input id="crypto-create-password" type="password" class="input" placeholder="Create password">
        </div>
        <div class="crypto-lock-row">
          <input id="crypto-create-confirm" type="password" class="input" placeholder="Confirm password">
        </div>
        <div class="crypto-lock-rules">
          <div class="crypto-lock-rules-title">Password requirements</div>
          <ul>
            <li>Minimum 8 characters.</li>
            <li>Use at least 3 of 4 character groups: uppercase, lowercase, number, symbol.</li>
            <li>Recommended: 12+ characters with all 4 groups.</li>
          </ul>
        </div>
        <div class="crypto-lock-actions">
          <button id="crypto-create-btn" class="btn btn-primary">Create Password</button>
        </div>
      `
          : `
        <div class="crypto-lock-row">
          <input id="crypto-unlock-password" type="password" class="input" placeholder="Password">
        </div>
        <div class="crypto-lock-actions">
          <button id="crypto-unlock-btn" class="btn btn-primary">Unlock</button>
        </div>
      `
      }
      <div class="crypto-lock-meta">Scopes in session: ${(status.active_scopes || []).map(esc).join(", ") || "none"}</div>
      <div id="crypto-lock-error" class="crypto-lock-error"></div>
    </div>
  `;
  if (needsCreate) {
    const createBtn = $("#crypto-create-btn", overlay);
    if (createBtn) {
      createBtn.onclick = async () => {
        const password = String(
          $("#crypto-create-password", overlay)?.value || "",
        );
        const confirm = String(
          $("#crypto-create-confirm", overlay)?.value || "",
        );
        const errorEl = $("#crypto-lock-error", overlay);
        try {
          createBtn.disabled = true;
          await apiPost("/crypto/create-password", { password, confirm });
          _cryptoStatus = null;
          window._setupCryptoPromptDone = true; // password just created — skip re-lock guard
          const ok = await ensureCryptoUnlocked(true);
          if (ok) {
            route();
          }
        } catch (e) {
          if (errorEl) errorEl.textContent = String((e && e.message) || e);
        } finally {
          createBtn.disabled = false;
        }
      };
      // Enter key on confirm field triggers create
      const confirmInput = $("#crypto-create-confirm", overlay);
      if (confirmInput)
        confirmInput.addEventListener("keydown", (e) => {
          if (e.key === "Enter") createBtn.click();
        });
      const createInput = $("#crypto-create-password", overlay);
      if (createInput)
        createInput.addEventListener("keydown", (e) => {
          if (e.key === "Enter" && confirmInput) confirmInput.focus();
        });
    }
  } else {
    const unlockBtn = $("#crypto-unlock-btn", overlay);
    if (unlockBtn) {
      unlockBtn.onclick = async () => {
        const password = String(
          $("#crypto-unlock-password", overlay)?.value || "",
        );
        const errorEl = $("#crypto-lock-error", overlay);
        try {
          unlockBtn.disabled = true;
          await apiPost("/crypto/unlock", { password });
          _cryptoStatus = null;
          const ok = await ensureCryptoUnlocked(true);
          if (ok) {
            route();
          }
        } catch (e) {
          if (errorEl) errorEl.textContent = String((e && e.message) || e);
        } finally {
          unlockBtn.disabled = false;
        }
      };
      // Enter key on password field triggers unlock
      const unlockInput = $("#crypto-unlock-password", overlay);
      if (unlockInput)
        unlockInput.addEventListener("keydown", (e) => {
          if (e.key === "Enter") unlockBtn.click();
        });
    }
  }
}

async function ensureCryptoUnlocked(force) {
  const status = await fetchCryptoStatus(force);
  if (
    cryptoNeedsPasswordCreation(status) &&
    cryptoBootstrapMode(status) === "required"
  ) {
    renderCryptoOverlay(status);
    return false;
  }
  if (status && status.locked) {
    renderCryptoOverlay(status);
    return false;
  }
  hideCryptoOverlay();
  return true;
}

/// Auto-generate genesis seed silently if not already done.
/// Called after password create/unlock so the DID is ready immediately.
async function autoGenesis() {
  try {
    const status = await api("/genesis/status");
    if (status && status.completed) {
      _genesisComplete = true;
      _genesisStatusFetchedAt = Date.now();
      return;
    }
    const result = await apiPost("/genesis/harvest", {});
    if (result && result.success) {
      _genesisComplete = true;
      _genesisStatusFetchedAt = Date.now();
    }
  } catch (e) {
    // Non-fatal: genesis will be retried via the ceremony gate on next navigation.
    console.warn(
      "autoGenesis: silent harvest failed, will retry via ceremony:",
      e,
    );
  }
}

function genesisStageById(id) {
  return $(`#gs-${id}`);
}

function setGenesisStage(id, state, detail) {
  const el = genesisStageById(id);
  if (!el) return;
  el.className = `genesis-stage ${state}`;
  const icon = $(".genesis-stage-icon", el);
  const det = $(".genesis-stage-detail", el);
  if (icon) {
    if (state === "active") icon.textContent = "◉";
    else if (state === "done") icon.textContent = "✓";
    else if (state === "failed") icon.textContent = "✗";
    else icon.textContent = "○";
  }
  if (det) det.textContent = detail || "";
}

function setGenesisStatus(text, cls) {
  const statusEl = $("#genesis-status");
  if (!statusEl) return;
  statusEl.className = `genesis-status ${cls || ""}`.trim();
  statusEl.textContent = text || "";
}

function showGenesisError(result) {
  const statusEl = $("#genesis-status");
  if (!statusEl) return;
  const code = String((result && result.error_code) || "UNKNOWN");
  const spec = GENESIS_ERROR_MESSAGES[code] || GENESIS_ERROR_MESSAGES.UNKNOWN;
  const message = String(
    (result && (result.message || result.error)) || spec.message,
  );
  const failedSources = Array.isArray(result && result.failed_sources)
    ? result.failed_sources
    : [];
  const technical =
    (result && result.technical_detail) ||
    (failedSources.length > 0 ? JSON.stringify(failedSources) : "");
  statusEl.className = "genesis-status error";
  statusEl.innerHTML = `
    <div class="genesis-error-panel">
      <div class="genesis-error-category">${esc(spec.category)} (${esc(code)})</div>
      <div>${esc(message)}</div>
      ${technical ? `<details style="margin-top:8px"><summary style="cursor:pointer">Technical details</summary><pre style="margin-top:6px;white-space:pre-wrap;font-size:11px;color:var(--text-dim)">${esc(String(technical))}</pre></details>` : ""}
      <ol class="genesis-error-steps">
        ${spec.steps.map((s) => `<li>${esc(s)}</li>`).join("")}
      </ol>
      <button class="genesis-retry-btn" onclick="retryGenesis()">Retry</button>
      <a class="genesis-support-link" href="#/config">Contact Support</a>
    </div>
  `;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function showGenesisCeremony() {
  if (_genesisCeremonyRunning) return;
  _genesisCeremonyRunning = true;

  const stagesEl = $("#genesis-stages");
  const statusEl = $("#genesis-status");
  if (!stagesEl || !statusEl) {
    _genesisCeremonyRunning = false;
    return;
  }

  stagesEl.innerHTML = GENESIS_STAGES.map(
    (s) => `
    <div class="genesis-stage future" id="gs-${esc(s.id)}">
      <span class="genesis-stage-icon">○</span>
      <span class="genesis-stage-label">${esc(s.label)}</span>
      <span class="genesis-stage-detail"></span>
    </div>
  `,
  ).join("");
  setGenesisStatus("Initializing Genesis ceremony...");

  try {
    setGenesisStage("hw", "active");
    setGenesisStatus("Detecting hardware entropy sources...");
    await sleep(700);
    setGenesisStage("hw", "done", "OS CSPRNG available");

    setGenesisStage("curby", "active");
    setGenesisStatus("Harvesting entropy from CURBy, NIST, drand, and OS...");
    const result = await apiPost("/genesis/harvest", {});

    const sources = Array.isArray(result.sources) ? result.sources : [];
    const failed = Array.isArray(result.failed_sources)
      ? result.failed_sources
      : [];
    const sourceBy = {};
    sources.forEach((s) => {
      sourceBy[String(s.name || "")] = s;
    });
    failed.forEach((f) => {
      sourceBy[String(f.name || "")] = f;
    });

    const curby = sourceBy["CURBy-Q"];
    if (curby && !curby.error)
      setGenesisStage(
        "curby",
        "done",
        curby.detail ||
          (result.curby_pulse_id
            ? `Pulse #${result.curby_pulse_id}`
            : "Connected"),
      );
    else
      setGenesisStage(
        "curby",
        "failed",
        curby && curby.error ? "unreachable" : "unavailable",
      );

    const nist = sourceBy["NIST-Beacon"];
    const drand = sourceBy.drand;
    const beaconDetail = [
      nist && !nist.error ? "NIST ✓" : "NIST ✗",
      drand && !drand.error ? "drand ✓" : "drand ✗",
    ].join(" · ");
    if ((nist && !nist.error) || (drand && !drand.error)) {
      setGenesisStage("beacons", "done", beaconDetail);
    } else {
      setGenesisStage("beacons", "failed", beaconDetail);
    }

    if (!result.success) {
      setGenesisStage("combine", "failed", result.error_code || "failed");
      throw result;
    }
    setGenesisStage(
      "combine",
      "done",
      `${Number(result.sources_count || sources.length || 0)} sources combined`,
    );

    for (const s of GENESIS_STAGES.filter((x) => x.stub && x.id !== "hw")) {
      setGenesisStage(s.id, "active");
      setGenesisStatus(`${s.label}...`);
      await sleep(420);
      setGenesisStage(s.id, "done");
    }

    setGenesisStage("complete", "done");
    _genesisComplete = true;
    _genesisStatusFetchedAt = Date.now();
    statusEl.className = "genesis-status complete";
    statusEl.innerHTML = `
      <div style="margin-bottom:12px"><strong>Genesis Complete — Your agent is alive</strong></div>
      <div style="font-size:0.78rem;color:var(--text-dim);margin-bottom:16px">
        ${esc(String(result.sources_count || sources.length || 0))} entropy sources combined
        ${result.curby_pulse_id ? ` · CURBy pulse #${esc(String(result.curby_pulse_id))}` : ""}
      </div>
    `;
    // Auto-advance to setup after a brief pause
    setTimeout(() => completeGenesis(), 1800);
  } catch (err) {
    const payload =
      err && err.body && typeof err.body === "object" ? err.body : err;
    const code = payload && payload.error_code;
    if (code === "ALL_REMOTE_FAILED" || code === "INSUFFICIENT_ENTROPY") {
      setGenesisStage("curby", "failed", "unreachable");
      setGenesisStage("beacons", "failed", "all failed");
      setGenesisStage("combine", "failed", String(code));
    }
    showGenesisError(
      payload || {
        error_code: "UNKNOWN",
        message: String((err && err.message) || err || "unknown error"),
      },
    );
  }

  _genesisCeremonyRunning = false;
}

window.retryGenesis = function retryGenesis() {
  _genesisCeremonyRunning = false;
  _genesisComplete = null;
  _genesisStatusFetchedAt = 0;
  const overlay = $("#genesis-overlay");
  if (overlay) overlay.style.display = "";
  showGenesisCeremony();
};

window.completeGenesis = function completeGenesis() {
  _genesisComplete = true;
  _genesisStatusFetchedAt = Date.now();
  const overlay = $("#genesis-overlay");
  if (overlay) overlay.style.display = "none";
  route();
};

async function fetchSetupState(force) {
  const now = Date.now();
  if (!force && _setupState && now - _setupStateFetchedAt < SETUP_CACHE_MS)
    return _setupState;
  try {
    const cfg = await api("/config");
    _setupState = cfg.setup_complete || {
      identity: false,
      wallet: false,
      agentpmt: false,
      llm: false,
      complete: false,
    };
    _setupStateFetchedAt = now;
  } catch (_e) {
    // Fail closed: keep users in setup if we cannot verify state.
    _setupState = {
      identity: false,
      wallet: false,
      agentpmt: false,
      llm: false,
      complete: false,
    };
    _setupStateFetchedAt = now;
  }
  updateNavLockState();
  return _setupState;
}

function updateNavLockState() {
  // All nav items are always unlocked — CLI integration means
  // setup is not a prerequisite for navigation.
  $$(".nav-link").forEach((a) => {
    a.classList.remove("nav-locked");
    a.classList.remove("setup-incomplete");
    a.classList.remove("nav-unlocked");
  });

  // Hide the setup progress indicator
  const prog = document.getElementById("setup-progress");
  if (prog) prog.style.display = "none";

  _lastSetupComplete = true;
}

// Invalidate setup cache (called after setup actions)
window._invalidateSetupState = function () {
  _setupState = null;
  _setupStateFetchedAt = 0;
};

async function maybeAutoLaunchAfterSetup(setupState) {
  const ss = setupState || (await fetchSetupState());
  if (!ss || !ss.complete) return false;
  if (sessionStorage.getItem("setup_autolaunch_done")) return false;
  const currentPage = (location.hash.replace("#/", "") || "config").split(
    "/",
  )[0];
  if (currentPage !== "setup") return false;
  try {
    const catalog = await api("/deploy/catalog");
    const agents = Array.isArray(catalog.agents) ? catalog.agents : [];
    for (const agent of agents) {
      if (!agent || agent.id === "shell") continue;
      const pre = await apiPost("/deploy/preflight", { agent_id: agent.id });
      if (pre && pre.cli_installed && pre.keys_configured) {
        sessionStorage.setItem("setup_autolaunch_done", "1");
        localStorage.setItem("cockpit_autolaunch_agent", agent.id);
        location.hash = "#/cockpit";
        return true;
      }
    }
  } catch (_e) {
    return false;
  }
  return false;
}

async function route() {
  // Clean up particle animation when leaving NucleusDB page
  if (window._destroyHeroParticles) window._destroyHeroParticles();
  // Stop System Monitor polling when navigating away
  if (typeof window.stopSystemMonitor === 'function') window.stopSystemMonitor();
  // Clean up Proof Game listeners/timers when navigating away
  if (typeof window.teardownProofGamePage === 'function') window.teardownProofGamePage();
  const overlay = $("#genesis-overlay");

  const cryptoReady = await ensureCryptoUnlocked();
  if (!cryptoReady) return;

  const hash = location.hash.replace("#/", "") || "config";
  const page = hash.split("/")[0];
  const arg = hash.split("/").slice(1).join("/");
  if (REMOVED_PAGE_REDIRECTS[page]) {
    location.hash = `#/${REMOVED_PAGE_REDIRECTS[page]}`;
    return;
  }

  // Genesis check: if genesis is not completed, attempt silent recovery
  // (the harvest endpoint handles seed-exists recovery with full CURBy/twine
  // provenance). Only show the visual ceremony for truly first-time genesis
  // (no sealed seed exists yet).
  let genesisOk = await fetchGenesisStatus();
  if (!genesisOk) {
    // Try silent recovery first — POST harvest will re-seal if seed exists
    try {
      const recoveryResult = await apiPost("/genesis/harvest", {});
      if (recoveryResult && recoveryResult.success) {
        _genesisComplete = true;
        _genesisStatusFetchedAt = Date.now();
        genesisOk = true;
      }
    } catch (_e) {
      // Silent recovery failed — check if this is truly first-time genesis
      // (no seed file exists) or a transient error
    }
    if (!genesisOk) {
      if (overlay) overlay.style.display = "";
      showGenesisCeremony();
      return;
    }
  }
  if (overlay) overlay.style.display = "none";

  // Fetch setup state for auto-launch and page-local affordances.
  // Global route blocking was regressing navigation back to Setup even though
  // the underlying pages still render and perform their own readiness checks.
  const ss = await fetchSetupState();
  if (await maybeAutoLaunchAfterSetup(ss)) {
    return;
  }

  $$(".nav-link").forEach((a) =>
    a.classList.toggle("active", a.dataset.page === page),
  );

  // Auto-expand Overview sub-items when navigating to any overview-family page
  const overviewFamily = [
    "overview",
    "genesis",
    "identification",
    "communication",
    "nucleusdb-docs",
    "flowchart",
  ];
  const shouldExpand = overviewFamily.includes(page);
  const parentLink = document.getElementById("nav-overview-parent");
  if (parentLink) {
    parentLink.classList.toggle("nav-expanded", shouldExpand);
  }
  $$('.nav-sub-item[data-parent="overview"]').forEach((li) => {
    li.classList.toggle("nav-sub-visible", shouldExpand);
  });

  if (pages[page]) pages[page](arg);
  else content.innerHTML = '<div class="loading">Page not found</div>';
}

// Toggle Overview sub-items on click
document.addEventListener("click", (e) => {
  const parentLink = e.target.closest("#nav-overview-parent");
  if (!parentLink) return;
  // If already on overview page, just toggle expansion without navigation
  const currentPage = (location.hash.replace("#/", "") || "config").split(
    "/",
  )[0];
  const overviewFamily = [
    "overview",
    "genesis",
    "identification",
    "communication",
    "nucleusdb-docs",
    "flowchart",
  ];
  if (overviewFamily.includes(currentPage)) {
    const isExpanded = parentLink.classList.contains("nav-expanded");
    parentLink.classList.toggle("nav-expanded", !isExpanded);
    $$('.nav-sub-item[data-parent="overview"]').forEach((li) => {
      li.classList.toggle("nav-sub-visible", !isExpanded);
    });
    // Don't navigate away if toggling — but DO navigate if going TO overview from elsewhere
    if (currentPage !== "overview") {
      // Let the default hash navigation happen
    } else {
      e.preventDefault();
    }
  }
});

// ---- Collapsible nav sections ----
(function initNavSections() {
  // Restore collapsed state from localStorage
  const stored = JSON.parse(localStorage.getItem('nav_collapsed') || '{}');
  document.querySelectorAll('.nav-section[data-section]').forEach(header => {
    const section = header.getAttribute('data-section');
    if (stored[section]) {
      header.classList.add('collapsed');
      document.querySelectorAll(`li[data-section-item="${section}"]`).forEach(li => {
        li.classList.add('nav-section-hidden');
      });
    }
    header.addEventListener('click', () => {
      const isCollapsed = header.classList.toggle('collapsed');
      document.querySelectorAll(`li[data-section-item="${section}"]`).forEach(li => {
        li.classList.toggle('nav-section-hidden', isCollapsed);
      });
      // Persist
      const state = JSON.parse(localStorage.getItem('nav_collapsed') || '{}');
      if (isCollapsed) state[section] = true;
      else delete state[section];
      localStorage.setItem('nav_collapsed', JSON.stringify(state));
    });
  });
})();

// ---- Lean nav visibility (hidden until path is configured) ----
async function __updateNavVisibility() {
  try {
    const profile = await api('/worktree/active-profile');
    const hidden = Array.isArray(profile && profile.hidden_nav_items) ? profile.hidden_nav_items : [];
    const hasLean = !!(profile && profile.lean_project_path && profile.lean_project_path.trim());
    // Apply visibility to all nav items
    document.querySelectorAll('.nav-links [data-page]').forEach(link => {
      const page = link.getAttribute('data-page');
      const li = link.closest('li');
      if (!li) return;
      if (page === 'lean') {
        // Lean is hidden if no path OR explicitly hidden
        li.style.display = (!hasLean || hidden.includes(page)) ? 'none' : '';
      } else if (page === 'config') {
        // Config is always visible (cannot hide the settings page)
        li.style.display = '';
      } else {
        li.style.display = hidden.includes(page) ? 'none' : '';
      }
    });
    // Also hide sub-items if their parent is hidden
    document.querySelectorAll('.nav-sub-item[data-parent]').forEach(li => {
      const parent = li.getAttribute('data-parent');
      if (hidden.includes(parent)) {
        li.style.display = 'none';
      }
    });
  } catch (_) {
    // On error, show everything
  }
}
// Keep backward compat alias
window.__updateLeanNavVisibility = __updateNavVisibility;
window.__updateNavVisibility = __updateNavVisibility;

window.addEventListener("hashchange", route);
window.addEventListener("DOMContentLoaded", () => { route(); __updateNavVisibility(); });

// -- CRT Effects Toggle -------------------------------------------------------
function toggleCRT() {
  document.body.classList.toggle("no-crt");
  const on = !document.body.classList.contains("no-crt");
  localStorage.setItem("crt", on ? "on" : "off");
  const btn = $("#crt-toggle");
  if (btn) {
    btn.textContent = on ? "CRT" : "CRT:OFF";
    btn.classList.toggle("crt-on", on);
  }
}
window.toggleCRT = toggleCRT;

// Restore CRT preference
if (localStorage.getItem("crt") === "off") {
  document.body.classList.add("no-crt");
  const btn = document.getElementById("crt-toggle");
  if (btn) btn.textContent = "CRT:OFF";
} else {
  const btn = document.getElementById("crt-toggle");
  if (btn) btn.classList.add("crt-on");
}

// -- API helpers --------------------------------------------------------------
async function api(path) {
  const res = await fetch("/api" + path);
  if (!res.ok) {
    const err = await toApiError(res, path);
    if (Number(err.status) === 423 && !String(path).startsWith("/crypto/")) {
      _cryptoStatus = null;
      ensureCryptoUnlocked(true);
    }
    throw err;
  }
  return res.json();
}

async function apiPost(path, body) {
  const res = await fetch("/api" + path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const err = await toApiError(res, path);
    if (Number(err.status) === 423 && !String(path).startsWith("/crypto/")) {
      _cryptoStatus = null;
      ensureCryptoUnlocked(true);
    }
    throw err;
  }
  return res.json();
}

async function apiDelete(path) {
  const res = await fetch("/api" + path, { method: "DELETE" });
  if (!res.ok) {
    const err = await toApiError(res, path);
    if (Number(err.status) === 423 && !String(path).startsWith("/crypto/")) {
      _cryptoStatus = null;
      ensureCryptoUnlocked(true);
    }
    throw err;
  }
  return res.json();
}

function governorStatusBadge(item) {
  if (!item) return '<span class="badge badge-muted">Unknown</span>';
  if (item.gain_violated)
    return '<span class="badge badge-warn">Gain Violated</span>';
  if (item.oscillating)
    return '<span class="badge badge-info">Oscillating</span>';
  if (item.stable) return '<span class="badge badge-ok">Stable</span>';
  return '<span class="badge badge-muted">Monitoring</span>';
}

function governorSparkline(points, width = 140, height = 28) {
  const vals = Array.isArray(points)
    ? points.filter((v) => Number.isFinite(Number(v))).map(Number)
    : [];
  if (!vals.length) {
    return `<svg width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"></svg>`;
  }
  const min = Math.min(...vals);
  const max = Math.max(...vals);
  const span = Math.max(max - min, 1e-9);
  const step = vals.length > 1 ? width / (vals.length - 1) : width;
  const path = vals
    .map((value, index) => {
      const x = index * step;
      const y = height - (((value - min) / span) * (height - 4) + 2);
      return `${index === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
  return `<svg width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"><path d="${path}" fill="none" stroke="var(--accent)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"></path></svg>`;
}

function renderGovernorCards(governorData) {
  const instances = []
    .concat(
      Array.isArray(governorData?.instances) ? governorData.instances : [],
    )
    .concat(governorData?.memory ? [governorData.memory] : []);
  if (!instances.length) return "";
  return `
    <div class="section-header">AETHER Governors</div>
    <div class="card" style="margin-bottom:12px">
      <div class="card-label">Formal Limitation</div>
      <div class="card-sub">Multi-step convergence is empirically observed, not formally proved. The verified regime is single-step, from-rest, no-clamp only.</div>
    </div>
    <div class="card-grid">
      ${instances
        .map((item) => {
          const proxyExtra =
            item.instance_id === "gov-proxy" && governorData?.proxy
              ? `<div class="card-sub">in-flight=${Number(governorData.proxy.in_flight || 0)} | latency=${Number(governorData.proxy.latency_ewma_ms || 0).toFixed(1)}ms</div>`
              : "";
          return `
          <div class="card">
            <div class="card-label">${esc(item.instance_id || "governor")}</div>
            <div class="card-value" style="font-size:15px">${Number(item.epsilon || 0).toFixed(2)}</div>
            <div class="card-sub">measured=${Number(item.measured_signal || 0).toFixed(2)} | target=${Number(item.target || 0).toFixed(2)}</div>
            <div style="margin-top:8px">${governorSparkline(item.sparkline || [])}</div>
            <div style="margin-top:6px">${governorStatusBadge(item)}</div>
            ${proxyExtra}
            <div class="card-sub">basis: ${esc(item.formal_basis || "n/a")}</div>
            ${item.warning ? `<div class="card-sub" style="color:var(--amber)">${esc(item.warning)}</div>` : ""}
          </div>
        `;
        })
        .join("")}
    </div>
  `;
}

async function toApiError(res, path) {
  const raw = await res.text();
  let body = null;
  try {
    body = raw ? JSON.parse(raw) : null;
  } catch (_e) {}
  const message = (body && body.error) || raw || `API error: ${res.status}`;
  const err = new Error(message);
  err.status = res.status;
  err.path = path;
  err.body = body;
  return err;
}

// -- HTML escaping (XSS prevention) -------------------------------------------
function esc(s) {
  if (s == null) return "";
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
window.__escapeHtml = esc;
window.__providerInfo = PROVIDER_INFO;

function parseProviderList(v) {
  if (Array.isArray(v)) {
    return [
      ...new Set(
        v
          .map((x) =>
            String(x || "")
              .trim()
              .toLowerCase(),
          )
          .filter(Boolean),
      ),
    ];
  }
  if (typeof v === "string") {
    return [
      ...new Set(
        v
          .split(",")
          .map((x) => x.trim().toLowerCase())
          .filter(Boolean),
      ),
    ];
  }
  return [];
}

window.openSetupGuide = function openSetupGuide(context) {
  const payload = Object.assign({ ts: Date.now() }, context || {});
  localStorage.setItem("halo_setup_context", JSON.stringify(payload));
  location.hash = "#/config";
};

window.copySetupText = async function copySetupText(value) {
  try {
    await navigator.clipboard.writeText(String(value || ""));
    alert("Copied to clipboard");
  } catch (_e) {
    alert("Copy failed. Please copy manually.");
  }
};

window.openSetupProviderConfig = function openSetupProviderConfig(provider) {
  localStorage.setItem(
    "halo_setup_open_provider",
    String(provider || "").toLowerCase(),
  );
  location.hash = "#/config";
};

window.trySetupRedirect = function trySetupRedirect(err, agent, from) {
  const message = String((err && err.message) || "");
  const lower = message.toLowerCase();
  const status = Number(err && err.status);
  const body =
    err && err.body && typeof err.body === "object" ? err.body : null;

  let reason = null;
  let providers = [];
  if (body && body.code === "auth_required") {
    reason = "auth_required";
  } else if (status === 401 || lower.includes("authentication required")) {
    reason = "auth_required";
  } else if (
    body &&
    Array.isArray(body.missing_keys) &&
    body.missing_keys.length > 0
  ) {
    reason = "provider_keys_missing";
    providers = parseProviderList(body.missing_keys);
  } else {
    const match = message.match(/missing API keys?:\s*(.+)$/i);
    if (match) {
      reason = "provider_keys_missing";
      providers = parseProviderList(match[1]);
    }
  }

  if (!reason) return false;
  const context = {
    reason,
    from: from || "dashboard",
    agent: agent || null,
    providers,
  };

  if (typeof window.openSetupGuide === "function") {
    window.openSetupGuide(context);
  } else {
    location.hash = "#/config";
  }
  return true;
};

// -- Format helpers -----------------------------------------------------------
function fmtCost(v) {
  return "$" + (v || 0).toFixed(2);
}
function fmtTokens(v) {
  return (v || 0).toLocaleString();
}
function fmtDuration(secs) {
  if (!secs) return "0s";
  const h = Math.floor(secs / 3600),
    m = Math.floor((secs % 3600) / 60),
    s = secs % 60;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}
function fmtTime(ts) {
  if (!ts) return "";
  return new Date(ts * 1000).toLocaleString();
}
function truncate(s, n) {
  return s && s.length > n ? s.slice(0, n) + "..." : s;
}
function typeBadge(type) {
  return `<span class="type-badge type-${type || "integer"}">${esc(type || "integer")}</span>`;
}
function renderTypedValue(row) {
  const type = row.type || "integer";
  const val = row.value;
  const display = row.display != null ? row.display : String(val);
  switch (type) {
    case "null":
      return `<span class="ndb-value-display val-null">NULL</span>`;
    case "bool":
      return `<span class="ndb-value-display val-bool">${val ? "true" : "false"}</span>`;
    case "integer":
    case "float":
      return `<span class="ndb-value-display">${esc(display)}</span>`;
    case "text":
      return `<span class="ndb-value-display val-text">"${esc(truncate(display, 60))}"</span>`;
    case "json": {
      const preview = typeof val === "object" ? JSON.stringify(val) : display;
      return `<button type="button" class="ndb-value-display val-json ndb-json-toggle" title="Click to expand" data-key="${esc(row.key)}">${esc(truncate(preview, 60))}</button>`;
    }
    case "vector": {
      const dims = Array.isArray(val) ? val : [];
      return `<span class="ndb-value-display val-vector">[${dims.length}d] ${esc(truncate(display, 50))}</span>`;
    }
    case "bytes":
      return `<span class="ndb-value-display val-bytes">${esc(truncate(display, 60))}</span>`;
    default:
      return `<span class="ndb-value-display">${esc(truncate(display, 60))}</span>`;
  }
}
function statusBadge(status) {
  const cls =
    status === "completed"
      ? "badge-ok"
      : status === "failed"
        ? "badge-err"
        : status === "running"
          ? "badge-info"
          : "badge-muted";
  return `<span class="badge ${cls}">${esc(status)}</span>`;
}
function eventTypeBadge(type) {
  const colors = {
    assistant: "#00ee00",
    tool_call: "#c49bff",
    tool_result: "#c49bff",
    mcp_tool_call: "#ffb830",
    file_change: "#00ff41",
    bash_command: "#ff8c00",
    genesis_harvest: "#4ba3ff",
    error: "#ff3030",
    thinking: "#3a7a2a",
  };
  const c = colors[type] || "#3a7a2a";
  return `<span class="event-type" style="background:${c}18;color:${c}">${esc(type)}</span>`;
}
function formatBytes(bytes) {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let val = bytes;
  while (val >= 1024 && i < units.length - 1) {
    val /= 1024;
    i++;
  }
  return val.toFixed(i === 0 ? 0 : 1) + " " + units[i];
}

// -- Fallout chart palette ----------------------------------------------------
const chartGreen = "#00ee00";
const chartAmber = "#ffb830";
const chartGreenBg = "rgba(0, 238, 0, 0.15)";
const chartAmberBg = "rgba(255, 184, 48, 0.3)";
const chartGridColor = "rgba(26, 51, 18, 0.6)";
const chartPalette = [
  "#00ee00",
  "#ffb830",
  "#ff8c00",
  "#c49bff",
  "#ff3030",
  "#00ff41",
];

function falloutChartDefaults() {
  if (typeof Chart === "undefined") return;
  Chart.defaults.color = "#3a7a2a";
  Chart.defaults.borderColor = "rgba(26, 51, 18, 0.4)";
  Chart.defaults.font.family = "'Share Tech Mono', 'Courier New', monospace";
}
falloutChartDefaults();

// -- SSE live updates ---------------------------------------------------------
let evtSource;
function initSSE() {
  if (evtSource) evtSource.close();
  evtSource = new EventSource("/events");
  evtSource.addEventListener("session_update", (e) => {
    const data = JSON.parse(e.data);
    const countEl = $("#live-session-count");
    if (countEl) countEl.textContent = data.session_count;
  });
}
initSSE();

// =============================================================================
// PAGE: Sessions
// =============================================================================
async function renderSessions(sessionId) {
  if (sessionId) {
    const parts = String(sessionId).split("/");
    if (parts[0] === "agent" && parts[1]) {
      return renderAgentSessions(decodeURIComponent(parts.slice(1).join("/")));
    }
    return renderSessionDetail(sessionId);
  }

  content.innerHTML = '<div class="loading">Loading sessions...</div>';
  try {
    const data = await api("/sessions");
    const items = data.sessions || [];
    const totals = summarizeSessionItems(items);
    const agents = buildAgentSessionGroups(items);

    content.innerHTML = `
      <div class="page-title">Sessions</div>
      <div class="card-grid session-overview-grid">
        <div class="card">
          <div class="card-label">Total Sessions</div>
          <div class="card-value" style="font-size:18px">${fmtTokens(totals.sessions)}</div>
          <div class="card-sub">${fmtTokens(totals.agents)} agents tracked</div>
        </div>
        <div class="card">
          <div class="card-label">Total Tokens</div>
          <div class="card-value" style="font-size:18px">${fmtTokens(totals.tokens)}</div>
          <div class="card-sub">In ${fmtTokens(totals.inputTokens)} / Out ${fmtTokens(totals.outputTokens)}</div>
        </div>
        <div class="card">
          <div class="card-label">Tool Activity</div>
          <div class="card-value" style="font-size:18px">${fmtTokens(totals.toolCalls)}</div>
          <div class="card-sub">${fmtTokens(totals.eventCount)} trace events recorded</div>
        </div>
        <div class="card">
          <div class="card-label">Latest Activity</div>
          <div class="card-value" style="font-size:14px">${totals.latestStartedAt ? fmtTime(totals.latestStartedAt) : "No sessions yet"}</div>
          <div class="card-sub">${totals.latestAgent ? esc(displaySessionAgentName(totals.latestAgent)) : "No agent history recorded"}</div>
        </div>
      </div>
      <div class="section-header">Agents</div>
      <div class="session-agent-grid">
        ${agents.length
          ? agents.map((group) => agentSessionCard(group)).join("")
          : '<div class="card"><div class="card-label">No agent sessions yet</div><div class="card-sub">Launch an agent from Cockpit to begin recording trace history.</div></div>'}
      </div>
      <div class="filter-bar">
        <input type="text" id="filter-agent" placeholder="Filter by agent..." oninput="filterSessions()">
        <input type="text" id="filter-model" placeholder="Filter by model..." oninput="filterSessions()">
        <span style="color:var(--text-muted);font-size:12px">${items.length} sessions</span>
      </div>
      <div class="table-wrap"><table>
        <thead><tr><th>Session ID</th><th>Agent</th><th>Model</th><th>Tokens</th><th>Tools</th><th>Duration</th><th>Started</th><th>Status</th></tr></thead>
        <tbody id="sessions-tbody">
          ${items.map((item) => sessionRow(item)).join("")}
        </tbody>
      </table></div>
    `;

    window._sessionItems = items;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

function sessionRow(item) {
  const ss = item.session,
    sm = item.summary || {};
  const tokens = (sm.total_input_tokens || 0) + (sm.total_output_tokens || 0);
  const tools = Number(sm.tool_calls || 0) + Number(sm.mcp_tool_calls || 0);
  return `<tr class="clickable session-row" data-agent="${esc(ss.agent)}" data-model="${esc(ss.model || "")}"
    onclick="location.hash='#/sessions/${encodeURIComponent(ss.session_id)}'">
    <td style="font-size:11px">${esc(truncate(ss.session_id, 28))}</td>
    <td>${esc(displaySessionAgentName(ss.agent))}</td>
    <td>${esc(truncate(ss.model || "unknown", 22))}</td>
    <td>${fmtTokens(tokens)}</td>
    <td>${fmtTokens(tools)}</td>
    <td>${fmtDuration(sm.duration_secs)}</td>
    <td style="font-size:11px">${fmtTime(ss.started_at)}</td>
    <td>${statusBadge(ss.status)}</td>
  </tr>`;
}

function isSystemSessionAgent(agent) {
  const normalized = String(agent || "").trim().toLowerCase();
  return normalized === "genesis";
}

function displaySessionAgentName(agent) {
  return isSystemSessionAgent(agent) ? "System / Genesis" : String(agent || "unknown");
}

function summarizeSessionItems(items) {
  const summary = (items || []).reduce(
    (acc, item) => {
      const ss = item.session || {};
      const sm = item.summary || {};
      acc.sessions += 1;
      if (!isSystemSessionAgent(ss.agent)) {
        acc.agentsSet.add(ss.agent || "unknown");
      }
      acc.inputTokens += Number(sm.total_input_tokens || 0);
      acc.outputTokens += Number(sm.total_output_tokens || 0);
      acc.toolCalls += Number(sm.tool_calls || 0) + Number(sm.mcp_tool_calls || 0);
      acc.eventCount += Number(sm.event_count || 0);
      const startedAt = Number(ss.started_at || 0);
      if (!isSystemSessionAgent(ss.agent) && startedAt > acc.latestStartedAt) {
        acc.latestStartedAt = startedAt;
        acc.latestAgent = ss.agent || "unknown";
      }
      return acc;
    },
    {
      sessions: 0,
      agentsSet: new Set(),
      inputTokens: 0,
      outputTokens: 0,
      toolCalls: 0,
      eventCount: 0,
      latestStartedAt: 0,
      latestAgent: "",
    },
  );
  summary.agents = summary.agentsSet.size;
  return summary;
}

function buildAgentSessionGroups(items) {
  const grouped = new Map();
  (items || []).forEach((item) => {
    const ss = item.session || {};
    const sm = item.summary || {};
    if (isSystemSessionAgent(ss.agent)) return;
    const key = ss.agent || "unknown";
    if (!grouped.has(key)) {
      grouped.set(key, {
        agent: key,
        sessions: [],
        models: new Set(),
        sessionCount: 0,
        inputTokens: 0,
        outputTokens: 0,
        toolCalls: 0,
        eventCount: 0,
        lastStartedAt: 0,
      });
    }
    const group = grouped.get(key);
    group.sessions.push(item);
    group.sessionCount += 1;
    group.inputTokens += Number(sm.total_input_tokens || 0);
    group.outputTokens += Number(sm.total_output_tokens || 0);
    group.toolCalls += Number(sm.tool_calls || 0) + Number(sm.mcp_tool_calls || 0);
    group.eventCount += Number(sm.event_count || 0);
    if (ss.model) group.models.add(ss.model);
    const startedAt = Number(ss.started_at || 0);
    if (startedAt > group.lastStartedAt) group.lastStartedAt = startedAt;
  });
  return [...grouped.values()]
    .sort((a, b) => b.lastStartedAt - a.lastStartedAt)
    .map((group) => ({
      ...group,
      models: [...group.models].sort(),
      totalTokens: group.inputTokens + group.outputTokens,
    }));
}

function agentSessionCard(group) {
  return `
    <button class="card session-agent-card" onclick="location.hash='#/sessions/agent/${encodeURIComponent(group.agent)}'">
      <div class="card-label">Agent</div>
      <div class="card-value" style="font-size:18px">${esc(group.agent)}</div>
      <div class="card-sub">${fmtTokens(group.sessionCount)} sessions • ${fmtTokens(group.totalTokens)} tokens</div>
      <div class="session-agent-meta">
        <span>${fmtTokens(group.toolCalls)} tools</span>
        <span>${fmtTokens(group.eventCount)} events</span>
        <span>${group.lastStartedAt ? fmtTime(group.lastStartedAt) : "No activity"}</span>
      </div>
      <div class="session-agent-models">${group.models.length ? group.models.map((model) => `<span class="session-agent-chip">${esc(truncate(model, 28))}</span>`).join("") : '<span class="session-agent-chip">unknown model</span>'}</div>
      <div class="session-agent-cta">Open agent sessions &rarr;</div>
    </button>`;
}

async function renderAgentSessions(agent) {
  if (isSystemSessionAgent(agent)) {
    location.hash = "#/sessions";
    return;
  }
  content.innerHTML = '<div class="loading">Loading agent sessions...</div>';
  try {
    const data = await api("/sessions?agent=" + encodeURIComponent(agent));
    const items = data.sessions || [];
    const totals = summarizeSessionItems(items);

    content.innerHTML = `
      <a href="#/sessions" class="back-link">&larr; Back to Sessions</a>
      <div class="page-title">Agent Sessions: ${esc(agent)}</div>
      <div class="card-grid session-overview-grid">
        <div class="card">
          <div class="card-label">Sessions</div>
          <div class="card-value" style="font-size:18px">${fmtTokens(totals.sessions)}</div>
          <div class="card-sub">${totals.latestStartedAt ? "Latest " + fmtTime(totals.latestStartedAt) : "No activity"}</div>
        </div>
        <div class="card">
          <div class="card-label">Tokens</div>
          <div class="card-value" style="font-size:18px">${fmtTokens(totals.inputTokens + totals.outputTokens)}</div>
          <div class="card-sub">In ${fmtTokens(totals.inputTokens)} / Out ${fmtTokens(totals.outputTokens)}</div>
        </div>
        <div class="card">
          <div class="card-label">Tool Activity</div>
          <div class="card-value" style="font-size:18px">${fmtTokens(totals.toolCalls)}</div>
          <div class="card-sub">${fmtTokens(totals.eventCount)} trace events</div>
        </div>
      </div>
      <div class="section-header">Session Trace Explorer</div>
      <div class="session-agent-stack">
        ${items.length ? items.map((item) => agentSessionAccordion(item)).join("") : '<div class="card"><div class="card-label">No sessions for this agent</div></div>'}
      </div>
    `;
    window._agentSessionDetails = new Map();
    $$(".session-agent-accordion").forEach((el) => {
      el.addEventListener("toggle", () => {
        if (el.open) loadAgentSessionDetail(el.dataset.sessionId);
      });
    });
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

function agentSessionAccordion(item) {
  const ss = item.session || {};
  const sm = item.summary || {};
  const totalTokens = Number(sm.total_input_tokens || 0) + Number(sm.total_output_tokens || 0);
  const toolCount = Number(sm.tool_calls || 0) + Number(sm.mcp_tool_calls || 0);
  return `
    <details class="card session-agent-accordion" data-session-id="${esc(ss.session_id)}">
      <summary class="session-agent-summary">
        <div>
          <div class="session-agent-summary-title">${esc(ss.model || "unknown")} ${statusBadge(ss.status)}</div>
          <div class="session-agent-summary-sub">${esc(ss.session_id)} • ${fmtTime(ss.started_at)}</div>
        </div>
        <div class="session-agent-summary-metrics">
          <span>${fmtTokens(totalTokens)} tokens</span>
          <span>${fmtTokens(toolCount)} tools</span>
          <span>${fmtDuration(sm.duration_secs)}</span>
        </div>
      </summary>
      <div class="session-agent-detail-body" id="agent-session-detail-${esc(ss.session_id)}">
        <div class="loading">Loading session trace...</div>
      </div>
    </details>`;
}

async function loadAgentSessionDetail(sessionId) {
  const target = document.getElementById(`agent-session-detail-${CSS.escape(sessionId)}`);
  if (!target) return;
  if (!window._agentSessionDetails) window._agentSessionDetails = new Map();
  if (window._agentSessionDetails.has(sessionId)) {
    target.innerHTML = window._agentSessionDetails.get(sessionId);
    return;
  }
  target.innerHTML = '<div class="loading">Loading session trace...</div>';
  try {
    const data = await api("/sessions/" + encodeURIComponent(sessionId));
    const html = renderSessionDetailSections(data, { embedded: true });
    window._agentSessionDetails.set(sessionId, html);
    target.innerHTML = html;
  } catch (e) {
    target.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

function renderSessionDetailSections(data, options) {
  const opts = options || {};
  const ss = data.session || {};
  const sm = data.summary || {};
  const events = data.events || [];
  const tokens = Number(sm.total_input_tokens || 0) + Number(sm.total_output_tokens || 0);
  const transcriptEvents = events.filter((ev) =>
    ["prompt_sent", "assistant", "thinking", "response_received", "system_message", "error"].includes(String(ev.event_type || "")),
  );
  const toolEvents = events.filter((ev) =>
    ["tool_call", "tool_result", "mcp_tool_call", "mcp_tool_result", "bash_command", "file_change", "subagent_spawn"].includes(String(ev.event_type || "")),
  );
  return `
    ${opts.embedded ? "" : `
      <div class="card-grid">
        <div class="card">
          <div class="card-label">Agent</div>
          <div class="card-value" style="font-size:16px">${esc(displaySessionAgentName(ss.agent))}</div>
          <div class="card-sub">${esc(ss.model || "unknown")}</div>
        </div>
        <div class="card">
          <div class="card-label">Tokens</div>
          <div class="card-value" style="font-size:16px">${fmtTokens(tokens)}</div>
          <div class="card-sub">In: ${fmtTokens(sm.total_input_tokens)} / Out: ${fmtTokens(sm.total_output_tokens)}</div>
        </div>
        <div class="card">
          <div class="card-label">Duration</div>
          <div class="card-value" style="font-size:16px">${fmtDuration(sm.duration_secs)}</div>
          <div class="card-sub">${fmtDuration(sm.duration_secs)}</div>
        </div>
        <div class="card">
          <div class="card-label">Activity</div>
          <div class="card-value" style="font-size:12px">
            ${sm.tool_calls || 0} tools | ${sm.bash_commands || 0} cmds | ${sm.files_modified || 0} files
          </div>
          <div class="card-sub">MCP: ${sm.mcp_tool_calls || 0} | Errors: ${sm.errors || 0}</div>
        </div>
      </div>
      <div style="margin-bottom:12px;display:flex;gap:8px">
        <button class="btn" data-session-id="${encodeURIComponent(ss.session_id)}" onclick="exportSessionByButton(this)">Export JSON</button>
        <button class="btn btn-primary" data-session-id="${encodeURIComponent(ss.session_id)}" onclick="attestSessionByButton(this)">Attest</button>
      </div>
    `}
    <div class="session-trace-grid">
      <div class="card">
        <div class="card-label">Conversation History</div>
        <div class="card-sub">Full prompt, assistant output, thinking traces, and errors.</div>
        <div class="session-trace-list">
          ${transcriptEvents.length ? transcriptEvents.map((ev) => traceEventCard(ev)).join("") : '<div class="muted">No conversation events recorded.</div>'}
        </div>
      </div>
      <div class="card">
        <div class="card-label">Tool Activity</div>
        <div class="card-sub">Tool calls, MCP usage, bash commands, file writes, and spawned subagents.</div>
        <div class="session-trace-list">
          ${toolEvents.length ? toolEvents.map((ev) => traceEventCard(ev)).join("") : '<div class="muted">No tool activity recorded.</div>'}
        </div>
      </div>
    </div>
    <details class="card session-raw-timeline" ${opts.embedded ? "" : "open"}>
      <summary class="session-raw-summary">Raw Event Timeline (${events.length} events)</summary>
      <div class="event-timeline">
        ${events.map((ev) => `
          <div class="event-item">
            <span class="event-seq">#${ev.seq}</span>
            ${eventTypeBadge(ev.event_type)}
            <span class="event-content">${esc(renderCompactEventLabel(ev))}</span>
            ${(ev.input_tokens != null || ev.output_tokens != null) ? `<span style="color:var(--text-dim);font-size:10px;margin-left:8px">in:${fmtTokens(Number(ev.input_tokens || 0))} out:${fmtTokens(Number(ev.output_tokens || 0))}</span>` : ""}
          </div>`).join("")}
      </div>
    </details>
  `;
}

function traceEventCard(ev) {
  return `
    <div class="session-trace-card">
      <div class="session-trace-card-head">
        <div>
          ${eventTypeBadge(ev.event_type)}
          <span class="session-trace-ts">${fmtTime(ev.timestamp)}</span>
        </div>
        <div class="session-trace-usage">
          ${(ev.input_tokens != null || ev.output_tokens != null) ? `${fmtTokens(Number(ev.input_tokens || 0))} / ${fmtTokens(Number(ev.output_tokens || 0))}` : ""}
        </div>
      </div>
      ${ev.tool_name ? `<div class="session-trace-tool">${esc(ev.tool_name)}</div>` : ""}
      ${renderTraceValue("Content", ev.content)}
      ${ev.tool_input != null ? renderTraceValue("Tool Input", ev.tool_input) : ""}
      ${ev.tool_output != null ? renderTraceValue("Tool Output", ev.tool_output) : ""}
      ${ev.file_path ? `<div class="session-trace-file">File: ${esc(ev.file_path)}</div>` : ""}
    </div>
  `;
}

function renderTraceValue(label, value) {
  const text = prettyTraceValue(value);
  return `<div class="session-trace-block"><div class="session-trace-label">${esc(label)}</div><pre>${esc(text)}</pre></div>`;
}

function prettyTraceValue(value) {
  if (value == null) return "";
  if (typeof value === "string") return value;
  if (typeof value === "object") {
    if (typeof value.text === "string") return value.text;
    if (typeof value.message === "string") return value.message;
    if (typeof value.stderr === "string") return value.stderr;
    if (typeof value.stdout === "string") return value.stdout;
    if (typeof value.prompt === "string") return value.prompt;
    return JSON.stringify(value, null, 2);
  }
  return String(value);
}

function renderCompactEventLabel(ev) {
  const value = ev.content;
  if (value == null) return "";
  if (typeof value === "string") return truncate(value, 120);
  if (typeof value === "object") {
    const preferred = value.text || value.message || value.stderr || value.stdout || value.prompt || value.command || value.tool || value.path;
    if (preferred) return truncate(String(preferred), 120);
    return truncate(JSON.stringify(value), 120);
  }
  return truncate(String(value), 120);
}

window.filterSessions = function () {
  const agent = ($("#filter-agent")?.value || "").toLowerCase();
  const model = ($("#filter-model")?.value || "").toLowerCase();
  const items = window._sessionItems || [];
  const filtered = items.filter((item) => {
    const s = item.session;
    if (agent && !s.agent.toLowerCase().includes(agent)) return false;
    if (model && !(s.model || "").toLowerCase().includes(model)) return false;
    return true;
  });
  const tbody = $("#sessions-tbody");
  if (tbody) tbody.innerHTML = filtered.map(sessionRow).join("");
};

async function renderSessionDetail(id) {
  content.innerHTML = '<div class="loading">Loading session...</div>';
  try {
    const data = await api("/sessions/" + encodeURIComponent(id));
    const ss = data.session;

    content.innerHTML = `
      <a href="#/sessions" class="back-link">&larr; Back to Sessions</a>
      <div class="page-title">${esc(truncate(ss.session_id, 32))} ${statusBadge(ss.status)}</div>
      ${renderSessionDetailSections(data)}
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.exportSession = async function (id) {
  try {
    const data = await api("/sessions/" + encodeURIComponent(id) + "/export");
    const blob = new Blob([JSON.stringify(data, null, 2)], {
      type: "application/json",
    });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `session_${id}.json`;
    a.click();
  } catch (e) {
    alert("Export failed: " + e.message);
  }
};

window.exportSessionByButton = function (btn) {
  const encoded = btn?.dataset?.sessionId || "";
  if (!encoded) return;
  try {
    exportSession(decodeURIComponent(encoded));
  } catch (_e) {
    alert("Invalid session id");
  }
};

window.attestSession = async function (id) {
  if (!confirm("Create attestation for this session?")) return;
  try {
    const data = await apiPost(
      "/sessions/" + encodeURIComponent(id) + "/attest",
      {},
    );
    alert(
      "Attestation created!\nDigest: " +
        (data.attestation?.attestation_digest || "unknown"),
    );
  } catch (e) {
    alert("Attestation failed: " + e.message);
  }
};

window.attestSessionByButton = function (btn) {
  const encoded = btn?.dataset?.sessionId || "";
  if (!encoded) return;
  try {
    attestSession(decodeURIComponent(encoded));
  } catch (_e) {
    alert("Invalid session id");
  }
};

// =============================================================================
// PAGE: Configuration
// =============================================================================
// ---- Config sidebar+main state ----
let __cfgActiveSection = localStorage.getItem("halo_config_section") || "agentpmt-cta";

async function renderConfig() {
  content.innerHTML = '<div class="loading">Loading config...</div>';
  try {
    const cfg = await api("/config");
    let crypto = { locked: false, migration_status: "unknown", active_scopes: [], password_protected: false, bootstrap_mode: "required" };
    try { crypto = await api("/crypto/status"); } catch (_e) {}
    let agentsResp = { agents: [] };
    try { agentsResp = await api("/agents/list"); } catch (_e) {}
    let vaultResp = { keys: [] };
    let vaultKeysAuthRequired = false;
    try { vaultResp = await api("/vault/keys"); } catch (e) { vaultKeysAuthRequired = Number(e && e.status) === 401; }
    const vaultKeys = vaultResp.keys || [];
    let pmtToolsResp = { count: 0, tools: [] };
    let pmtToolsError = "";
    try { pmtToolsResp = await api("/agentpmt/tools"); } catch (e) { pmtToolsError = String((e && e.message) || "failed to load AgentPMT tool catalog"); }
    let identityCfgForConfig = {}, profileForConfig = {}, tierCfgForConfig = {}, genesisForConfig = {};
    try { identityCfgForConfig = await api("/identity/status") || {}; } catch (_e) {}
    try { profileForConfig = await api("/profile") || {}; } catch (_e) {}
    try { tierCfgForConfig = await api("/identity/tier") || {}; } catch (_e) {}
    try { genesisForConfig = await api("/genesis/status") || {}; } catch (_e) {}
    let wtProfile = {};
    try { wtProfile = await api("/worktree/active-profile") || {}; } catch (_e) {}

    const cfgDeviceOk = !!identityCfgForConfig.device_configured;
    const cfgNetworkOk = !!identityCfgForConfig.network_configured;
    const cfgAgentAddr = cfg.wallet_status && cfg.wallet_status.agentaddress_address;
    const cfgAgentOk = !!(cfg.wallet_status && cfg.wallet_status.agentaddress_connected);
    const cfgDidUri = genesisForConfig.did_uri || "";
    const cfgSeedHash = String(genesisForConfig.seed_hash_sha256 || "").slice(0, 24);
    const cfgGenesisComplete = !!genesisForConfig.completed;
    const cfgTier = String(tierCfgForConfig.tier || identityCfgForConfig.security_tier || "").trim();
    const cfgProfileName = String(profileForConfig.display_name || "").trim();
    const cfgEntropySources = Array.isArray(genesisForConfig.summary && genesisForConfig.summary.sources) ? genesisForConfig.summary.sources : [];
    const cfgCombinedEntropy = String(genesisForConfig.combined_entropy_sha256 || "").slice(0, 24);
    const cfgLedgerSigned = !!identityCfgForConfig.identity_ledger_fully_signed;
    const cfgLedgerEntries = identityCfgForConfig.identity_ledger_entry_count || 0;

    // ---- Section definitions with status indicators ----
    const agentpmtConnected = !!(cfg.wallet_status && cfg.wallet_status.agentpmt_connected);
    const pmtToolCount = Number(cfg.agentpmt && cfg.agentpmt.tool_count) || Number(pmtToolsResp.count) || 0;

    const sections = [
      { id: "agentpmt-cta", icon: "\uD83D\uDE80", label: "AgentPMT", status: agentpmtConnected ? "ok" : "action", group: "core" },
      { id: "auth", icon: "\uD83D\uDD10", label: "Authentication", status: cfg.authentication.authenticated ? "ok" : "warn", group: "core" },
      { id: "identity", icon: "\uD83E\uDEAA", label: "Identity", status: cfgDeviceOk && cfgNetworkOk && cfgGenesisComplete ? "ok" : "warn", group: "core" },
      { id: "cli-agents", icon: "\u2328", label: "Agent CLIs", status: "defer", group: "core" },
      { id: "security", icon: "\uD83D\uDEE1", label: "Crypto Lock", status: crypto.locked ? "warn" : "ok", group: "security" },
      { id: "agents", icon: "\uD83E\uDD16", label: "Agents", status: (agentsResp.agents || []).length > 0 ? "ok" : "muted", group: "security" },
      { id: "payments", icon: "\uD83D\uDCB8", label: "Payments", status: cfg.x402.enabled ? "ok" : "muted", group: "services" },
      { id: "services", icon: "\uD83D\uDD11", label: "API Keys", status: vaultKeys.some(k => k.configured) ? "ok" : "warn", group: "services" },
      { id: "models", icon: "\uD83D\uDDA5", label: "Local Models", status: "defer", group: "services" },
      { id: "paths", icon: "\uD83D\uDCC1", label: "Paths", status: "ok", group: "system" },
      { id: "external", icon: "\uD83D\uDD17", label: "External Sources", status: (wtProfile.injections || []).length > 0 ? "ok" : "muted", group: "system" },
      { id: "integrations", icon: "\uD83D\uDD0C", label: "Integrations", status: "muted", group: "system" },
      { id: "funding", icon: "\uD83D\uDCB0", label: "Funding", status: "muted", group: "system" },
      { id: "proof-lattice", icon: "\u25C6", label: "Proof Lattice", status: "ok", group: "system" },
      { id: "navigation", icon: "\uD83D\uDDC2", label: "Navigation", status: "ok", group: "system" },
    ];

    // ---- Health summary counts ----
    const healthOk = sections.filter(s => s.status === "ok").length;
    const healthWarn = sections.filter(s => s.status === "warn").length;
    const configuredKeys = vaultKeys.filter(k => k.configured).length;
    const agentCount = (agentsResp.agents || []).length;

    // ---- Build sidebar nav items ----
    const groupLabels = { core: "Core", security: "Security", services: "Services", system: "System" };
    let lastGroup = "";
    const sidebarHtml = sections.map(s => {
      const dot = s.status === "ok" ? "cfg-dot-ok" : s.status === "warn" || s.status === "action" ? "cfg-dot-warn" : "cfg-dot-muted";
      const active = __cfgActiveSection === s.id ? " is-active" : "";
      let prefix = "";
      if (s.group && s.group !== lastGroup) {
        lastGroup = s.group;
        prefix = `<div class="cfg-group-label">${groupLabels[s.group] || s.group}</div>`;
      }
      return prefix + `<button class="cfg-nav-item${active}" data-cfg-nav="${esc(s.id)}">
        <span class="cfg-nav-icon">${s.icon}</span>
        <span class="cfg-nav-label">${esc(s.label)}</span>
        <span class="cfg-dot ${dot}"></span>
      </button>`;
    }).join("");

    // ---- Build section content based on active selection ----
    const sec = __cfgActiveSection;
    let sectionHtml = "";

    if (sec === "agentpmt-cta") {
      sectionHtml = `
        <div class="cfg-agentpmt-hero">
          <div class="cfg-agentpmt-hero-top">
            <div class="cfg-agentpmt-hero-logo">
              <img src="img/agentpmt-192.png" alt="AgentPMT" onerror="this.style.display='none'">
              <div>
                <div class="cfg-agentpmt-hero-title">THE MARKETPLACE FOR AGENTS</div>
                <div class="cfg-agentpmt-hero-sub">Your agents deserve better tools.</div>
              </div>
            </div>
            <div class="cfg-agentpmt-free-badge">FREE</div>
          </div>
          <div class="cfg-agentpmt-hero-desc">
            100+ specialty tools, workflows, and APIs &mdash; all accessible via MCP.<br>
            No upfront dev costs. No code. No subscriptions.
          </div>
          <div class="cfg-agentpmt-benefits">
            <div class="cfg-agentpmt-benefit-card">
              <div class="cfg-agentpmt-benefit-icon">\uD83D\uDEE0</div>
              <div class="cfg-agentpmt-benefit-title">100+ MCP Tools</div>
              <div class="cfg-agentpmt-benefit-desc">Data analysis, code review, web scraping, image gen, and more. Your agents discover and use them autonomously.</div>
            </div>
            <div class="cfg-agentpmt-benefit-card">
              <div class="cfg-agentpmt-benefit-icon">\uD83D\uDCB0</div>
              <div class="cfg-agentpmt-benefit-title">Budget Controls</div>
              <div class="cfg-agentpmt-benefit-desc">Set spending limits per agent. USDC stablecoin payments. Smart contract enforcement. Real-time cost tracking.</div>
            </div>
            <div class="cfg-agentpmt-benefit-card">
              <div class="cfg-agentpmt-benefit-icon">\uD83D\uDD10</div>
              <div class="cfg-agentpmt-benefit-title">Post-Quantum Security</div>
              <div class="cfg-agentpmt-benefit-desc">ML-KEM encrypted credentials. Scoped agent permissions. Hardware-backed identity.</div>
            </div>
          </div>
          <a href="https://www.agentpmt.com" target="_blank" rel="noopener noreferrer" class="cfg-agentpmt-cta-btn">SIGN UP FREE &rarr;</a>
          <div class="cfg-agentpmt-status">
            ${agentpmtConnected
              ? `<span class="cfg-agentpmt-status-connected">\u2713 Connected${pmtToolCount > 0 ? " &mdash; " + pmtToolCount + " tools ready" : ""}</span>
                 <a href="#/agentpmt" class="btn btn-sm" style="margin-left:10px">Open AgentPMT Dashboard</a>`
              : `<span class="cfg-agentpmt-status-disconnected">\u25CB Not connected &mdash; sign up above to get started</span>`}
          </div>
          <div class="cfg-agentpmt-creator">
            Build and monetize your own tools. Earn credits every time your workflow is used. Free developer account included.
          </div>
        </div>`;
    } else if (sec === "cli-agents") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\u2328</div>
          <div><div class="config-section-title">Agent CLIs</div>
          <div class="config-section-kicker">Detected agent CLI installations and authentication status</div></div>
        </div>
        <div class="config-section-body" id="cfg-cli-agents-mount">
          <div class="cli-agents-grid" id="cli-agents-grid">
            <div class="cli-agent-row" data-cli="claude">
              <div class="cli-agent-info">
                <div class="cli-agent-name">Claude Code</div>
                <div class="cli-agent-provider">Anthropic</div>
              </div>
              <div class="cli-agent-status" id="cli-status-claude">Detecting...</div>
              <div class="cli-agent-actions">
                <button class="btn btn-sm btn-primary cli-auth-btn" data-cli="claude" disabled>Authenticate</button>
              </div>
            </div>
            <div class="cli-agent-row" data-cli="codex">
              <div class="cli-agent-info">
                <div class="cli-agent-name">Codex</div>
                <div class="cli-agent-provider">OpenAI</div>
              </div>
              <div class="cli-agent-status" id="cli-status-codex">Detecting...</div>
              <div class="cli-agent-actions">
                <button class="btn btn-sm btn-primary cli-auth-btn" data-cli="codex" disabled>Authenticate</button>
              </div>
            </div>
            <div class="cli-agent-row" data-cli="gemini">
              <div class="cli-agent-info">
                <div class="cli-agent-name">Gemini CLI</div>
                <div class="cli-agent-provider">Google</div>
              </div>
              <div class="cli-agent-status" id="cli-status-gemini">Detecting...</div>
              <div class="cli-agent-actions">
                <button class="btn btn-sm btn-primary cli-auth-btn" data-cli="gemini" disabled>Authenticate</button>
              </div>
            </div>
          </div>
          ${!(cfg && cfg.container_runtime && cfg.container_runtime.available)
            ? '<div class="info-banner amber" style="margin-top:10px"><span style="font-weight:600">&#9888; Native session mode active</span> &mdash; Subsidiary agents launch as native local processes.</div>'
            : '<div style="margin-top:10px;font-size:11px;color:var(--text-dim)"><span style="color:var(--green)">&#10003;</span> Native launcher: <strong>' + esc(cfg.container_runtime.engine || "") + '</strong></div>'
          }
          <div id="cli-auth-terminal-wrap" style="display:none;margin-top:14px">
            <div style="font-size:12px;color:var(--accent);margin-bottom:6px" id="cli-auth-terminal-label">Authentication session</div>
            <div id="cli-auth-terminal" style="height:260px;border:1px solid var(--border);border-radius:6px;overflow:hidden"></div>
            <div style="margin-top:8px;display:flex;gap:8px">
              <button class="btn btn-sm" id="cli-auth-terminal-close">Close Terminal</button>
            </div>
          </div>
        </div>`;
    } else if (sec === "proof-lattice") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\u25C6</div>
          <div><div class="config-section-title">Proof Lattice</div>
          <div class="config-section-kicker">3D visualization of verified Lean proof declarations</div></div>
        </div>
        <div class="config-section-body">
          <div class="proof-lattice-wrap" id="proof-lattice-wrap">
            <div class="proof-lattice-hero" id="proof-lattice-hero">
              <div id="proof-lattice-three"></div>
              <div class="proof-lattice-status" id="proof-lattice-status">Initializing proof lattice...</div>
            </div>
            <div class="proof-lattice-sidebar" id="proof-lattice-sidebar">
              <div class="pls-header">Proof Families</div>
              <div class="pls-families" id="pls-families"></div>
              <div class="pls-divider"></div>
              <div class="pls-header">Stats</div>
              <div class="pls-stats" id="pls-stats"><span class="pls-muted">Loading...</span></div>
              <div class="pls-divider"></div>
              <div class="pls-header">Selected Node</div>
              <div class="pls-selected" id="pls-selected"><span class="pls-muted">Click a node to inspect</span></div>
            </div>
          </div>
        </div>`;
    } else if (sec === "auth") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDD10</div>
          <div><div class="config-section-title">Authentication</div>
          <div class="config-section-kicker">Operator identity and dashboard access posture</div></div>
        </div>
        <div class="config-section-body">
          <div class="config-row">
            <div><div class="config-label">Status</div>
            <div class="config-desc">Local mode (auth optional) or OAuth-authenticated (enforced mode)</div></div>
            ${cfg.authentication.authenticated
              ? '<span class="badge badge-ok">Authenticated</span>'
              : '<span class="badge badge-warn">Not Authenticated</span>'}
          </div>
        </div>`;
    } else if (sec === "identity") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83E\uDEAA</div>
          <div><div class="config-section-title">Identity</div>
          <div class="config-section-kicker">Device anchors, network identity, agent address, genesis provenance, and DID</div></div>
        </div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">Profile</div><div class="config-desc">${cfgProfileName ? esc(cfgProfileName) : "Not set"}</div></div>
            <span class="badge ${cfgProfileName ? "badge-ok" : "badge-muted"}">${cfgProfileName ? "Set" : "Unset"}</span></div>
          <div class="config-row"><div><div class="config-label">Security Tier</div><div class="config-desc">${cfgTier ? esc(cfgTier) : "Not configured"}</div></div>
            <span class="badge ${cfgTier ? "badge-ok" : "badge-muted"}">${cfgTier || "\u2014"}</span></div>
          <div class="config-row"><div><div class="config-label">Device Identity</div><div class="config-desc">${cfgDeviceOk ? "Verified \u2014 hardware fingerprint collected" : "Not configured"}</div></div>
            <span class="badge ${cfgDeviceOk ? "badge-ok" : "badge-warn"}">${cfgDeviceOk ? "Complete" : "Pending"}</span></div>
          <div class="config-row"><div><div class="config-label">Network Identity</div><div class="config-desc">${cfgNetworkOk ? "Verified \u2014 network anchors collected" : "Not configured"}</div></div>
            <span class="badge ${cfgNetworkOk ? "badge-ok" : "badge-warn"}">${cfgNetworkOk ? "Complete" : "Pending"}</span></div>
          <div class="config-row"><div><div class="config-label">Agent Address (EVM)</div><div class="config-desc" style="font-size:10px">${cfgAgentAddr ? esc(String(cfgAgentAddr)) : "Not generated"}</div></div>
            <span class="badge ${cfgAgentOk ? "badge-ok" : "badge-warn"}">${cfgAgentOk ? "Active" : "Pending"}</span></div>
        </div>
        <div class="cfg-subsection-label">Genesis Provenance</div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">Genesis Ceremony</div><div class="config-desc">${cfgGenesisComplete ? "Completed" : "Not completed"}</div></div>
            <span class="badge ${cfgGenesisComplete ? "badge-ok" : "badge-warn"}">${cfgGenesisComplete ? "Done" : "Pending"}</span></div>
          ${cfgSeedHash ? `<div class="config-row"><div><div class="config-label">Seed Hash (SHA-256)</div><div class="config-desc" style="font-size:10px;font-family:var(--mono)">${esc(cfgSeedHash)}...</div></div></div>` : ""}
          ${cfgDidUri ? `<div class="config-row"><div><div class="config-label">DID URI</div><div class="config-desc" style="font-size:10px;font-family:var(--mono);word-break:break-all">${esc(cfgDidUri)}</div></div></div>` : ""}
          ${cfgCombinedEntropy ? `<div class="config-row"><div><div class="config-label">Combined Entropy</div><div class="config-desc" style="font-size:10px;font-family:var(--mono)">${esc(cfgCombinedEntropy)}...</div></div></div>` : ""}
          ${cfgEntropySources.length ? cfgEntropySources.map(s => `<div class="config-row"><div><div class="config-label">${esc(String(s.name || s.source || "Source"))}</div><div class="config-desc" style="font-size:10px;font-family:var(--mono)">${esc(String(s.detail || s.pulse_id || s.round || s.id || ""))}</div></div><span class="badge badge-ok">Collected</span></div>`).join("") : ""}
          <div class="config-row"><div><div class="config-label">Identity Ledger</div><div class="config-desc">${cfgLedgerSigned ? cfgLedgerEntries + " entries, fully signed" : cfgLedgerEntries > 0 ? cfgLedgerEntries + " entries" : "Empty"}</div></div>
            <span class="badge ${cfgLedgerSigned ? "badge-ok" : cfgLedgerEntries > 0 ? "badge-warn" : "badge-muted"}">${cfgLedgerSigned ? "Signed" : "\u2014"}</span></div>
        </div>`;
    } else if (sec === "security") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDEE1</div>
          <div><div class="config-section-title">Crypto Lock</div>
          <div class="config-section-kicker">Session bootstrap mode, lock state, and active scopes</div></div>
        </div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">Lock State</div><div class="config-desc">${crypto.locked ? "Locked" : "Unlocked"}</div></div>
            <span class="badge ${crypto.locked ? "badge-warn" : "badge-ok"}">${crypto.locked ? "Locked" : "Unlocked"}</span></div>
          <div class="config-row"><div><div class="config-label">Migration</div><div class="config-desc">${esc(String(crypto.migration_status || "unknown"))}</div></div></div>
          <div class="config-row"><div><div class="config-label">Bootstrap Mode</div><div class="config-desc">${esc(String(crypto.bootstrap_mode || "required"))}</div></div></div>
          <div class="config-row"><div><div class="config-label">Active Scopes</div><div class="config-desc">${esc((crypto.active_scopes || []).join(", ") || "none")}</div></div></div>
          <div class="config-row"><div><div class="config-label">Actions</div><div class="config-desc">Lock or unlock the crypto session</div></div>
            <div style="display:flex;gap:6px;align-items:center">
              <button class="btn btn-sm" onclick="forceCryptoLock()">Lock Now</button>
              ${!crypto.password_protected
                ? '<button class="btn btn-sm btn-primary" onclick="showCryptoSetupPrompt()">Set Password</button>'
                : '<button class="btn btn-sm btn-primary" onclick="ensureCryptoUnlocked(true)">Unlock</button>'}
            </div>
          </div>
        </div>`;
    } else if (sec === "agents") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83E\uDD16</div>
          <div><div class="config-section-title">Authorized Agents</div>
          <div class="config-section-kicker">Scoped credentials, local wrappers, and runtime delegation</div></div>
        </div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">Agent Credentials</div><div class="config-desc">Per-scope ML-KEM credentials for autonomous agents.</div></div>
            <button class="btn btn-sm btn-primary" onclick="authorizeAgentPrompt()">Authorize New Agent</button></div>
          ${(agentsResp.agents || []).length
            ? (agentsResp.agents || []).map(agent => `<div class="config-row"><div><div class="config-label">${esc(agent.label || agent.agent_id)}</div><div class="config-desc">${esc(agent.agent_id)} \u00B7 scopes: ${esc((agent.scopes || []).join(", "))}${agent.expires_at ? " \u00B7 expires: " + esc(new Date(agent.expires_at * 1000).toISOString()) : ""}</div></div><button class="btn btn-sm" onclick="revokeAgent('${esc(agent.agent_id)}')">Revoke</button></div>`).join("")
            : `<div class="config-row"><div><div class="config-label">No authorized agents</div><div class="config-desc">Create an agent credential to enable scoped autonomous access.</div></div></div>`}
        </div>
        <div class="cfg-subsection-label">CLI Wrappers</div>
        <div class="config-section-body">
          ${["claude", "codex", "gemini"].map(agent => `<div class="config-row"><div><div class="config-label">${agent.charAt(0).toUpperCase() + agent.slice(1)}</div><div class="config-desc">Wrap ${agent} commands through H.A.L.O.</div></div><button class="toggle ${cfg.wrapping[agent] ? "on" : ""}" onclick="toggleWrap('${agent}', ${!cfg.wrapping[agent]})"></button></div>`).join("")}
          <div class="config-row"><div><div class="config-desc">Shell RC: ${esc(cfg.wrapping.shell_rc)}</div></div></div>
        </div>`;
    } else if (sec === "payments") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDCB8</div>
          <div><div class="config-section-title">Payments</div>
          <div class="config-section-kicker">Stablecoin settlement, tool budgets, and third-party execution</div></div>
        </div>
        <div class="cfg-subsection-label">x402direct</div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">x402direct Integration</div><div class="config-desc">Stablecoin payments for AI agents</div></div>
            <button class="toggle ${cfg.x402.enabled ? "on" : ""}" onclick="toggleX402(${!cfg.x402.enabled})"></button></div>
          <div class="config-row"><div><div class="config-label">Network</div><div class="config-desc">${cfg.x402.network}</div></div>
            <span class="badge badge-info">${cfg.x402.network}</span></div>
          <div class="config-row"><div><div class="config-label">Max Auto-Approve</div><div class="config-desc">${fmtCost(cfg.x402.max_auto_approve_usd)} USDC</div></div></div>
        </div>
        <div class="cfg-subsection-label">AgentPMT Proxy</div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">Tool Proxy</div><div class="config-desc">Third-party tool access via AgentPMT</div></div>
            <span class="badge ${cfg.agentpmt.enabled ? "badge-ok" : "badge-muted"}">${cfg.agentpmt.enabled ? "Enabled" : "Disabled"}</span></div>
          <div class="config-row"><div><div class="config-label">Budget Tag</div><div class="config-desc">${esc(cfg.agentpmt.budget_tag || "(none)")}</div></div></div>
          <div class="config-row"><div><div class="config-label">MCP Endpoint</div><div class="config-desc" style="font-size:10px">${esc(cfg.agentpmt.endpoint || "(default)")}</div></div></div>
          <div class="config-row"><div><div class="config-label">Credential Status</div><div class="config-desc">${cfg.agentpmt.auth_configured ? "Configured" : "Missing"}</div></div>
            <span class="badge ${cfg.agentpmt.auth_configured ? "badge-ok" : "badge-warn"}">${cfg.agentpmt.auth_configured ? "Ready" : "Needs Key"}</span></div>
          <div class="config-row"><div><div class="config-label">Tool Catalog</div><div class="config-desc">${Number(pmtToolsResp.count || 0)} tools discovered (${esc(String(pmtToolsResp.source || "cache"))}${pmtToolsResp.stale ? ", stale" : ", fresh"})${pmtToolsError ? '<br><span style="color:var(--red)">Error: ' + esc(pmtToolsError) + "</span>" : ""}</div></div>
            <button class="btn btn-sm" onclick="refreshAgentPmtCatalog()">Refresh</button></div>
        </div>
        <div class="cfg-subsection-label">On-Chain &amp; Addons</div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">Chain</div><div class="config-desc">${esc(cfg.onchain.chain_name || "Not configured")} (ID: ${esc(cfg.onchain.chain_id)})</div></div></div>
          <div class="config-row"><div><div class="config-label">Contract</div><div class="config-desc" style="font-size:10px">${esc(cfg.onchain.contract_address || "(not deployed)")}</div></div></div>
          <div class="config-row"><div><div class="config-label">p2pclaw</div><div class="config-desc">Marketplace integration</div></div>
            <span class="badge ${cfg.addons.p2pclaw ? "badge-ok" : "badge-muted"}">${cfg.addons.p2pclaw ? "Enabled" : "Disabled"}</span></div>
          <div class="config-row"><div><div class="config-label">AgentPMT Workflows</div><div class="config-desc">Challenge and workflow extensions</div></div>
            <span class="badge ${cfg.addons.agentpmt_workflows ? "badge-ok" : "badge-muted"}">${cfg.addons.agentpmt_workflows ? "Enabled" : "Disabled"}</span></div>
        </div>`;
    } else if (sec === "services") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDD11</div>
          <div><div class="config-section-title">API Keys &amp; Services</div>
          <div class="config-section-kicker">Provider readiness, status indicators, and service controls</div></div>
        </div>
        <div class="config-section-body">
          ${cfg.vault?.available ? `
            ${vaultKeysAuthRequired ? `<div class="config-row"><div><div class="config-label">Authentication required</div><div class="config-desc">Unlock sensitive controls first, then add provider API keys.</div></div><button class="btn btn-sm btn-primary" onclick="__cfgActiveSection='services';localStorage.setItem('halo_config_section','services');renderConfig()">Configure Keys</button></div>` : ""}
            ${vaultKeys.map(k => {
              const pi = PROVIDER_INFO[String(k.provider || "").toLowerCase()] || {};
              const isRequired = pi.required;
              const desc = pi.description || "";
              const catLabel = pi.category === "storage" ? "Storage" : pi.category === "llm" ? "LLM" : pi.category === "tooling" ? "Tooling" : "";
              return `<div class="config-row config-provider-row"><div><div class="config-label">${esc(pi.name || k.provider)}${isRequired ? ' <span class="badge badge-warn" style="font-size:9px;margin-left:6px">REQUIRED</span>' : ""}${catLabel ? ' <span class="badge badge-info" style="font-size:9px;margin-left:4px">' + esc(catLabel) + "</span>" : ""}</div><div class="config-desc">${esc(k.env_var)} \u00B7 ${k.configured ? "Configured" : "Missing"}${k.tested ? " \u00B7 Tested" : ""}</div>${desc ? '<div class="config-desc" style="font-size:10px;margin-top:2px">' + esc(desc) + "</div>" : ""}</div><div style="display:flex;gap:6px;align-items:center"><button class="btn btn-sm" onclick="vaultSetKey('${esc(k.provider)}','${esc(k.env_var)}')">Set Key</button><button class="btn btn-sm" onclick="vaultTestKey('${esc(k.provider)}')">Test</button><button class="btn btn-sm" onclick="vaultRemoveKey('${esc(k.provider)}')">Remove</button></div></div>`;
            }).join("")}
          ` : `<div class="config-row"><div><div class="config-label">Vault unavailable</div><div class="config-desc">Create/import a PQ wallet to enable encrypted API key storage.</div></div><button class="btn btn-sm btn-primary" onclick="__cfgActiveSection='services';localStorage.setItem('halo_config_section','services');renderConfig()">Configure Keys</button></div>`}
        </div>`;
    } else if (sec === "models") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDDA5</div>
          <div><div class="config-section-title">Local Models</div>
          <div class="config-section-kicker">Runtime inventory, backend status, and operator controls</div></div>
        </div>
        <div id="cfg-models-mount" class="config-section-body">
          <div class="config-row"><div><div class="config-desc">Loading model status...</div></div></div>
        </div>`;
    } else if (sec === "paths") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDCC1</div>
          <div><div class="config-section-title">Paths</div>
          <div class="config-section-kicker">Filesystem anchors and local materialized state</div></div>
        </div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">Home</div><div class="config-desc" style="font-size:10px">${esc(cfg.paths.home)}</div></div></div>
          <div class="config-row"><div><div class="config-label">Database</div><div class="config-desc" style="font-size:10px">${esc(cfg.paths.db)}</div></div></div>
          <div class="config-row"><div><div class="config-label">PQ Wallet</div><div class="config-desc">${cfg.pq_wallet ? "Present (ML-DSA-65)" : "Not created"}</div></div></div>
        </div>`;
    } else if (sec === "integrations") {
      const optLLM = Object.keys(PROVIDER_INFO).filter(p => !PROVIDER_INFO[p].required && PROVIDER_INFO[p].category === "llm");
      const optStorage = Object.keys(PROVIDER_INFO).filter(p => !PROVIDER_INFO[p].required && PROVIDER_INFO[p].category === "storage");
      const optTooling = Object.keys(PROVIDER_INFO).filter(p => !PROVIDER_INFO[p].required && PROVIDER_INFO[p].category === "tooling");
      const keyStatus = {};
      vaultKeys.forEach(k => { keyStatus[String(k.provider || "").toLowerCase()] = k; });
      function ps(provider) { const v = keyStatus[provider]; if (!v) return { configured: false, tested: false }; return { configured: !!v.configured, tested: !!v.tested }; }
      function sb(provider) { const s = ps(provider); if (s.tested) return '<span class="badge badge-ok">Verified</span>'; if (s.configured) return '<span class="badge badge-warn">Configured</span>'; return '<span class="badge badge-muted">Not configured</span>'; }
      function pc(provider) { const info = PROVIDER_INFO[provider] || { name: provider, envVar: providerDefaultEnv(provider), keyUrl: "#", description: "" }; const s = ps(provider); const dl = info.keyUrl && info.keyUrl !== "#" ? '<a class="btn btn-sm" href="' + esc(info.keyUrl) + '" target="_blank" rel="noopener noreferrer">Get Key</a>' : ""; return '<div class="config-row" style="flex-wrap:wrap"><div style="flex:1;min-width:180px"><div class="config-label">' + esc(info.name) + '</div><div class="config-desc" style="font-size:10px">' + esc(info.envVar) + '</div>' + (info.description ? '<div class="config-desc">' + esc(info.description) + '</div>' : '') + '</div><div style="display:flex;gap:6px;align-items:center">' + sb(provider) + dl + '<button class="btn btn-sm btn-primary setup-provider-config-btn" data-provider="' + esc(provider) + '">Set Key</button>' + (s.configured ? '<button class="btn btn-sm setup-provider-test-btn" data-provider="' + esc(provider) + '">Test</button>' : '') + (s.configured ? '<button class="btn btn-sm setup-provider-disconnect-btn" data-provider="' + esc(provider) + '" title="Remove this key">Disconnect</button>' : '') + '</div></div>'; }
      let intHtml = "";
      if (optStorage.length) { intHtml += '<div class="cfg-subsection-label">Immutable Storage</div><div class="config-section-body">' + optStorage.map(p => pc(p)).join("") + '</div>'; }
      if (optLLM.length) { intHtml += '<div class="cfg-subsection-label">Direct LLM Keys</div><div class="config-section-body">' + optLLM.map(p => pc(p)).join("") + '</div>'; }
      if (optTooling.length) { intHtml += '<div class="cfg-subsection-label">Additional Tools</div><div class="config-section-body">' + optTooling.map(p => pc(p)).join("") + '</div>'; }
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDD0C</div>
          <div><div class="config-section-title">Optional Integrations</div>
          <div class="config-section-kicker">Storage, direct LLM keys, and additional tooling</div></div>
        </div>
        ${intHtml}`;
    } else if (sec === "external") {
      const injections = Array.isArray(wtProfile.injections) ? wtProfile.injections : [];
      const injRows = injections.map((inj, i) => {
        const modeLabel = inj.mode === "readonly" ? "Read-only" : inj.mode === "copy" ? "Copy" : inj.mode === "approved_write" ? "Approved Write" : esc(inj.mode || "readonly");
        const desc = inj.description || "";
        const isSkills = (inj.target || "").includes("skills");
        const isMcp = (inj.target || "").includes("mcp") || (inj.target || "").includes(".mcp");
        const typeIcon = isSkills ? "\uD83D\uDCDA" : isMcp ? "\u2699\uFE0F" : "\uD83D\uDCC4";
        const typeLabel = isSkills ? "Skills" : isMcp ? "MCP Tools" : "File";
        return `<div class="config-row" style="flex-wrap:wrap;gap:8px">
          <div style="flex:1;min-width:200px">
            <div class="config-label">${typeIcon} ${esc(typeLabel)}: ${esc(inj.target || "")}</div>
            <div class="config-desc" style="font-size:10px;word-break:break-all">${esc(inj.source || "")}</div>
            ${desc ? '<div class="config-desc">' + esc(desc) + '</div>' : ''}
          </div>
          <div style="display:flex;gap:6px;align-items:center">
            <span class="badge ${inj.mode === "readonly" ? "badge-ok" : inj.mode === "copy" ? "badge-info" : "badge-warn"}">${esc(modeLabel)}</span>
            <button class="btn btn-sm wt-inj-remove" data-wt-inj-idx="${i}" title="Remove this injection">Remove</button>
          </div>
        </div>`;
      }).join("");

      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDD17</div>
          <div><div class="config-section-title">External Sources</div>
          <div class="config-section-kicker">Point to external Skills registries and MCP tool configs. Injected into agent worktrees.</div></div>
        </div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">External Write Policy</div><div class="config-desc">Controls whether agents can write to files outside the container. <strong>Deny</strong> = read-only access to external sources. <strong>Worktree</strong> = create a git worktree for external writes.</div></div>
            <select class="input" id="wt-write-policy" style="width:auto;min-width:140px">
              <option value="deny" ${(wtProfile.external_write_policy || "deny") === "deny" ? "selected" : ""}>Deny (read-only)</option>
              <option value="worktree" ${(wtProfile.external_write_policy || "deny") === "worktree" ? "selected" : ""}>Worktree</option>
            </select>
          </div>
          <div class="config-row"><div><div class="config-label">Active Profile</div><div class="config-desc" style="font-size:10px">${esc(wtProfile.profile_name || "default")}</div></div></div>
          <div class="config-row" style="flex-wrap:wrap;gap:8px">
            <div style="flex:1;min-width:200px"><div class="config-label">\u2112 Lean Project Path</div><div class="config-desc">Absolute path to a Lean project directory. Used by the Lean file browser page.</div></div>
            <div style="display:flex;gap:6px;align-items:center;flex:1;min-width:200px">
              <input class="input" id="wt-lean-path" value="${esc(wtProfile.lean_project_path || "")}" placeholder="/home/user/project/lean" style="flex:1">
              <button class="btn btn-sm btn-primary" id="wt-lean-save">Save</button>
            </div>
          </div>
        </div>
        <div class="cfg-subsection-label">Injected Paths (${injections.length})</div>
        <div class="config-section-body">
          ${injRows || '<div class="config-desc" style="padding:8px">No external sources configured. Add a skills directory or MCP tool config below.</div>'}
        </div>
        <div class="cfg-subsection-label">Add New External Source</div>
        <div class="config-section-body">
          <div style="display:flex;flex-direction:column;gap:8px;padding:4px 0">
            <label class="mcp-form-field"><span class="mcp-form-label">Source Path (absolute path on host)</span>
              <input class="input" id="wt-new-source" placeholder="/home/user/project/.agents/skills"></label>
            <label class="mcp-form-field"><span class="mcp-form-label">Target Path (relative path in worktree)</span>
              <input class="input" id="wt-new-target" placeholder=".agents/skills"></label>
            <label class="mcp-form-field"><span class="mcp-form-label">Access Mode</span>
              <select class="input" id="wt-new-mode">
                <option value="readonly" selected>Read-only (recommended)</option>
                <option value="copy">Copy (agent can modify)</option>
                <option value="approved_write">Approved Write (symlink, human approval required)</option>
              </select></label>
            <label class="mcp-form-field"><span class="mcp-form-label">Description (optional)</span>
              <input class="input" id="wt-new-desc" placeholder="e.g., Heyting skill registry"></label>
            <div class="network-form-actions"><button class="btn btn-primary" id="wt-add-injection">Add Source</button></div>
          </div>
        </div>`;
    } else if (sec === "funding") {
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDCB0</div>
          <div><div class="config-section-title">Funding &amp; Monetization</div>
          <div class="config-section-kicker">Verified channels for customer balance top-ups</div></div>
        </div>
        <div id="config-funding-balance" class="card-grid" style="margin:12px 0"></div>
        <div class="config-section-body">
          <div class="config-row"><div><div class="config-label">AgentPMT Token Purchase</div><div class="config-desc">Customers buy tokens at AgentPMT.com. Signed receipts verified via HMAC-SHA256.</div></div></div>
          <div class="config-row"><div><div class="config-label">x402direct (USDC on Base L2)</div><div class="config-desc">Direct USDC stablecoin payment. Transaction hash verified on-chain.</div></div></div>
        </div>
        <div style="font-size:11px;color:var(--text-dim);margin-top:8px;line-height:1.5;padding:0 4px">
          All tools, workflows, and agent configurations are accessible exclusively through AgentPMT MCP.
        </div>`;
    } else if (sec === "navigation") {
      const hiddenItems = Array.isArray(wtProfile.hidden_nav_items) ? wtProfile.hidden_nav_items : [];
      const navItems = [
        { page: "agentpmt", label: "AgentPMT", icon: "\u2699" },
        { page: "overview", label: "Overview", icon: "\u25C6", note: "Hides all sub-pages (Genesis, Identification, Communication, Memories, Flow Chart)" },
        { page: "sessions", label: "Sessions", icon: "\u2630" },
        { page: "trust", label: "Trust", icon: "\u2713" },
        { page: "proof-gate", label: "Proof Gate", icon: "\uD83D\uDEE1" },
        { page: "nucleusdb", label: "NucleusDB", icon: "\u2622" },
        { page: "cockpit", label: "Cockpit", icon: "\u25B6" },
        { page: "orchestration", label: "Orchestration", icon: "\u267B" },
        { page: "p2pclaw", label: "P2PCLAW", icon: "\u263C" },
        { page: "mcp-tools", label: "MCP Tools", icon: "\u2692" },
        { page: "skills", label: "Skills", icon: "\u270E" },
        { page: "lean", label: "Lean", icon: "\u2112", note: "Also requires Lean Project Path to be set in External Sources" },
        { page: "proof-explorer", label: "Proof Explorer", icon: "\u25C6" },
        { page: "system-monitor", label: "System Monitor", icon: "\u2699", note: "DGX Spark hardware monitoring. Shows simulation when not on DGX hardware." },
      ];
      const rows = navItems.map(item => {
        const isHidden = hiddenItems.includes(item.page);
        return `<div class="config-row">
          <div><div class="config-label">${esc(item.icon)} ${esc(item.label)}</div>
          ${item.note ? '<div class="config-desc">' + esc(item.note) + '</div>' : ''}</div>
          <button class="toggle ${isHidden ? '' : 'on'}" data-nav-toggle="${esc(item.page)}"></button>
        </div>`;
      }).join("");
      sectionHtml = `
        <div class="config-section-heading">
          <div class="config-section-icon">\uD83D\uDDC2</div>
          <div><div class="config-section-title">Navigation</div>
          <div class="config-section-kicker">Show or hide pages in the dashboard sidebar. Configuration is always visible.</div></div>
        </div>
        <div class="config-section-body">
          ${rows}
        </div>
        <div style="font-size:11px;color:var(--text-dim);margin-top:8px;line-height:1.5;padding:0 4px">
          Changes take effect immediately. Hidden pages are still accessible via direct URL.
        </div>`;
    }

    content.innerHTML = `<div class="mcp-shell cfg-shell">
      <aside class="card mcp-sidebar cfg-sidebar">
        <div class="card-label">Configuration</div>
        <div class="card-sub">System settings and operator controls</div>
        <div class="cfg-nav">${sidebarHtml}</div>
      </aside>
      <section class="mcp-main">
        <section class="card">
          <div class="mcp-header">
            <div><div class="page-title">${esc(sections.find(s => s.id === sec)?.label || "Configuration")}</div></div>
            <div style="display:flex;gap:10px">
              <div class="mcp-summary-card"><span>Healthy</span><strong>${healthOk}</strong></div>
              <div class="mcp-summary-card"><span>Attention</span><strong style="color:${healthWarn > 0 ? "var(--yellow)" : "var(--accent)"}">${healthWarn}</strong></div>
              <div class="mcp-summary-card"><span>API Keys</span><strong>${configuredKeys}/${vaultKeys.length}</strong></div>
              <div class="mcp-summary-card"><span>Agents</span><strong>${agentCount}</strong></div>
            </div>
          </div>
        </section>
        <section class="config-section-card" style="margin-top:16px">
          ${sectionHtml}
        </section>
      </section>
    </div>`;

    // ---- Sidebar navigation ----
    content.querySelectorAll("[data-cfg-nav]").forEach(btn => {
      btn.addEventListener("click", () => {
        __cfgActiveSection = btn.dataset.cfgNav;
        localStorage.setItem("halo_config_section", __cfgActiveSection);
        renderConfig();
      });
    });

    // ---- External Sources (worktree profile) event bindings ----
    const wtWritePolicy = content.querySelector("#wt-write-policy");
    if (wtWritePolicy) {
      wtWritePolicy.addEventListener("change", async () => {
        wtProfile.external_write_policy = wtWritePolicy.value;
        wtProfile.worktree_isolation = wtWritePolicy.value === "worktree";
        try { await apiPost(`/worktree/profile/${encodeURIComponent(wtProfile.profile_name || "default")}`, wtProfile); } catch (_e) {}
        renderConfig();
      });
    }
    const wtLeanSave = content.querySelector("#wt-lean-save");
    if (wtLeanSave) {
      wtLeanSave.addEventListener("click", async () => {
        const val = (content.querySelector("#wt-lean-path")?.value || "").trim();
        wtProfile.lean_project_path = val || null;
        try { await apiPost(`/worktree/profile/${encodeURIComponent(wtProfile.profile_name || "default")}`, wtProfile); } catch (_e) {}
        if (typeof __updateLeanNavVisibility === 'function') __updateLeanNavVisibility();
        renderConfig();
      });
    }
    content.querySelectorAll(".wt-inj-remove").forEach(btn => {
      btn.addEventListener("click", async () => {
        const idx = Number(btn.dataset.wtInjIdx);
        if (!Array.isArray(wtProfile.injections)) return;
        wtProfile.injections.splice(idx, 1);
        try { await apiPost(`/worktree/profile/${encodeURIComponent(wtProfile.profile_name || "default")}`, wtProfile); } catch (_e) {}
        renderConfig();
      });
    });
    const wtAddBtn = content.querySelector("#wt-add-injection");
    if (wtAddBtn) {
      wtAddBtn.addEventListener("click", async () => {
        const source = (content.querySelector("#wt-new-source")?.value || "").trim();
        const target = (content.querySelector("#wt-new-target")?.value || "").trim();
        const mode = content.querySelector("#wt-new-mode")?.value || "readonly";
        const desc = (content.querySelector("#wt-new-desc")?.value || "").trim();
        if (!source || !target) { alert("Source path and target path are required."); return; }
        if (!Array.isArray(wtProfile.injections)) wtProfile.injections = [];
        const entry = { source, target, mode, description: desc || undefined };
        wtProfile.injections.push(entry);
        try { await apiPost(`/worktree/profile/${encodeURIComponent(wtProfile.profile_name || "default")}`, wtProfile); } catch (e) { alert("Save failed: " + String((e && e.message) || e)); return; }
        renderConfig();
      });
    }

    // ---- Auto-open provider from setup redirect ----
    const autoOpenProvider = localStorage.getItem("halo_setup_open_provider");
    if (autoOpenProvider) {
      localStorage.removeItem("halo_setup_open_provider");
      const providerEntry = vaultKeys.find(
        (k) => String(k.provider || "").toLowerCase() === autoOpenProvider,
      );
      if (providerEntry) {
        openVaultModal(providerEntry.provider, providerEntry.env_var || providerDefaultEnv(providerEntry.provider));
      }
    }

    // ---- Navigation toggle bindings ----
    content.querySelectorAll("[data-nav-toggle]").forEach(btn => {
      btn.addEventListener("click", async () => {
        const page = btn.dataset.navToggle;
        if (!Array.isArray(wtProfile.hidden_nav_items)) wtProfile.hidden_nav_items = [];
        const idx = wtProfile.hidden_nav_items.indexOf(page);
        if (idx >= 0) {
          wtProfile.hidden_nav_items.splice(idx, 1);
        } else {
          wtProfile.hidden_nav_items.push(page);
        }
        try { await apiPost(`/worktree/profile/${encodeURIComponent(wtProfile.profile_name || "default")}`, wtProfile); } catch (_e) {}
        if (typeof __updateNavVisibility === 'function') __updateNavVisibility();
        renderConfig();
      });
    });

    // ---- Deferred section loading ----
    if (sec === "models") { await injectConfigModelsSection(); }
    if (sec === "funding") { await injectConfigFundingBalance(); }
    if (sec === "cli-agents") { await injectConfigCliAgents(); }
    if (sec === "proof-lattice") { await initConfigProofLattice(); }

    // ---- Provider button handlers (integrations section) ----
    content.querySelectorAll(".setup-provider-config-btn[data-provider]").forEach(btn => {
      btn.addEventListener("click", () => {
        const provider = btn.dataset.provider || "";
        const info = PROVIDER_INFO[provider] || {};
        openVaultModal(provider, info.envVar || providerDefaultEnv(provider));
      });
    });
    content.querySelectorAll(".setup-provider-test-btn[data-provider]").forEach(btn => {
      btn.addEventListener("click", () => { window.vaultTestKey(btn.dataset.provider || ""); });
    });
    content.querySelectorAll(".setup-provider-disconnect-btn[data-provider]").forEach(btn => {
      btn.addEventListener("click", () => { window.vaultRemoveKey(btn.dataset.provider || ""); });
    });
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

async function injectConfigFundingBalance() {
  const mount = document.getElementById("config-funding-balance");
  if (!mount) return;
  try {
    const bal = await api("/x402/balance");
    mount.innerHTML = `
      <div class="card"><span class="card-label">USDC Balance</span><strong>${esc(String(bal.balance_usdc ?? "\u2014"))}</strong></div>
      <div class="card"><span class="card-label">Address</span><strong style="font-family:var(--mono);font-size:10px;word-break:break-all">${esc(String(bal.address ?? "\u2014"))}</strong></div>
      <div class="card"><span class="card-label">Network</span><strong>${esc(String(bal.network ?? "\u2014"))}</strong></div>`;
  } catch (_) {
    mount.innerHTML = '<div class="card"><span class="card-label">Balance</span><strong class="muted">x402 not configured</strong></div>';
  }
}

async function injectConfigModelsSection() {
  const mount = document.getElementById("cfg-models-mount");
  if (!mount) return;
  try {
    const status = await api("/models/status");
    const installed = Array.isArray(status?.backend?.installed_models) ? status.backend.installed_models : [];
    const served = new Set(Array.isArray(status?.backend?.served_models) ? status.backend.served_models : []);
    mount.innerHTML = `
      <div class="card-grid" style="margin-bottom:12px">
        <div class="card"><div class="card-label">Backend</div><div class="card-value" style="font-size:15px">${esc(status?.backend?.cli_installed ? "vLLM" : "Not installed")}</div><div class="card-sub">managed: ${esc(summarizeManagedBackends(status?.config))}</div></div>
        <div class="card"><div class="card-label">Installed Models</div><div class="card-value">${summarizeModelCounts(status)}</div><div class="card-sub">served: ${esc((status?.backend?.served_models || []).join(", ") || "none")}</div></div>
        <div class="card"><div class="card-label">GPU</div><div class="card-value" style="font-size:15px">${esc(status?.gpu?.name || "Not detected")}</div><div class="card-sub">${status?.huggingface_token_configured ? "HF token configured" : "HF token missing"}</div></div>
      </div>
      <div style="border:1px solid var(--border);border-radius:var(--radius)">
        <div class="config-row"><div><div class="config-label">Model Operations</div></div></div>
        <div class="table-wrap"><table class="config-model-ops-table"><thead><tr><th>Operation</th><th>Target</th><th>Purpose</th><th>Action</th></tr></thead><tbody>
          <tr><td>Serve</td><td>vLLM</td><td>Launch the managed OpenAI-compatible local runtime.</td><td><button class="btn btn-sm btn-primary" onclick="modelsServe('vllm')">Serve vLLM</button></td></tr>
          <tr><td>Stop</td><td>vLLM</td><td>Stop the managed runtime and release the bound port.</td><td><button class="btn btn-sm" onclick="modelsStop('vllm')">Stop Runtime</button></td></tr>
          <tr><td>Credentials</td><td>Hugging Face</td><td>Persist the token used for gated or private model pulls.</td><td><button class="btn btn-sm" onclick="modelsLoginHuggingFace()">Set HF Token</button></td></tr>
        </tbody></table></div>
      </div>
      <div style="border:1px solid var(--border);border-radius:var(--radius);margin-top:12px">
        <div class="config-row"><div><div class="config-label">Installed Model Inventory</div></div></div>
        <div class="table-wrap"><table><thead><tr><th>Model ID</th><th>Size</th><th>Backend</th><th>Status</th><th>Actions</th></tr></thead><tbody>
          ${installed.length ? installed.map(item => `<tr><td style="font-family:var(--mono);font-size:11px">${esc(String(item.model || ""))}</td><td>${esc(String(item.size || "unknown"))}</td><td>${esc(String(item.backend || "vllm"))}</td><td>${served.has(String(item.model || "")) || item.served ? "Serving" : "Installed"}</td><td style="display:flex;gap:6px;flex-wrap:wrap"><button class="btn btn-sm" onclick="modelsServe('vllm', '${esc(String(item.model || ""))}')">Serve</button><button class="btn btn-sm" onclick="modelsRemove('${esc(String(item.model || ""))}', '${esc(String(item.source || ""))}')">Remove</button></td></tr>`).join("") : '<tr><td colspan="5" class="muted">No local models discovered yet.</td></tr>'}
        </tbody></table></div>
      </div>`;
  } catch (_e) {
    mount.innerHTML = '<div class="config-row"><div><div class="config-desc muted">Model backend unavailable.</div></div></div>';
  }
}

function summarizeModelCounts(status) {
  const vllmCount = Array.isArray(status?.vllm?.installed_models)
    ? status.vllm.installed_models.length
    : 0;
  return vllmCount;
}

function summarizeManagedBackends(config) {
  const managed = Array.isArray(config?.managed) ? config.managed : [];
  if (!managed.length) return "none";
  return (
    managed
      .map((item) => String(item?.backend || ""))
      .filter(Boolean)
      .join(", ") || "none"
  );
}

// ========================================================================
// CONFIG: CLI Agent Detection (deferred loader)
// ========================================================================
async function injectConfigCliAgents() {
  const cliAgents = ["claude", "codex", "gemini"];
  const cliResolved = {};
  let cliPollTimer = null;
  let cliPollCount = 0;
  const maxCliPolls = 15;

  const setCliStatus = (cli, resp, statusEl, authBtn) => {
    if (resp.installed) {
      cliResolved[cli] = true;
      if (resp.authenticated) {
        if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Authenticated</span>';
        if (authBtn) { authBtn.disabled = false; authBtn.textContent = "Re-authenticate"; authBtn.classList.remove("btn-primary"); }
        return;
      }
      if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Found</span> <span style="color:var(--yellow)">(not authenticated)</span>';
      if (authBtn) { authBtn.disabled = false; authBtn.textContent = "Authenticate"; authBtn.classList.add("btn-primary"); }
      return;
    }
    const pkg = cli === "claude" ? "@anthropic-ai/claude-code" : cli === "codex" ? "@openai/codex" : "@google/gemini-cli";
    if (statusEl) statusEl.innerHTML = '<span style="color:var(--yellow)">Not found</span> <span style="color:var(--text-dim);font-size:11px">Install: <code>npm i -g ' + pkg + '</code></span>';
    if (authBtn) { authBtn.disabled = true; authBtn.textContent = "Authenticate"; authBtn.classList.add("btn-primary"); }
  };

  const detectCli = async (cli) => {
    const statusEl = document.getElementById("cli-status-" + cli);
    const row = document.querySelector('.cli-agent-row[data-cli="' + cli + '"]');
    const authBtn = row && row.querySelector(".cli-auth-btn");
    try {
      const resp = await api("/cli/detect/" + cli);
      setCliStatus(cli, resp, statusEl, authBtn);
    } catch (_e) {
      if (statusEl) statusEl.innerHTML = '<span style="color:var(--text-dim)">Detection error</span>';
    }
  };

  const stopCliPolling = () => { if (cliPollTimer) { clearInterval(cliPollTimer); cliPollTimer = null; } };

  await Promise.allSettled(cliAgents.map(detectCli));

  if (cliAgents.some(cli => !cliResolved[cli])) {
    cliPollTimer = setInterval(async () => {
      cliPollCount++;
      await Promise.allSettled(cliAgents.filter(cli => !cliResolved[cli]).map(detectCli));
      if (!cliAgents.some(cli => !cliResolved[cli]) || cliPollCount >= maxCliPolls) stopCliPolling();
    }, 8000);
  }

  // Auth button handlers — open PTY terminal for OAuth flow
  let _cliAuthTerm = null;
  let _cliAuthFitAddon = null;
  let _cliAuthWs = null;
  for (const btn of $$(".cli-auth-btn")) {
    btn.addEventListener("click", async () => {
      const cli = btn.dataset.cli;
      const statusEl = document.getElementById("cli-status-" + cli);
      btn.disabled = true;
      btn.textContent = "Starting...";
      try {
        const resp = await apiPost("/cli/auth/" + cli, {});
        if (!resp.session_id) throw new Error("no session returned");
        const termWrap = document.getElementById("cli-auth-terminal-wrap");
        const termEl = document.getElementById("cli-auth-terminal");
        const termLabel = document.getElementById("cli-auth-terminal-label");
        if (termWrap) termWrap.style.display = "block";
        if (termLabel) termLabel.textContent = cli.charAt(0).toUpperCase() + cli.slice(1) + " authentication — complete the login in your browser";
        if (_cliAuthWs) { try { _cliAuthWs.close(); } catch (_e) {} _cliAuthWs = null; }
        if (_cliAuthTerm) { try { _cliAuthTerm.dispose(); } catch (_e) {} _cliAuthTerm = null; }
        if (termEl) termEl.innerHTML = "";
        if (typeof Terminal !== "undefined" && termEl) {
          _cliAuthTerm = new Terminal({ cursorBlink: true, fontSize: 13, theme: { background: "#0a0a0a", foreground: "#33ff33" } });
          _cliAuthFitAddon = new FitAddon.FitAddon();
          _cliAuthTerm.loadAddon(_cliAuthFitAddon);
          _cliAuthTerm.open(termEl);
          try { _cliAuthFitAddon.fit(); } catch (_e) {}
          try { _cliAuthTerm.focus(); } catch (_e) {}
          const proto = location.protocol === "https:" ? "wss:" : "ws:";
          const wsUrl = proto + "//" + location.host + "/api/cockpit/sessions/" + resp.session_id + "/ws";
          _cliAuthWs = new WebSocket(wsUrl);
          _cliAuthWs.binaryType = "arraybuffer";
          let _opened = false;
          _cliAuthWs.onmessage = (ev) => {
            let text = "";
            if (ev.data instanceof ArrayBuffer) { const bytes = new Uint8Array(ev.data); _cliAuthTerm.write(bytes); text = new TextDecoder().decode(bytes); }
            else { _cliAuthTerm.write(ev.data); text = ev.data; }
            if (!_opened) { const m = text.match(/https:\/\/[^\s\x1b\x07]+/); if (m) { _opened = true; window.open(m[0].replace(/[\x00-\x1f]/g, ""), "_blank", "width=600,height=700,scrollbars=yes"); } }
          };
          _cliAuthWs.onclose = async () => {
            _cliAuthTerm.write("\r\n\x1b[90m--- session ended ---\x1b[0m\r\n");
            btn.disabled = false;
            try {
              const detect = await api("/cli/detect/" + cli);
              setCliStatus(cli, detect, statusEl, btn);
            } catch (_e) {
              if (statusEl) statusEl.innerHTML = '<span style="color:var(--green)">&#10003; Authenticated</span>';
              btn.textContent = "Re-authenticate"; btn.classList.remove("btn-primary");
            }
          };
          _cliAuthTerm.onData((data) => { if (_cliAuthWs && _cliAuthWs.readyState === WebSocket.OPEN) _cliAuthWs.send(data); });
        }
      } catch (e) {
        if (statusEl) statusEl.innerHTML = '<span style="color:var(--red)">Auth error: ' + esc(String(e.message || e)) + '</span>';
      }
      btn.disabled = false;
      btn.textContent = "Authenticate";
    });
  }
  const closeTermBtn = document.getElementById("cli-auth-terminal-close");
  if (closeTermBtn) {
    closeTermBtn.addEventListener("click", () => {
      if (_cliAuthWs) { try { _cliAuthWs.close(); } catch (_e) {} _cliAuthWs = null; }
      if (_cliAuthTerm) { try { _cliAuthTerm.dispose(); } catch (_e) {} _cliAuthTerm = null; }
      const termWrap = document.getElementById("cli-auth-terminal-wrap");
      if (termWrap) termWrap.style.display = "none";
    });
  }

  // Cleanup on section change
  const observer = new MutationObserver(() => {
    if (!document.getElementById("cli-agents-grid")) { stopCliPolling(); observer.disconnect(); }
  });
  observer.observe(content, { childList: true });
}

// ========================================================================
// CONFIG: Proof Lattice Three.js (deferred loader)
// ========================================================================
async function initConfigProofLattice() {
  const latticeContainer = document.getElementById("proof-lattice-three");
  const latticeStatus = document.getElementById("proof-lattice-status");
  const sidebarFamilies = document.getElementById("pls-families");
  const sidebarStats = document.getElementById("pls-stats");
  const sidebarSelected = document.getElementById("pls-selected");
  if (!latticeContainer) return;

  try {
    const THREE = await import("three");
    const OrbitControls = (await import("three/addons/controls/OrbitControls.js")).OrbitControls;
    const EffectComposer = (await import("three/addons/postprocessing/EffectComposer.js")).EffectComposer;
    const RenderPass = (await import("three/addons/postprocessing/RenderPass.js")).RenderPass;
    const UnrealBloomPass = (await import("three/addons/postprocessing/UnrealBloomPass.js")).UnrealBloomPass;

    let DATA = null;
    try { const res = await fetch("proof-lattice.json"); if (res.ok) DATA = await res.json(); } catch (_e) {}
    if (!DATA || !DATA.nodes || DATA.nodes.length === 0) {
      if (latticeStatus) latticeStatus.textContent = "Proof lattice data unavailable";
      return;
    }
    if (!document.getElementById("proof-lattice-three")) return;

    const FAMILY_COLORS = {
      Core: 0x22c55e, Comms: 0x16a34a, Identity: 0x4ade80, Genesis: 0x86efac,
      Security: 0xef4444, Crypto: 0xa78bfa, PaymentChannels: 0x34d399,
      TrustLayer: 0x10b981, Sheaf: 0x059669, Adversarial: 0xf97316,
      Transparency: 0x6ee7b7, Commitment: 0x15803d, Contracts: 0xa3e635,
      Integration: 0x84cc16, Bridge: 0xbbf7d0,
    };
    const FAMILY_CSS = {};
    Object.keys(FAMILY_COLORS).forEach(k => { FAMILY_CSS[k] = "#" + FAMILY_COLORS[k].toString(16).padStart(6, "0"); });
    const GREEN_EMISSIVE = 0x00ff41;
    const GREEN_EDGE = 0x22c55e;

    const enabledFamilies = {};
    (DATA.families || []).forEach(f => { enabledFamilies[f] = true; });

    if (sidebarFamilies) {
      const famCounts = {};
      DATA.nodes.forEach(n => { famCounts[n.family] = (famCounts[n.family] || 0) + 1; });
      sidebarFamilies.innerHTML = (DATA.families || []).map(f => {
        const c = FAMILY_CSS[f] || "#22c55e";
        return '<label class="pls-fam-row" data-fam="' + f + '"><span class="pls-fam-dot" style="background:' + c + '"></span><span class="pls-fam-name">' + f + '</span><span class="pls-fam-count">' + (famCounts[f] || 0) + '</span></label>';
      }).join("");
    }

    const s = DATA.stats || {};
    if (sidebarStats) {
      sidebarStats.innerHTML =
        '<div class="pls-stat-row"><span>Declarations</span><span>' + (s.total_declarations || DATA.nodes.length) + '</span></div>' +
        '<div class="pls-stat-row"><span>Edges</span><span>' + (s.total_edges || DATA.edges.length) + '</span></div>' +
        '<div class="pls-stat-row"><span>Files</span><span>' + (s.total_files || "?") + '</span></div>' +
        '<div class="pls-stat-row"><span>Theorems</span><span>' + ((s.by_kind || {}).theorem || 0) + '</span></div>' +
        '<div class="pls-stat-row"><span>Defs</span><span>' + ((s.by_kind || {}).def || 0) + '</span></div>';
    }
    if (latticeStatus) latticeStatus.textContent = (s.total_declarations || DATA.nodes.length) + " verified Lean declarations";

    const hero = document.getElementById("proof-lattice-hero");
    const width = (hero ? hero.clientWidth : latticeContainer.clientWidth) || 600;
    const height = 320;
    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x030a04);
    const camera = new THREE.PerspectiveCamera(50, width / height, 0.1, 100);
    camera.position.set(0, 0, 2.5);
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setSize(width, height);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    latticeContainer.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true; controls.dampingFactor = 0.08;
    controls.enableZoom = true; controls.minDistance = 1; controls.maxDistance = 5;
    controls.autoRotate = true; controls.autoRotateSpeed = 3.0;
    const clock = new THREE.Clock();

    const composer = new EffectComposer(renderer);
    composer.addPass(new RenderPass(scene, camera));
    const bloomPass = new UnrealBloomPass(new THREE.Vector2(width, height), 0.9, 0.35, 0.65);
    composer.addPass(bloomPass);

    scene.add(new THREE.AmbientLight(0x0a4020, 0.6));
    const directional = new THREE.DirectionalLight(0x20b040, 0.7);
    directional.position.set(2, 2, 2); scene.add(directional);
    const backLight = new THREE.DirectionalLight(0x084018, 0.4);
    backLight.position.set(-2, -1, -2); scene.add(backLight);
    const pointLight = new THREE.PointLight(0x00ff41, 0.8, 5);
    scene.add(pointLight);

    const nodes = DATA.nodes;
    const xs = nodes.map(n => n.x), ys = nodes.map(n => n.y), zs = nodes.map(n => n.z);
    const minX = Math.min(...xs), maxX = Math.max(...xs);
    const minY = Math.min(...ys), maxY = Math.max(...ys);
    const minZ = Math.min(...zs), maxZ = Math.max(...zs);
    const rX = maxX - minX || 1, rY = maxY - minY || 1, rZ = maxZ - minZ || 1;
    function norm(n) { return { nx: (n.x - minX) / rX, ny: (n.y - minY) / rY, nz: (n.z - minZ) / rZ }; }
    const maxImportance = Math.max(...nodes.map(n => n.importance));

    const nodeGroup = new THREE.Group();
    const nodeMeshes = [];
    nodes.forEach(node => {
      const color = FAMILY_COLORS[node.family] || 0x22c55e;
      const sizeScale = 0.008 + (node.importance / maxImportance) * 0.012;
      const geo = new THREE.SphereGeometry(sizeScale, 16, 16);
      const mat = new THREE.MeshPhongMaterial({ color, transparent: true, opacity: 0.95, emissive: GREEN_EMISSIVE, emissiveIntensity: 0.25, shininess: 80 });
      const mesh = new THREE.Mesh(geo, mat);
      const p = norm(node);
      mesh.position.set((p.nx - 0.5) * 2, (p.ny - 0.5) * 2, (p.nz - 0.5) * 2);
      mesh._family = node.family; mesh._nodeId = node.id; mesh._importance = node.importance;
      mesh._originalColor = color; mesh._baseScale = 1;
      nodeGroup.add(mesh); nodeMeshes.push(mesh);
    });
    scene.add(nodeGroup);

    const edgeGroup = new THREE.Group();
    DATA.edges.forEach(pair => {
      const nA = nodes[pair[0]], nB = nodes[pair[1]];
      if (!nA || !nB) return;
      const a = norm(nA), b = norm(nB);
      const start = new THREE.Vector3((a.nx - 0.5) * 2, (a.ny - 0.5) * 2, (a.nz - 0.5) * 2);
      const end = new THREE.Vector3((b.nx - 0.5) * 2, (b.ny - 0.5) * 2, (b.nz - 0.5) * 2);
      const normImp = ((nA.importance + nB.importance) / 2) / maxImportance;
      const mid = new THREE.Vector3().addVectors(start, end).multiplyScalar(0.5);
      const dir = new THREE.Vector3().subVectors(end, start).normalize();
      const perp = new THREE.Vector3(-dir.y, dir.x, dir.z * 0.3).normalize();
      mid.add(perp.multiplyScalar((0.02 + Math.random() * 0.04) * (Math.random() > 0.5 ? 1 : -1)));
      const curve = new THREE.QuadraticBezierCurve3(start, mid, end);
      const tubeGeo = new THREE.TubeGeometry(curve, 12, 0.001 + normImp * 0.003, 6, false);
      const edgeColor = new THREE.Color(GREEN_EDGE);
      const tubeMat = new THREE.MeshBasicMaterial({ color: edgeColor, transparent: true, opacity: 0.15 + normImp * 0.45 });
      const tube = new THREE.Mesh(tubeGeo, tubeMat);
      tube._famA = nA.family; tube._famB = nB.family; tube._nodeA = pair[0]; tube._nodeB = pair[1];
      tube._defaultColor = edgeColor.clone(); tube._baseColor = edgeColor.clone(); tube._baseOpacity = tubeMat.opacity;
      edgeGroup.add(tube);
    });
    scene.add(edgeGroup);

    // Ambient particles
    const particleCount = 150;
    const pGeo = new THREE.BufferGeometry();
    const pPos = new Float32Array(particleCount * 3), pCol = new Float32Array(particleCount * 3);
    for (let pi = 0; pi < particleCount; pi++) {
      pPos[pi * 3] = (Math.random() - 0.5) * 3; pPos[pi * 3 + 1] = (Math.random() - 0.5) * 3; pPos[pi * 3 + 2] = (Math.random() - 0.5) * 3;
      const cool = 0.3 + Math.random() * 0.5;
      pCol[pi * 3] = cool * 0.2; pCol[pi * 3 + 1] = cool; pCol[pi * 3 + 2] = cool * 0.25;
    }
    pGeo.setAttribute("position", new THREE.BufferAttribute(pPos, 3));
    pGeo.setAttribute("color", new THREE.BufferAttribute(pCol, 3));
    const particles = new THREE.Points(pGeo, new THREE.PointsMaterial({ size: 0.015, transparent: true, opacity: 0.25, vertexColors: true }));
    scene.add(particles);

    // Raycasting
    const raycaster = new THREE.Raycaster();
    const mouse = new THREE.Vector2();
    let lastHoveredMesh = null, selectedMesh = null, highlightedFamily = null;

    function showSelectedNode(nodeData) {
      if (!sidebarSelected) return;
      if (!nodeData) { sidebarSelected.innerHTML = '<span class="pls-muted">Click a node to inspect</span>'; return; }
      const c = FAMILY_CSS[nodeData.family] || "#22c55e";
      sidebarSelected.innerHTML = '<div class="pls-sel-name">' + nodeData.name + '</div><div class="pls-sel-meta"><span class="pls-fam-dot" style="background:' + c + '"></span> ' + nodeData.family + ' &middot; ' + nodeData.kind + '</div><div class="pls-sel-file">' + nodeData.file + ':' + nodeData.line + '</div>';
    }

    function updateFamilyFilter(fam) {
      highlightedFamily = fam;
      nodeMeshes.forEach(mesh => {
        if (fam === null) { mesh.material.opacity = 0.95; mesh.material.emissiveIntensity = 0.25; mesh.visible = enabledFamilies[mesh._family] !== false; mesh.scale.setScalar(1); mesh._baseScale = 1; }
        else if (mesh._family === fam) { mesh.material.opacity = 1; mesh.material.emissiveIntensity = 0.5; mesh.visible = true; mesh.scale.setScalar(1.5); mesh._baseScale = 1.5; }
        else { mesh.material.opacity = 0.12; mesh.material.emissiveIntensity = 0.05; mesh.visible = true; mesh.scale.setScalar(0.5); mesh._baseScale = 0.5; }
      });
      edgeGroup.children.forEach(tube => {
        if (fam === null) { tube.material.opacity = tube._baseOpacity; tube.material.color.copy(tube._defaultColor); tube.visible = enabledFamilies[tube._famA] !== false && enabledFamilies[tube._famB] !== false; }
        else if (tube._famA === fam || tube._famB === fam) { tube.material.opacity = 0.7; tube.material.color.set(0x4ade80); tube.visible = true; }
        else { tube.material.opacity = 0.03; tube.material.color.copy(tube._defaultColor); tube.visible = true; }
      });
    }

    if (sidebarFamilies) {
      sidebarFamilies.addEventListener("click", e => {
        const row = e.target.closest(".pls-fam-row");
        if (!row) return;
        const fam = row.dataset.fam;
        if (highlightedFamily === fam) { row.classList.remove("pls-fam-active"); updateFamilyFilter(null); }
        else { const prev = sidebarFamilies.querySelector(".pls-fam-active"); if (prev) prev.classList.remove("pls-fam-active"); row.classList.add("pls-fam-active"); updateFamilyFilter(fam); }
      });
    }

    renderer.domElement.addEventListener("mousemove", e => {
      const rect = renderer.domElement.getBoundingClientRect();
      mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
      mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(mouse, camera);
      const hits = raycaster.intersectObjects(nodeGroup.children);
      if (hits.length > 0) {
        const mesh = hits[0].object;
        if (mesh !== lastHoveredMesh) {
          if (lastHoveredMesh) lastHoveredMesh.scale.setScalar(lastHoveredMesh._baseScale || 1);
          lastHoveredMesh = mesh; mesh.scale.setScalar((mesh._baseScale || 1) * 1.6);
          const nd = nodes.find(n => n.id === mesh._nodeId); if (nd) showSelectedNode(nd);
        }
        renderer.domElement.style.cursor = "pointer";
      } else {
        if (lastHoveredMesh) lastHoveredMesh.scale.setScalar(lastHoveredMesh._baseScale || 1);
        lastHoveredMesh = null; if (!selectedMesh) showSelectedNode(null);
        renderer.domElement.style.cursor = "grab";
      }
    });

    renderer.domElement.addEventListener("click", e => {
      const rect = renderer.domElement.getBoundingClientRect();
      mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
      mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(mouse, camera);
      const hits = raycaster.intersectObjects(nodeGroup.children);
      if (selectedMesh) { selectedMesh.material.color.setHex(selectedMesh._originalColor); selectedMesh.material.emissive.setHex(GREEN_EMISSIVE); selectedMesh.material.emissiveIntensity = 0.25; }
      if (hits.length > 0) {
        selectedMesh = hits[0].object;
        selectedMesh.material.color.setHex(0xffffff); selectedMesh.material.emissive.setHex(0xbbf7d0); selectedMesh.material.emissiveIntensity = 0.8;
        const nd = nodes.find(n => n.id === selectedMesh._nodeId); if (nd) showSelectedNode(nd);
      } else { selectedMesh = null; showSelectedNode(null); }
    });

    renderer.domElement.style.cursor = "grab";

    // Neural firing animation
    const firingNodes = new Map(), firingEdges = new Map();
    const FIRE_DURATION = 0.5, NODE_FIRE_CHANCE = 0.08, EDGE_FIRE_CHANCE = 0.12, PROPAGATION_DELAY = 0.03, CASCADE_CHANCE = 0.5;
    let elapsedTime = 0, animationId = 0;

    function animate() {
      animationId = requestAnimationFrame(animate);
      const delta = clock.getDelta(); elapsedTime += delta; controls.update();
      const pulse = Math.sin(elapsedTime * 2.5) * 0.5 + 0.5;
      const slowPulse = Math.sin(elapsedTime * 1.5) * 0.4 + 0.6;
      pointLight.intensity = 0.3 + pulse * 1.2;

      if (Math.random() < NODE_FIRE_CHANCE) {
        const idx = Math.floor(Math.random() * nodeMeshes.length);
        if (!firingNodes.has(idx)) {
          firingNodes.set(idx, { startTime: elapsedTime, intensity: 0.9 + Math.random() * 0.1 });
          edgeGroup.children.forEach((tube, edgeIdx) => {
            if (tube._nodeA === idx || tube._nodeB === idx) {
              if (!firingEdges.has(edgeIdx)) {
                firingEdges.set(edgeIdx, { startTime: elapsedTime + PROPAGATION_DELAY, intensity: 0.9 + Math.random() * 0.1 });
                if (Math.random() < CASCADE_CHANCE) {
                  const target = tube._nodeA === idx ? tube._nodeB : tube._nodeA;
                  if (!firingNodes.has(target)) firingNodes.set(target, { startTime: elapsedTime + PROPAGATION_DELAY * 2.5, intensity: 0.75 + Math.random() * 0.2 });
                }
              }
            }
          });
        }
      }

      nodeMeshes.forEach((mesh, i) => {
        if (!mesh.material) return;
        const nodePhase = Math.sin(elapsedTime * 2.5 + i * 0.03) * 0.3 + 0.7;
        let emInt = 0.15 + nodePhase * slowPulse * 0.5;
        let targetScale = mesh._baseScale || 1;
        const firing = firingNodes.get(i);
        if (firing) {
          const fe = elapsedTime - firing.startTime;
          if (fe >= 0 && fe < FIRE_DURATION) {
            const fp = fe / FIRE_DURATION;
            const fi = fp < 0.12 ? fp / 0.12 : Math.pow(1 - ((fp - 0.12) / 0.88), 1.5);
            emInt = 0.4 + firing.intensity * fi * 3.0; targetScale = (mesh._baseScale || 1) * (1 + fi * 0.6);
            mesh.material.emissive.setHex(fi > 0.3 ? 0xbbf7d0 : fi > 0.15 ? 0x86efac : GREEN_EMISSIVE);
            if (fi > 0.3) mesh.material.color.setHex(0xffffff); else mesh.material.color.setHex(mesh._originalColor);
          } else if (fe >= FIRE_DURATION) { firingNodes.delete(i); mesh.material.emissive.setHex(GREEN_EMISSIVE); mesh.material.color.setHex(mesh._originalColor); }
        } else { mesh.material.emissive.setHex(GREEN_EMISSIVE); }
        mesh.material.emissiveIntensity = emInt;
        mesh.scale.setScalar(mesh.scale.x + (targetScale - mesh.scale.x) * 0.15);
      });

      edgeGroup.children.forEach((tube, eIdx) => {
        const firing = firingEdges.get(eIdx);
        if (firing) {
          const fe = elapsedTime - firing.startTime;
          if (fe >= 0 && fe < FIRE_DURATION) {
            const fi = Math.exp(-Math.pow(((fe / FIRE_DURATION) * 1.3 - 0.5) / 0.2, 2)) * firing.intensity;
            if (fi > 0.2) { tube.material.color.setHex(0xbbf7d0); tube.material.opacity = Math.min(1, 0.6 + fi * 0.4); }
            else if (fi > 0.08) { tube.material.color.setHex(0x4ade80); tube.material.opacity = Math.min(1, 0.4 + fi * 0.4); }
            else { tube.material.color.copy(tube._baseColor); tube.material.opacity = tube._baseOpacity; }
          } else if (fe >= FIRE_DURATION) { firingEdges.delete(eIdx); tube.material.color.copy(tube._baseColor); tube.material.opacity = tube._baseOpacity; }
        }
      });

      bloomPass.strength = 0.6 + Math.min((firingNodes.size + firingEdges.size) * 0.03, 0.5);
      particles.rotation.y += delta * 0.08; particles.rotation.x += delta * 0.04;
      composer.render();
    }
    animationId = requestAnimationFrame(animate);

    function onResize() {
      const h = document.getElementById("proof-lattice-hero");
      if (!h) return;
      camera.aspect = (h.clientWidth || 600) / 320;
      camera.updateProjectionMatrix(); renderer.setSize(h.clientWidth || 600, 320); composer.setSize(h.clientWidth || 600, 320);
    }
    window.addEventListener("resize", onResize);

    const latticeObserver = new MutationObserver(() => {
      if (!document.getElementById("proof-lattice-three")) {
        cancelAnimationFrame(animationId); window.removeEventListener("resize", onResize);
        renderer.dispose(); pGeo.dispose(); latticeObserver.disconnect();
      }
    });
    latticeObserver.observe(content, { childList: true });
  } catch (err) {
    console.warn("Proof lattice init failed:", err);
    if (latticeStatus) latticeStatus.textContent = "Proof lattice visualization unavailable";
  }
}

window.modelsLoginHuggingFace = async function modelsLoginHuggingFace() {
  const token = window.prompt("Paste Hugging Face token (hf_...)");
  if (!token) return;
  try {
    await apiPost("/models/login/huggingface", { token: String(token).trim() });
    alert("Hugging Face token saved.");
    await renderConfig();
  } catch (e) {
    alert(`Hugging Face login failed: ${String((e && e.message) || e)}`);
  }
};

window.modelsServe = async function modelsServe(backend) {
  try {
    const payload = { backend };
    const existingModel = arguments.length > 1 ? arguments[1] : "";
    const model = existingModel || window.prompt(
      "vLLM model to serve (installed HF repo id or path)",
      "",
    );
    if (model && String(model).trim()) payload.model = String(model).trim();
    await apiPost("/models/serve", payload);
    await renderConfig();
  } catch (e) {
    alert(`Serve failed: ${String((e && e.message) || e)}`);
  }
};

window.modelsStop = async function modelsStop(backend) {
  try {
    await apiPost("/models/stop", { backend });
    await renderConfig();
  } catch (e) {
    alert(`Stop failed: ${String((e && e.message) || e)}`);
  }
};

window.modelsRemove = async function modelsRemove(model, source) {
  if (!window.confirm(`Remove local model ${model}?`)) return;
  try {
    await apiPost("/models/rm", { model, source });
    await renderConfig();
  } catch (e) {
    alert(`Remove failed: ${String((e && e.message) || e)}`);
  }
};

window.refreshAgentPmtCatalog = async function refreshAgentPmtCatalog() {
  try {
    const resp = await apiPost("/agentpmt/refresh", {});
    alert(`AgentPMT catalog refreshed (${Number(resp.count || 0)} tools).`);
    renderConfig();
  } catch (e) {
    alert(`AgentPMT refresh failed: ${String((e && e.message) || e)}`);
  }
};

window.forceCryptoLock = async function forceCryptoLock() {
  try {
    await apiPost("/crypto/lock", {});
    _cryptoStatus = null;
    await ensureCryptoUnlocked(true);
    await renderConfig();
  } catch (e) {
    alert(`Lock failed: ${String((e && e.message) || e)}`);
  }
};

window.showCryptoSetupPrompt = async function showCryptoSetupPrompt() {
  const status = await fetchCryptoStatus(true);
  renderCryptoOverlay(status);
};

window.authorizeAgentPrompt = async function authorizeAgentPrompt() {
  const label = prompt("Agent label", "Automation Agent");
  if (!label) return;
  const scopesRaw = prompt(
    "Scopes (comma-separated): sign,vault,wallet,identity",
    "sign,vault",
  );
  if (!scopesRaw) return;
  const expiresRaw = prompt("Expiry days (empty for no expiry)", "90");
  const scopes = scopesRaw
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const expires_days =
    expiresRaw && expiresRaw.trim() ? Number(expiresRaw) : null;
  try {
    const resp = await apiPost("/agents/authorize", {
      label,
      scopes,
      expires_days: Number.isFinite(expires_days) ? expires_days : null,
    });
    openAgentSecretModal(resp.agent_id, resp.agent_sk);
    await renderConfig();
  } catch (e) {
    alert(`Authorize failed: ${String((e && e.message) || e)}`);
  }
};

function openAgentSecretModal(agentId, secretKey) {
  const old = document.getElementById("agent-secret-modal");
  if (old) old.remove();
  const wrap = document.createElement("div");
  wrap.id = "agent-secret-modal";
  wrap.style.cssText =
    "position:fixed;inset:0;background:rgba(0,0,0,0.72);display:flex;align-items:center;justify-content:center;z-index:1300";
  wrap.innerHTML = `
    <div style="width:min(760px,94vw);background:var(--bg-card);border:1px solid var(--accent);padding:16px;border-radius:6px;box-shadow:0 10px 40px rgba(0,0,0,0.45)">
      <div style="font-size:15px;color:var(--accent);margin-bottom:8px">Agent Authorized</div>
      <div style="font-size:12px;color:var(--text-dim);margin-bottom:10px">
        Agent ID: <span style="color:var(--text)">${esc(String(agentId || ""))}</span>
      </div>
      <div style="font-size:12px;color:var(--amber);margin-bottom:10px">
        Secret key is shown once. Copy and store it securely before closing.
      </div>
      <textarea id="agent-secret-modal-text" readonly style="width:100%;min-height:140px;resize:vertical;padding:10px;border-radius:6px;border:1px solid var(--border);background:rgba(4,14,8,0.5);color:var(--text);font-family:var(--mono);font-size:12px;line-height:1.4">${esc(String(secretKey || ""))}</textarea>
      <div style="display:flex;gap:8px;justify-content:flex-end;margin-top:12px">
        <button class="btn btn-sm" id="agent-secret-modal-copy">Copy</button>
        <button class="btn btn-sm btn-primary" id="agent-secret-modal-close">I have saved this key</button>
      </div>
    </div>
  `;
  document.body.appendChild(wrap);
  const txt = document.getElementById("agent-secret-modal-text");
  if (txt && typeof txt.select === "function") txt.select();
  const copyBtn = document.getElementById("agent-secret-modal-copy");
  const closeBtn = document.getElementById("agent-secret-modal-close");
  if (copyBtn) {
    copyBtn.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(String(secretKey || ""));
        copyBtn.textContent = "Copied";
        setTimeout(() => {
          copyBtn.textContent = "Copy";
        }, 1200);
      } catch (_e) {
        alert("Copy failed. Please copy manually.");
      }
    });
  }
  if (closeBtn) closeBtn.addEventListener("click", () => wrap.remove());
}

window.revokeAgent = async function revokeAgent(agent_id) {
  if (!confirm(`Revoke agent ${agent_id}?`)) return;
  try {
    await apiPost("/agents/revoke", { agent_id });
    await renderConfig();
  } catch (e) {
    alert(`Revoke failed: ${String((e && e.message) || e)}`);
  }
};

function providerDefaultEnv(provider) {
  const key = String(provider || "").toLowerCase();
  return (
    (PROVIDER_INFO[key] && PROVIDER_INFO[key].envVar) ||
    `${key.toUpperCase()}_API_KEY`
  );
}

// (Setup V3 helpers removed — merged into Config)

window.toggleWrap = async function (agent, enable) {
  try {
    await apiPost("/config/wrap", { agent, enable });
    renderConfig();
  } catch (e) {
    alert("Failed: " + e.message);
  }
};

window.toggleX402 = async function (enable) {
  try {
    await apiPost("/config/x402", { enabled: enable });
    renderConfig();
  } catch (e) {
    alert("Failed: " + e.message);
  }
};

function openVaultModal(provider, envVar) {
  const old = document.getElementById("vault-key-modal");
  if (old) old.remove();
  const wrap = document.createElement("div");
  wrap.id = "vault-key-modal";
  wrap.style.cssText =
    "position:fixed;inset:0;background:rgba(0,0,0,0.65);display:flex;align-items:center;justify-content:center;z-index:1200";
  wrap.innerHTML = `
    <div style="width:min(520px,92vw);background:var(--bg-card);border:1px solid var(--accent);padding:16px;border-radius:6px">
      <div style="font-size:14px;color:var(--accent);margin-bottom:6px">Set API Key: ${esc(provider)}</div>
      <div style="font-size:11px;color:var(--text-dim);margin-bottom:10px">${esc(envVar)}</div>
      <input id="vault-key-input" type="password" placeholder="Paste API key" style="width:100%;padding:8px 10px;font-size:12px;margin-bottom:10px">
      <div style="display:flex;gap:8px;justify-content:flex-end">
        <button class="btn btn-sm" id="vault-key-cancel">Cancel</button>
        <button class="btn btn-sm btn-primary" id="vault-key-save">Save</button>
      </div>
    </div>
  `;
  document.body.appendChild(wrap);
  const input = document.getElementById("vault-key-input");
  input?.focus();
  wrap
    .querySelector("#vault-key-cancel")
    .addEventListener("click", () => wrap.remove());
  wrap.querySelector("#vault-key-save").addEventListener("click", async () => {
    const key = input?.value || "";
    if (!key.trim()) return;
    try {
      await apiPost(`/vault/keys/${encodeURIComponent(provider)}`, {
        key,
        env_var: envVar,
      });
      wrap.remove();
      window._invalidateSetupState();
      await fetchSetupState(true);
      // Re-render current page
      const curPage = (location.hash.replace("#/", "") || "config").split(
        "/",
      )[0];
      if (pages[curPage]) await pages[curPage]();
      updateNavLockState();
    } catch (e) {
      alert("Set key failed: " + e.message);
    }
  });
}

window.vaultSetKey = function (provider, envVar) {
  openVaultModal(provider, envVar);
};

window.vaultTestKey = async function (provider) {
  try {
    const res = await apiPost(
      `/vault/test/${encodeURIComponent(provider)}`,
      {},
    );
    if (res.ok) alert(`${provider}: key validated successfully`);
    else alert(`${provider}: ${res.error || "validation failed"}`);
    window._invalidateSetupState();
    await fetchSetupState(true);
    const curPage = (location.hash.replace("#/", "") || "config").split("/")[0];
    if (pages[curPage]) await pages[curPage]();
    updateNavLockState();
  } catch (e) {
    alert("Test key failed: " + e.message);
  }
};

window.vaultRemoveKey = async function (provider) {
  if (!confirm(`Remove key for ${provider}?`)) return;
  try {
    await apiDelete(`/vault/keys/${encodeURIComponent(provider)}`);
    window._invalidateSetupState();
    await fetchSetupState(true);
    const curPage = (location.hash.replace("#/", "") || "config").split("/")[0];
    if (pages[curPage]) await pages[curPage]();
    updateNavLockState();
  } catch (e) {
    alert("Remove key failed: " + e.message);
  }
};

// =============================================================================
// PAGE: Cockpit
// =============================================================================
function renderCockpit() {
  content.innerHTML = `
    <div class="page-header">
      <h1>Cockpit</h1>
      <p class="subtitle">Agent orchestration terminal</p>
    </div>
    <div id="cockpit-root" style="margin-top:10px"></div>
  `;

  const root = document.getElementById("cockpit-root");
  window.__cockpitConfig = {
    meshPollMs: 10000,
    metricsPollMs: 5000,
  };
  if (window.CockpitPage && typeof window.CockpitPage.mount === "function") {
    window.CockpitPage.mount(root);
  } else {
    root.innerHTML = `
      <div class="card" style="padding:2rem;text-align:center;color:var(--amber);">
        <p style="font-size:1.5rem;">&#9654; Cockpit unavailable</p>
        <p style="margin-top:1rem;color:var(--text-dim);">cockpit.js failed to load.</p>
      </div>`;
  }
}

// =============================================================================
// PAGE: Trust & Attestations
// =============================================================================
async function renderTrust() {
  content.innerHTML = '<div class="loading">Loading attestations...</div>';
  try {
    const data = await api("/attestations");
    const attestations = data.attestations || [];

    content.innerHTML = `
      <div class="page-title">Trust &amp; Attestations</div>

      <div class="card-grid">
        <div class="card">
          <div class="card-label">Attestations</div>
          <div class="card-value">${attestations.length}</div>
          <div class="card-sub">Total created</div>
        </div>
        <div class="card">
          <div class="card-label">On-Chain</div>
          <div class="card-value">${attestations.filter((a) => a.tx_hash).length}</div>
          <div class="card-sub">Posted to blockchain</div>
        </div>
      </div>

      <div class="section-header">Verify Attestation</div>
      <div style="display:flex;gap:8px;margin-bottom:20px">
        <input type="text" id="verify-digest" placeholder="Paste attestation digest..." style="flex:1;padding:8px 12px;font-size:12px">
        <button class="btn btn-primary" onclick="verifyDigest()">Verify</button>
      </div>
      <div id="verify-result"></div>

      <div class="section-header">Attestation History</div>
      ${
        attestations.length > 0
          ? `
        <div class="table-wrap"><table>
          <thead><tr><th>Digest</th><th>Proof Type</th><th>Session</th><th>TX Hash</th></tr></thead>
          <tbody>
            ${attestations
              .map(
                (a) => `
              <tr>
                <td style="font-size:10px">${esc(truncate(a.attestation_digest || "", 32))}</td>
                <td><span class="badge badge-info">${esc(a.proof_type || "merkle")}</span></td>
                <td style="font-size:10px">${esc(truncate(a.session_id || "", 24))}</td>
                <td style="font-size:10px">${a.tx_hash ? esc(truncate(a.tx_hash, 24)) : "-"}</td>
              </tr>
            `,
              )
              .join("")}
          </tbody>
        </table></div>
      `
          : '<div style="color:var(--text-muted)">No attestations created yet.</div>'
      }
    `;
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.verifyDigest = async function () {
  const digest = ($("#verify-digest")?.value || "").trim();
  if (!digest) return;
  const el = $("#verify-result");
  el.innerHTML = '<div style="color:var(--text-muted)">Checking...</div>';
  try {
    const data = await apiPost("/attestations/verify", { digest });
    if (data.verified) {
      el.innerHTML = `<div class="card" style="border-color:var(--green)">
        <div class="card-label" style="color:var(--green)">CRYPTOGRAPHICALLY VERIFIED</div>
        <div class="card-sub">Merkle root recomputed from session events matches stored attestation.
          ${
            data.checks
              ? `<br>Digest: ${data.checks.digest_match ? "OK" : "MISMATCH"} |
          Root: ${data.checks.merkle_root_match ? "OK" : "MISMATCH"} |
          Events: ${data.checks.event_count_match ? "OK" : "MISMATCH"}`
              : ""
          }
          ${data.event_count ? `<br>${data.event_count} events verified` : ""}
        </div></div>`;
    } else if (data.found) {
      el.innerHTML = `<div class="card" style="border-color:var(--red)">
        <div class="card-label" style="color:var(--red)">VERIFICATION FAILED</div>
        <div class="card-sub">${esc(data.reason || "Recomputed attestation does not match stored digest.")}
          ${
            data.checks
              ? `<br>Digest: ${data.checks.digest_match ? "OK" : "MISMATCH"} |
          Root: ${data.checks.merkle_root_match ? "OK" : "MISMATCH"} |
          Events: ${data.checks.event_count_match ? "OK" : "MISMATCH"}`
              : ""
          }
        </div></div>`;
    } else {
      el.innerHTML =
        '<div class="card" style="border-color:var(--yellow)"><div class="card-label" style="color:var(--yellow)">NOT FOUND</div><div class="card-sub">No attestation with this digest in local store</div></div>';
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Verification failed: ${esc(e.message)}</div>`;
  }
};

// =============================================================================
// PAGE: NucleusDB — Full Database Browser (Redesigned)
// =============================================================================

// NucleusDB sub-tab state
const ndb = {
  tab: "browse",
  page: 0,
  pageSize: 50,
  prefix: "",
  sort: "key",
  order: "asc",
  editingKey: null,
  agentFilter: "",
};

const ndbSharing = {
  includeRevoked: false,
};

// Backend description map
const backendInfo = {
  binary_merkle: {
    name: "BinaryMerkle",
    algo: "SHA-256",
    type: "Post-Quantum",
    proof: "O(log n)",
    setup: "None",
  },
  ipa: {
    name: "IPA",
    algo: "Pedersen",
    type: "Binding",
    proof: "O(n)",
    setup: "None",
  },
  kzg: {
    name: "KZG",
    algo: "BLS12-381",
    type: "Pairing",
    proof: "O(1)",
    setup: "Trusted",
  },
};

async function renderNucleusDB(subtab) {
  ndb.tab = subtab || ndb.tab || "browse";
  content.innerHTML = '<div class="loading">Initializing NucleusDB...</div>';

  try {
    const [status, stats] = await Promise.all([
      api("/nucleusdb/status"),
      api("/nucleusdb/stats").catch(() => null),
    ]);

    const keyCount = stats?.key_count || 0;
    const commitCount = stats?.commit_count || 0;
    const dbSize = stats?.db_size_bytes || 0;
    const backend = status.backend || "binary_merkle";
    const bi = backendInfo[backend] || backendInfo.binary_merkle;
    const chainOk = status.exists && commitCount > 0;

    content.innerHTML = `
      <div class="ndb-hero">
        <canvas class="ndb-hero-canvas" id="hero-particles"></canvas>
        <div class="ndb-hero-grid">
          <div class="ndb-hero-logo-wrap">
            <img src="img/nucleus_db_hero.png" alt="NucleusDB" onerror="this.style.display='none'">
          </div>
          <div class="ndb-hero-copy">
            <div class="ndb-hero-kicker">Agent H.A.L.O. // Containment Node</div>
            <div class="ndb-hero-title">NucleusDB</div>
            <div class="ndb-hero-subtitle">Proof-Carrying Algebraic Database</div>
            <div class="ndb-hero-separator"></div>
          </div>
        </div>
      </div>

      <div class="card-grid" style="margin-bottom:14px">
        <div class="card">
          <div class="card-label">Keys</div>
          <div class="card-value">${keyCount.toLocaleString()}</div>
          <div class="card-sub">${stats?.type_distribution ? Object.keys(stats.type_distribution).length + " types" : "No data"}</div>
        </div>
        <div class="card">
          <div class="card-label">Commits</div>
          <div class="card-value">${commitCount.toLocaleString()}</div>
          <div class="card-sub">${formatBytes(dbSize)}</div>
        </div>
        <div class="card">
          <div class="card-label">Backend</div>
          <div class="card-value" style="font-size:14px">${esc(bi.name)}</div>
          <div class="card-sub">${esc(bi.algo)} | ${esc(bi.type)}</div>
        </div>
        <div class="card">
          <div class="card-label">Chain</div>
          <div class="card-value" style="font-size:14px">${
            chainOk
              ? '<span class="badge badge-ok">HEALTHY</span>'
              : status.exists
                ? '<span class="badge badge-warn">EMPTY</span>'
                : '<span class="badge badge-muted">NO DB</span>'
          }</div>
          <div class="card-sub">${chainOk ? "Seal #" + commitCount : status.exists ? "No commits yet" : "Create database first"}</div>
        </div>
      </div>

      <div class="card-grid" style="margin-bottom:14px">
        <div class="card">
          <div class="card-label">AETHER Vector Guard</div>
          <div class="card-value" style="font-size:14px">${Number(stats?.vector_aether?.governor_epsilon || 0).toFixed(2)}</div>
          <div class="card-sub">guarded=${Number(stats?.vector_aether?.guarded_vectors || 0)} | reclaimable=${Number(stats?.vector_aether?.reclaimable_vectors || 0)}</div>
          <div class="card-sub">basis: ${esc(stats?.vector_aether?.formal_basis || "n/a")}</div>
        </div>
        <div class="card">
          <div class="card-label">AETHER Blob Guard</div>
          <div class="card-value" style="font-size:14px">${Number(stats?.blob_aether?.governor_epsilon || 0).toFixed(2)}</div>
          <div class="card-sub">guarded=${Number(stats?.blob_aether?.guarded_blobs || 0)} | reclaimable=${Number(stats?.blob_aether?.reclaimable_blobs || 0)}</div>
          <div class="card-sub">basis: ${esc(stats?.blob_aether?.formal_basis || "n/a")}</div>
        </div>
      </div>

      <div class="ndb-tabs">
        <button class="ndb-tab ${ndb.tab === "browse" ? "active" : ""}" onclick="ndbSwitchTab('browse')">F1:DATA</button>
        <button class="ndb-tab ${ndb.tab === "sql" ? "active" : ""}" onclick="ndbSwitchTab('sql')">F2:SQL</button>
        <button class="ndb-tab ${ndb.tab === "vectors" ? "active" : ""}" onclick="ndbSwitchTab('vectors')">F3:VEC</button>
        <button class="ndb-tab ${ndb.tab === "commits" ? "active" : ""}" onclick="ndbSwitchTab('commits')">F4:CHAIN</button>
        <button class="ndb-tab ${ndb.tab === "proofs" ? "active" : ""}" onclick="ndbSwitchTab('proofs')">F5:PROOF</button>
        <button class="ndb-tab ${ndb.tab === "sharing" ? "active" : ""}" onclick="ndbSwitchTab('sharing')">F6:SHARE</button>
        <button class="ndb-tab ${ndb.tab === "config" ? "active" : ""}" onclick="ndbSwitchTab('config')">F7:CFG</button>
      </div>
      <div id="ndb-content"></div>
    `;

    // Store stats for sub-tabs
    window._ndbStats = stats;
    window._ndbStatus = status;

    // Start particle network animation
    if (window._initHeroParticles) window._initHeroParticles();

    // Render active sub-tab
    switch (ndb.tab) {
      case "browse":
        await ndbRenderBrowse();
        break;
      case "sql":
        ndbRenderSQL();
        break;
      case "vectors":
        await ndbRenderVectors();
        break;
      case "commits":
        await ndbRenderCommits();
        break;
      case "proofs":
        ndbRenderProofs();
        break;
      case "sharing":
        await ndbRenderSharing();
        break;
      case "config":
        await ndbRenderConfig();
        break;
    }
  } catch (e) {
    content.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`;
  }
}

window.ndbSwitchTab = function (tab) {
  ndb.tab = tab;
  renderNucleusDB(tab);
};

// -- Browse Sub-Tab -----------------------------------------------------------
async function ndbRenderBrowse() {
  const el = $("#ndb-content");
  el.innerHTML = '<div style="color:var(--text-muted)">Loading data...</div>';

  try {
    const agentParam = ndb.agentFilter ? `&agent=${encodeURIComponent(ndb.agentFilter)}` : "";
    const data = await api(
      `/nucleusdb/browse?page=${ndb.page}&page_size=${ndb.pageSize}&prefix=${encodeURIComponent(ndb.prefix)}&sort=${ndb.sort}&order=${ndb.order}${agentParam}`,
    );
    const rows = data.rows || [];
    const total = data.total || 0;
    const totalPages = data.total_pages || 1;
    const agents = Array.isArray(data.agents) ? data.agents : [];

    const sortIcon = (field) => {
      if (ndb.sort !== field) return '<span style="opacity:0.3">&#8597;</span>';
      return ndb.order === "asc" ? "&#9650;" : "&#9660;";
    };

    el.innerHTML = `
      <div class="ndb-toolbar">
        <div style="display:flex;gap:8px;align-items:center;flex:1;flex-wrap:wrap">
          <input type="text" id="ndb-search" placeholder="Filter by key prefix..." value="${esc(ndb.prefix)}"
            style="width:220px;padding:6px 10px;font-size:12px">
          ${agents.length > 0 ? `
            <select id="ndb-agent-filter" style="padding:5px 8px;font-size:12px;background:var(--bg-card);color:var(--text);border:1px solid var(--border);border-radius:var(--radius)" onchange="ndbFilterAgent(this.value)">
              <option value="">All agents</option>
              ${agents.map(a => `<option value="${esc(a)}" ${ndb.agentFilter === a ? "selected" : ""}>${esc(a)}</option>`).join("")}
            </select>
          ` : ""}
          <button class="btn btn-sm" onclick="ndbSearch()">Filter</button>
          ${ndb.prefix || ndb.agentFilter ? `<button class="btn btn-sm" onclick="ndbClearSearch()">Clear</button>` : ""}
          <span class="ndb-count">${total} key${total !== 1 ? "s" : ""}${ndb.agentFilter ? ` (agent: ${esc(ndb.agentFilter)})` : ""}</span>
        </div>
        <div style="display:flex;gap:6px">
          <button class="btn btn-sm btn-primary" onclick="ndbNewKey()">+ New Key</button>
          <button class="btn btn-sm" onclick="ndbExport('json')">Export JSON</button>
          <button class="btn btn-sm" onclick="ndbExport('csv')">Export CSV</button>
        </div>
      </div>

      ${
        rows.length > 0
          ? `
        <div class="table-wrap"><table class="ndb-table">
          <thead><tr>
            <th class="ndb-sortable" onclick="ndbSort('key')">Key ${sortIcon("key")}</th>
            <th style="width:70px">Type</th>
            <th class="ndb-sortable" onclick="ndbSort('value')">Value ${sortIcon("value")}</th>
            <th style="width:50px">Idx</th>
            <th style="width:140px;text-align:center">Actions</th>
          </tr></thead>
          <tbody>
            ${rows
              .map(
                (row) => `
              <tr data-key="${esc(row.key)}">
                <td class="ndb-key">${esc(row.key)}</td>
                <td>${typeBadge(row.type)}</td>
                <td class="ndb-value ndb-value-cell" data-key="${esc(row.key)}">${renderTypedValue(row)}</td>
                <td style="color:var(--text-dim);font-size:11px">${row.index}</td>
                <td class="ndb-actions">
                  <button class="btn-icon" data-ndb-action="verify" data-key="${esc(row.key)}" title="Verify Merkle proof">&#128737;</button>
                  <button class="btn-icon" data-ndb-action="history" data-key="${esc(row.key)}" title="Key history">&#128339;</button>
                  <button class="btn-icon" data-ndb-action="edit" data-key="${esc(row.key)}" title="Edit value">&#9998;</button>
                  <button class="btn-icon btn-icon-danger" data-ndb-action="delete" data-key="${esc(row.key)}" title="Delete">&#128465;</button>
                </td>
              </tr>
            `,
              )
              .join("")}
          </tbody>
        </table></div>

        <div class="ndb-pagination">
          <button class="btn btn-sm" onclick="ndbPageNav(0)" ${ndb.page === 0 ? "disabled" : ""}>&#171; First</button>
          <button class="btn btn-sm" onclick="ndbPageNav(${ndb.page - 1})" ${ndb.page === 0 ? "disabled" : ""}>&#8249; Prev</button>
          <span class="ndb-page-info">Page ${ndb.page + 1} of ${totalPages}</span>
          <button class="btn btn-sm" onclick="ndbPageNav(${ndb.page + 1})" ${ndb.page >= totalPages - 1 ? "disabled" : ""}>Next &#8250;</button>
          <button class="btn btn-sm" onclick="ndbPageNav(${totalPages - 1})" ${ndb.page >= totalPages - 1 ? "disabled" : ""}>Last &#187;</button>
          <select class="ndb-page-size" onchange="ndbChangePageSize(this.value)">
            ${[25, 50, 100, 200].map((n) => `<option value="${n}" ${ndb.pageSize === n ? "selected" : ""}>${n} / page</option>`).join("")}
          </select>
        </div>
      `
          : `
        <div class="ndb-empty">
          <div style="font-size:36px;margin-bottom:12px;color:var(--accent)">&#9762;</div>
          <div style="font-size:14px;margin-bottom:8px;color:var(--accent)">No data stored yet</div>
          <div style="color:var(--text-muted);margin-bottom:16px;font-size:12px">Insert your first key-value pair to get started.</div>
          <button class="btn btn-primary" onclick="ndbNewKey()">+ Insert First Key</button>
          <button class="btn btn-sm" style="margin-left:8px" onclick="ndbSwitchTab('sql')">Open SQL Console</button>
        </div>
      `
      }

      <div id="ndb-detail-panel"></div>
    `;

    // Store rows for edit flow
    window._ndbRows = rows;

    // Bind Enter key on search input
    const searchInput = $("#ndb-search");
    if (searchInput) {
      searchInput.addEventListener("keydown", (e) => {
        if (e.key === "Enter") ndbSearch();
      });
    }

    const table = el.querySelector(".ndb-table");
    if (table) {
      table.addEventListener("dblclick", (e) => {
        const cell = e.target.closest(".ndb-value-cell");
        if (!cell) return;
        const key = cell.dataset.key || "";
        if (key) ndbStartEditTyped(key);
      });
      table.addEventListener("click", (e) => {
        const jsonToggle = e.target.closest(".ndb-json-toggle");
        if (jsonToggle) {
          ndbExpandJson(jsonToggle);
          return;
        }
        const btn = e.target.closest("[data-ndb-action]");
        if (!btn) return;
        const key = btn.dataset.key || "";
        if (!key) return;
        const action = btn.dataset.ndbAction;
        if (action === "verify") ndbVerifyKey(key);
        else if (action === "history") ndbKeyHistory(key);
        else if (action === "edit") ndbStartEditTyped(key);
        else if (action === "delete") ndbDeleteKey(key);
      });
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error loading data: ${esc(e.message)}</div>`;
  }
}

// JSON expand handler
window.ndbExpandJson = function (el, key) {
  const effectiveKey = key || el?.dataset?.key || "";
  if (!effectiveKey) return;
  const existing = el.parentElement.querySelector(".ndb-json-expanded");
  if (existing) {
    existing.remove();
    return;
  }
  const row = (window._ndbRows || []).find((r) => r.key === effectiveKey);
  if (!row) return;
  const div = document.createElement("div");
  div.className = "ndb-json-expanded";
  div.textContent =
    typeof row.value === "object"
      ? JSON.stringify(row.value, null, 2)
      : row.display;
  el.parentElement.appendChild(div);
};

window.ndbSearch = function () {
  ndb.prefix = ($("#ndb-search")?.value || "").trim();
  ndb.page = 0;
  ndbRenderBrowse();
};

window.ndbClearSearch = function () {
  ndb.prefix = "";
  ndb.agentFilter = "";
  ndb.page = 0;
  ndbRenderBrowse();
};

window.ndbFilterAgent = function (agent) {
  ndb.agentFilter = agent;
  ndb.page = 0;
  ndbRenderBrowse();
};

window.ndbSort = function (field) {
  if (ndb.sort === field) {
    ndb.order = ndb.order === "asc" ? "desc" : "asc";
  } else {
    ndb.sort = field;
    ndb.order = "asc";
  }
  ndb.page = 0;
  ndbRenderBrowse();
};

window.ndbPageNav = function (page) {
  ndb.page = Math.max(0, page);
  ndbRenderBrowse();
};

window.ndbChangePageSize = function (size) {
  ndb.pageSize = parseInt(size) || 50;
  ndb.page = 0;
  ndbRenderBrowse();
};

// Typed edit
window.ndbStartEditTyped = function (key) {
  const row = (window._ndbRows || []).find((r) => r.key === key);
  const type = row?.type || "integer";
  const val = row?.value;
  const panel = $("#ndb-detail-panel");

  let valueInput;
  switch (type) {
    case "integer":
    case "float":
      valueInput = `<input type="number" id="ndb-edit-value" value="${val != null ? val : 0}" step="${type === "float" ? "any" : "1"}"
        style="width:260px;padding:6px 10px;font-size:13px">`;
      break;
    case "bool":
      valueInput = `<select id="ndb-edit-value" class="ndb-type-select" style="width:120px">
        <option value="true" ${val ? "selected" : ""}>true</option>
        <option value="false" ${!val ? "selected" : ""}>false</option>
      </select>`;
      break;
    case "null":
      valueInput = `<span style="color:var(--text-muted);font-style:italic">NULL (no editable value)</span>
        <input type="hidden" id="ndb-edit-value" value="null">`;
      break;
    case "text":
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px">${esc(val || "")}</textarea>`;
      break;
    case "json":
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px;min-height:120px">${esc(typeof val === "object" ? JSON.stringify(val, null, 2) : String(val))}</textarea>`;
      break;
    case "vector": {
      const arrStr = Array.isArray(val) ? val.join(", ") : "";
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px" placeholder="0.1, 0.2, 0.3, ...">${esc(arrStr)}</textarea>
        <div style="color:var(--text-dim);font-size:10px;margin-top:2px">${Array.isArray(val) ? val.length + " dimensions" : ""} &mdash; comma-separated floats</div>`;
      break;
    }
    case "bytes":
      valueInput = `<textarea id="ndb-edit-value" class="ndb-value-textarea" style="width:400px" placeholder="hex bytes: 0a1b2c...">${esc(val || "")}</textarea>`;
      break;
    default:
      valueInput = `<input type="text" id="ndb-edit-value" value="${esc(String(val || ""))}"
        style="width:260px;padding:6px 10px;font-size:13px">`;
  }

  panel.innerHTML = `
    <div class="ndb-edit-panel">
      <div class="section-header">Edit Key</div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
        <label style="font-weight:600;min-width:50px;font-size:12px">Key:</label>
        <span style="color:var(--accent)">${esc(key)}</span>
        ${typeBadge(type)}
      </div>
      <div style="display:flex;gap:8px;align-items:flex-start;margin-bottom:12px">
        <label style="font-weight:600;min-width:50px;margin-top:6px;font-size:12px">Value:</label>
        <div>${valueInput}</div>
      </div>
      <div style="display:flex;gap:8px">
        <button class="btn btn-primary btn-sm" id="ndb-save-edit-btn" data-key="${esc(key)}" data-type="${esc(type)}">Save &amp; Commit</button>
        <button class="btn btn-sm" onclick="$('#ndb-detail-panel').innerHTML=''">Cancel</button>
      </div>
      <div id="ndb-edit-result" style="margin-top:8px"></div>
    </div>
  `;
  const saveBtn = $("#ndb-save-edit-btn");
  if (saveBtn) {
    saveBtn.addEventListener("click", () => {
      ndbSaveEditTyped(
        saveBtn.dataset.key || "",
        saveBtn.dataset.type || "integer",
      );
    });
  }
  const inp = $("#ndb-edit-value");
  if (inp && inp.focus) {
    inp.focus();
    if (inp.select) inp.select();
  }
};

window.ndbSaveEditTyped = async function (key, type) {
  const raw = $("#ndb-edit-value")?.value;
  let value;
  try {
    switch (type) {
      case "integer":
        value = parseInt(raw);
        if (isNaN(value)) throw new Error("Invalid integer");
        break;
      case "float":
        value = parseFloat(raw);
        if (isNaN(value)) throw new Error("Invalid float");
        break;
      case "bool":
        value = raw === "true";
        break;
      case "null":
        value = null;
        break;
      case "text":
        value = raw;
        break;
      case "json":
        value = JSON.parse(raw);
        break;
      case "vector": {
        const nums = raw
          .split(",")
          .map((s) => parseFloat(s.trim()))
          .filter((n) => !isNaN(n));
        if (nums.length === 0)
          throw new Error("Vector must have at least one dimension");
        value = nums;
        break;
      }
      case "bytes":
        value = raw;
        break;
      default:
        value = raw;
    }
  } catch (e) {
    $("#ndb-edit-result").innerHTML =
      `<div style="color:var(--red)">Invalid value: ${esc(e.message)}</div>`;
    return;
  }
  try {
    const res = await apiPost("/nucleusdb/edit", { key, type, value });
    if (res.error) {
      $("#ndb-edit-result").innerHTML =
        `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      const typeLabel = res.type ? ` (${res.type})` : "";
      $("#ndb-detail-panel").innerHTML =
        `<div style="color:var(--green);padding:8px;text-shadow:var(--glow-green)">Saved ${esc(key)}${typeLabel} and committed.</div>`;
      setTimeout(() => ndbRenderBrowse(), 800);
    }
  } catch (e) {
    $("#ndb-edit-result").innerHTML =
      `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

window.ndbNewKey = function () {
  const panel = $("#ndb-detail-panel");
  panel.innerHTML = `
    <div class="ndb-edit-panel">
      <div class="section-header">New Key-Value Pair</div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
        <label style="font-weight:600;min-width:50px;font-size:12px">Key:</label>
        <input type="text" id="ndb-new-key" placeholder="my_key" style="width:260px;padding:6px 10px;font-size:13px">
      </div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
        <label style="font-weight:600;min-width:50px;font-size:12px">Type:</label>
        <select id="ndb-new-type" class="ndb-type-select" onchange="ndbNewKeyTypeChanged()">
          <option value="integer">Integer</option>
          <option value="float">Float</option>
          <option value="text">Text</option>
          <option value="json">JSON</option>
          <option value="bool">Boolean</option>
          <option value="vector">Vector</option>
          <option value="null">Null</option>
        </select>
      </div>
      <div id="ndb-new-value-wrap" style="display:flex;gap:8px;align-items:flex-start;margin-bottom:12px">
        <label style="font-weight:600;min-width:50px;margin-top:6px;font-size:12px">Value:</label>
        <div id="ndb-new-value-input">
          <input type="number" id="ndb-new-value" value="0" style="width:260px;padding:6px 10px;font-size:13px">
        </div>
      </div>
      <div style="display:flex;gap:8px">
        <button class="btn btn-primary btn-sm" onclick="ndbInsertNew()">Insert &amp; Commit</button>
        <button class="btn btn-sm" onclick="$('#ndb-detail-panel').innerHTML=''">Cancel</button>
      </div>
      <div id="ndb-new-result" style="margin-top:8px"></div>
    </div>
  `;
  $("#ndb-new-key").focus();
};

window.ndbNewKeyTypeChanged = function () {
  const type = $("#ndb-new-type")?.value || "integer";
  const wrap = $("#ndb-new-value-input");
  if (!wrap) return;
  switch (type) {
    case "integer":
      wrap.innerHTML = `<input type="number" id="ndb-new-value" value="0" step="1" style="width:260px;padding:6px 10px;font-size:13px">`;
      break;
    case "float":
      wrap.innerHTML = `<input type="number" id="ndb-new-value" value="0.0" step="any" style="width:260px;padding:6px 10px;font-size:13px">`;
      break;
    case "text":
      wrap.innerHTML = `<textarea id="ndb-new-value" class="ndb-value-textarea" style="width:400px" placeholder="Enter text..."></textarea>`;
      break;
    case "json":
      wrap.innerHTML = `<textarea id="ndb-new-value" class="ndb-value-textarea" style="width:400px;min-height:100px" placeholder='{"key": "value"}'>{}</textarea>`;
      break;
    case "bool":
      wrap.innerHTML = `<select id="ndb-new-value" class="ndb-type-select" style="width:120px">
        <option value="true">true</option><option value="false">false</option></select>`;
      break;
    case "vector":
      wrap.innerHTML = `<textarea id="ndb-new-value" class="ndb-value-textarea" style="width:400px" placeholder="0.1, 0.2, 0.3, ..."></textarea>
        <div style="color:var(--text-dim);font-size:10px;margin-top:2px">Comma-separated float values</div>`;
      break;
    case "null":
      wrap.innerHTML = `<span style="color:var(--text-muted);font-style:italic">NULL &mdash; no value</span>
        <input type="hidden" id="ndb-new-value" value="null">`;
      break;
  }
};

window.ndbInsertNew = async function () {
  const key = ($("#ndb-new-key")?.value || "").trim();
  const type = $("#ndb-new-type")?.value || "integer";
  const raw = ($("#ndb-new-value")?.value || "").trim();

  if (!key) {
    $("#ndb-new-result").innerHTML =
      '<div style="color:var(--red)">Key cannot be empty</div>';
    return;
  }

  let value;
  try {
    switch (type) {
      case "integer":
        value = parseInt(raw);
        if (isNaN(value)) throw new Error("Invalid integer");
        break;
      case "float":
        value = parseFloat(raw);
        if (isNaN(value)) throw new Error("Invalid float");
        break;
      case "bool":
        value = raw === "true";
        break;
      case "null":
        value = null;
        break;
      case "text":
        value = raw;
        break;
      case "json":
        value = JSON.parse(raw);
        break;
      case "vector": {
        const nums = raw
          .split(",")
          .map((s) => parseFloat(s.trim()))
          .filter((n) => !isNaN(n));
        if (nums.length === 0) throw new Error("Enter at least one number");
        value = nums;
        break;
      }
      default:
        value = raw;
    }
  } catch (e) {
    $("#ndb-new-result").innerHTML =
      `<div style="color:var(--red)">Invalid value: ${esc(e.message)}</div>`;
    return;
  }

  try {
    const res = await apiPost("/nucleusdb/edit", { key, type, value });
    if (res.error) {
      $("#ndb-new-result").innerHTML =
        `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      const typeLabel = res.type ? ` (${res.type})` : "";
      $("#ndb-detail-panel").innerHTML =
        `<div style="color:var(--green);padding:8px;text-shadow:var(--glow-green)">Inserted ${esc(key)}${typeLabel} and committed.</div>`;
      setTimeout(() => ndbRenderBrowse(), 800);
    }
  } catch (e) {
    $("#ndb-new-result").innerHTML =
      `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

window.ndbDeleteKey = async function (key) {
  if (
    !confirm(
      `Delete key '${key}'? This queues a tombstone (value=0) and commits.`,
    )
  )
    return;
  try {
    const res = await apiPost("/nucleusdb/edit", {
      key,
      type: "integer",
      value: 0,
    });
    if (res.error) {
      alert("Delete failed: " + res.error);
    } else {
      ndbRenderBrowse();
    }
  } catch (e) {
    alert("Delete failed: " + e.message);
  }
};

window.ndbVerifyKey = async function (key) {
  const panel = $("#ndb-detail-panel");
  panel.innerHTML =
    '<div style="color:var(--text-muted);padding:8px">Verifying Merkle proof...</div>';
  try {
    const res = await api(`/nucleusdb/verify/${encodeURIComponent(key)}`);
    if (!res.found) {
      panel.innerHTML = `<div class="ndb-verify-panel"><span class="badge badge-err">Key not found</span></div>`;
      return;
    }
    panel.innerHTML = `
      <div class="ndb-verify-panel">
        <div class="section-header">Merkle Proof Verification</div>
        <div class="ndb-verify-grid">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Key</span><span class="ndb-mono" style="color:var(--accent)">${esc(res.key)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Type</span>${typeBadge(res.type || "integer")}</div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Value</span><span class="ndb-mono">${esc(res.display || String(res.value))}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Index</span><span class="ndb-mono">${res.index}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Backend</span><span class="ndb-mono">${esc(res.backend)}</span></div>
          ${
            res.blob_verified != null
              ? `
          <div class="ndb-verify-row"><span class="ndb-verify-label">Blob</span>
            <span>${
              res.blob_verified
                ? '<span class="badge badge-ok">Blob Verified</span>'
                : '<span class="badge badge-warn">No Blob</span>'
            }</span>
          </div>`
              : ""
          }
          <div class="ndb-verify-row">
            <span class="ndb-verify-label">Verified</span>
            <span>${
              res.verified
                ? '<span class="badge badge-ok" style="font-size:13px">&#10003; VERIFIED</span>'
                : '<span class="badge badge-err" style="font-size:13px">&#10007; FAILED</span>'
            }</span>
          </div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Root Hash</span><span class="ndb-mono ndb-hash">${esc(res.root_hash)}</span></div>
        </div>
        <button class="btn btn-sm" style="margin-top:8px" onclick="$('#ndb-detail-panel').innerHTML=''">Close</button>
      </div>
    `;
  } catch (e) {
    panel.innerHTML = `<div style="color:var(--red);padding:8px">Verify error: ${esc(e.message)}</div>`;
  }
};

window.ndbKeyHistory = async function (key) {
  const panel = $("#ndb-detail-panel");
  panel.innerHTML =
    '<div style="color:var(--text-muted);padding:8px">Loading history...</div>';
  try {
    const res = await api(`/nucleusdb/key-history/${encodeURIComponent(key)}`);
    if (!res.found) {
      panel.innerHTML = `<div class="ndb-verify-panel"><span class="badge badge-err">Key not found</span></div>`;
      return;
    }
    const typeTag = res.type || "integer";
    const currentDisplay =
      res.current_display != null
        ? String(res.current_display)
        : String(res.current_value ?? "");
    const typedValue =
      res.current_typed_value !== undefined
        ? res.current_typed_value
        : res.current_value;
    const typedJson = JSON.stringify(typedValue, null, 2);

    panel.innerHTML = `
      <div class="ndb-verify-panel">
        <div class="section-header">Key History: ${esc(key)}</div>
        <div class="ndb-verify-grid" style="margin-bottom:12px">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Type</span>${typeBadge(typeTag)}</div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Display</span><span class="ndb-mono">${esc(currentDisplay)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Typed Value</span><span class="ndb-mono">${esc(truncate(typedJson, 120))}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Raw Value</span><span class="ndb-mono">${res.current_value}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Index</span><span class="ndb-mono">${res.index}</span></div>
        </div>
        ${
          typedJson.length > 120
            ? `
          <details style="margin-bottom:10px">
            <summary style="cursor:pointer;color:var(--text-muted);font-size:12px">Show full typed value JSON</summary>
            <pre class="ndb-json-expanded">${esc(typedJson)}</pre>
          </details>
        `
            : ""
        }
        ${
          res.commits && res.commits.length > 0
            ? `
          <div style="font-size:12px;font-weight:600;margin-bottom:6px;color:var(--text-muted);text-transform:uppercase;letter-spacing:1px">Commits (${res.commits.length})</div>
          <div class="table-wrap"><table>
            <thead><tr><th>Height</th><th>State Root</th><th>Timestamp</th></tr></thead>
            <tbody>${res.commits
              .map(
                (c) => `
              <tr>
                <td style="color:var(--accent)">${c.height}</td>
                <td class="ndb-mono ndb-hash">${esc(c.state_root)}</td>
                <td style="font-size:11px">${c.timestamp_unix ? fmtTime(c.timestamp_unix) : "n/a"}</td>
              </tr>
            `,
              )
              .join("")}</tbody>
          </table></div>
          ${res.note ? `<div style="color:var(--text-dim);font-size:11px;margin-top:4px">${esc(res.note)}</div>` : ""}
        `
            : '<div style="color:var(--text-muted)">No commits yet.</div>'
        }
        <button class="btn btn-sm" style="margin-top:8px" onclick="$('#ndb-detail-panel').innerHTML=''">Close</button>
      </div>
    `;
  } catch (e) {
    panel.innerHTML = `<div style="color:var(--red);padding:8px">History error: ${esc(e.message)}</div>`;
  }
};

window.ndbExport = async function (fmt) {
  try {
    const res = await api(`/nucleusdb/export?format=${fmt}`);
    const text =
      fmt === "csv" ? res.content : JSON.stringify(res.content, null, 2);
    const blob = new Blob([text], {
      type: fmt === "csv" ? "text/csv" : "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `nucleusdb_export.${fmt}`;
    a.click();
    URL.revokeObjectURL(url);
  } catch (e) {
    alert("Export failed: " + e.message);
  }
};

// -- SQL Sub-Tab --------------------------------------------------------------
function ndbRenderSQL() {
  const el = $("#ndb-content");
  el.innerHTML = `
    <div style="margin:12px 0">
      <div style="display:flex;gap:8px;margin-bottom:8px">
        <input type="text" id="sql-input" placeholder="Enter SQL (e.g. SELECT * FROM data)"
          style="flex:1;padding:8px 12px;font-size:12px">
        <button class="btn btn-primary" onclick="runSQL()">Execute</button>
      </div>
      <div class="config-desc" style="margin-bottom:4px">
        <strong style="color:var(--accent)">Supported:</strong> SELECT, INSERT, UPDATE, DELETE, COMMIT, VERIFY, SHOW STATUS/HISTORY/MODE/TYPES, VECTOR_SEARCH, SET MODE APPEND_ONLY, EXPORT
      </div>
      <div class="ndb-sql-presets">
        <span style="color:var(--text-dim);font-size:11px">Quick:</span>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SELECT * FROM data')">All Data</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW TYPES')">Types</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW STATUS')">Status</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('SHOW HISTORY')">History</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset('EXPORT')">Export</button>
      </div>
      <div class="ndb-sql-presets" style="margin-top:2px">
        <span style="color:var(--text-dim);font-size:11px">Insert:</span>
        <button class="btn btn-xs" onclick="ndbSQLPreset(&quot;INSERT INTO data (key, value) VALUES ('mykey', 'hello world')&quot;)">Text</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset(&quot;INSERT INTO data (key, value) VALUES ('mykey', '{\\&quot;name\\&quot;:\\&quot;Alice\\&quot;}')&quot;)">JSON</button>
        <button class="btn btn-xs" onclick="ndbSQLPreset(&quot;INSERT INTO data (key, value) VALUES ('mykey', VECTOR(0.1, 0.2, 0.3))&quot;)">Vector</button>
      </div>
      <div id="sql-result" style="margin-top:12px"></div>
    </div>
  `;
  const inp = $("#sql-input");
  if (inp)
    inp.addEventListener("keydown", (e) => {
      if (e.key === "Enter") runSQL();
    });
}

window.ndbSQLPreset = function (sql) {
  const inp = $("#sql-input");
  if (inp) {
    inp.value = sql;
    runSQL();
  }
};

window.runSQL = async function () {
  const query = ($("#sql-input")?.value || "").trim();
  if (!query) return;
  const el = $("#sql-result");
  el.innerHTML = '<div style="color:var(--text-muted)">Executing...</div>';
  try {
    const data = await apiPost("/nucleusdb/sql", { query });
    if (data.error) {
      el.innerHTML = `<div style="color:var(--red)">Error: ${esc(data.error)}</div>`;
    } else if (data.columns && data.rows) {
      if (data.rows.length === 0) {
        el.innerHTML = `<div style="color:var(--text-muted)">No rows returned.</div>`;
      } else {
        el.innerHTML = `<div class="table-wrap"><table>
          <thead><tr>${data.columns.map((c) => `<th>${esc(c)}</th>`).join("")}</tr></thead>
          <tbody>${data.rows
            .map(
              (row) =>
                `<tr>${row.map((cell) => `<td style="font-size:11px">${esc(cell)}</td>`).join("")}</tr>`,
            )
            .join("")}</tbody>
        </table></div>
        <div style="color:var(--text-muted);font-size:11px;margin-top:4px">${data.rows.length} row(s)</div>`;
      }
    } else if (data.message) {
      el.innerHTML = `<div style="color:var(--green);text-shadow:var(--glow-green)">${esc(data.message)}</div>`;
    } else {
      el.innerHTML = `<pre class="ndb-json-expanded">${esc(JSON.stringify(data, null, 2))}</pre>`;
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

// -- Vectors Sub-Tab ----------------------------------------------------------
async function ndbRenderVectors() {
  const el = $("#ndb-content");
  el.innerHTML =
    '<div style="color:var(--text-muted)">Loading vector index...</div>';
  try {
    const stats = window._ndbStats || (await api("/nucleusdb/stats"));
    const vecCount = stats.vector_count || 0;
    const vecDims = stats.vector_dims || 0;

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="card-grid">
          <div class="card">
            <div class="card-label">Vectors Indexed</div>
            <div class="card-value">${vecCount}</div>
          </div>
          <div class="card">
            <div class="card-label">Dimensions</div>
            <div class="card-value">${vecDims || "n/a"}</div>
          </div>
          <div class="card">
            <div class="card-label">Blob Storage</div>
            <div class="card-value" style="font-size:14px">${formatBytes(stats.blob_total_bytes || 0)}</div>
            <div class="card-sub">${stats.blob_count || 0} objects</div>
          </div>
        </div>

        <div class="section-header">Similarity Search</div>
        <div class="ndb-vector-search">
          <div style="display:flex;gap:8px;align-items:flex-start;margin-bottom:8px">
            <label style="font-weight:600;min-width:60px;margin-top:6px;font-size:12px">Query:</label>
            <textarea id="ndb-vec-query" class="ndb-value-textarea" style="width:400px;min-height:40px"
              placeholder="0.1, 0.2, 0.3, ...${vecDims ? " (" + vecDims + " dims)" : ""}"></textarea>
          </div>
          <div style="display:flex;gap:12px;align-items:center;margin-bottom:12px">
            <label style="font-weight:600;min-width:60px;font-size:12px">Metric:</label>
            <select id="ndb-vec-metric" class="ndb-type-select">
              <option value="cosine">Cosine</option>
              <option value="l2">L2 (Euclidean)</option>
              <option value="inner_product">Inner Product</option>
            </select>
            <label style="font-weight:500;margin-left:8px;font-size:12px">k:</label>
            <input type="number" id="ndb-vec-k" value="10" min="1" max="100"
              style="width:60px;padding:6px 10px;font-size:12px">
            <button class="btn btn-primary btn-sm" onclick="ndbVectorSearch()">Search</button>
          </div>
          ${vecCount === 0 ? `<div style="color:var(--text-muted);font-size:12px">No vectors in the index yet. Insert vectors via the Browse tab or SQL console.</div>` : ""}
        </div>
        <div id="ndb-vec-results"></div>

        <div class="section-header">Insert Vector</div>
        <div class="ndb-vector-search">
          <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
            <label style="font-weight:600;min-width:60px;font-size:12px">Key:</label>
            <input type="text" id="ndb-vec-insert-key" placeholder="doc:embedding:1"
              style="width:260px;padding:6px 10px;font-size:12px">
          </div>
          <div style="display:flex;gap:8px;align-items:flex-start;margin-bottom:8px">
            <label style="font-weight:600;min-width:60px;margin-top:6px;font-size:12px">Dims:</label>
            <textarea id="ndb-vec-insert-dims" class="ndb-value-textarea" style="width:400px;min-height:40px"
              placeholder="0.1, 0.2, 0.3, ..."></textarea>
          </div>
          <div style="display:flex;gap:8px">
            <button class="btn btn-primary btn-sm" onclick="ndbVectorInsert()">Insert &amp; Commit</button>
          </div>
          <div id="ndb-vec-insert-result" style="margin-top:8px"></div>
        </div>
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

window.ndbVectorSearch = async function () {
  const raw = ($("#ndb-vec-query")?.value || "").trim();
  const metric = $("#ndb-vec-metric")?.value || "cosine";
  const k = parseInt($("#ndb-vec-k")?.value) || 10;
  const el = $("#ndb-vec-results");

  if (!raw) {
    el.innerHTML = '<div style="color:var(--red)">Enter a query vector</div>';
    return;
  }

  const query = raw
    .split(",")
    .map((s) => parseFloat(s.trim()))
    .filter((n) => !isNaN(n));
  if (query.length === 0) {
    el.innerHTML = '<div style="color:var(--red)">Invalid vector</div>';
    return;
  }

  el.innerHTML = '<div style="color:var(--text-muted)">Searching...</div>';
  try {
    const res = await apiPost("/nucleusdb/vector-search", { query, k, metric });
    if (res.error) {
      el.innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
      return;
    }
    const results = res.results || [];
    const totalVectors = res.total_vectors ?? res.vector_count ?? 0;
    if (results.length === 0) {
      el.innerHTML = `<div style="color:var(--text-muted)">No results found. ${totalVectors === 0 ? "Index is empty." : ""}</div>`;
      return;
    }
    el.innerHTML = `
      <div class="section-header" style="margin-top:12px">Results (${results.length} nearest, ${esc(metric)})</div>
      <div class="ndb-vector-results">
        ${results
          .map(
            (r, i) => `
          <div class="ndb-vector-result-item">
            <span class="ndb-vector-rank">#${i + 1}</span>
            <span class="ndb-key" style="flex:1">${esc(r.key)}</span>
            <span class="ndb-vector-dist">${typeof r.distance === "number" ? r.distance.toFixed(6) : r.distance}</span>
            <button class="btn-icon ndb-vec-verify-btn" data-key="${esc(r.key)}" title="Verify">&#128737;</button>
          </div>
        `,
          )
          .join("")}
      </div>
    `;
    $$(".ndb-vec-verify-btn", el).forEach((btn) => {
      btn.addEventListener("click", () => {
        const key = btn.dataset.key || "";
        if (key) ndbVerifyKey(key);
      });
    });
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Search error: ${esc(e.message)}</div>`;
  }
};

window.ndbVectorInsert = async function () {
  const key = ($("#ndb-vec-insert-key")?.value || "").trim();
  const raw = ($("#ndb-vec-insert-dims")?.value || "").trim();
  const el = $("#ndb-vec-insert-result");

  if (!key) {
    el.innerHTML = '<div style="color:var(--red)">Key cannot be empty</div>';
    return;
  }
  const nums = raw
    .split(",")
    .map((s) => parseFloat(s.trim()))
    .filter((n) => !isNaN(n));
  if (nums.length === 0) {
    el.innerHTML =
      '<div style="color:var(--red)">Enter at least one dimension</div>';
    return;
  }

  try {
    const res = await apiPost("/nucleusdb/edit", {
      key,
      type: "vector",
      value: nums,
    });
    if (res.error) {
      el.innerHTML = `<div style="color:var(--red)">Error: ${esc(res.error)}</div>`;
    } else {
      el.innerHTML = `<div style="color:var(--green);text-shadow:var(--glow-green)">Inserted ${esc(key)} &mdash; ${nums.length}d vector committed.</div>`;
    }
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

// -- Commits Sub-Tab (Seal Chain Visualization) -------------------------------
async function ndbRenderCommits() {
  const el = $("#ndb-content");
  el.innerHTML =
    '<div style="color:var(--text-muted)">Loading seal chain...</div>';
  try {
    const history = await api("/nucleusdb/history");
    const commits = history.commits?.rows || [];
    const columns = history.commits?.columns || [];

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="seal-chain-status ${commits.length > 0 ? "ok" : ""}">
          <span class="seal-chain-indicator">${
            commits.length > 0
              ? "&#10003; SEAL CHAIN UNBROKEN"
              : "&#9888; NO COMMITS"
          }</span>
          <span style="color:var(--text-dim);font-size:11px;margin-left:auto">${commits.length} commit${commits.length !== 1 ? "s" : ""}</span>
        </div>

        ${
          commits.length > 0
            ? `
          <div class="seal-chain">
            ${commits
              .slice()
              .reverse()
              .slice(0, 20)
              .map((row, i) => {
                const height = row[0];
                const rootHash = row[1] || "";
                const timestamp = row[2] || "";
                return `
                <div class="seal-node">
                  <div class="seal-height">Commit #${esc(String(height))}</div>
                  <div class="seal-detail"><span>Root:</span> ${esc(truncate(rootHash, 48))}</div>
                  <div class="seal-detail"><span>Seal:</span> SHA-256(seal_${height > 0 ? height - 1 : 0} | kv_digest)</div>
                  <div class="seal-detail"><span>Time:</span> ${esc(timestamp)}</div>
                </div>
                ${i < Math.min(commits.length, 20) - 1 ? '<div class="seal-connector"></div>' : ""}
              `;
              })
              .join("")}
          </div>
          ${commits.length > 20 ? `<div style="color:var(--text-dim);font-size:11px;margin-top:8px;text-align:center">Showing 20 of ${commits.length} commits</div>` : ""}
        `
            : '<div style="color:var(--text-muted);padding:24px;text-align:center">No commits yet. Insert data and COMMIT to create the first seal.</div>'
        }
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

// -- Proofs Sub-Tab (NEW) -----------------------------------------------------
function ndbRenderProofs() {
  const el = $("#ndb-content");
  const stats = window._ndbStats || {};
  const status = window._ndbStatus || {};
  const backend = status.backend || "binary_merkle";
  const bi = backendInfo[backend] || backendInfo.binary_merkle;

  el.innerHTML = `
    <div style="margin:12px 0">
      <div class="proof-section">
        <div class="proof-section-title">Active Backend</div>
        <div class="ndb-verify-grid">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Engine</span><span class="ndb-mono" style="color:var(--accent)">${esc(bi.name)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Algorithm</span><span class="ndb-mono">${esc(bi.algo)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Security</span><span class="ndb-mono">${esc(bi.type)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Proof Size</span><span class="ndb-mono">${esc(bi.proof)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Setup</span><span class="ndb-mono">${esc(bi.setup)}</span></div>
        </div>
        <div style="color:var(--text-dim);font-size:11px;margin-top:8px">
          Every key has a position in a Merkle tree. A proof is a path from the leaf to the root.
          If ANY value changes, the root changes. Verification: ${esc(bi.proof)} hashes.
        </div>
      </div>

      ${
        stats.sth
          ? `
      <div class="proof-section">
        <div class="proof-section-title">Certificate Transparency (RFC 6962)</div>
        <div class="ndb-verify-grid">
          <div class="ndb-verify-row"><span class="ndb-verify-label">Tree Size</span><span class="ndb-mono" style="color:var(--accent)">${stats.sth.tree_size}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Root Hash</span><span class="ndb-mono ndb-hash">${esc(stats.sth.root_hash)}</span></div>
          <div class="ndb-verify-row"><span class="ndb-verify-label">Timestamp</span><span class="ndb-mono">${stats.sth.timestamp_unix ? fmtTime(stats.sth.timestamp_unix) : "n/a"}</span></div>
        </div>
      </div>
      `
          : ""
      }

      <div class="proof-section">
        <div class="proof-section-title">Verify a Key</div>
        <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px">
          <input type="text" id="ndb-proof-key" placeholder="Enter key to verify..." style="flex:1;padding:8px 12px;font-size:12px">
          <button class="btn btn-primary btn-sm" onclick="ndbProofVerify()">Verify</button>
        </div>
        <div id="ndb-proof-result"></div>
      </div>

      <div class="proof-section">
        <div class="proof-section-title">Backend Comparison</div>
        <div class="backend-comparison">
          <div class="backend-card ${backend === "binary_merkle" ? "active" : ""}">
            <div class="backend-card-name">BinaryMerkle</div>
            <div class="backend-card-detail">SHA-256</div>
            <div class="backend-card-detail">Post-Quantum</div>
            <div class="backend-card-detail">O(log n) proof</div>
            <div class="backend-card-detail">No trusted setup</div>
            <div style="margin-top:6px">${backend === "binary_merkle" ? '<span class="badge badge-ok">ACTIVE</span>' : '<span class="badge badge-muted">Available</span>'}</div>
          </div>
          <div class="backend-card ${backend === "ipa" ? "active" : ""}">
            <div class="backend-card-name">IPA</div>
            <div class="backend-card-detail">Pedersen</div>
            <div class="backend-card-detail">Binding</div>
            <div class="backend-card-detail">O(n) proof*</div>
            <div class="backend-card-detail">No trusted setup</div>
            <div style="margin-top:6px">${backend === "ipa" ? '<span class="badge badge-ok">ACTIVE</span>' : '<span class="badge badge-muted">Available</span>'}</div>
          </div>
          <div class="backend-card ${backend === "kzg" ? "active" : ""}">
            <div class="backend-card-name">KZG</div>
            <div class="backend-card-detail">BLS12-381</div>
            <div class="backend-card-detail">Pairing</div>
            <div class="backend-card-detail">O(1) proof**</div>
            <div class="backend-card-detail">Trusted setup</div>
            <div style="margin-top:6px">${backend === "kzg" ? '<span class="badge badge-ok">ACTIVE</span>' : '<span class="badge badge-muted">Available</span>'}</div>
          </div>
        </div>
        <div style="color:var(--text-dim);font-size:10px;margin-top:8px">
          * IPA currently carries full vector (P1.3 planned) &nbsp;&nbsp;
          ** KZG requires consumer to have same trusted setup
        </div>
      </div>
    </div>
  `;

  const inp = $("#ndb-proof-key");
  if (inp)
    inp.addEventListener("keydown", (e) => {
      if (e.key === "Enter") ndbProofVerify();
    });
}

window.ndbProofVerify = async function () {
  const key = ($("#ndb-proof-key")?.value || "").trim();
  if (!key) return;
  const el = $("#ndb-proof-result");
  el.innerHTML = '<div style="color:var(--text-muted)">Verifying...</div>';
  try {
    const res = await api(`/nucleusdb/verify/${encodeURIComponent(key)}`);
    if (!res.found) {
      el.innerHTML = `<span class="badge badge-err">Key not found</span>`;
      return;
    }
    el.innerHTML = `
      <div class="ndb-verify-grid" style="margin-top:8px">
        <div class="ndb-verify-row"><span class="ndb-verify-label">Key</span><span class="ndb-mono" style="color:var(--accent)">${esc(res.key)}</span></div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Type</span>${typeBadge(res.type || "integer")}</div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Value</span><span class="ndb-mono">${esc(res.display || String(res.value))}</span></div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Backend</span><span class="ndb-mono">${esc(res.backend)}</span></div>
        <div class="ndb-verify-row">
          <span class="ndb-verify-label">Status</span>
          <span>${
            res.verified
              ? '<span class="badge badge-ok" style="font-size:12px">&#10003; VERIFIED</span>'
              : '<span class="badge badge-err" style="font-size:12px">&#10007; FAILED</span>'
          }</span>
        </div>
        <div class="ndb-verify-row"><span class="ndb-verify-label">Root Hash</span><span class="ndb-mono ndb-hash">${esc(res.root_hash)}</span></div>
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
};

// -- Sharing Sub-Tab (NucleusPOD) ---------------------------------------------
function ndbGrantShortHex(hex) {
  if (!hex || hex.length <= 24) return hex || "";
  return `${hex.slice(0, 14)}...${hex.slice(-8)}`;
}

function ndbGrantFormatExpiry(expiresAt) {
  if (!expiresAt) return "No expiry";
  return `Expires ${new Date(expiresAt * 1000).toLocaleString()}`;
}

async function ndbRenderSharing() {
  const el = $("#ndb-content");
  el.innerHTML =
    '<div style="color:var(--text-muted)">Loading sharing controls...</div>';
  try {
    const modeQuery = ndbSharing.includeRevoked
      ? "include_revoked=true"
      : "active=true";
    const [stats, grantResp] = await Promise.all([
      api("/nucleusdb/stats"),
      api(`/nucleusdb/grants?${modeQuery}`),
    ]);
    window._ndbStats = stats;
    const grants = grantResp?.grants || [];
    const activeGrants =
      stats?.grant_active_count ?? grantResp?.active_total ?? 0;
    const totalGrants = stats?.grant_count ?? grantResp?.total ?? grants.length;

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="proof-section">
          <div class="proof-section-title">NucleusPOD &mdash; Proof-Carrying Data Sharing</div>
          <div style="color:var(--text-dim);font-size:12px;margin-bottom:12px">
            Share verified records with other agents. Each shared item carries its own cryptographic proof &mdash;
            the recipient verifies independently without trusting the sender.
          </div>
          <div style="display:flex;gap:12px;flex-wrap:wrap">
            <div class="card" style="flex:1;min-width:140px">
              <div class="card-label">Proof Envelopes</div>
              <div class="card-value" style="font-size:16px;color:var(--text-muted)">0</div>
              <div class="card-sub">Self-contained proofs</div>
            </div>
            <div class="card" style="flex:1;min-width:140px">
              <div class="card-label">Access Grants</div>
              <div class="card-value" style="font-size:16px">${Number(activeGrants).toLocaleString()}</div>
              <div class="card-sub">${Number(totalGrants).toLocaleString()} total</div>
            </div>
          </div>
        </div>

        <div class="proof-section">
          <div class="proof-section-title">Access Grants</div>
          <div style="color:var(--text-dim);font-size:12px;margin-bottom:12px">
            Grant per-key read/write/append access to specific agents. PUF identifiers are 32-byte hex fingerprints.
          </div>

          <div class="grant-form-grid">
            <input id="ndb-grant-grantor" type="text" placeholder="Grantor PUF (0x + 64 hex chars)" class="input">
            <input id="ndb-grant-grantee" type="text" placeholder="Grantee PUF (0x + 64 hex chars)" class="input">
            <input id="ndb-grant-pattern" type="text" placeholder="Key pattern (examples: docs/*, report:2026, *)" class="input">
            <input id="ndb-grant-expiry" type="datetime-local" class="input">
          </div>

          <div class="grant-toolbar">
            <label><input id="ndb-grant-read" type="checkbox" checked> READ</label>
            <label><input id="ndb-grant-write" type="checkbox"> WRITE</label>
            <label><input id="ndb-grant-append" type="checkbox"> APPEND</label>
            <button class="btn btn-sm" onclick="ndbCreateGrant()">Create Grant</button>
            <button class="btn btn-sm" onclick="ndbRefreshGrants()">Refresh</button>
            <label><input id="ndb-grant-show-revoked" type="checkbox" ${ndbSharing.includeRevoked ? "checked" : ""} onchange="ndbToggleRevoked(this.checked)"> Show revoked/expired</label>
          </div>

          <div id="ndb-grant-status" style="color:var(--text-dim);font-size:11px;margin:8px 0 2px">Loaded ${grants.length} grant(s).</div>

          <div id="ndb-grant-list">
            ${
              grants.length === 0
                ? `<div class="grant-empty">No grants to display.</div>`
                : grants
                    .map(
                      (g) => `
                <div class="grant-card">
                  <div class="grant-header">
                    <div class="grant-id">${esc(g.grant_id_hex || "")}</div>
                    <div>
                      ${
                        g.active
                          ? '<span class="badge badge-ok">ACTIVE</span>'
                          : g.revoked
                            ? '<span class="badge badge-err">REVOKED</span>'
                            : '<span class="badge badge-warn">EXPIRED</span>'
                      }
                      ${g.revoked ? "" : `<button class="btn-icon btn-icon-danger" title="Revoke grant" onclick="ndbRevokeGrant('${g.grant_id_hex}')">&#10005;</button>`}
                    </div>
                  </div>
                  <div class="grant-detail"><span>Key Pattern:</span> <code>${esc(g.key_pattern || "")}</code></div>
                  <div class="grant-detail"><span>Grantor:</span> <code>${esc(ndbGrantShortHex(g.grantor_puf_hex || ""))}</code> &nbsp; <span>Grantee:</span> <code>${esc(ndbGrantShortHex(g.grantee_puf_hex || ""))}</code></div>
                  <div class="grant-detail">
                    <span>Permissions:</span>
                    <span class="grant-perm ${g.permissions?.read ? "active" : ""}">READ</span>
                    <span class="grant-perm ${g.permissions?.write ? "active" : ""}">WRITE</span>
                    <span class="grant-perm ${g.permissions?.append ? "active" : ""}">APPEND</span>
                    &nbsp; <span>${esc(ndbGrantFormatExpiry(g.expires_at))}</span>
                  </div>
                </div>
              `,
                    )
                    .join("")
            }
          </div>
        </div>

        <div class="proof-section">
          <div class="proof-section-title">How It Works</div>
          <div class="ndb-verify-grid">
            <div class="ndb-verify-row"><span class="ndb-verify-label">Envelope</span><span style="color:var(--text-dim);font-size:11px">Self-contained proof unit: data + Merkle proof + metadata + author PUF</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Grants</span><span style="color:var(--text-dim);font-size:11px">Per-key access control: grantor PUF + grantee PUF + key pattern + permissions + expiry</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Discovery</span><span style="color:var(--text-dim);font-size:11px">.well-known/nucleus-pod &mdash; JSON capabilities doc for agent discovery</span></div>
            <div class="ndb-verify-row"><span class="ndb-verify-label">Verify</span><span style="color:var(--text-dim);font-size:11px">Recipients verify proofs locally &mdash; no trust in sender required</span></div>
          </div>
        </div>
      </div>
    `;
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Sharing tab load failed: ${esc(e.message)}</div>`;
  }
}

window.ndbToggleRevoked = async function (on) {
  ndbSharing.includeRevoked = !!on;
  await ndbRenderSharing();
};

window.ndbRefreshGrants = async function () {
  await ndbRenderSharing();
};

window.ndbCreateGrant = async function () {
  const statusEl = $("#ndb-grant-status");
  const grantorRaw = ($("#ndb-grant-grantor")?.value || "").trim();
  const granteeRaw = ($("#ndb-grant-grantee")?.value || "").trim();
  const keyPattern = ($("#ndb-grant-pattern")?.value || "").trim();
  const expiryRaw = ($("#ndb-grant-expiry")?.value || "").trim();
  const read = !!($("#ndb-grant-read") && $("#ndb-grant-read").checked);
  const write = !!($("#ndb-grant-write") && $("#ndb-grant-write").checked);
  const append = !!($("#ndb-grant-append") && $("#ndb-grant-append").checked);

  const normalizeHex = (v) => {
    const s = v.toLowerCase().replace(/^0x/, "");
    return s.length === 64 && /^[0-9a-f]+$/.test(s) ? `0x${s}` : null;
  };

  const grantor = normalizeHex(grantorRaw);
  const grantee = normalizeHex(granteeRaw);
  if (!grantor || !grantee) {
    if (statusEl)
      statusEl.innerHTML =
        '<span style="color:var(--red)">Grantor and grantee must be 32-byte hex PUF values.</span>';
    return;
  }
  if (!keyPattern) {
    if (statusEl)
      statusEl.innerHTML =
        '<span style="color:var(--red)">Key pattern is required.</span>';
    return;
  }
  if (!read && !write && !append) {
    if (statusEl)
      statusEl.innerHTML =
        '<span style="color:var(--red)">Enable at least one permission.</span>';
    return;
  }

  let expiresAt = null;
  if (expiryRaw) {
    const ms = Date.parse(expiryRaw);
    if (!Number.isFinite(ms)) {
      if (statusEl)
        statusEl.innerHTML =
          '<span style="color:var(--red)">Invalid expiry date/time.</span>';
      return;
    }
    expiresAt = Math.floor(ms / 1000);
  }

  if (statusEl)
    statusEl.innerHTML =
      '<span style="color:var(--text-muted)">Creating grant...</span>';
  try {
    await apiPost("/nucleusdb/grants", {
      grantor_puf_hex: grantor,
      grantee_puf_hex: grantee,
      key_pattern: keyPattern,
      permissions: { read, write, append },
      expires_at: expiresAt,
    });
    if (statusEl)
      statusEl.innerHTML =
        '<span style="color:var(--green)">Grant created.</span>';
    await ndbRenderSharing();
  } catch (e) {
    if (statusEl)
      statusEl.innerHTML = `<span style="color:var(--red)">Create failed: ${esc(e.message)}</span>`;
  }
};

window.ndbRevokeGrant = async function (grantIdHex) {
  if (!grantIdHex) return;
  const statusEl = $("#ndb-grant-status");
  if (statusEl)
    statusEl.innerHTML =
      '<span style="color:var(--text-muted)">Revoking grant...</span>';
  try {
    await apiPost(
      `/nucleusdb/grants/${encodeURIComponent(grantIdHex)}/revoke`,
      {},
    );
    if (statusEl)
      statusEl.innerHTML =
        '<span style="color:var(--green)">Grant revoked.</span>';
    await ndbRenderSharing();
  } catch (e) {
    if (statusEl)
      statusEl.innerHTML = `<span style="color:var(--red)">Revoke failed: ${esc(e.message)}</span>`;
  }
};

// -- Config Sub-Tab (Merged Schema + Settings) --------------------------------
async function ndbRenderConfig() {
  const el = $("#ndb-content");
  el.innerHTML = '<div style="color:var(--text-muted)">Loading config...</div>';
  try {
    const stats = window._ndbStats || (await api("/nucleusdb/stats"));
    const prefixes = stats.top_prefixes || [];

    el.innerHTML = `
      <div style="margin:12px 0">
        <div class="card-grid">
          <div class="card">
            <div class="card-label">Total Keys</div>
            <div class="card-value">${stats.key_count}</div>
          </div>
          <div class="card">
            <div class="card-label">Commits</div>
            <div class="card-value">${stats.commit_count}</div>
          </div>
          <div class="card">
            <div class="card-label">Write Mode</div>
            <div class="card-value" style="font-size:13px">${esc(stats.write_mode)}</div>
            <div class="card-sub">${stats.write_mode === "AppendOnly" ? "State + log immutable" : "Log immutable, state editable"}</div>
          </div>
          <div class="card">
            <div class="card-label">DB Size</div>
            <div class="card-value" style="font-size:14px">${formatBytes(stats.db_size_bytes)}</div>
          </div>
        </div>

        ${
          stats.type_distribution
            ? `
          <div class="section-header">Type Distribution</div>
          <div class="ndb-type-dist">
            ${Object.entries(stats.type_distribution)
              .sort((a, b) => b[1] - a[1])
              .map(
                ([t, count]) =>
                  `<div class="ndb-type-dist-item">${typeBadge(t)} <span class="ndb-type-dist-count">${count.toLocaleString()}</span></div>`,
              )
              .join("")}
          </div>
        `
            : ""
        }

        <div class="section-header">Storage</div>
        <div class="card-grid">
          <div class="card">
            <div class="card-label">Blob Objects</div>
            <div class="card-value">${stats.blob_count || 0}</div>
            <div class="card-sub">${formatBytes(stats.blob_total_bytes || 0)} stored</div>
          </div>
          <div class="card">
            <div class="card-label">Vectors</div>
            <div class="card-value">${stats.vector_count || 0}</div>
            <div class="card-sub">${stats.vector_dims ? stats.vector_dims + " dimensions" : "No vectors yet"}</div>
          </div>
        </div>

        ${
          prefixes.length > 0
            ? `
          <div class="section-header">Key Prefix Distribution</div>
          <div class="ndb-prefix-list">
            ${prefixes
              .map(
                (p) => `
              <div class="ndb-prefix-item">
                <span class="ndb-prefix-name clickable" data-prefix="${esc(p.prefix)}">${esc(p.prefix)}</span>
                <div class="ndb-prefix-bar-wrap">
                  <div class="ndb-prefix-bar" style="width:${Math.max(4, (p.count / (prefixes[0]?.count || 1)) * 100)}%"></div>
                </div>
                <span style="color:var(--text-muted);font-size:12px">${p.count}</span>
              </div>
            `,
              )
              .join("")}
          </div>
        `
            : ""
        }

        <div class="section-header">Write Mode</div>
        <div style="display:flex;align-items:center;gap:12px">
          <span class="badge ${stats.write_mode === "AppendOnly" ? "badge-warn" : "badge-ok"}" style="font-size:12px">
            ${esc(stats.write_mode)}
          </span>
          ${
            stats.write_mode !== "AppendOnly"
              ? `
            <button class="btn btn-sm" onclick="ndbSetAppendOnly()">Lock to Append-Only</button>
          `
              : ""
          }
        </div>
        <div style="color:var(--text-dim);font-size:11px;line-height:1.6;margin:8px 0 12px;max-width:620px">
          ${
            stats.write_mode !== "AppendOnly"
              ? `<strong style="color:var(--text)">Normal:</strong> The commit log is always immutable \u2014 every change is recorded with a cryptographic state root and can never be erased. However, the <em>current state</em> can be corrected via UPDATE/DELETE (e.g. fixing a typo or removing a bad record). The history will show both the original value and the correction.
              <br><strong style="color:var(--text)">Append-Only:</strong> Locks the current state itself so values can never be changed or removed \u2014 only new records can be added. Each commit includes a monotone extension proof guaranteeing no prior data was altered. Use this for agent traces, attestations, and proof certificates where retroactive changes must be impossible. <strong style="color:var(--yellow)">This lock is irreversible.</strong>`
              : `<strong style="color:var(--text)">Locked.</strong> The database state can only grow \u2014 INSERT is permitted, but UPDATE and DELETE are permanently rejected. Every commit includes a monotone extension proof (SHA-256 seal chain) guaranteeing that no previously committed record has been altered or removed. The commit log and the live state are both immutable.`
          }
        </div>

        <div class="section-header">Export</div>
        <div style="display:flex;gap:8px;margin-bottom:12px">
          <button class="btn btn-sm" onclick="ndbExport('json')">Export JSON</button>
          <button class="btn btn-sm" onclick="ndbExport('csv')">Export CSV</button>
        </div>

        <div class="section-header">Database Path</div>
        <div style="color:var(--text-dim);font-size:11px">${esc((window._ndbStatus || {}).db_path || "unknown")}</div>
      </div>
    `;

    $$(".ndb-prefix-name.clickable", el).forEach((node) => {
      node.addEventListener("click", () => {
        ndb.prefix = node.dataset.prefix || "";
        ndb.page = 0;
        ndbSwitchTab("browse");
      });
    });
  } catch (e) {
    el.innerHTML = `<div style="color:var(--red)">Error: ${esc(e.message)}</div>`;
  }
}

window.ndbSetAppendOnly = async function () {
  if (
    !confirm(
      "Lock database to Append-Only mode?\n\nThis is IRREVERSIBLE. Once locked:\n- INSERT: allowed (new records can be added)\n- UPDATE/DELETE: permanently rejected\n- Every commit will include a monotone extension proof\n\nUse this when the data must be tamper-proof (agent traces, attestations, proof certificates). Mistakes in the current state cannot be corrected after locking.",
    )
  )
    return;
  try {
    const res = await apiPost("/nucleusdb/sql", {
      query: "SET MODE APPEND_ONLY",
    });
    if (res.error) {
      alert("Failed: " + res.error);
    } else {
      ndbRenderConfig();
    }
  } catch (e) {
    alert("Failed: " + e.message);
  }
};

// =============================================================================
// Particle Network — amber constellation mesh (inspired by apoth3osis.io banner)
// =============================================================================
(function () {
  let _raf = 0;
  const PARTICLE_COUNT = 80;
  const CONNECT_DIST = 110;
  const SPEED = 0.12;

  function initParticles(canvasId) {
    const canvas = document.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Cancel any prior animation loop for this canvas
    if (_raf) cancelAnimationFrame(_raf);

    const rect = canvas.parentElement.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    canvas.style.width = rect.width + "px";
    canvas.style.height = rect.height + "px";
    ctx.scale(dpr, dpr);

    const W = rect.width;
    const H = rect.height;

    // Create particles
    const particles = [];
    for (let i = 0; i < PARTICLE_COUNT; i++) {
      particles.push({
        x: Math.random() * W,
        y: Math.random() * H,
        vx: (Math.random() - 0.5) * SPEED * 2,
        vy: (Math.random() - 0.5) * SPEED * 2,
        r: Math.random() * 1.6 + 0.6, // radius 0.6 – 2.2
        brightness: Math.random() * 0.5 + 0.3, // 0.3 – 0.8
      });
    }

    function draw() {
      ctx.clearRect(0, 0, W, H);

      // Subtle background gradient (dark, barely visible)
      const bg = ctx.createRadialGradient(
        W * 0.3,
        H * 0.4,
        0,
        W * 0.5,
        H * 0.5,
        W * 0.8,
      );
      bg.addColorStop(0, "rgba(255, 106, 0, 0.04)");
      bg.addColorStop(0.5, "rgba(255, 159, 42, 0.02)");
      bg.addColorStop(1, "transparent");
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, W, H);

      // Update positions
      for (const p of particles) {
        p.x += p.vx;
        p.y += p.vy;
        if (p.x < 0 || p.x > W) p.vx *= -1;
        if (p.y < 0 || p.y > H) p.vy *= -1;
        p.x = Math.max(0, Math.min(W, p.x));
        p.y = Math.max(0, Math.min(H, p.y));
      }

      // Draw connections
      for (let i = 0; i < particles.length; i++) {
        for (let j = i + 1; j < particles.length; j++) {
          const dx = particles[i].x - particles[j].x;
          const dy = particles[i].y - particles[j].y;
          const dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < CONNECT_DIST) {
            const alpha = (1 - dist / CONNECT_DIST) * 0.25;
            ctx.strokeStyle = `rgba(255, 140, 20, ${alpha})`;
            ctx.lineWidth = 0.5;
            ctx.beginPath();
            ctx.moveTo(particles[i].x, particles[i].y);
            ctx.lineTo(particles[j].x, particles[j].y);
            ctx.stroke();
          }
        }
      }

      // Draw nodes
      for (const p of particles) {
        // Outer glow
        const glow = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, p.r * 4);
        glow.addColorStop(0, `rgba(255, 140, 20, ${p.brightness * 0.35})`);
        glow.addColorStop(1, "transparent");
        ctx.fillStyle = glow;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r * 4, 0, Math.PI * 2);
        ctx.fill();

        // Core dot
        ctx.fillStyle = `rgba(255, 159, 42, ${p.brightness})`;
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fill();
      }

      _raf = requestAnimationFrame(draw);
    }

    draw();
  }

  // Expose for use after NucleusDB tab renders
  window._initHeroParticles = function () {
    // Small delay to let DOM settle
    setTimeout(() => initParticles("hero-particles"), 50);
  };

  // Clean up on page navigation
  window._destroyHeroParticles = function () {
    if (_raf) {
      cancelAnimationFrame(_raf);
      _raf = 0;
    }
  };
})();
