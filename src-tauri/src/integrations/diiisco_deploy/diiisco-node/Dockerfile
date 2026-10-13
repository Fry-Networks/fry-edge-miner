###############################################################################
# Diiisco Node Dockerfile
#
# Multi-stage build:
#   Stage 1 - Clone repo, inject wallet config, compile TypeScript
#   Stage 2 - Slim runtime image with only compiled JS + deps
#
# Build args (injected at build time from 1Password / CI):
#   ALGO_ADDRESS   - Your Algorand wallet address
#   ALGO_MNEMONIC  - Your 24-word Algorand mnemonic
#   API_KEY        - Bearer token for the Diiisco API endpoint
#
# Usage:
#   docker build \
#     --build-arg ALGO_ADDRESS="YOUR_ADDR" \
#     --build-arg ALGO_MNEMONIC="YOUR_MNEMONIC" \
#     --build-arg API_KEY="sk-your-production-key" \
#     -t diiisco-node:latest .
###############################################################################

# ── Stage 1: Build ──────────────────────────────────────────────────────
FROM node:22-slim AS builder

RUN apt-get update && apt-get install -y git curl && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Upstream is pinned to one reviewed commit, never to moving HEAD. Evidence
# (B1 review, 2026-10-13): this commit is upstream HEAD as of 2026-10-09; its
# template listens on api port 8080 on 127.0.0.1 by default, so the seds below
# move it to 8181 on all interfaces (the compose publishes it on host loopback
# only), `/health` is unauthenticated and `/v1/*` is guarded by the bearer. A
# pin bump changes the image tag, which triggers the one-time gated rebuild.
ARG DIIISCO_NODE_SHA=45ef1cc8c68a804d716be7b5511a9fece36b0f85
RUN git init . && \
    git remote add origin https://github.com/Diiisco-Inc/diiisco-node.git && \
    git fetch --depth 1 origin "$DIIISCO_NODE_SHA" && \
    git checkout --detach FETCH_HEAD && \
    test "$(git rev-parse HEAD)" = "$DIIISCO_NODE_SHA"

# Install dependencies. Upstream's package.json has historically imported
# `json-canonicalize` without declaring it, so `npm install` alone left it
# missing and the container died at runtime with MODULE_NOT_FOUND (F10).
# Install it explicitly so the dependency is always present in node_modules.
RUN npm install && npm install --save json-canonicalize

# Build args — these are injected at image build time (CI/CD pipeline)
ARG ALGO_ADDRESS
ARG ALGO_MNEMONIC
ARG API_KEY=""

# Generate environment.ts from the upstream template (example.environment.ts)
# so it always matches upstream's Environment contract: the configureEnvironment
# export, full typing, and every required field. Hand-writing this file drifts
# from upstream and drops exports (root cause of the TS2613/TS2614 build failure).
# We inject credentials + the FEM compose ollama endpoint + a container-writable
# peer-id path. sed uses '#' as delimiter (absent from base32 addrs / bip39 words).
RUN cp src/environment/example.environment.ts src/environment/environment.ts && \
    sed -i "s#YOUR_ALGORAND_ADDRESS_HERE#${ALGO_ADDRESS}#g" src/environment/environment.ts && \
    sed -i "s#YOUR_ALGORAND_MNEMONIC_HERE#${ALGO_MNEMONIC}#g" src/environment/environment.ts && \
    sed -i "s#\"sk-testkey1\"#\"${API_KEY}\"#g" src/environment/environment.ts && \
    sed -i "/\"sk-testkey2\"/d" src/environment/environment.ts && \
    sed -i "s#baseURL: \"http://localhost\"#baseURL: \"http://diiisco-ollama\"#g" src/environment/environment.ts && \
    sed -i "s#\"~/Desktop/\"#\"/app/data/\"#g" src/environment/environment.ts && \
    sed -i 's#^    port: 8080,#    port: 8181,\n    host: "0.0.0.0",#' src/environment/environment.ts && \
    grep -q '^    port: 8181,$' src/environment/environment.ts && \
    grep -q '^    host: "0.0.0.0",$' src/environment/environment.ts

# Compile TypeScript. Force-emit JS even when upstream ships genuine type bugs
# (e.g. statusPages.ts string|string[]) so the per-user image build never dies.
# Guarantee dist/ exists so the Stage-2 COPY never fails; the check below then
# fails the build if the entry point (dist/dev.js) was not produced.
RUN (npm run build || npx tsc --noEmitOnError false --skipLibCheck || true) && mkdir -p dist

# The self-starting entry is dist/dev.js (dist/index.js is a library export).
# Fail the build rather than ship an image whose CMD cannot start.
RUN test -f dist/dev.js

# ── Stage 2: Runtime ────────────────────────────────────────────────────
FROM node:22-slim

RUN apt-get update && apt-get install -y curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy compiled output and dependencies
COPY --from=builder /build/dist ./dist
COPY --from=builder /build/node_modules ./node_modules
COPY --from=builder /build/package.json ./package.json

# Fallback: if no dist/ was produced, copy src/ directly
COPY --from=builder /build/src ./src

RUN mkdir -p /app/data

EXPOSE 8181

# dist/dev.js is the self-starting entry and loads the environment.ts written
# above. No runtime package fetch: everything the node needs is baked in at
# build time. F10: do NOT swallow stderr — the real error must reach
# `docker logs` so failures are diagnosable.
CMD ["node", "dist/dev.js"]
