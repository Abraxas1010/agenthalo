FROM rust:1.88-slim-trixie AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev g++ && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
RUN cargo build --release --bin agenthalo --bin agenthalo-mcp-server --bin nucleusdb --bin nucleusdb-server --bin nucleusdb-mcp --bin nucleusdb-discord

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tini curl jq python3 python3-pip docker-cli && rm -rf /var/lib/apt/lists/*
RUN command -v docker >/dev/null
RUN pip3 install --break-system-packages "huggingface_hub[cli]"
RUN groupadd --gid 10001 nucleusdb && useradd --uid 10001 --gid 10001 --home-dir /data --create-home --shell /usr/sbin/nologin nucleusdb
COPY --from=builder /build/target/release/agenthalo /usr/local/bin/
COPY --from=builder /build/target/release/nucleusdb /usr/local/bin/
COPY --from=builder /build/target/release/nucleusdb-server /usr/local/bin/
COPY --from=builder /build/target/release/nucleusdb-mcp /usr/local/bin/
COPY --from=builder /build/target/release/nucleusdb-discord /usr/local/bin/
COPY --from=builder /build/target/release/agenthalo-mcp-server /usr/local/bin/
COPY --from=builder /build/dashboard /dashboard
COPY scripts/nucleusdb-entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh && mkdir -p /data && chown -R 10001:10001 /data /dashboard
USER 10001:10001
VOLUME ["/data"]
ENV AGENTHALO_HOME=/data
ENV NUCLEUSDB_HOME=/data
ENV NUCLEUSDB_DISCORD_DB_PATH=/data/discord_records.ndb
ENV NUCLEUSDB_MCP_HOST=0.0.0.0
ENV NUCLEUSDB_MCP_PORT=3000
ENV NUCLEUSDB_API_PORT=8088
ENV NUCLEUSDB_DASHBOARD_PORT=3100
EXPOSE 3000 3100 8088
ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint.sh"]
