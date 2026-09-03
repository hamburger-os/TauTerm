#!/usr/bin/env bash
set -euo pipefail

VERSION="3.0.0.0"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/.cache/tcnopen-$VERSION"
ZIP="$CACHE/$VERSION.zip"
SRC="$CACHE/src"
OUT="$ROOT/src-tauri/binaries"
URL="https://sourceforge.net/projects/tcnopen/files/TRDP/$VERSION/$VERSION.zip/download"

command -v curl >/dev/null || { echo "curl is required" >&2; exit 1; }
command -v unzip >/dev/null || { echo "unzip is required" >&2; exit 1; }
command -v make >/dev/null || { echo "make is required" >&2; exit 1; }
command -v cc >/dev/null || { echo "a C compiler (cc) is required" >&2; exit 1; }

mkdir -p "$CACHE" "$OUT"
if [[ ! -f "$ZIP" ]]; then
  echo "Downloading TCNOpen $VERSION from SourceForge..."
  curl -fL --retry 3 --output "$ZIP" "$URL"
fi
if [[ ! -d "$SRC" ]]; then
  mkdir -p "$SRC"
  unzip -q "$ZIP" -d "$SRC"
fi

TRDP_DIR="$(find "$SRC" -type f -name Makefile -path '*/trdp/Makefile' -print -quit | xargs -r dirname)"
if [[ -z "$TRDP_DIR" || ! -d "$TRDP_DIR" ]]; then
  echo "Could not locate trdp/Makefile in TCNOpen archive" >&2
  exit 1
fi

case "$(uname -s)" in
  Linux)
    CONFIG="LINUX_config"
    OS_DEFINE="LINUX"
    EXTRA_LIBS="-pthread -lrt -ldl"
    ;;
  Darwin)
    CONFIG="OSX_X86_64_config"
    OS_DEFINE="OSX"
    EXTRA_LIBS="-pthread -ldl"
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

echo "Building TCNOpen $VERSION (PD + MD)..."
make -C "$TRDP_DIR" clean >/dev/null 2>&1 || true
make -C "$TRDP_DIR" MD_SUPPORT=1 libtrdp
LIB="$(find "$TRDP_DIR/bld/output" -type f -name libtrdp.a -print -quit)"
if [[ -z "$LIB" ]]; then
  echo "TCNOpen libtrdp.a was not produced" >&2
  exit 1
fi

echo "Building TauTerm TRDP bridge..."
cc -std=c11 -O2 -D"$OS_DEFINE" -DMD_SUPPORT=1 -DL_ENDIAN \
  -I"$TRDP_DIR/src/api" -I"$TRDP_DIR/src/common" -I"$TRDP_DIR/src/vos/api" \
  "$ROOT/src-tauri/native/trdp_bridge.c" "$LIB" $EXTRA_LIBS \
  -o "$OUT/tauterm-trdp-bridge"
chmod +x "$OUT/tauterm-trdp-bridge"

echo "TRDP bridge ready: $OUT/tauterm-trdp-bridge"
echo "TCNOpen source remains in: $SRC"
echo "TCNOpen TRDP is MPL-2.0; see THIRD_PARTY_LICENSES.md."
