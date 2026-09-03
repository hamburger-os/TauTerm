#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/src-tauri/vendor/tcnopen"
NATIVE="$ROOT/src-tauri/native"
BUILD="$ROOT/.cache/trdp-native-build"
OUT="$ROOT/src-tauri/binaries"
TOOLS_OUT="$ROOT/tools/trdp-test-peer/bin"

command -v cmake >/dev/null || { echo "cmake 3.20+ is required" >&2; exit 1; }
command -v cc >/dev/null || { echo "a C compiler is required" >&2; exit 1; }

if [[ ! -f "$VENDOR/src/api/trdp_if_light.h" || ! -f "$VENDOR/src/common/trdp_private.h" ]]; then
  echo "Vendored TCNOpen 3.0.0.0 source is incomplete under $VENDOR" >&2
  exit 1
fi

mkdir -p "$BUILD" "$OUT" "$TOOLS_OUT"

echo "Configuring vendored TCNOpen 3.0.0.0 + TauTerm TRDP native helpers..."
cmake -S "$NATIVE" -B "$BUILD" -DCMAKE_BUILD_TYPE=Release
cmake --build "$BUILD" --config Release --parallel

BRIDGE="$BUILD/bin/tauterm-trdp-bridge"
PEER="$BUILD/bin/trdp-test-peer"
if [[ ! -x "$BRIDGE" || ! -x "$PEER" ]]; then
  echo "TRDP native build did not produce expected executables in $BUILD/bin" >&2
  exit 1
fi

cp "$BRIDGE" "$OUT/tauterm-trdp-bridge"
cp "$PEER" "$TOOLS_OUT/trdp-test-peer"
chmod +x "$OUT/tauterm-trdp-bridge" "$TOOLS_OUT/trdp-test-peer"

printf '%s\n%s\n' \
  '{"command":"monitor_open","params":{"mode":"monitor"}}' \
  '{"command":"shutdown"}' \
  | "$OUT/tauterm-trdp-bridge" | grep -q '"command":"shutdown"'

set +e
"$TOOLS_OUT/trdp-test-peer" >/dev/null 2>&1
peer_status=$?
set -e
if [[ $peer_status -ne 2 ]]; then
  echo "TRDP reference peer usage smoke test failed: exit $peer_status" >&2
  exit 1
fi

echo "TRDP bridge ready: $OUT/tauterm-trdp-bridge"
echo "Reference peer ready: $TOOLS_OUT/trdp-test-peer"
echo "TCNOpen source: $VENDOR (MPL-2.0, vendored 3.0.0.0 snapshot)"
