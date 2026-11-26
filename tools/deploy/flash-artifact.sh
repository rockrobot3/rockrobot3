#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# flash-artifact —— 烧录 CI 制品（ELF），**不构建**（W7-b4 起：本地资源有限，
# 固件一律来自流水线制品；本脚本只做校验 + espflash）。
#
# 为什么必须显式带分区表：classic/S3 共用 crate 根自定义 partitions.csv（factory
# 0x2F0000 + spool 1MB）。espflash 从当前目录猜 partitions.csv；在下载目录直接
# 跑会把内置默认表烧上去，spool 分区缺位→语音段静默降级。本脚本把分区表钉到
# 工程内真值，并在烧录前核验 spool 条目（缺即拒烧，fail-closed）。
#
# 为什么建议显式带 bootloader：ELF 只含 app，不含引导镜像；espflash 不给
# --bootloader 时用其自带镜像（IDF 版本与 app 不保证一致）。发布包随带与 app
# 同版 bootloader 时用 --bootloader 指过去；未带时 PASS 行如实打印"内置"。
set -euo pipefail

usage() {
  cat <<'EOF'
用法:
  bash tools/deploy/flash-artifact.sh <elf> [--chip esp32|esp32s3]
                                       [--bootloader <bin>] [--port <dev>] [--monitor]
例:
  bash tools/deploy/flash-artifact.sh ./rr3-periph-base-esp32-safe-dev.elf \
    --bootloader ./rr3-bootloader-esp32-dev.bin --monitor
EOF
  exit "${1:-2}"
}

[ $# -ge 1 ] || usage
case "$1" in -h|--help) usage 0 ;; esac
elf="$1"
shift
chip="esp32"
boot=""
port=""
monitor=0
while [ $# -gt 0 ]; do
  case "$1" in
    --chip) chip="${2:?--chip 缺参数}"; shift 2 ;;
    --bootloader) boot="${2:?--bootloader 缺参数}"; shift 2 ;;
    --port) port="${2:?--port 缺参数}"; shift 2 ;;
    --monitor) monitor=1; shift ;;
    -h|--help) usage ;;
    *) echo "ERROR: 未知参数 $1" >&2; usage ;;
  esac
done

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内（需从仓库任意子目录运行）" >&2
  exit 1
}
csv="$root/tools/esp32-periph-base/partitions.csv"
die() { echo "ERROR: $*" >&2; exit 1; }

echo "== ① 输入校验（制品 + 工具链，不构建）=="
[ -f "$elf" ] || die "ELF 不存在: ${elf}（先下载 release 资产或 Actions 固件制品）"
[ "$(head -c 4 "$elf" | od -An -tx1 | tr -d ' \n')" = "7f454c46" ] || die "不是 ELF: $elf"
[ -f "$csv" ] || die "分区表缺位: $csv"
grep -Eq '^[[:space:]]*spool' "$csv" || die "分区表无 spool 条目: ${csv}（烧上去语音段会静默降级）"
boot_desc="espflash 内置"
if [ -n "$boot" ]; then
  [ -f "$boot" ] || die "bootloader 不存在: $boot"
  [ "$(head -c 1 "$boot" | od -An -tx1 | tr -d ' \n')" = "e9" ] \
    || die "不是 ESP 引导镜像（首字节应为 e9）: $boot"
  boot_desc="$boot"
fi
command -v espflash >/dev/null 2>&1 || die "espflash 未安装（cargo install espflash --locked）"
espflash --version | grep -qE '^espflash 4\.' || die "espflash 非 4.x: $(espflash --version)"
case "$chip" in esp32|esp32s3) ;; *) die "--chip 只支持 esp32/esp32s3: $chip" ;; esac
echo "PASS  ELF=$elf chip=$chip bootloader=$boot_desc partition-table=$csv"

echo "== ② 烧录（espflash，bootloader/分区表/应用一次写入；NVS 保留）=="
args=(flash --chip "$chip" --partition-table "$csv")
[ -n "$boot" ] && args+=(--bootloader "$boot")
[ -n "$port" ] && args+=(--port "$port")
[ "$monitor" = 1 ] && args+=(--monitor)
espflash "${args[@]}" "$elf" || die "espflash 烧录失败（查串口占用/供电/波特率）"
echo "PASS  烧录完成；看门狗/monitor 输出以 [spool]/[mic]/[mem] 行取证"
