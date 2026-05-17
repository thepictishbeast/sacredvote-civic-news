#!/usr/bin/env bash
# Install sacredvote-civic-news as a systemd service.
#
# Idempotent: re-running just refreshes the binary + restarts.
# Requires: cargo (for the build), root (for systemd install).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL_PREFIX="${INSTALL_PREFIX:-/opt/sacredvote-civic-news}"
SYSTEMD_DIR="${SYSTEMD_DIR:-/etc/systemd/system}"
SERVICE_USER="${SERVICE_USER:-civicnews}"
RATINGS_TOML="${RATINGS_TOML:-/etc/sacredvote-civic-news/ratings.toml}"

if [[ "$(id -u)" != "0" ]]; then
  echo "must run as root" >&2
  exit 1
fi

echo "[install] building release binary"
cd "${REPO_ROOT}"
cargo build --release

echo "[install] creating service user ${SERVICE_USER} if absent"
if ! id "${SERVICE_USER}" >/dev/null 2>&1; then
  useradd --system --shell /usr/sbin/nologin --home-dir /nonexistent "${SERVICE_USER}"
fi

echo "[install] installing binary to ${INSTALL_PREFIX}/bin/"
install -d "${INSTALL_PREFIX}/bin"
install -m 0755 "${REPO_ROOT}/target/release/sacredvote-civic-news" \
  "${INSTALL_PREFIX}/bin/sacredvote-civic-news"

echo "[install] preparing log + config dirs"
install -d -o "${SERVICE_USER}" -g "${SERVICE_USER}" /var/log/sacredvote-civic-news
install -d /etc/sacredvote-civic-news

if [[ ! -f "${RATINGS_TOML}" ]]; then
  echo "[install] ratings.toml not present at ${RATINGS_TOML}; installing empty stub"
  cat >"${RATINGS_TOML}" <<EOT
# sacredvote-civic-news source-rating registry.
# See https://github.com/thepictishbeast/sacredvote-civic-news/blob/main/src/registry.rs

# [[source]]
# url = "https://example.com/rss"
# bias = "center"
# factual = "high"
EOT
fi

if [[ ! -f /etc/sacredvote-civic-news/env ]]; then
  echo "[install] writing default env file"
  cat >/etc/sacredvote-civic-news/env <<EOT
# sacredvote-civic-news environment overrides.
# CIVIC_NEWS_SOURCES=https://feed-a.example/rss,https://feed-b.example/atom
CIVIC_NEWS_RATINGS_TOML=${RATINGS_TOML}
EOT
fi

echo "[install] installing systemd unit"
install -m 0644 "${REPO_ROOT}/deploy/systemd/sacredvote-civic-news.service" \
  "${SYSTEMD_DIR}/sacredvote-civic-news.service"

echo "[install] reloading systemd + enabling service"
systemctl daemon-reload
systemctl enable --now sacredvote-civic-news.service

echo "[install] done. Status:"
systemctl --no-pager status sacredvote-civic-news.service | head -10
echo ""
echo "  curl http://127.0.0.1:3005/health"
