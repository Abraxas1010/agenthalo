FROM rust:1.88-slim-trixie AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev g++ && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
RUN cargo build --release --bin nucleusdb --bin nucleusdb-server --bin nucleusdb-mcp --bin nucleusdb-discord

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tini && rm -rf /var/lib/apt/lists/*
RUN groupadd --gid 10001 nucleusdb && useradd --uid 10001 --gid 10001 --home-dir /data --create-home --shell /usr/sbin/nologin nucleusdb
COPY --from=builder /build/target/release/nucleusdb /usr/local/bin/
COPY --from=builder /build/target/release/nucleusdb-server /usr/local/bin/
COPY --from=builder /build/target/release/nucleusdb-mcp /usr/local/bin/
COPY --from=builder /build/target/release/nucleusdb-discord /usr/local/bin/
COPY --from=builder /build/dashboard /dashboard
COPY deploy/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh && mkdir -p /data && chown -R 10001:10001 /data /dashboard
USER 10001:10001
VOLUME ["/data"]
ENV NUCLEUSDB_DISCORD_DB_PATH=/data/discord_records.ndb
ENV NUCLEUSDB_MCP_HOST=0.0.0.0
ENV NUCLEUSDB_MCP_PORT=3000
ENV NUCLEUSDB_API_ADDR=0.0.0.0:8088
ENV NUCLEUSDB_DASHBOARD_PORT=3100
EXPOSE 3000 3100 8088
ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint.sh"]
