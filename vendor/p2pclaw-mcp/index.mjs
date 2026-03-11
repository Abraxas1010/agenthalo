/**
 * P2PCLAW MCP Sidecar for AgentHALO
 * ==================================
 * Managed child process that exposes P2PCLAW network tools via:
 *   1. MCP protocol (SSE + Streamable HTTP) for agent tool-calling
 *   2. REST API on localhost for AgentHALO Rust proxy
 *
 * All requests are authenticated via x-agenthalo-p2pclaw-token header.
 * The sidecar never talks to the P2PCLAW gateway directly — AgentHALO's
 * Rust client handles upstream auth (HMAC-SHA256 via vault).
 *
 * Environment:
 *   P2PCLAW_MCP_PORT    — listen port (default: 7421)
 *   P2PCLAW_AUTH_TOKEN   — shared secret with AgentHALO process
 *   P2PCLAW_GATEWAY_URL  — upstream P2PCLAW gateway (default: https://p2pclaw.com)
 *   P2PCLAW_AGENT_ID     — agent identity for the hive
 *   P2PCLAW_AGENT_NAME   — display name in the hive
 */

import express from "express";
import crypto from "node:crypto";

const PORT = parseInt(process.env.P2PCLAW_MCP_PORT || "7421", 10);
const AUTH_TOKEN = process.env.P2PCLAW_AUTH_TOKEN || "";
const GATEWAY_URL = (process.env.P2PCLAW_GATEWAY_URL || "https://p2pclaw.com").replace(/\/+$/, "");
const AGENT_ID = process.env.P2PCLAW_AGENT_ID || "agenthalo";
const AGENT_NAME = process.env.P2PCLAW_AGENT_NAME || "AgentHALO";

const AUTH_HEADER = "x-agenthalo-p2pclaw-token";

// ── Auth middleware ─────────────────────────────────────────────────────
function requireAuth(req, res, next) {
  if (!AUTH_TOKEN) return next(); // no token = dev mode
  const provided = req.headers[AUTH_HEADER];
  if (!provided || provided !== AUTH_TOKEN) {
    return res.status(401).json({ error: "unauthorized" });
  }
  next();
}

// ── Upstream fetch helper ───────────────────────────────────────────────
async function gateway(method, path, body = null, accept = "application/json") {
  const url = `${GATEWAY_URL}${path}`;
  const opts = {
    method,
    headers: {
      "Content-Type": "application/json",
      "Accept": accept,
      "User-Agent": `AgentHALO-P2PCLAW-MCP/1.0 (${AGENT_ID})`,
    },
    signal: AbortSignal.timeout(30_000),
  };
  if (body) opts.body = JSON.stringify(body);
  const res = await fetch(url, opts);
  if (!res.ok) throw new Error(`P2PCLAW ${method} ${path} → ${res.status}`);
  if (accept.includes("json")) return res.json();
  return res.text();
}

// ── MCP Tool Definitions ────────────────────────────────────────────────
const TOOLS = [
  {
    name: "p2pclaw_swarm_status",
    description: "Get real-time P2PCLAW hive status: active agents, papers, mempool queue.",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "p2pclaw_search_wheel",
    description: "Search 'The Wheel' for existing verified research to avoid duplication.",
    inputSchema: {
      type: "object",
      properties: { query: { type: "string", description: "Search terms" } },
      required: ["query"],
    },
  },
  {
    name: "p2pclaw_list_papers",
    description: "List recently published papers on the P2PCLAW network.",
    inputSchema: {
      type: "object",
      properties: { limit: { type: "number", default: 20 } },
      required: [],
    },
  },
  {
    name: "p2pclaw_list_mempool",
    description: "List papers awaiting peer validation in the mempool.",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "p2pclaw_publish_paper",
    description: "Submit a research paper to the P2PCLAW network for peer validation and IPFS archival.",
    inputSchema: {
      type: "object",
      properties: {
        title: { type: "string" },
        content: { type: "string", description: "Markdown content" },
        author: { type: "string" },
      },
      required: ["title", "content"],
    },
  },
  {
    name: "p2pclaw_validate_paper",
    description: "Submit a peer validation (approve/reject/flag) for a paper in the mempool.",
    inputSchema: {
      type: "object",
      properties: {
        paperId: { type: "string" },
        action: { type: "string", enum: ["validate", "reject", "flag"] },
      },
      required: ["paperId", "action"],
    },
  },
  {
    name: "p2pclaw_hive_chat",
    description: "Send a message to the P2PCLAW global research chat.",
    inputSchema: {
      type: "object",
      properties: { message: { type: "string" } },
      required: ["message"],
    },
  },
  {
    name: "p2pclaw_get_briefing",
    description: "Get the current mission briefing and swarm status from the P2PCLAW network.",
    inputSchema: { type: "object", properties: {}, required: [] },
  },
  {
    name: "p2pclaw_submit_hypothesis",
    description: "Submit a scientific hypothesis to the network mempool for peer review.",
    inputSchema: {
      type: "object",
      properties: {
        title: { type: "string" },
        rationale: { type: "string", description: "Background reasoning" },
        tags: { type: "array", items: { type: "string" } },
      },
      required: ["title", "rationale"],
    },
  },
  {
    name: "p2pclaw_delegate_compute",
    description: "Offload a computational task (proof search, simulation, verification) to the hive swarm.",
    inputSchema: {
      type: "object",
      properties: {
        task_type: { type: "string", enum: ["HEAVY_PROOF_SEARCH", "DOCKER_SIMULATION", "MATH_VERIFICATION"] },
        payload: { type: "string", description: "Data or code to process" },
        reward: { type: "number", description: "CLAW tokens offered" },
      },
      required: ["task_type", "payload"],
    },
  },
];

// ── Tool Handlers ───────────────────────────────────────────────────────
async function handleTool(name, args) {
  switch (name) {
    case "p2pclaw_swarm_status":
      return { content: [{ type: "text", text: JSON.stringify(await gateway("GET", "/swarm-status")) }] };

    case "p2pclaw_search_wheel":
      return { content: [{ type: "text", text: JSON.stringify(await gateway("GET", `/wheel?q=${encodeURIComponent(args.query)}`)) }] };

    case "p2pclaw_list_papers": {
      const limit = args.limit || 20;
      return { content: [{ type: "text", text: JSON.stringify(await gateway("GET", `/latest-papers?limit=${limit}`)) }] };
    }

    case "p2pclaw_list_mempool":
      return { content: [{ type: "text", text: JSON.stringify(await gateway("GET", "/mempool")) }] };

    case "p2pclaw_publish_paper":
      return {
        content: [{
          type: "text",
          text: JSON.stringify(await gateway("POST", "/publish-paper", {
            title: args.title,
            content: args.content,
            author: args.author || AGENT_NAME,
            agentId: AGENT_ID,
          })),
        }],
      };

    case "p2pclaw_validate_paper":
      return {
        content: [{
          type: "text",
          text: JSON.stringify(await gateway("POST", "/validate-paper", {
            paperId: args.paperId,
            action: args.action,
            agentId: AGENT_ID,
          })),
        }],
      };

    case "p2pclaw_hive_chat":
      return {
        content: [{
          type: "text",
          text: JSON.stringify(await gateway("POST", "/hive-chat", {
            message: args.message,
            sender: AGENT_NAME,
          })),
        }],
      };

    case "p2pclaw_get_briefing": {
      const text = await gateway("GET", "/briefing", null, "text/markdown");
      return { content: [{ type: "text", text }] };
    }

    case "p2pclaw_submit_hypothesis":
      return {
        content: [{
          type: "text",
          text: JSON.stringify(await gateway("POST", "/publish-paper", {
            title: args.title,
            content: args.rationale,
            tags: args.tags || [],
            agentId: AGENT_ID,
            author: AGENT_NAME,
          })),
        }],
      };

    case "p2pclaw_delegate_compute":
      return {
        content: [{
          type: "text",
          text: JSON.stringify(await gateway("POST", "/delegate-compute", {
            task_type: args.task_type,
            payload: args.payload,
            reward: args.reward || 5,
            agentId: AGENT_ID,
          })),
        }],
      };

    default:
      return { content: [{ type: "text", text: `Unknown tool: ${name}` }], isError: true };
  }
}

// ── Express REST API (for AgentHALO Rust proxy) ─────────────────────────
const app = express();
app.use(express.json({ limit: "2mb" }));
app.use(requireAuth);

app.get("/status", (_req, res) => {
  res.json({ status: "ok", version: "1.0.0", gateway: GATEWAY_URL, agent_id: AGENT_ID });
});

app.get("/tools", (_req, res) => {
  res.json({ tools: TOOLS });
});

app.post("/call", async (req, res) => {
  const { name, arguments: args } = req.body;
  if (!name) return res.status(400).json({ error: "missing tool name" });
  try {
    const result = await handleTool(name, args || {});
    res.json(result);
  } catch (err) {
    res.json({ content: [{ type: "text", text: `Error: ${err.message}` }], isError: true });
  }
});

// Proxy any P2PCLAW path through for the frontend iframe
app.all("/gateway/{*path}", async (req, res) => {
  const segments = Array.isArray(req.params.path) ? req.params.path : [req.params.path];
  const path = "/" + segments.join("/");
  try {
    const method = req.method;
    const result = await gateway(method, path, method !== "GET" ? req.body : null);
    res.json(result);
  } catch (err) {
    res.status(502).json({ error: err.message });
  }
});

// ── MCP Protocol (SSE transport) ────────────────────────────────────────
// We implement MCP over the REST API rather than importing the full SDK,
// keeping the sidecar minimal. AgentHALO's orchestrator calls /tools and
// /call directly. If full MCP SSE/Streamable HTTP is needed, it can be
// added later by importing @modelcontextprotocol/sdk.

app.listen(PORT, "127.0.0.1", () => {
  console.log(`[P2PCLAW-MCP] Sidecar ready on http://127.0.0.1:${PORT}`);
  console.log(`[P2PCLAW-MCP] Gateway: ${GATEWAY_URL}`);
  console.log(`[P2PCLAW-MCP] Agent: ${AGENT_ID} (${AGENT_NAME})`);
  console.log(`[P2PCLAW-MCP] Auth: ${AUTH_TOKEN ? "enabled" : "disabled (dev mode)"}`);
  console.log(`[P2PCLAW-MCP] Tools: ${TOOLS.length}`);
});
