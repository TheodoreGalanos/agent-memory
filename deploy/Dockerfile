# ABOUTME: Production image for the memory Host and its Node worker pool (§20.2).
# ABOUTME: Multi-stage: Rust host binary, then Node runtime with the pinned Pi packages.
FROM rust:1.96-bookworm AS host-build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY evals/property-location ./evals/property-location
COPY evals/formation ./evals/formation
COPY evals/judgement ./evals/judgement
RUN cargo build --release -p memory-host

FROM node:22-bookworm-slim AS runtime
ENV NODE_ENV=production
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates python3 tini && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# The pinned Pi source is fetched by `npm run setup` on the build machine and copied in.
COPY package.json package-lock.json ./
COPY .upstream ./.upstream
COPY packages ./packages
COPY contracts ./contracts
COPY scripts ./scripts
COPY evals/property-location ./evals/property-location
RUN npm ci --ignore-scripts --omit=dev && npm run build:pi
COPY --from=host-build /src/target/release/memory-host /usr/local/bin/memory-host
RUN useradd --system --home /app --uid 10001 memory && chown -R memory:memory /app
USER memory
EXPOSE 7331
HEALTHCHECK --interval=30s --timeout=5s CMD ["sh", "-c", "wget -qO- http://127.0.0.1:7331/readyz || exit 1"]
ENTRYPOINT ["/usr/bin/tini", "--"]
# The host reads its private configuration from a mounted file; secrets arrive as files or env.
CMD ["memory-host", "/run/memory/host.json"]
