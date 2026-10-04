#!/usr/bin/env bash
# 在 Linux 上编译「魔改版」org.lux.strm-media-info 并打包成 Lux 可安装的 zip。
#
# 用法（在 USBVillage-Plugins 目录下）：
#   bash scripts/build-custom-plugin.sh [x86_64|aarch64]
set -euo pipefail

PLUGIN_ID="org.lux.strm-media-info"
PLUGIN_BIN="lux-plugin-strm-media-info"
VERSION="4.3.0"
ARCH="${1:-x86_64}"

case "$ARCH" in
  x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
  *) echo "unsupported arch: $ARCH" >&2; exit 1 ;;
esac

cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1; then
  echo ">>> 未检测到 cargo，安装 Rust 工具链（最小化 profile）"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

command -v cargo >/dev/null 2>&1 || { echo "cargo 仍不可用" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "需要 python3 用于打包" >&2; exit 1; }

echo ">>> 环境信息"
rustc --version
uname -m
ldd --version 2>/dev/null | head -1 || true

echo ">>> 编译 $PLUGIN_BIN ($TARGET)"
cargo build --release --locked --target "$TARGET" --bin "$PLUGIN_BIN"

BIN="target/$TARGET/release/$PLUGIN_BIN"
[ -f "$BIN" ] || { echo "未找到编译产物 $BIN" >&2; exit 1; }
file "$BIN" || true

OUT="dist/${PLUGIN_ID}-${VERSION}-linux-${ARCH}.zip"
echo ">>> 打包 $OUT"
python3 scripts/package_plugin.py \
  --id "$PLUGIN_ID" \
  --version "$VERSION" \
  --manifest "manifests/${PLUGIN_ID}.json" \
  --binary "$BIN" \
  --platform linux \
  --arch "$ARCH" \
  --output "$OUT"

echo ">>> 完成"
ls -la "$OUT"
sha256sum "$OUT"
echo
echo "校验产物内容："
python3 - "$OUT" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as z:
    for info in z.infolist():
        print(f"  {info.filename}  ({info.file_size} bytes, mode={oct(info.external_attr >> 16)})")
PY
