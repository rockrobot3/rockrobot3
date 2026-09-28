#!/bin/bash
# firmware-rs 烧录脚本：解析 esp-idf-sys 构建产物 flasher_args.json 并用 esptool 烧录
# 用法：./flash.sh [串口]   （默认 /dev/tty.usbserial-1410）
set -e
PORT="${1:-/dev/tty.usbserial-1410}"
ESPTOOL="$HOME/.espressif/python_env/idf6.1_py3.12_env/bin/esptool"
cd "$(dirname "$0")"

TARGET_DIR="target/xtensa-esp32-espidf/release"
ARGS_JSON=$(find $TARGET_DIR/build/esp-idf-sys-*/out -maxdepth 2 -name flasher_args.json | head -1)
if [ -z "$ARGS_JSON" ]; then
  echo "错误：未找到 flasher_args.json，请先 cargo +esp build --release"
  exit 1
fi
BUILD_DIR=$(dirname "$ARGS_JSON")
echo "构建目录：$BUILD_DIR"

# python 解析 flash_files（偏移->文件）
OFFSET_FILES=$(python3 - "$ARGS_JSON" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
print(" ".join(f"{o}={f}" for o, f in d["flash_files"].items()))
EOF
)

echo "烧录映射：$OFFSET_FILES"

FLASH_ARGS=""
for pair in $OFFSET_FILES; do
  off="${pair%%=*}"; f="${pair#*=}"
  FLASH_ARGS="$FLASH_ARGS $off $BUILD_DIR/$f"
done

exec "$ESPTOOL" --chip esp32 --port "$PORT" --baud 921600 \
  --before default_reset --after hard_reset write_flash $FLASH_ARGS
