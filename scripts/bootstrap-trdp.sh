#!/usr/bin/env bash
set -euo pipefail

VERSION="3.0.0.0"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/.cache/tcnopen-$VERSION"
ZIP="$CACHE/$VERSION.zip"
SRC="$CACHE/src"
OUT="$ROOT/src-tauri/binaries"
TOOLS_OUT="$ROOT/tools/trdp-test-peer/bin"
URL="https://sourceforge.net/projects/tcnopen/files/TRDP/$VERSION/$VERSION.zip/download"

command -v curl >/dev/null || { echo "curl is required" >&2; exit 1; }
command -v unzip >/dev/null || { echo "unzip is required" >&2; exit 1; }
command -v make >/dev/null || { echo "make is required" >&2; exit 1; }
command -v cc >/dev/null || { echo "a C compiler (cc) is required" >&2; exit 1; }

mkdir -p "$CACHE" "$OUT" "$TOOLS_OUT"
if [[ ! -f "$ZIP" ]]; then
  echo "Downloading TCNOpen $VERSION from SourceForge..."
  curl -fL --retry 3 --retry-delay 2 --output "$ZIP" "$URL"
fi
if [[ ! -d "$SRC" ]]; then
  mkdir -p "$SRC"
  unzip -q "$ZIP" -d "$SRC"
fi

MAKEFILE="$(find "$SRC" -type f -name Makefile -path '*/trdp/Makefile' -print -quit)"
if [[ -z "$MAKEFILE" ]]; then
  echo "Could not locate trdp/Makefile in TCNOpen archive" >&2
  exit 1
fi
TRDP_DIR="${MAKEFILE%/Makefile}"

case "$(uname -s)" in
  Linux)
    CONFIG="LINUX_config"
    OS_DEFINE="LINUX"
    EXTRA_LIBS=(-pthread -lrt -ldl -luuid)
    ;;
  Darwin)
    CONFIG="OSX_X86_64_config"
    OS_DEFINE="POSIX"
    EXTRA_LIBS=(-pthread)
    ;;
  *)
    echo "Unsupported Unix platform: $(uname -s). Use bootstrap-trdp.ps1 on Windows." >&2
    exit 1
    ;;
esac

if [[ ! -f "$TRDP_DIR/config/$CONFIG" ]]; then
  echo "TCNOpen config $CONFIG not found" >&2
  exit 1
fi
cp "$TRDP_DIR/config/$CONFIG" "$TRDP_DIR/config/config.mk"
# The upstream macOS config predates Apple Silicon and contains an x86-only
# compiler option. TCNOpen's POSIX VOS sources themselves build natively on arm64.
if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
  sed -i.bak 's/-m64//g' "$TRDP_DIR/config/config.mk"
fi

echo "Building TCNOpen $VERSION (PD + MD)..."
make -C "$TRDP_DIR" clean >/dev/null 2>&1 || true
make -C "$TRDP_DIR" MD_SUPPORT=1 libtrdp
LIB="$(find "$TRDP_DIR/bld/output" -type f -name libtrdp.a -print -quit)"
if [[ -z "$LIB" || ! -f "$LIB" ]]; then
  echo "TCNOpen libtrdp.a was not produced" >&2
  exit 1
fi

COMMON_CFLAGS=(
  -std=c11 -O2 -Wall -Wextra -Werror -D"$OS_DEFINE" -DMD_SUPPORT=1 -DL_ENDIAN
  -I"$TRDP_DIR/src/api" -I"$TRDP_DIR/src/common" -I"$TRDP_DIR/src/vos/api"
)
BRIDGE_SOURCES=(
  "$ROOT/src-tauri/native/trdp_bridge_common.c"
  "$ROOT/src-tauri/native/trdp_bridge_node.c"
  "$ROOT/src-tauri/native/trdp_bridge_capture.c"
  "$ROOT/src-tauri/native/trdp_bridge_main.c"
)

echo "Building TauTerm TRDP bridge..."
cc "${COMMON_CFLAGS[@]}" "${BRIDGE_SOURCES[@]}" "$LIB" "${EXTRA_LIBS[@]}" \
  -o "$OUT/tauterm-trdp-bridge"
chmod +x "$OUT/tauterm-trdp-bridge"

echo "Building TCNOpen interoperability reference peer..."
cc "${COMMON_CFLAGS[@]}" "$ROOT/tools/trdp-test-peer/trdp_test_peer.c" "$LIB" \
  "${EXTRA_LIBS[@]}" -o "$TOOLS_OUT/trdp-test-peer"
chmod +x "$TOOLS_OUT/trdp-test-peer"

printf '%s\n%s\n' \
  '{"command":"monitor_open","params":{"mode":"monitor"}}' \
  '{"command":"shutdown"}' \
  | "$OUT/tauterm-trdp-bridge" | grep -q '"command":"shutdown"'
"$TOOLS_OUT/trdp-test-peer" >/dev/null 2>&1 || [[ $? -eq 2 ]]

echo "TRDP bridge ready: $OUT/tauterm-trdp-bridge"
echo "Reference peer ready: $TOOLS_OUT/trdp-test-peer"
echo "TCNOpen source remains in: $SRC"
echo "TCNOpen TRDP is MPL-2.0; see THIRD_PARTY_LICENSES.md."
