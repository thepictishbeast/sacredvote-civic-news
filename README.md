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

```bash
cargo build --release
CIVIC_NEWS_SOURCES="https://example.com/rss,https://another.example/atom" \
  ./target/release/sacredvote-civic-news
```

## Endpoints (all GET, JSON responses)

| Path | Purpose |
|------|---------|
| `/health` | 200 OK with service name + version + uptime |
| `/version` | Service identity (semver + git SHA when built with `GIT_SHA=...`) |
| `/sources` | Configured RSS source URLs (HTTPS-only) |
| `/feeds` | Aggregated news items (placeholder in v0.1; aggregation in next iter) |

## Environment

| Variable | Default | Purpose |
|----------|---------|---------|
| `CIVIC_NEWS_BIND` | `127.0.0.1:3005` | Listen address. **Do not bind to 0.0.0.0** — sidecar should never be public. |
| `CIVIC_NEWS_SOURCES` | (empty) | Comma-separated HTTPS RSS/Atom URLs. Non-HTTPS entries silently dropped. |
| `RUST_LOG` | `info` | Tracing verbosity. |
| `GIT_SHA` | (unset) | Build-time only; surfaces in `/version` for ops correlation. |

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
