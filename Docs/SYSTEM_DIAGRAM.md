# Agent H.A.L.O. / NucleusDB — System Architecture Diagrams

> Auto-rendered by GitHub. For local rendering: `npx @mermaid-js/mermaid-cli -i Docs/SYSTEM_DIAGRAM.md`

---

## 1. Top-Level System Context (C4 Level 1)

Who uses the system, and what external systems does it touch.

```mermaid
graph TB
    subgraph Users
        OP((Operator))
        DEV((Developer))
    end

    subgraph External
        OR[OpenRouter API]
        OL[Ollama Backend]
        VL[vLLM Backend]
        BC[Base L2 Chain]
        NYM[Nym Mixnet]
        IPFS[IPFS / Bitswap]
        CT[CT Log Server]
    end

    subgraph "Agent H.A.L.O. / NucleusDB"
        HALO[HALO Platform]
    end

    OP -- "Dashboard / CLI" --> HALO
    DEV -- "MCP tools / TUI" --> HALO
    HALO -- "LLM proxy" --> OR
    HALO -- "Local inference" --> OL
    HALO -- "Local inference" --> VL
    HALO -- "On-chain attestation" --> BC
    HALO -- "Anonymous routing" --> NYM
    HALO -- "Content distribution" --> IPFS
    HALO -- "Certificate transparency" --> CT

    style HALO fill:#0d1117,stroke:#58a6ff,stroke-width:3px,color:#c9d1d9
    style OP fill:#1f6feb,stroke:#58a6ff,color:#fff
    style DEV fill:#1f6feb,stroke:#58a6ff,color:#fff
```

---

## 2. Binary Targets & Entry Points

The six executables and how they relate.

```mermaid
graph LR
    subgraph "Binary Targets"
        NDB["nucleusdb<br/><i>CLI REPL</i>"]
        SRV["nucleusdb-server<br/><i>Multi-tenant HTTP</i>"]
        TUI["nucleusdb-tui<br/><i>Terminal UI</i>"]
        NMCP["nucleusdb-mcp<br/><i>MCP Tool Server</i>"]
        AH["agenthalo<br/><i>HALO CLI</i>"]
        AHMCP["agenthalo-mcp-server<br/><i>HALO MCP Server</i>"]
    end

    subgraph "Shared Library (src/lib.rs)"
        LIB[nucleusdb crate]
    end

    NDB --> LIB
    SRV --> LIB
    TUI --> LIB
    NMCP --> LIB
    AH --> LIB
    AHMCP --> LIB

    style LIB fill:#161b22,stroke:#f78166,stroke-width:2px,color:#c9d1d9
```

---

## 3. High-Level Module Architecture (C4 Level 2)

Major subsystems and their data flow.

```mermaid
graph TB
    subgraph CLI["CLI / Entry Points"]
        AHCLI[agenthalo CLI]
        NDBCLI[nucleusdb CLI]
    end

    subgraph Dashboard["Dashboard (axum)"]
        DAPI[API Handlers<br/><code>dashboard/api.rs</code>]
        DUI[Frontend SPA<br/><code>dashboard/*.js</code>]
        DASSETS[rust-embed Assets]
    end

    subgraph MCP["MCP Servers"]
        MCPT[NucleusDB MCP<br/><code>mcp/tools.rs</code>]
        MCPS[MCP Server Framework<br/><code>mcp/server/</code>]
    end

    subgraph Orchestrator["Orchestrator"]
        OMOD[Orchestrator Core<br/><code>orchestrator/mod.rs</code>]
        APOOL[Agent Pool<br/><code>agent_pool.rs</code>]
        TGRAPH[Task Graph DAG<br/><code>task_graph.rs</code>]
        TBRIDGE[Trace Bridge<br/><code>trace_bridge.rs</code>]
        A2A_ORCH[A2A Mesh Delegation<br/><code>a2a.rs</code>]
    end

    subgraph Cockpit["Cockpit"]
        PTY[PTY Manager<br/><code>pty_manager.rs</code>]
        WSB[WebSocket Bridge<br/><code>ws_bridge.rs</code>]
        DEP[Deploy Catalog<br/><code>deploy.rs</code>]
        ADM[Admission Policy<br/><code>admission.rs</code>]
    end

    subgraph HALO["HALO Subsystem"]
        direction TB
        ID[Identity & PQ Crypto]
        P2P[P2P Mesh & Comms]
        OBS[Observability & Trace]
        TRUST[Trust, ZK & Attestation]
        EPIST[Epistemic Calculi]
        PROXY[API Proxy]
        LMOD[Local Models]
        VAULT[Encrypted Vault]
        GOV[Governor Registry]
    end

    subgraph NucleusDB["NucleusDB Core"]
        direction TB
        PROTO[Protocol Layer]
        COMMIT[Commitment Schemes]
        DATA[Data Layer]
        ACCESS[Access & Multi-tenancy]
    end

    subgraph External["External Subsystems"]
        CONT[Container Runtime]
        SWARM[Swarm / Bitswap]
        LEAN[Lean 4 Proofs]
        CONTRACTS[Solidity Contracts]
    end

    AHCLI --> HALO
    AHCLI --> Dashboard
    AHCLI --> Orchestrator
    NDBCLI --> NucleusDB

    DUI -- "HTTP/WS" --> DAPI
    DAPI --> HALO
    DAPI --> NucleusDB
    DAPI --> Orchestrator
    DAPI --> Cockpit

    MCPT --> NucleusDB
    MCPT --> HALO
    MCPT --> Orchestrator

    OMOD --> APOOL
    OMOD --> TGRAPH
    APOOL --> PTY
    TBRIDGE --> OBS

    DEP --> ADM
    ADM --> GOV
    WSB --> PTY

    PROXY --> VAULT
    PROXY --> LMOD
    LMOD -.-> OBS

    HALO --> NucleusDB
    OBS --> NucleusDB
    TRUST --> NucleusDB

    CONT -.-> SWARM
    LEAN -.-> CONTRACTS

    style HALO fill:#0d1117,stroke:#58a6ff,stroke-width:2px,color:#c9d1d9
    style NucleusDB fill:#0d1117,stroke:#f78166,stroke-width:2px,color:#c9d1d9
    style Dashboard fill:#0d1117,stroke:#3fb950,stroke-width:2px,color:#c9d1d9
    style Orchestrator fill:#0d1117,stroke:#d2a8ff,stroke-width:2px,color:#c9d1d9
    style Cockpit fill:#0d1117,stroke:#f0883e,stroke-width:2px,color:#c9d1d9
```

---

## 4. NucleusDB Core — Internal Structure

The verifiable database engine.

```mermaid
graph TB
    subgraph Protocol["Protocol Layer"]
        NDBP["NucleusDb Trait<br/><code>protocol.rs</code>"]
        STATE["State + Delta<br/><code>state.rs</code>"]
        PERSIST["Snapshot + WAL<br/><code>persistence.rs</code>"]
        KEYMAP["Key-to-Index Map<br/><code>keymap.rs</code>"]
        IMMUT["Append-Only Keys<br/><code>immutable.rs</code>"]
    end

    subgraph Commitment["Commitment & Verification"]
        IPA["IPA Backend<br/><code>vc/ipa.rs</code>"]
        KZG["KZG Backend<br/><code>vc/kzg.rs</code>"]
        MERKLE["Binary Merkle<br/><code>vc/binary_merkle.rs</code>"]
        WITNESS["Witness Signatures<br/>Ed25519 + ML-DSA-65"]
        SEC["Security Profiles<br/><code>security.rs</code>"]
        TRANS["CT Log Integration<br/><code>transparency/</code>"]
    end

    subgraph Data["Data Layer"]
        TV["8 Typed Values<br/><code>typed_value.rs</code>"]
        TM["Type Map<br/><code>type_map.rs</code>"]
        VI["Vector Index (kNN)<br/><code>vector_index.rs</code>"]
        BLOB["Blob Store (SHA-256)<br/><code>blob_store.rs</code>"]
        SQL["SQL Parser + Executor<br/><code>sql/</code>"]
        MEM["Memory Store<br/><code>memory.rs</code>"]
        EMB["Embeddings<br/><code>embeddings.rs</code>"]
    end

    subgraph Access["Access Control"]
        POD["Solid POD Protocol<br/><code>pod/</code>"]
        MT["Multi-tenant RBAC<br/><code>multitenant.rs</code>"]
        LIC["CAB License Gate<br/><code>license.rs</code>"]
    end

    NDBP --> STATE
    NDBP --> KEYMAP
    NDBP --> IMMUT
    STATE --> PERSIST

    NDBP -- "commit proof" --> IPA
    NDBP -- "commit proof" --> KZG
    NDBP -- "commit proof" --> MERKLE
    NDBP -- "sign" --> WITNESS
    IPA --> SEC
    KZG --> SEC

    SQL --> NDBP
    VI --> EMB
    MEM --> VI
    MEM --> BLOB
    MEM --> EMB
    TV --> TM
    NDBP --> TV

    POD --> NDBP
    MT --> NDBP
    MT --> LIC

    style Protocol fill:#1a1e24,stroke:#f78166,stroke-width:2px,color:#c9d1d9
    style Commitment fill:#1a1e24,stroke:#d29922,stroke-width:2px,color:#c9d1d9
    style Data fill:#1a1e24,stroke:#58a6ff,stroke-width:2px,color:#c9d1d9
    style Access fill:#1a1e24,stroke:#3fb950,stroke-width:2px,color:#c9d1d9
```

---

## 5. HALO Subsystem — Internal Structure

Sovereign agent identity, communication, and observability.

```mermaid
graph TB
    subgraph Identity["Identity & Post-Quantum Crypto"]
        DID["DID Document<br/>Ed25519 + ML-DSA-65"]
        GSEED["Genesis Seed<br/>BIP-39 Mnemonic"]
        GENTROPY["Genesis Entropy"]
        IDLEDGER["Identity Ledger<br/>Hash-chained (SHA-512)"]
        PQ["PQ Wallet<br/>ML-DSA-65"]
        HASH["Hash Dispatch<br/>SHA-256 / SHA-512"]
        HKEM["Hybrid KEM<br/>X25519 + ML-KEM-768"]
        EVMW["EVM Wallet<br/>secp256k1 BIP-32"]
        EVMG["EVM PQ Gate<br/>Dual-sign before secp256k1"]
        TWINE["Twine Anchor<br/>CURBy-Q Triple-signed"]
    end

    subgraph Comms["P2P Mesh & Communication"]
        P2PN["libp2p Swarm<br/>Noise XX + Gossipsub + Kademlia"]
        P2PD["Discovery<br/>DHT + GossipPrivacy"]
        A2AB["A2A Bridge<br/>DIDComm HTTP"]
        DCOMM["DIDComm v2<br/>Hybrid KEM Authcrypt"]
        DCOMMH["DIDComm Handler"]
        STARTUP["Full Stack Bootstrap"]
        NYMM["Nym SOCKS5 Proxy"]
        NYMN["Nym Native Sphinx"]
    end

    subgraph Observe["Observability & Trace"]
        SCHEMA["TraceEvent Schema"]
        TRACE["TraceWriter / Reader<br/>redb-backed"]
        WRAP["Agent Wrapper<br/>stdin/stdout intercept"]
        RUNNER["Process Runner"]
        DETECT["Agent Auto-detect"]
        VIEWER["Session Exporter"]
        ADAPT["Adapters<br/>Claude | Codex | Gemini | Generic"]
    end

    subgraph TrustZK["Trust, ZK & Attestation"]
        ATTEST["Session Attestation<br/>Merkle SHA-512"]
        TRUSTS["Trust Scores + Epistemic<br/>Heyting Algebra"]
        CIRC["Groth16 ZK Circuits<br/>BN254 arkworks"]
        CIRCPOL["Circuit Policy<br/>Dev vs Prod"]
        ZKCOMP["ZK Compute Receipts"]
        ZKCRED["ZK Credentials<br/>Anonymous Membership"]
        AUDIT_H["Solidity Static Analysis"]
    end

    subgraph Epistemic["Epistemic Calculi"]
        DIVERS["Tsallis Diversity<br/>Strategy diversity gauge"]
        EVID["Evidence Combiner<br/>Bayesian odds-update"]
        UNCERT["Uncertainty Translation<br/>Probability / CF / Possibility"]
        TTOPO["Trace Topology<br/>H0 Persistence (Rips)"]
    end

    subgraph ServingProxy["Serving & Proxy"]
        PRXY["API Proxy<br/>OpenAI-compat multi-provider"]
        LMOD["Local Models<br/>Ollama + vLLM"]
        VLT["Encrypted Vault<br/>AES-256-GCM"]
        PRICE["Token Pricing"]
        X402M["HTTP 402 Payments"]
        GOV["Governor Registry<br/>AETHER gain/stability"]
        GOVTEL["Governor Telemetry"]
        CHEV["Chebyshev Evictor"]
        ADM["Admission Policy<br/>Warn / Block / Force"]
    end

    GSEED --> DID
    GENTROPY --> GSEED
    DID --> PQ
    DID --> HKEM
    EVMG --> EVMW
    EVMG --> PQ
    TWINE --> DID

    P2PN --> P2PD
    P2PN --> DCOMM
    DCOMM --> HKEM
    DCOMMH --> DCOMM
    A2AB --> DCOMM
    STARTUP --> P2PN
    STARTUP --> NYMM
    NYMN --> NYMM

    WRAP --> ADAPT
    WRAP --> SCHEMA
    RUNNER --> WRAP
    DETECT --> RUNNER
    SCHEMA --> TRACE
    VIEWER --> TRACE

    ATTEST --> TRACE
    TRUSTS --> TRACE
    CIRC --> CIRCPOL
    ZKCRED --> CIRC

    DIVERS --> TRACE
    TTOPO --> TRACE
    EVID --> TRUSTS

    PRXY --> VLT
    PRXY --> LMOD
    PRXY --> PRICE
    PRXY --> X402M
    ADM --> GOV
    GOV --> GOVTEL
    CHEV --> GOV

    style Identity fill:#1a1e24,stroke:#d2a8ff,stroke-width:2px,color:#c9d1d9
    style Comms fill:#1a1e24,stroke:#58a6ff,stroke-width:2px,color:#c9d1d9
    style Observe fill:#1a1e24,stroke:#3fb950,stroke-width:2px,color:#c9d1d9
    style TrustZK fill:#1a1e24,stroke:#d29922,stroke-width:2px,color:#c9d1d9
    style Epistemic fill:#1a1e24,stroke:#f47067,stroke-width:2px,color:#c9d1d9
    style ServingProxy fill:#1a1e24,stroke:#f0883e,stroke-width:2px,color:#c9d1d9
```

---

## 6. Cockpit & Orchestrator — Agent Lifecycle

How agents are launched, managed, and traced.

```mermaid
sequenceDiagram
    participant Browser as Browser (xterm.js)
    participant Dashboard as Dashboard API
    participant Deploy as Deploy Catalog
    participant Admission as Admission Policy
    participant Governor as Governor Registry
    participant Orch as Orchestrator
    participant Pool as Agent Pool
    participant PTY as PTY Manager
    participant Trace as Trace Bridge
    participant TraceDB as Trace Store (redb)

    Browser->>Dashboard: POST /deploy/launch {agent, cwd, model}
    Dashboard->>Admission: evaluate_launch_admission(mode, registry, topology)
    Admission->>Governor: snapshot_one("gov-compute"), snapshot_one("gov-pty")
    Governor-->>Admission: GovernorSnapshot (gain_violated? oscillating? stable?)
    Admission-->>Dashboard: AdmissionReport {allowed, issues}

    alt allowed = true
        Dashboard->>Deploy: launch(agent_id, cwd)
        Deploy->>PTY: create_session(cmd, args, cwd)
        PTY-->>Deploy: session_id
        Deploy-->>Dashboard: {session_id, status: "active"}
        Dashboard-->>Browser: 200 OK

        Browser->>Dashboard: WS /cockpit/ws/{session_id}
        Dashboard->>PTY: subscribe(session_id)

        loop Terminal I/O
            Browser->>PTY: keystrokes (Binary frames)
            PTY->>PTY: write to PTY fd
            PTY-->>Browser: terminal output (Binary frames)
        end

        Note over Orch,Pool: Orchestrator wraps PTY for multi-agent tasks
        Orch->>Pool: launch(LaunchSpec)
        Pool->>PTY: create_session(...)
        Orch->>Pool: send_task(agent_id, prompt)
        Pool->>PTY: write_to_pty(prompt)
        PTY-->>Trace: output stream
        Trace->>TraceDB: append TraceEvent
        Pool-->>Orch: TaskResult {output, usage}
    else allowed = false
        Dashboard-->>Browser: 403 Blocked + issues[]
    end
```

---

## 7. Proxy & Local Model Routing

Request routing through the API proxy.

```mermaid
flowchart TB
    REQ[/"Incoming Chat Request<br/>model: 'claude-sonnet-4-20250514'"/]

    REQ --> RESOLVE{resolve_backend_for_request}

    RESOLVE --> CHK_LOCAL{"starts with<br/>'local/'?"}
    CHK_LOCAL -- Yes --> LOCAL_ROUTE["resolve_local_route()"]

    CHK_LOCAL -- No --> CHK_SLASH{"contains '/'?"}
    CHK_SLASH -- Yes --> HINT_CHECK{"installed_backend_for_model()<br/><i>hint-only, zero network</i>"}
    HINT_CHECK -- found --> LOCAL_ROUTE
    HINT_CHECK -- not found --> OPENROUTER

    CHK_SLASH -- No --> CHK_CLOUD{"looks_like_openrouter<br/>_cloud_model()?<br/><i>claude/gpt/gemini/...</i>"}
    CHK_CLOUD -- Yes --> OPENROUTER["OpenRouter API<br/><i>via Vault key</i>"]
    CHK_CLOUD -- No --> HINT_CHECK2{"installed_backend_for_model()"}
    HINT_CHECK2 -- found --> LOCAL_ROUTE
    HINT_CHECK2 -- not found --> OPENROUTER

    LOCAL_ROUTE --> LOCAL_BACKEND{Backend Type}
    LOCAL_BACKEND -- Ollama --> OLLAMA["Ollama<br/>localhost:11434"]
    LOCAL_BACKEND -- vLLM --> VLLM["vLLM<br/>localhost:8000"]

    subgraph "Hint-Only Resolution (P2-A Fix)"
        direction TB
        H1["1. Check HF model path on disk"]
        H2["2. Check in-process model cache"]
        H3["3. Check persisted installed_hints"]
        H1 --> H2 --> H3
    end

    HINT_CHECK -.-> H1
    HINT_CHECK2 -.-> H1

    style RESOLVE fill:#1f6feb,stroke:#58a6ff,color:#fff
    style LOCAL_ROUTE fill:#238636,stroke:#3fb950,color:#fff
    style OPENROUTER fill:#8957e5,stroke:#d2a8ff,color:#fff
    style OLLAMA fill:#238636,stroke:#3fb950,color:#fff
    style VLLM fill:#238636,stroke:#3fb950,color:#fff
```

---

## 8. Post-Quantum Cryptographic Stack

Key hierarchy and signing/encryption paths.

```mermaid
graph TB
    subgraph "Genesis Ceremony"
        ENTROPY["Entropy Sources<br/>(system + user + hardware)"]
        MNEMONIC["BIP-39 Mnemonic<br/>(24 words)"]
        SEED["Genesis Secret Seed"]
    end

    subgraph "Key Derivation"
        ED["Ed25519 Keypair<br/><i>Classical signing</i>"]
        MLDSA["ML-DSA-65 Keypair<br/><i>Post-quantum signing</i>"]
        X25519["X25519 Keypair<br/><i>Classical ECDH</i>"]
        MLKEM["ML-KEM-768 Keypair<br/><i>Post-quantum KEM</i>"]
        BIP32["BIP-32 secp256k1<br/><i>EVM wallet</i>"]
    end

    subgraph "Operations"
        DUAL_SIGN["Dual Signature<br/>Ed25519 + ML-DSA-65"]
        HYBRID_ENC["Hybrid Encrypt<br/>X25519 + ML-KEM-768<br/>→ HKDF-SHA-512<br/>→ AES-256-GCM"]
        PQ_GATE["PQ-Gated EVM Sign<br/>Dual-sign auth<br/>→ secp256k1 ECDSA"]
        VAULT_ENC["Vault Encrypt<br/>HKDF-SHA-256 from seed<br/>→ AES-256-GCM"]
    end

    subgraph "Protocols"
        DIDCOMM["DIDComm v2<br/>Authcrypt / Anoncrypt"]
        IDCHAIN["Identity Ledger<br/>SHA-512 hash chain"]
        ATTEST_PQ["Session Attestation<br/>Merkle root SHA-512"]
        TWINE_PQ["Twine Anchor<br/>Triple-signed CURBy-Q"]
        EVM_TX["EVM Transactions<br/>Base L2"]
    end

    ENTROPY --> MNEMONIC --> SEED
    SEED --> ED
    SEED --> MLDSA
    SEED --> X25519
    SEED --> MLKEM
    SEED --> BIP32

    ED --> DUAL_SIGN
    MLDSA --> DUAL_SIGN
    X25519 --> HYBRID_ENC
    MLKEM --> HYBRID_ENC
    DUAL_SIGN --> PQ_GATE
    BIP32 --> PQ_GATE
    SEED --> VAULT_ENC

    HYBRID_ENC --> DIDCOMM
    DUAL_SIGN --> IDCHAIN
    DUAL_SIGN --> ATTEST_PQ
    DUAL_SIGN --> TWINE_PQ
    PQ_GATE --> EVM_TX

    style DUAL_SIGN fill:#8957e5,stroke:#d2a8ff,color:#fff
    style HYBRID_ENC fill:#8957e5,stroke:#d2a8ff,color:#fff
    style PQ_GATE fill:#da3633,stroke:#f85149,color:#fff
    style VAULT_ENC fill:#d29922,stroke:#e3b341,color:#fff
```

---

## 9. Container & Mesh Networking

Docker container lifecycle with P2P mesh.

```mermaid
graph TB
    subgraph "Container Lifecycle"
        BUILD["Image Builder<br/><code>builder.rs</code>"]
        LAUNCH["Container Launcher<br/><code>launcher.rs</code>"]
        SHIM["Container Shim<br/><code>shim/</code>"]
        SIDECAR["WDK Sidecar<br/><code>wdk-sidecar/</code><br/>(Node.js)"]
    end

    subgraph "Mesh Network"
        MESH["Mesh Registry<br/><code>mesh.rs</code>"]
        MINIT["Mesh Init<br/>register/deregister self"]
        PEER["Peer Discovery<br/>ping + latency"]
        REMOTE["Remote Tool Call<br/>cross-container MCP"]
        ENV["DIDComm Envelope<br/>exchange"]
    end

    subgraph "Swarm / Bitswap"
        CHUNK["Chunk Engine<br/>content-split"]
        CSTORE["Chunk Store<br/>local cache"]
        BSWAP["Bitswap Protocol<br/>peer exchange"]
        MANIFEST["Manifest Builder<br/>verify integrity"]
    end

    BUILD --> LAUNCH
    LAUNCH --> SHIM
    LAUNCH --> SIDECAR
    LAUNCH --> MINIT

    MINIT --> MESH
    MESH --> PEER
    MESH --> REMOTE
    MESH --> ENV

    CHUNK --> CSTORE
    CSTORE --> BSWAP
    MANIFEST --> CHUNK

    REMOTE -.-> BSWAP

    style MESH fill:#1a1e24,stroke:#58a6ff,stroke-width:2px,color:#c9d1d9
```

---

## 10. Dashboard Frontend Architecture

SPA page routing and module structure.

```mermaid
graph TB
    subgraph "HTML Shell"
        INDEX["index.html<br/>Sidebar nav + content div"]
    end

    subgraph "Core SPA"
        APP["app.js<br/>Router + Page renderers"]
    end

    subgraph "Page Modules (IIFE)"
        COCKPIT_JS["cockpit.js<br/>CockpitManager<br/>Diversity gauge<br/>Trace topology chart"]
        DEPLOY_JS["deploy.js<br/>Agent cards<br/>Preflight + Launch<br/>Admission controls"]
        ORCH_JS["orchestrator.js<br/>Agent/Task tables<br/>Graph topology"]
        GENESIS_JS["genesis-docs.js<br/>Documentation pages"]
    end

    subgraph "Vendor"
        XTERM["xterm.js<br/>+ fit + webgl addons"]
        CHARTJS["Chart.js"]
    end

    subgraph "Pages (via app.js)"
        P_OVER["#/overview<br/>Status, costs, trust"]
        P_SESS["#/sessions<br/>Session list + detail"]
        P_COST["#/costs<br/>Daily/agent/model charts"]
        P_CONF["#/config<br/>Wrap, x402, vault keys"]
        P_TRUST["#/trust<br/>Attestations, scores"]
        P_NDB["#/nucleusdb<br/>SQL console, vector search"]
        P_COCK["#/cockpit<br/>Terminal panels"]
        P_DEP["#/deploy<br/>Agent catalog + launch"]
        P_MOD["#/models<br/>Local model management"]
    end

    INDEX --> APP
    APP --> P_OVER
    APP --> P_SESS
    APP --> P_COST
    APP --> P_CONF
    APP --> P_TRUST
    APP --> P_NDB
    APP --> P_COCK
    APP --> P_DEP
    APP --> P_MOD

    P_COCK --> COCKPIT_JS
    P_DEP --> DEPLOY_JS
    COCKPIT_JS --> XTERM
    COCKPIT_JS --> CHARTJS
    P_COST --> CHARTJS

    APP -.-> ORCH_JS
    APP -.-> GENESIS_JS

    style APP fill:#238636,stroke:#3fb950,color:#fff
```

---

## 11. MCP Tool Surface

Tools exposed to AI agents via MCP protocol.

```mermaid
graph TB
    subgraph "MCP Server Framework"
        RMCP["rmcp crate<br/>JSON-RPC transport"]
        AUTH_MCP["Auth Layer<br/><code>server/auth.rs</code>"]
        REMOTE_MCP["Remote Bridge<br/><code>server/remote.rs</code>"]
    end

    subgraph "NucleusDB Tools"
        T_GET["nucleusdb_get"]
        T_SET["nucleusdb_set"]
        T_DEL["nucleusdb_delete"]
        T_SQL["nucleusdb_sql"]
        T_VEC["nucleusdb_vector_search"]
        T_BLOB["nucleusdb_blob_store / _get"]
        T_MEM["nucleusdb_memorize / _recall"]
        T_CHUNK["nucleusdb_chunk / _reassemble"]
        T_VCS["nucleusdb_work_record_*"]
        T_VERIFY["nucleusdb_verify"]
    end

    subgraph "Orchestrator Tools"
        T_LAUNCH["orchestrator_launch"]
        T_TASK["orchestrator_send_task"]
        T_PIPE["orchestrator_pipe"]
        T_GRAPH["orchestrator_graph"]
        T_MESH["orchestrator_mesh_status"]
        T_STOP["orchestrator_stop"]
    end

    subgraph "HALO Tools"
        T_ATTEST["agenthalo_attest"]
        T_TRUST["agenthalo_trust"]
        T_EVID["agenthalo_evidence_combine"]
        T_UNCERT["agenthalo_uncertainty_translate"]
        T_DEPLOY_T["deploy_preflight / _launch"]
        T_ADMIT["agenthalo_admission_check"]
    end

    subgraph "Container Tools"
        T_CLNCH["container_launch"]
        T_CLOGS["container_logs"]
        T_CSTOP["container_stop"]
        T_CMESH["mesh_*"]
    end

    RMCP --> AUTH_MCP
    RMCP --> REMOTE_MCP

    RMCP --> T_GET
    RMCP --> T_LAUNCH
    RMCP --> T_ATTEST
    RMCP --> T_CLNCH

    style RMCP fill:#1f6feb,stroke:#58a6ff,color:#fff
```

---

## 12. Lean 4 Formal Verification Layer

Proof modules that back the Rust runtime.

```mermaid
graph LR
    subgraph "Lean 4 Proofs (lean/NucleusDB/)"
        CORE_L["Core<br/>Base types & axioms"]
        CRYPTO_L["Crypto<br/>Hash, KEM, signatures"]
        COMMIT_L["Commitment<br/>IPA, KZG proofs"]
        GENESIS_L["Genesis<br/>Seed ceremony"]
        IDENTITY_L["Identity<br/>DID, ledger"]
        TRUST_L["TrustLayer<br/>Nucleus operator,<br/>Heyting algebra"]
        COMMS_L["Comms<br/>DIDComm, hybrid KEM"]
        SECURITY_L["Security<br/>Parameter sets"]
        SHEAF_L["Sheaf<br/>Coherence conditions"]
        TRANS_L["Transparency<br/>CT log"]
        PCN_L["PaymentChannels"]
        CONTRACTS_L["Contracts<br/>EVM verification"]
        ADV_L["Adversarial<br/>Security games"]
        BRIDGE_L["Bridge<br/>Rust ↔ Lean binding"]
        INTEG_L["Integration<br/>End-to-end properties"]
    end

    CORE_L --> CRYPTO_L
    CORE_L --> COMMIT_L
    CRYPTO_L --> GENESIS_L
    GENESIS_L --> IDENTITY_L
    IDENTITY_L --> TRUST_L
    CRYPTO_L --> COMMS_L
    CRYPTO_L --> SECURITY_L
    TRUST_L --> SHEAF_L
    COMMIT_L --> TRANS_L
    IDENTITY_L --> CONTRACTS_L
    SECURITY_L --> ADV_L
    BRIDGE_L --> INTEG_L

    style TRUST_L fill:#8957e5,stroke:#d2a8ff,color:#fff
    style CRYPTO_L fill:#da3633,stroke:#f85149,color:#fff
```

---

## 13. On-Chain Attestation Flow

Session attestation to Base L2.

```mermaid
sequenceDiagram
    participant Agent as Agent Session
    participant Trace as Trace Store
    participant Attest as Attestation Engine
    participant ZK as Groth16 Prover
    participant Wallet as EVM Wallet (PQ-Gated)
    participant Contract as TrustVerifier.sol
    participant Chain as Base L2

    Agent->>Trace: Complete session (events logged)
    Trace->>Attest: Build Merkle tree (SHA-512)
    Attest->>Attest: Compute session root hash

    opt ZK Proof Required
        Attest->>ZK: Generate Groth16 proof (BN254)
        ZK-->>Attest: π, public inputs
    end

    Attest->>Wallet: Request EVM signature
    Note over Wallet: PQ Gate: Ed25519 + ML-DSA-65<br/>must both sign authorization
    Wallet-->>Attest: secp256k1 signature

    Attest->>Contract: submitAttestation(root, proof, sig)
    Contract->>Contract: verify Groth16 proof on-chain
    Contract->>Chain: Store attestation record
    Chain-->>Contract: tx receipt
    Contract-->>Attest: attestation confirmed
```

---

## 14. Data Flow — Memory Recall Pipeline

How agent memory is stored and retrieved.

```mermaid
flowchart TB
    STORE["memorize(text, tags)"]
    CHUNK_M["Chunk text<br/>(overlap windows)"]
    EMBED["Generate embedding<br/>(384-dim model)"]
    KV["Store in NucleusDB<br/>mem:chunk:* keys"]
    VEC["Index in Vector Store"]
    BLOB_M["Store blob<br/>(SHA-256 addressed)"]

    RECALL["recall(query, k)"]
    QEXP["Query Expansion<br/>(optional LLM)"]
    VSEARCH["Vector kNN search<br/>(4x candidate pool)"]
    ACCESS["Access-aware read<br/>(get_typed_touching)"]
    RERANK["Fused rerank<br/>(similarity 0.50<br/>+ biencoder 0.28<br/>+ lexical 0.12<br/>+ negation 0.10)"]
    RESULT["Top-k results<br/>(max 20)"]

    STORE --> CHUNK_M --> EMBED --> KV
    KV --> VEC
    KV --> BLOB_M

    RECALL --> QEXP --> VSEARCH --> ACCESS --> RERANK --> RESULT

    style STORE fill:#238636,stroke:#3fb950,color:#fff
    style RECALL fill:#1f6feb,stroke:#58a6ff,color:#fff
    style RERANK fill:#8957e5,stroke:#d2a8ff,color:#fff
```

---

## 15. Complete File Map

Module-to-file reference for the entire codebase.

```mermaid
mindmap
  root((NucleusDB /<br/>Agent HALO))
    src/
      bin/
        nucleusdb.rs
        nucleusdb_server.rs
        nucleusdb_tui.rs
        nucleusdb_mcp.rs
        agenthalo.rs
        agenthalo_mcp_server.rs
      protocol.rs
      state.rs
      persistence.rs
      keymap.rs
      immutable.rs
      typed_value.rs
      type_map.rs
      vector_index.rs
      blob_store.rs
      memory.rs
      embeddings.rs
      security.rs
      witness.rs
      materialize.rs
      commitment/
      vc/
        ipa.rs
        kzg.rs
        binary_merkle.rs
      sql/
      pod/
      multitenant.rs
      license.rs
      transparency/
      halo/
        Identity
          did.rs
          genesis_seed.rs
          genesis_entropy.rs
          identity.rs
          identity_ledger.rs
          pq.rs
          hash.rs
          hybrid_kem.rs
          evm_wallet.rs
          evm_gate.rs
          twine_anchor.rs
        Comms
          p2p_node.rs
          p2p_discovery.rs
          a2a_bridge.rs
          didcomm.rs
          didcomm_handler.rs
          startup.rs
          nym.rs
          nym_native.rs
        Observe
          schema.rs
          trace.rs
          wrap.rs
          runner.rs
          detect.rs
          viewer.rs
          adapters/
        Trust
          attest.rs
          trust.rs
          circuit.rs
          zk_compute.rs
          zk_credential.rs
          audit.rs
        Epistemic
          evidence.rs
          uncertainty.rs
          trace_topology.rs
          metrics/diversity.rs
        Serving
          proxy.rs
          local_models.rs
          vault.rs
          pricing.rs
          config.rs
          admission.rs
          governor.rs
          governor_registry.rs
      cockpit/
        pty_manager.rs
        ws_bridge.rs
        deploy.rs
      orchestrator/
        agent_pool.rs
        task.rs
        task_graph.rs
        trace_bridge.rs
        a2a.rs
      mcp/
        tools.rs
        server/
      container/
        builder.rs
        launcher.rs
        mesh.rs
        sidecar.rs
      swarm/
        bitswap.rs
        chunk_engine.rs
        chunk_store.rs
        manifest.rs
      puf/
        core.rs
        dgx.rs
        tpm.rs
      pcn/
      sheaf/
      trust/
        composite_cab.rs
        onchain.rs
      vcs/
      verifier/
    dashboard/
      index.html
      app.js
      cockpit.js
      deploy.js
      orchestrator.js
      style.css
    contracts/
      TrustVerifier.sol
      Groth16VerifierAdapter.sol
      CrossChainAttestationQuery.sol
    lean/NucleusDB/
      Core
      Crypto
      Commitment
      Genesis
      Identity
      TrustLayer
      Comms
      Security
      Sheaf
      Adversarial
      Bridge
    wdk-sidecar/
```
