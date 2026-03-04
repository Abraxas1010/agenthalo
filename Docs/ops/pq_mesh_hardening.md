# PQ Mesh Hardening Audit Report

**Date:** 2026-03-04
**Scope:** WP-4 — P2P mesh audit for DIDComm bypass paths
**Prerequisite:** WP-1/WP-2 (hybrid KEM) committed at b2c4fd0, WP-3 (SHA-512) committed at 9f8d386

## Audit Scope

Audited `src/halo/p2p_node.rs`, `src/halo/p2p_discovery.rs`, `src/halo/startup.rs`,
and all callers of gossipsub/Kademlia publish functions to determine whether any
agent-to-agent payload path bypasses DIDComm encryption.

## Architecture Summary

```
                    ┌─────────────────────────┐
                    │   libp2p Swarm           │
                    │   Noise XX (X25519)      │  ← Transport encryption (classical)
                    │   Yamux multiplexing     │
                    ├─────────┬────────────────┤
                    │ Gossipsub│  Kademlia DHT  │  ← Discovery layer
                    │ (pubsub) │  (KV store)    │
                    └────┬─────┴───────┬────────┘
                         │             │
              AgentAnnouncement  AgentAnnouncement
              (signed JSON)      (signed JSON)
                         │             │
                    ┌────┴─────────────┴────────┐
                    │   AgentDiscovery           │  ← Signature verification gate
                    │   verify_and_upsert()      │     (Ed25519 + ML-DSA-65 dual)
                    └───────────────────────────┘

    Separate path (agent-to-agent payloads):

    ┌───────────┐    ┌──────────────┐    ┌─────────────────┐
    │ Nym Mixnet │───→│ DIDCommHandler│───→│ Hybrid KEM      │
    │ Inbound    │    │ handle_incoming│   │ X25519+ML-KEM-768│
    └───────────┘    └──────────────┘    └─────────────────┘

    ┌───────────┐    ┌──────────────┐    ┌─────────────────┐
    │ A2A Bridge │───→│ DIDCommHandler│───→│ Hybrid KEM      │
    │ HTTP       │    │ handle_incoming│   │ X25519+ML-KEM-768│
    └───────────┘    └──────────────┘    └─────────────────┘
```

## Message Path Analysis

### Path 1: Gossipsub (Discovery Announcements)

- **Source:** `AgentDiscovery::announce()` → `gossipsub.publish()`
- **Payload:** Serialized `AgentAnnouncement` JSON (DID, capabilities, listen addresses, peer ID)
- **Protection:** Ed25519 + ML-DSA-65 dual signature (integrity + PQ authenticity)
- **Confidentiality:** None (public discovery metadata)
- **DIDComm bypass:** N/A — not confidential data
- **PQ risk:** LOW — attacker gains agent metadata (already semi-public by design)

### Path 2: Kademlia DHT (Discovery Records)

- **Source:** `AgentDiscovery::publish_to_dht()` → `kademlia.put_record()`
- **Payload:** Same `AgentAnnouncement` JSON as gossipsub
- **Protection:** Ed25519 + ML-DSA-65 dual signature
- **Confidentiality:** None (public DHT records)
- **DIDComm bypass:** N/A — not confidential data
- **PQ risk:** LOW — same as gossipsub

### Path 3: Nym Mixnet Inbound

- **Source:** `startup.rs` lines 128-172, Nym subscription → base64 decode → `DIDCommHandler::handle_incoming()`
- **Payload:** DIDComm-packed agent messages
- **Protection:** DIDComm authcrypt/anoncrypt with hybrid KEM (X25519 + ML-KEM-768)
- **DIDComm bypass:** NO — all mixnet payloads go through DIDCommHandler
- **PQ risk:** NONE after WP-1/WP-2

### Path 4: A2A Bridge (HTTP)

- **Source:** `a2a_bridge.rs` — HTTP endpoint → `DIDCommHandler::handle_incoming()`
- **Payload:** DIDComm-packed agent messages
- **Protection:** DIDComm authcrypt/anoncrypt with hybrid KEM
- **DIDComm bypass:** NO — all A2A payloads go through DIDCommHandler
- **PQ risk:** NONE after WP-1/WP-2

## Noise XX Transport Assessment

The libp2p transport uses Noise XX with X25519 key exchange (`noise::Config::new`
on lines 182 and 186 of `p2p_node.rs`). This is **classical-only** and vulnerable
to quantum key recovery.

**Impact if Noise XX is broken by quantum computer:**
- Attacker can decrypt transport-layer frames
- Exposed content: signed `AgentAnnouncement` JSON only (discovery metadata)
- Agent-to-agent DIDComm payloads are **NOT exposed** (encrypted at application layer with hybrid KEM)
- Discovery metadata (DIDs, capabilities, listen addresses) is semi-public by design

**Recommendation:** Noise XX upgrade to PQ is **not urgent** because:
1. No confidential data traverses gossipsub/Kademlia
2. DIDComm application-layer hybrid KEM protects all sensitive payloads
3. libp2p does not yet support PQ Noise variants (upstream dependency)
4. When libp2p adds PQ Noise (e.g., Kyber-Noise), migration is a config change

## `P2pNode::publish()` Audit

`P2pNode::publish(&mut self, topic: &str, payload: Vec<u8>)` is a public method
that allows arbitrary bytes to be published to any gossipsub topic.

**Finding:** This method has **zero external callers**. All gossipsub publishing
goes through `AgentDiscovery::announce()`, which:
1. Serializes an `AgentAnnouncement` struct (not arbitrary bytes)
2. Requires Ed25519 + ML-DSA-65 dual signature
3. Recipients verify signatures via `verify_and_upsert()`

**Recommendation:** Consider restricting `publish()` visibility to `pub(crate)` to
prevent future misuse, or document the requirement that all gossipsub payloads
must be signed `AgentAnnouncement` structs.

## Conclusion

**No DIDComm bypass exists.** All confidential agent-to-agent communication is
protected by hybrid KEM (X25519 + ML-KEM-768) at the DIDComm application layer.
The P2P transport layer (Noise XX) carries only signed discovery metadata and
does not require PQ upgrade at this time.

| Criterion | Status |
|-----------|--------|
| No raw payload sent via gossipsub | PASS |
| No raw payload sent via Kademlia | PASS |
| All DIDComm paths use hybrid KEM | PASS |
| Discovery announcements are dual-signed | PASS |
| P2pNode::publish() has no external callers | PASS |
| Noise XX PQ upgrade needed now | NO (deferred) |
