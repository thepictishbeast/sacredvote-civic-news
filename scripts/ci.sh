#!/usr/bin/env bash
#
# scripts/ci.sh — Local CI gate for sacredvote-civic-news.
#
# Runs the full pre-commit / pre-push battery. Exit 0 = ship; exit 1
# = something broken.
#
# Sibling artifact to sacredvote-axum-poc/scripts/ci.sh (LOOP-V3.1#207).
# Same 5-gate shape across all in-scope Rust crates so operators get
# consistent ship-readiness signals regardless of which repo.
#
# Gates:
#   1. cargo fmt --check     — formatting clean
#   2. cargo build           — compiles
#   3. cargo clippy --all-targets  — no lints (manifest-level deny)
#   4. cargo test --quiet    — full suite passes
#   5. cargo audit           — no known-vulnerable crates in Cargo.lock
#
# civic-news has no special feature flags; the build is a single
# crate with no conditional compilation paths. If that changes
# (e.g. journal/no-journal split), this script gets the same kind
# of feature-set override that plausiden-watchtower's ci.sh uses.

set -euo pipefail

if [[ ! -f Cargo.toml ]]; then
  echo "FATAL: must run from sacredvote-civic-news repo root" >&2
  exit 2
fi

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
hdr()   { printf '\n\033[1;36m═══ %s ═══\033[0m\n' "$*"; }

failures=0
ran=0

run_gate() {
  local name="$1"
  shift
  ran=$((ran + 1))
  hdr "Gate ${ran}: ${name}"
  if "$@"; then
    green "  ✓ ${name} passed"
  else
    red "  ✗ ${name} FAILED"
    failures=$((failures + 1))
  fi
}

run_gate "cargo fmt --check" cargo fmt --check
run_gate "cargo build (debug)" cargo build
run_gate "cargo clippy --all-targets" cargo clippy --all-targets
run_gate "cargo test --quiet" cargo test --quiet
run_gate "cargo audit" cargo audit

echo ""
if [[ "${failures}" -eq 0 ]]; then
  green "════════════════════════════════════════"
  green "  ALL ${ran} GATES PASSED — safe to commit/push"
  green "════════════════════════════════════════"
  exit 0
else
  red "════════════════════════════════════════"
  red "  ${failures} / ${ran} GATES FAILED — DO NOT push"
  red "════════════════════════════════════════"
  exit 1
fi
