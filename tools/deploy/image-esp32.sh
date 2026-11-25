#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# image-esp32 —— 生产烧录档装配：ELF → 单文件整片镜像（espflash save-image --merge）。
# 缺省从构建树取 classic ELF 并先过 flash-esp32.sh --check-only；--elf 供流水线
# 复用已构建产物（不重复编译）。装配后自检三点 magic 与整片尺寸，不上产线。
#
# 用法: image-esp32.sh [--release] [--elf <path>] [--chip <esp32|esp32s3>]
#                      [--flash-size <4mb|8mb|16mb>] [--out <path>]
#                      [--bootloader <bin>] [--partitions <csv>]
set -euo pipefail

cd "$(dirname "$0")/../.."
. tools/ci/lib/cargo-artifact.sh

BUILD=debug
ELF=""
OUT=""
BOOT=""
PARTS=""
CHIP=esp32
FLASH=4mb

die() { echo "FAIL  $1" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --release) BUILD=release; shift ;;
    --elf) ELF="${2:?--elf 缺路径}"; shift 2 ;;
    --out) OUT="${2:?--out 缺路径}"; shift 2 ;;
    --bootloader) BOOT="${2:?--bootloader 缺路径}"; shift 2 ;;
    --partitions) PARTS="${2:?--partitions 缺路径}"; shift 2 ;;
    --chip) CHIP="${2:?--chip 缺值}"; shift 2 ;;
    --flash-size) FLASH="${2:?--flash-size 缺值}"; shift 2 ;;
    *) die "未知参数: ${1}（用法见脚本头注释）" ;;
  esac
done

case "$CHIP" in
  esp32) BOOT_OFF=4096 ;;
  esp32s3) BOOT_OFF=0 ;;
  *) die "chip 只支持 esp32 / esp32s3: ${CHIP}" ;;
esac
case "$FLASH" in
  4mb) FLASH_BYTES=4194304 ;;
  8mb) FLASH_BYTES=8388608 ;;
  16mb) FLASH_BYTES=16777216 ;;
  *) die "flash-size 只支持 4mb / 8mb / 16mb: ${FLASH}" ;;
esac

if [ -z "$ELF" ]; then
  echo "== ① 验证面（flash-esp32.sh --check-only）=="
  verif_args=()
  if [ "$BUILD" = release ]; then verif_args+=(--release); fi
  bash tools/deploy/flash-esp32.sh --check-only ${verif_args[@]+"${verif_args[@]}"} \
    || die "验证面未过（flash-esp32.sh --check-only）"
  ELF="$( cd tools/esp32-periph-base \
    && rr3_cargo_artifact xtensa-esp32-espidf "$BUILD" esp32-periph-base )" \
    || die "无法解析 cargo target_directory（metadata 不可用）"
  [ -n "$OUT" ] || OUT="$(dirname "$ELF")/esp32-periph-base-merged.bin"
else
  echo "== ① 复用已构建 ELF（跳过构建）: ${ELF} =="
  [ -f "$ELF" ] || die "ELF 不在树: ${ELF}"
  [ -n "$OUT" ] || OUT="$(dirname "$ELF")/$(basename "${ELF%.elf}")-merged.bin"
fi
[ -f "$ELF" ] || die "ELF 不在树: ${ELF}"
mkdir -p "$(dirname "$OUT")"

echo "== ② 工具链门（espflash 4.x）=="
espflash_ver="$(espflash --version 2>/dev/null)" || die "espflash 未安装（cargo install espflash --locked）"
printf '%s\n' "$espflash_ver" | grep -qE '^espflash 4\.' \
  || die "espflash 非 4.x: ${espflash_ver}（版本钉 4.x）"

echo "== ③ 装配面（save-image --merge）=="
merge_args=(--chip "$CHIP" --merge --flash-size "$FLASH")
[ -n "$BOOT" ] && merge_args+=(--bootloader "$BOOT")
[ -n "$PARTS" ] && merge_args+=(--partition-table "$PARTS")
espflash save-image ${merge_args[@]+"${merge_args[@]}"} "$ELF" "$OUT" \
  || die "save-image 装配失败"

echo "== ④ 自校验面（三点 magic + 整片尺寸）=="
# 引导镜像@BOOT_OFF 首字节 0xE9（classic 0x1000 / S3 0x0，按芯片别）
boot_magic="$(xxd -p -s "$BOOT_OFF" -l 1 "$OUT")"
[ "$boot_magic" = "e9" ] || die "bootloader@${BOOT_OFF} 首字节 ${boot_magic} ≠ e9"
# 分区表@0x8000 头两字节 AA50（ESP-IDF 分区表条目 magic，两类芯片同位）
pt_magic="$(xxd -p -s 32768 -l 2 "$OUT")"
[ "$pt_magic" = "aa50" ] || die "分区表@0x8000 头两字节 ${pt_magic} ≠ aa50"
# app@0x10000 首字节 0xE9（factory 槽 app image magic）
app_magic="$(xxd -p -s 65536 -l 1 "$OUT")"
[ "$app_magic" = "e9" ] || die "app@0x10000 首字节 ${app_magic} ≠ e9"
img_bytes="$(wc -c < "$OUT" | tr -d ' ')"
[ "$img_bytes" = "$FLASH_BYTES" ] \
  || die "镜像尺寸 ${img_bytes} ≠ ${FLASH_BYTES}（${FLASH} 整片；merge padding 漂移即红）"
echo "PASS  三点 magic（${BOOT_OFF}=e9 / 0x8000=aa50 / 0x10000=e9）+ 整片 ${FLASH_BYTES} B"
echo "== ⑤ 产线交付物就绪: ${OUT}（整片直烧 0x0，不触串口）=="
