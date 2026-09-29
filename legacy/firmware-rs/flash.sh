#!/bin/bash
# firmware-rs 烧录脚本
# ⚠ 关键：esp-idf-sys 构建产物里的 libespidf.bin 是不含 Rust 代码的桩程序，
# 直接烧它开机后 app_main 秒返回、无任何 Rust 输出。
# 必须用 esptool elf2image 把 cargo 链接产物 firmware-rs（ELF）转成 app 镜像再烧。
# 用法：./flash.sh [串口]   （默认 /dev/tty.usbserial-1410）
set -e
PORT="${1:-/dev/tty.usbserial-1410}"
ESPTOOL="$HOME/.espressif/python_env/idf6.1_py3.12_env/bin/esptool"
cd "$(dirname "$0")"

ELF="target/xtensa-esp32-espidf/release/firmware-rs"
if [ ! -f "$ELF" ]; then
  echo "错误：未找到 $ELF，请先 cargo +esp build --release"
  exit 1
fi

# 1) ELF → app 镜像（dio / 40MHz / 2MB，与板上 flash 一致）
"$ESPTOOL" --chip esp32 elf2image --flash_mode dio --flash_freq 40m --flash_size 2MB \
  "$ELF" -o /tmp/firmware-rs.bin

# 2) 烧录分区表 + app（bootloader 首次烧过即可，换 IDF 版本才需要重烧）
PT=$(find target/xtensa-esp32-espidf/release/build/esp-idf-sys-*/out/build \
  -path "*partition_table/partition-table.bin" | head -1)
if [ -z "$PT" ]; then
  echo "错误：未找到 partition-table.bin"
  exit 1
fi

# 波特率用 460800：921600 在这块板（杜邦线供电）上偶发握手失败
exec "$ESPTOOL" --chip esp32 --port "$PORT" --baud 460800 \
  --before default-reset --after hard-reset \
  write-flash 0x8000 "$PT" 0x10000 /tmp/firmware-rs.bin
