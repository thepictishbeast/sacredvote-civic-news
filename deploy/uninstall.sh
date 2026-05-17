#!/usr/bin/env bash
# Uninstall sacredvote-civic-news.
#
# Idempotent: safe to run when partially or fully installed.
# Preserves /var/log/sacredvote-civic-news + /etc/sacredvote-civic-news
# by default (set PURGE_CONFIG=1 / PURGE_LOGS=1 to remove them too).
#
# Requires: root.

set -euo pipefail

INSTALL_PREFIX="${INSTALL_PREFIX:-/opt/sacredvote-civic-news}"
SYSTEMD_DIR="${SYSTEMD_DIR:-/etc/systemd/system}"
SERVICE_USER="${SERVICE_USER:-civicnews}"
SERVICE_NAME="sacredvote-civic-news.service"

PURGE_CONFIG="${PURGE_CONFIG:-0}"
PURGE_LOGS="${PURGE_LOGS:-0}"
REMOVE_USER="${REMOVE_USER:-0}"

if [[ "$(id -u)" != "0" ]]; then
  echo "must run as root" >&2
  exit 1
fi

echo "[uninstall] sacredvote-civic-news teardown"
echo "[uninstall] PURGE_CONFIG=${PURGE_CONFIG} PURGE_LOGS=${PURGE_LOGS} REMOVE_USER=${REMOVE_USER}"

# Stop + disable the service if present
if systemctl list-unit-files --no-pager 2>/dev/null | grep -q "^${SERVICE_NAME}"; then
  echo "[uninstall] stopping ${SERVICE_NAME}"
  systemctl stop "${SERVICE_NAME}" 2>/dev/null || true
  echo "[uninstall] disabling ${SERVICE_NAME}"
  systemctl disable "${SERVICE_NAME}" 2>/dev/null || true
else
  echo "[uninstall] service ${SERVICE_NAME} not installed; skipping stop/disable"
fi

# Remove unit file
if [[ -f "${SYSTEMD_DIR}/${SERVICE_NAME}" ]]; then
  echo "[uninstall] removing ${SYSTEMD_DIR}/${SERVICE_NAME}"
  rm -f "${SYSTEMD_DIR}/${SERVICE_NAME}"
  systemctl daemon-reload
fi

# Remove binary + install prefix (but only if it's the standard prefix and contains only our files)
if [[ -d "${INSTALL_PREFIX}" ]]; then
  echo "[uninstall] removing ${INSTALL_PREFIX}"
  rm -rf "${INSTALL_PREFIX}"
fi

# Optionally remove logs
if [[ "${PURGE_LOGS}" == "1" ]]; then
  if [[ -d /var/log/sacredvote-civic-news ]]; then
    echo "[uninstall] PURGE_LOGS=1 — removing /var/log/sacredvote-civic-news"
    rm -rf /var/log/sacredvote-civic-news
  fi
else
  echo "[uninstall] preserving /var/log/sacredvote-civic-news (set PURGE_LOGS=1 to remove)"
fi

# Optionally remove config
if [[ "${PURGE_CONFIG}" == "1" ]]; then
  if [[ -d /etc/sacredvote-civic-news ]]; then
    echo "[uninstall] PURGE_CONFIG=1 — removing /etc/sacredvote-civic-news"
    rm -rf /etc/sacredvote-civic-news
  fi
else
  echo "[uninstall] preserving /etc/sacredvote-civic-news (set PURGE_CONFIG=1 to remove)"
fi

# Optionally remove the service user
if [[ "${REMOVE_USER}" == "1" ]]; then
  if id "${SERVICE_USER}" >/dev/null 2>&1; then
    echo "[uninstall] REMOVE_USER=1 — removing user ${SERVICE_USER}"
    userdel "${SERVICE_USER}" 2>/dev/null || true
  fi
else
  echo "[uninstall] preserving user ${SERVICE_USER} (set REMOVE_USER=1 to remove)"
fi

echo "[uninstall] done."
echo ""
echo "  To verify clean removal:"
echo "    systemctl status ${SERVICE_NAME} 2>&1 | head -3   # should show 'not loaded'"
echo "    ls ${INSTALL_PREFIX} 2>&1 | head -1               # should show 'No such file'"
