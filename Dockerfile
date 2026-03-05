# =============================================================================
# AgentHALO Unified Container
# =============================================================================
# Single container runtime:
# - agenthalo dashboard (3100)
# - agenthalo-mcp-server (8390)
# - nucleusdb-server (8088 internal)
# - nym-socks5-client (1080 internal)
# - wdk-sidecar (7321 internal)
# =============================================================================

FROM debian:bookworm-slim AS nym_builder

ARG NYM_VERSION="nym-binaries-v2026.4-quark"
ARG NYM_SOCKS5_CLIENT_SHA256="a20d010532d1c15a44e07e154c09c926df5b21c16a149075e81c0a2bb678144a"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fL "https://github.com/nymtech/nym/releases/download/${NYM_VERSION}/nym-socks5-client" -o /tmp/nym-socks5-client && \
    echo "${NYM_SOCKS5_CLIENT_SHA256}  /tmp/nym-socks5-client" | sha256sum -c && \
    install -m 0755 /tmp/nym-socks5-client /usr/local/bin/nym-socks5-client

FROM debian:bookworm-slim AS foundry_builder

ARG FOUNDRY_VERSION="v1.5.0"
ARG TARGETARCH
ARG FOUNDRY_LINUX_AMD64_SHA256="5cd98f9092bcc28be087939491f786b2bf3ed55e492996a409e29519b8ab4dc8"
ARG FOUNDRY_LINUX_ARM64_SHA256="8138e1615568bfcca5999773830892d93a569370eb0ae4b7dd97db46e2af47f9"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl tar && \
    rm -rf /var/lib/apt/lists/*

RUN ARCH="${TARGETARCH:-$(dpkg --print-architecture)}" && \
    case "${ARCH}" in \
        amd64) \
            FOUNDRY_ASSET="foundry_${FOUNDRY_VERSION}_linux_amd64.tar.gz"; \
            FOUNDRY_SHA256="${FOUNDRY_LINUX_AMD64_SHA256}" ;; \
        arm64) \
            FOUNDRY_ASSET="foundry_${FOUNDRY_VERSION}_linux_arm64.tar.gz"; \
            FOUNDRY_SHA256="${FOUNDRY_LINUX_ARM64_SHA256}" ;; \
        *) echo "unsupported TARGETARCH: ${ARCH}" >&2; exit 1 ;; \
    esac && \
    curl -fL "https://github.com/foundry-rs/foundry/releases/download/${FOUNDRY_VERSION}/${FOUNDRY_ASSET}" -o /tmp/foundry.tar.gz && \
    echo "${FOUNDRY_SHA256}  /tmp/foundry.tar.gz" | sha256sum -c && \
    tar -xzf /tmp/foundry.tar.gz -C /tmp && \
    install -m 0755 /tmp/cast /usr/local/bin/cast

FROM rust:1.88-slim-bookworm AS rust_builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY vendor/ vendor/
COPY src/ src/
COPY dashboard/ dashboard/

RUN cargo build --release \
    --bin agenthalo \
    --bin agenthalo-mcp-server \
    --bin nucleusdb-server

FROM node:22-bookworm-slim AS wdk_builder

WORKDIR /wdk
COPY wdk-sidecar/package.json wdk-sidecar/package-lock.json ./
RUN npm ci --production
COPY wdk-sidecar/index.mjs ./

FROM node:22-bookworm-slim

ARG NOMIC_MODEL_DIR=/opt/models/nomic-embed-text
ARG NOMIC_MODEL_ONNX_URL="https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main/onnx/model.onnx"
ARG NOMIC_MODEL_TOKENIZER_URL="https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main/tokenizer.json"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    netcat-openbsd \
    tini \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 10001 agenthalo && \
    useradd --uid 10001 --gid 10001 --home-dir /data --create-home --shell /usr/sbin/nologin agenthalo

COPY --from=rust_builder /build/target/release/agenthalo /usr/local/bin/agenthalo
COPY --from=rust_builder /build/target/release/agenthalo-mcp-server /usr/local/bin/agenthalo-mcp-server
COPY --from=rust_builder /build/target/release/nucleusdb-server /usr/local/bin/nucleusdb-server
COPY --from=nym_builder /usr/local/bin/nym-socks5-client /usr/local/bin/nym-socks5-client
COPY --from=foundry_builder /usr/local/bin/cast /usr/local/bin/cast

COPY --from=wdk_builder --chown=10001:10001 /wdk /opt/wdk-sidecar
COPY --chown=10001:10001 scripts/agenthalo-entrypoint.sh /usr/local/bin/agenthalo-entrypoint.sh
COPY --chown=10001:10001 scripts/agenthalo-healthcheck.sh /usr/local/bin/agenthalo-healthcheck.sh

RUN mkdir -p "${NOMIC_MODEL_DIR}" && \
    curl -fL "${NOMIC_MODEL_ONNX_URL}" -o "${NOMIC_MODEL_DIR}/model.onnx" && \
    curl -fL "${NOMIC_MODEL_TOKENIZER_URL}" -o "${NOMIC_MODEL_DIR}/tokenizer.json"

RUN chmod +x /usr/local/bin/agenthalo-entrypoint.sh /usr/local/bin/agenthalo-healthcheck.sh && \
    mkdir -p /data /data/logs /data/nym && \
    chown -R 10001:10001 /data /opt/wdk-sidecar /opt/models && \
    chmod 700 /data

ENV HOME=/data
ENV AGENTHALO_HOME=/data
ENV AGENTHALO_DASHBOARD_HOST=0.0.0.0
ENV AGENTHALO_DASHBOARD_PORT=3100
ENV AGENTHALO_MCP_HOST=0.0.0.0
ENV AGENTHALO_MCP_PORT=8390
ENV WDK_PORT=7321
ENV WDK_SIDECAR_DIR=/opt/wdk-sidecar
ENV NOMIC_MODEL_DIR=/opt/models/nomic-embed-text

ENV SOCKS5_PROXY=socks5h://127.0.0.1:1080
ENV ALL_PROXY=socks5h://127.0.0.1:1080
ENV HTTP_PROXY=socks5h://127.0.0.1:1080
ENV HTTPS_PROXY=socks5h://127.0.0.1:1080
ENV NO_PROXY=localhost,127.0.0.1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16
ENV NYM_FAIL_OPEN=false
ENV NYM_ID=agenthalo
ENV NYM_PORT=1080
ENV NYM_DATA_DIR=/data/nym

EXPOSE 3100 8390
VOLUME ["/data"]
WORKDIR /data

HEALTHCHECK --interval=15s --timeout=5s --retries=5 --start-period=45s \
    CMD /usr/local/bin/agenthalo-healthcheck.sh

USER 10001:10001
ENTRYPOINT ["tini", "--", "/usr/local/bin/agenthalo-entrypoint.sh"]
