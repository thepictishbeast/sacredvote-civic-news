# Changelog

All notable changes to `sacredvote-civic-news` are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

(No unreleased changes.)

## [0.8.1] — 2026-05-19 (LOOP-V3.1#74-#83, #95, #216)

Hygiene + tooling patch release. No behavior change to the
sidecar's `/feeds` annotated-JSON contract. Aligns civic-news with
the sibling Rust crates on the same post-hygiene-trilogy +
ci.sh-runner baseline (sacredvote-axum-poc v0.12.1, plausiden-
watchtower v0.2.0, sacredvote-civic-news v0.8.1).

### Changed
- Repository hygiene pass (LOOP-V3.1#74, #75): `cargo fmt --check` clean
  across all modules + `cargo clippy --all-targets -- -D warnings` clean
  (3 errors fixed — `manual_range_contains` in `neutrality.rs` and two
  `assertions_on_constants` in `main.rs` promoted to `const { assert!(..) }`
  for compile-time enforcement of the request-body / timeout bounds).
- `cargo audit` baseline (LOOP-V3.1#81): 241 deps / 0 advisories.
- **CHANGELOG.md file** introduced (LOOP-V3.1#83, `408d227`). Format
  follows Keep-A-Changelog 1.1.0 + SemVer. Retroactively documents
  v0.1-v0.8 (Docker deployment, factual_label wiring, etc.) from
  git history.
- **Manifest-level `[lints]` policy** (LOOP-V3.1#95, `ae86f08`).
  Migrated the `-D warnings` gate from invocation-time flag into
  `[lints.rust]` / `[lints.clippy]` deny-all at manifest level. Plain
  `cargo clippy` AND `cargo build` (NO `-D warnings` flag) now exit
  clean against the deny gate — the policy is part of the project
  contract, not an external script's responsibility. Closes the
  3-repo trilogy alongside plausiden-watchtower (#93) and
  sacredvote-axum-poc (#94).

### Tooling
- **`scripts/ci.sh` local 5-gate runner** (LOOP-V3.1#216, `ffa0937`).
  Sibling artifact to sacredvote-axum-poc/scripts/ci.sh (#207) and
  plausiden-watchtower/scripts/ci.sh. Same 5-gate shape (fmt +
  build + clippy + test + audit) across all 3 in-scope Rust crates
  so operator habit `bash scripts/ci.sh && git push` works uniformly.
  civic-news has no special feature flags so this is the minimal
  ci.sh shape. Verified at ship time: 72 unit tests pass, 241 deps
  audited, 0 vulnerabilities.

## [0.8.0] — 2026-05-17

### Changed
- Both `bias_label` and `factual_label` are now wired through to the
  annotated `/feeds` JSON response. The factual field was modeled but
  never serialized in 0.7; this closes the dead-code warning and
  exposes the second neutrality dimension to consumers.

## [0.7.0] — 2026-05-17

### Added
- Multi-stage Dockerfile + `.dockerignore` for `docker build` /
  `docker run` deployment paths.

## [0.6.0] — 2026-05-17

### Added
- Reproducible Nix flake build via `crane` (pins Rust 1.83.0,
  incremental dep-cache reuse).
- `nixosModules.default` — a hardened systemd module: NoNewPrivileges,
  ProtectSystem=strict, MemoryDenyWriteExecute, narrow
  SystemCallFilter, 512 MiB / 50 % CPU / 64 task caps, dedicated
  `civicnews` system user.

## [0.5.0] — 2026-05-17

### Added
- Deployment artifacts: hardened systemd unit, Caddy reverse-proxy
  snippet for the main server to forward `/api/civic-news/*` to the
  sidecar, idempotent `deploy/install.sh` + companion
  `deploy/uninstall.sh` (preserves config + logs by default;
  `PURGE_CONFIG=1 PURGE_LOGS=1 REMOVE_USER=1` to fully wipe).

## [0.4.0] — 2026-05-16

### Added
- Per-source bias + factual-tier registry loaded from a TOML file
  (`ratings.toml`). Each annotated `/feeds` item carries `biasLabel`
  + `factualLabel` derived from the registry entry that matches its
  source URL.

## [0.3.0] — 2026-05-15

### Added
- Neutrality module — port of Sacred.Vote's
  `shared/news-neutrality-helpers.ts` (main repo iter #538). Computes
  bias distance + factual-tier weight; same scoring as the main app
  so the sidecar's annotations are byte-identical to what the main
  app would compute locally.

## [0.2.0] — 2026-05-14

### Added
- Feed fetcher loop: `reqwest` (rustls-tls only — no OpenSSL) +
  `feed-rs`, dedups by (normalized URL, normalized title), ranks by
  recency, holds an in-memory cache with TTL so the underlying feed
  servers don't get hit on every `/feeds` request.

## [0.1.0] — 2026-05-13

### Added
- Initial HTTP skeleton: `GET /health`, `GET /version`,
  `GET /sources`, `GET /feeds` (placeholder body). Binds local-
  loopback only by default. 11+ unit tests covering bind config,
  shape locks, and basic plumbing.

[Unreleased]: https://github.com/thepictishbeast/sacredvote-civic-news/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.8.0
[0.7.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.7.0
[0.6.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.6.0
[0.5.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.5.0
[0.4.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.4.0
[0.3.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.3.0
[0.2.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.2.0
[0.1.0]: https://github.com/thepictishbeast/sacredvote-civic-news/releases/tag/v0.1.0
