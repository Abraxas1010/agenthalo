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

ARG NYM_SOCKS5_CLIENT_URL="https://github.com/nymtech/nym/releases/latest/download/nym-socks5-client"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fL "${NYM_SOCKS5_CLIENT_URL}" -o /usr/local/bin/nym-socks5-client && \
    chmod +x /usr/local/bin/nym-socks5-client

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

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    netcat-openbsd \
    tini \
    && rm -rf /var/lib/apt/lists/*

# Install foundry cast for on-chain helpers.
RUN curl -L https://foundry.paradigm.xyz | bash && \
    /root/.foundry/bin/foundryup && \
    cp /root/.foundry/bin/cast /usr/local/bin/cast && \
    rm -rf /root/.foundry

RUN groupadd --gid 10001 agenthalo && \
    useradd --uid 10001 --gid 10001 --home-dir /data --create-home --shell /usr/sbin/nologin agenthalo

COPY --from=rust_builder /build/target/release/agenthalo /usr/local/bin/agenthalo
COPY --from=rust_builder /build/target/release/agenthalo-mcp-server /usr/local/bin/agenthalo-mcp-server
COPY --from=rust_builder /build/target/release/nucleusdb-server /usr/local/bin/nucleusdb-server
COPY --from=nym_builder /usr/local/bin/nym-socks5-client /usr/local/bin/nym-socks5-client

COPY --from=wdk_builder --chown=10001:10001 /wdk /opt/wdk-sidecar
COPY --chown=10001:10001 scripts/agenthalo-entrypoint.sh /usr/local/bin/agenthalo-entrypoint.sh
COPY --chown=10001:10001 scripts/agenthalo-healthcheck.sh /usr/local/bin/agenthalo-healthcheck.sh

RUN chmod +x /usr/local/bin/agenthalo-entrypoint.sh /usr/local/bin/agenthalo-healthcheck.sh && \
    mkdir -p /data /data/logs /data/nym && \
    chown -R 10001:10001 /data /opt/wdk-sidecar && \
    chmod 700 /data

ENV HOME=/data
ENV AGENTHALO_HOME=/data
ENV AGENTHALO_DASHBOARD_HOST=0.0.0.0
ENV AGENTHALO_DASHBOARD_PORT=3100
ENV AGENTHALO_MCP_HOST=0.0.0.0
ENV AGENTHALO_MCP_PORT=8390
ENV WDK_PORT=7321
ENV WDK_SIDECAR_DIR=/opt/wdk-sidecar

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
