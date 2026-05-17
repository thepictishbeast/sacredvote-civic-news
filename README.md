# sacredvote-civic-news

Civic news aggregator sidecar for [Sacred.Vote](https://sacred.vote) (closes
Sacred.Vote backlog item #154). Fetches curated RSS/Atom feeds, deduplicates
by URL + title, ranks by recency, and serves the result via JSON HTTP on
port 3005.

The Sacred.Vote main server proxies to this sidecar behind the
`NEWS_AGGREGATOR_ENABLED` paywall flag. The sidecar itself is private —
local-loopback bind only.

## Why a sidecar

1. Cold-start + memory cost of running a fully-async feed fetcher inside
   the main Express service is unattractive.
2. Rust's `feed-rs` parser is well-vetted and handles malformed RSS better
   than the JS alternatives.
3. Standalone binary matches `feedback_standalone_architecture` +
   `feedback_rust_default` conventions for the PlausiDen ecosystem.
4. Easy to roll out / roll back independently of the main service —
   `systemctl restart sacredvote-civic-news` doesn't drop voter sessions.

## Build + run

### With cargo (any Rust 1.83+ toolchain)

```bash
cargo build --release
CIVIC_NEWS_SOURCES="https://example.com/rss,https://another.example/atom" \
  ./target/release/sacredvote-civic-news
```

### With Nix flake (reproducible)

`flake.nix` pins the Rust toolchain (1.83.0) and builds via [crane](https://github.com/ipetkov/crane) for incremental dep-cache reuse:

```bash
# Build
nix build

# Run directly without installing
nix run -- --help

# Open a dev shell with rust-analyzer + cargo-watch
nix develop

# CI-style: build + clippy --deny-warnings + tests + rustfmt
nix flake check
```

### NixOS deployment (via the flake's nixosModules.default)

Drop the sidecar into a NixOS configuration:

```nix
{
  inputs.sacredvote-civic-news.url = "github:thepictishbeast/sacredvote-civic-news";

  outputs = { self, nixpkgs, sacredvote-civic-news, ... }: {
    nixosConfigurations.my-vps = nixpkgs.lib.nixosSystem {
      modules = [
        sacredvote-civic-news.nixosModules.default
        ({ ... }: {
          services.sacredvote-civic-news = {
            enable = true;
            bind = "127.0.0.1:3005";
            sources = [
              "https://example.com/rss"
              "https://another.example/atom"
            ];
            ratingsToml = ./ratings.toml; # optional
            logLevel = "info";
          };
        })
      ];
    };
  };
}
```

The module enables a hardened systemd service (NoNewPrivileges,
ProtectSystem=strict, MemoryDenyWriteExecute, SystemCallFilter=
@system-service minus dangerous syscalls, 512M / 50% CPU / 64 tasks
caps, dedicated `civicnews` system user).

### Non-NixOS systemd install (using cargo)

```bash
sudo ./deploy/install.sh
```

Builds via `cargo build --release`, creates a `civicnews` user, drops
the systemd unit into `/etc/systemd/system/`, enables + starts. Idempotent.

## Endpoints (all GET, JSON responses)

| Path | Purpose |
|------|---------|
| `/health` | 200 OK with service name + version + uptime |
| `/version` | Service identity (semver + git SHA when built with `GIT_SHA=...`) |
| `/sources` | Configured RSS source URLs (HTTPS-only) |
| `/feeds` | Aggregated news items (deduped + recency-ranked; optionally bias-annotated per `CIVIC_NEWS_RATINGS_TOML`) |

## Environment

| Variable | Default | Purpose |
|----------|---------|---------|
| `CIVIC_NEWS_BIND` | `127.0.0.1:3005` | Listen address. **Do not bind to 0.0.0.0** — sidecar should never be public. |
| `CIVIC_NEWS_SOURCES` | (empty) | Comma-separated HTTPS RSS/Atom URLs. Non-HTTPS entries silently dropped. |
| `RUST_LOG` | `info` | Tracing verbosity. |
| `CIVIC_NEWS_RATINGS_TOML` | (unset) | Path to source-rating TOML registry (see `src/registry.rs` doc comment for format). When set, items are annotated with bias / factual / neutrality_score fields on the wire. Unrated sources pass through unannotated. |
| `GIT_SHA` | (unset) | Build-time only; surfaces in `/version` for ops correlation. |

## Source-rating TOML format

When `CIVIC_NEWS_RATINGS_TOML` points at a file, the sidecar loads it at
startup and uses it to annotate matching feed items with bias + factual
tier + a composite neutrality score (0–100). Format:

```toml
[[source]]
url = "https://example.com/rss"
bias = "center"        # left-extreme | left | center-left | center | center-right | right | right-extreme | mixed | unknown
factual = "high"       # very-high | high | mixed | low | unknown

[[source]]
url = "https://another.example/atom"
bias = "center-right"
factual = "very-high"
```

The scoring formula mirrors Sacred.Vote's
`shared/news-neutrality-helpers.ts` (iter #538) so an auditor can compare
the TS and Rust outputs byte-identical:

```
raw   = (1 - bias_distance) * 0.6 + factual_weight * 0.4
score = round(raw * 100)
```

Where `bias_distance ∈ [0, 1]` and `factual_weight ∈ [0, 1]`. "Mixed" or
"unknown" bias yields a NaN score (the wire field is then omitted via
`#[serde(skip_serializing_if = "Option::is_none")]`).

## Design pins

- **LOCAL-LOOPBACK-ONLY-BY-DEFAULT** — binds to `127.0.0.1:3005`. The main
  Sacred.Vote server proxies. The sidecar MUST NOT be exposed to the
  public internet directly.
- **NO-INBOUND-WRITE-ENDPOINTS** — only GET routes. The sidecar fetches
  outbound from feed sources, but exposes only read views to its caller.
- **ASCII-ESCAPED-JSON-RESPONSES** — serde_json handles UTF-8 escaping;
  consumers (Sacred.Vote weekly digest #160) deserialize safely.
- **SOURCE-LIST-IS-ENV-BASED-NOT-DB** — keeps the sidecar independent of
  Sacred.Vote's PostgreSQL state.

## Roadmap

- **v0.1 (this iter)** — HTTP skeleton, /health + /version + /sources +
  /feeds (placeholder), 11+ unit tests, local-loopback binding.
- **v0.2** — Feed fetcher loop: reqwest + feed-rs, dedup by URL + title,
  recency ranking, in-memory cache with TTL.
- **v0.3** — Bias-rating annotation via Sacred.Vote `shared/news-neutrality-helpers.ts`
  (iter #538). The sidecar would tag each item with the neutrality
  scaffold's score so the main app doesn't re-compute.
- **v0.4** — systemd unit + Caddyfile reverse-proxy snippet for the main
  server to forward `/api/civic-news/*` to the sidecar.

## License

MIT.
