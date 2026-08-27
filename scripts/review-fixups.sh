#!/usr/bin/env bash
set -euo pipefail

git checkout origin/master -- \
  src-tauri/src/transfer/io.rs \
  src-tauri/src/transfer/orchestrator.rs \
  src-tauri/src/transfer/serial_transfer.rs \
  src-tauri/src/transfer/xmodem.rs \
  src-tauri/src/transfer/ymodem.rs

# Guard against the broad token-replacement failure mode found in review.
if grep -RInE 'external_pathuffer|external_pathox|bridge_pathuffer|bridge_pathox' src-tauri/src src; then
  echo 'accidental substring rename remains' >&2
  exit 1
fi
