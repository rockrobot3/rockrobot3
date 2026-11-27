#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 发布固件集装配（publish firmware job 的产物合同，2026-11-27 批135 从 workflow 收出）：
# 把 classic safe / S3 full 两只 release ELF 与同树 bootloader 装配成对外发布的
# 七件集，并做三道只读判据：
#   ① ELF 与 bootloader 必须在（缺一件即红——发布集缺件比不发更贵）；
#   ② 分区表必须含 spool 条目（CONFIG_PARTITION_TABLE_CUSTOM 未生效时编译照绿，
#      但板上没有 spool，只能靠发布面拦）；
#   ③ bootloader 首字节必须是 e9（不是 ESP 引导镜像即红）。
# 名称与 sha 清单是发布合同的一部分：改名/加件走发布评审，不在这里悄悄改。
#
# 用法：bash tools/deploy/fw-set.sh --from <classic-elf> <s3-elf> \
#         <classic-bootloader> <s3-bootloader> [--out <目录>]
#   缺省输出到仓根（CI 的 GITHUB_WORKSPACE）；--out 供本地干跑（不污染工作区）。
# 退出码：0 = 七件齐且判据全过；1 = 缺件/判据红；2 = 用法错。
set -euo pipefail

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——请在本仓内执行。" >&2
  exit 1
}
out="$root"
args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --from) shift; [ $# -ge 4 ] || { echo "ERROR: --from 需要四个路径。" >&2; exit 2; }
            args=("$1" "$2" "$3" "$4"); shift 4 ;;
    --out)  shift; out="${1:?--out 缺参数}"; shift ;;
    *)      echo "用法: $0 --from <classic-elf> <s3-elf> <classic-boot> <s3-boot> [--out <目录>]" >&2
            exit 2 ;;
  esac
done
[ ${#args[@]} -eq 4 ] || {
  echo "用法: $0 --from <classic-elf> <s3-elf> <classic-boot> <s3-boot> [--out <目录>]" >&2
  exit 2
}
mkdir -p "$out"

rr3_fw_copy() {
  local src="$1" dst="$out/$2"
  [ -f "$src" ] || { echo "::error title=fw-elf::发布集缺件: ${src}"; exit 1; }
  cp "$src" "$dst"
}

# ③ bootloader 首字节 e9：不是 ESP 引导镜像就不进发布集。
rr3_fw_magic() {
  local f="$1" b
  b="$(head -c 1 "$f" | od -An -tx1 | tr -d ' \n')"
  if [ "$b" != "e9" ]; then
    echo "::error title=fw-boot::${f##*/} 首字节非 e9（不是 ESP 引导镜像）"
    exit 1
  fi
}

# ② 分区表 spool 条目：发布面必拦（板侧没有 spool = 存储承载缺失）。
rr3_fw_parts() {
  cp "$root/tools/esp32-periph-base/partitions.csv" "$out/rr3-partitions.csv"
  if ! grep -Eq '^[[:space:]]*spool' "$out/rr3-partitions.csv"; then
    echo "::error title=fw-partitions::rr3-partitions.csv 缺 spool 条目"
    exit 1
  fi
}

rr3_fw_copy "${args[0]}" rr3-periph-base-esp32-safe-dev.elf
rr3_fw_copy "${args[1]}" rr3-periph-base-esp32s3-full-dev.elf
rr3_fw_copy "${args[2]}" rr3-bootloader-esp32-dev.bin
rr3_fw_copy "${args[3]}" rr3-bootloader-esp32s3-dev.bin
rr3_fw_parts
rr3_fw_magic "$out/rr3-bootloader-esp32-dev.bin"
rr3_fw_magic "$out/rr3-bootloader-esp32s3-dev.bin"
cp "$root/tools/deploy/README.md" "$out/FLASHING.md"
test -s "$out/FLASHING.md"
ls -l "$out"/rr3-periph-base-*.elf "$out"/rr3-bootloader-*.bin \
      "$out"/rr3-partitions.csv "$out"/FLASHING.md
(
  cd "$out"
  # sha256 工具：Linux runner 有 sha256sum；macOS 本机干跑用 shasum -a 256。
  if command -v sha256sum >/dev/null 2>&1; then
    sha=(sha256sum)
  else
    sha=(shasum -a 256)
  fi
  "${sha[@]}" rr3-periph-base-esp32-safe-dev.elf \
              rr3-periph-base-esp32s3-full-dev.elf \
              rr3-bootloader-esp32-dev.bin rr3-bootloader-esp32s3-dev.bin \
              rr3-partitions.csv FLASHING.md \
    | tee firmware-sha256.txt
)
