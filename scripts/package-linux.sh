#!/usr/bin/env bash
# Cross-compile idorc-server for Linux x86_64 and stage a runnable package.
# Requires: rustup target x86_64-unknown-linux-gnu, cargo-zigbuild, zig, curl/tar.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=x86_64-unknown-linux-gnu
ORT_VER="${ORT_VER:-1.24.2}"
OUT="${OUT:-$ROOT/dist/linux-x86_64}"
ORT_URL="https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VER}/onnxruntime-linux-x64-${ORT_VER}.tgz"

cd "$ROOT"

echo "==> cargo zigbuild --release --target $TARGET --bin idorc-server"
cargo zigbuild --release --target "$TARGET" --bin idorc-server

rm -rf "$OUT"
mkdir -p "$OUT"

cp -f "target/$TARGET/release/idorc-server" "$OUT/"
cp -f default.toml start.sh stop.sh "$OUT/"
chmod +x "$OUT/start.sh" "$OUT/stop.sh"

if [[ -d models ]]; then
  echo "==> copying models/"
  cp -a models "$OUT/"
fi

echo "==> fetching ONNX Runtime ${ORT_VER} shared library"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fsSL "$ORT_URL" -o "$TMP/ort.tgz"
tar -xzf "$TMP/ort.tgz" -C "$TMP"
LIBDIR="$(find "$TMP" -type d -name lib | head -1)"
# Prefer versioned soname + symlink so dlopen("libonnxruntime.so") works.
cp -a "$LIBDIR"/libonnxruntime.so* "$OUT/"

# Ensure libonnxruntime.so exists for ort load-dynamic default name.
if [[ ! -e "$OUT/libonnxruntime.so" ]]; then
  REAL="$(ls "$OUT"/libonnxruntime.so.* | head -1)"
  ln -sf "$(basename "$REAL")" "$OUT/libonnxruntime.so"
fi

cat > "$OUT/README-DEPLOY.txt" <<EOF
Linux x86_64 package (zigbuild + load-dynamic ORT)

Contents:
  idorc-server          binary
  libonnxruntime.so*    ONNX Runtime ${ORT_VER} (must sit next to binary)
  default.toml          config
  start.sh / stop.sh
  models/               optional; omit to download on first run

Run:
  ./start.sh

ORT is loaded from ./libonnxruntime.so (beside the binary).
Override with: export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
EOF

echo "==> package ready: $OUT"
ls -lh "$OUT"
