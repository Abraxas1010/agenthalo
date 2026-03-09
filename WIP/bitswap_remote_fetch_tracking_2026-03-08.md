## Bitswap Remote Fetch Tracking

Status: deferred after Phase 8 hardening

Why deferred:
- `swarm_fetch` currently runs inside `NucleusDbMcpService`, which owns persisted swarm state but not a live `P2pNode`.
- A real outbound Bitswap fetch needs a running request-response loop, peer discovery/bootstrap state, and a synchronization boundary between the MCP tool thread and the live libp2p swarm.
- Threading that handle through the current MCP startup path is a larger control-plane change than the security hardening pass.

What is already done:
- `swarm_fetch` is now explicitly documented as local-only.
- `swarm_remote_fetch` exists as an MCP stub so callers do not infer remote behavior from `swarm_fetch`.
- Bitswap receive-side hardening is in place: frame size cap, block hash verification, and optional grant-required mode.

Required implementation steps:
1. Extend MCP service state to accept an injected live `Arc<tokio::sync::Mutex<P2pNode>>` or equivalent command handle.
2. Add a `P2pNode` API for outbound Bitswap `Want` and `Block` exchange with timeout-bounded response collection.
3. Decide the peer source for remote fetch:
   - explicit peer list in request, or
   - bootstrap/discovery registry fan-out
4. Persist successfully fetched chunks back into `ChunkStore` and WAL before reassembly.
5. Add a two-node integration test proving:
   - node A publishes
   - node B lacks local chunks
   - node B remote-fetches over Bitswap
   - manifest reassembles and verifies
6. Add the grant-enforcement integration variant proving denial without a matching grant when `HALO_BITSWAP_REQUIRE_GRANTS=1`.

Acceptance target:
- `swarm_remote_fetch` replaces the stub and becomes the explicit remote orchestration tool.
- `swarm_fetch` remains the deterministic local reassembly path.
