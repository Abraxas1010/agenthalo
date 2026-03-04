# PQ Assessment: Nym Mixnet Transport

**Date:** 2026-03-04
**Scope:** WP-5 — Post-quantum readiness of Nym integration

## Current Architecture

AgentHALO integrates with the Nym mixnet via two mechanisms:

1. **SOCKS5 proxy** (`nym.rs`) — Outbound traffic routed through Nym SOCKS5 client
   (external or locally managed). Provides network-layer anonymity.
2. **Native mixnet transport** (`nym_native.rs`) — Direct Sphinx packet
   construction for mixnet message delivery. Supports inbound subscription,
   SURB-based replies, and cover traffic.

## PQ Exposure Analysis

### Sphinx Packet Encryption (Nym protocol layer)

Nym's Sphinx packets use a series of ECDH key exchanges (X25519) with each mix
node to construct layered encryption. This is **classical-only** and vulnerable
to quantum key recovery.

**Impact:** A quantum attacker observing mixnet traffic could strip all mix layers
and deanonymize senders/recipients. This breaks Nym's anonymity guarantee.

**AgentHALO impact:** The quantum attacker learns which agents communicate with
which other agents (traffic analysis). However, message **content** remains
protected by DIDComm hybrid KEM (X25519 + ML-KEM-768).

### SURB Replies

Single-Use Reply Blocks also rely on X25519 ECDH. Same quantum vulnerability
as forward Sphinx packets.

### Cover Traffic

Cover traffic generation (`nym_native.rs`) uses the same Sphinx construction.
No additional PQ concern beyond the base protocol.

## Threat Assessment

| Component | Quantum Vulnerable | Impact if Broken | Urgency |
|-----------|-------------------|------------------|---------|
| Sphinx ECDH | YES (X25519) | Traffic analysis (who talks to whom) | MEDIUM |
| Message content | NO (DIDComm hybrid KEM) | N/A — protected | NONE |
| SURB replies | YES (X25519) | Reply linkability | MEDIUM |
| Cover traffic | YES (X25519) | Cover stripped | LOW |

## Mitigation Status

- **Message confidentiality:** PROTECTED by DIDComm hybrid KEM (WP-1/WP-2)
- **Traffic anonymity:** UNPROTECTED — depends on upstream Nym protocol upgrade
- **AgentHALO can't fix this unilaterally** — Sphinx PQ upgrade requires
  coordinated changes across all Nym mix nodes and clients

## Recommendations

1. **No code changes required in AgentHALO.** The Nym integration is a transport
   adapter; when Nym upgrades to PQ Sphinx, AgentHALO inherits the protection
   automatically via SDK update.
2. **Monitor Nym PQ roadmap.** The Nym team has acknowledged PQ Sphinx as a
   future milestone. Track progress at https://nymtech.net.
3. **Accept residual risk.** Traffic analysis (metadata) is exposed to quantum
   adversaries, but message content is not. For HNDL (harvest now, decrypt later)
   scenarios, the attacker learns communication patterns but not message bodies.
4. **Optional defense-in-depth:** If traffic analysis is unacceptable before Nym
   upgrades, consider running DIDComm over Tor (which has a PQ migration path
   via Kyber) as an alternative anonymity transport.

## Conclusion

Nym transport anonymity is quantum-vulnerable but **message content is not**.
No AgentHALO code changes are needed. This is an upstream dependency issue.
