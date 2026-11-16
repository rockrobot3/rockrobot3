#!/usr/bin/env bash
# audio-rec —— 板面音频收流维护窗封装（#16 第二问取证入口，2026-11-16）。
#
# 为什么需要它：rr3-audio-rec 与 rr3-sense 竞争同一条板面 UART（flock 单消费者），
# 直接跑会被"设备忙"拒；而手工 stop/restore 容易把五模块留在停摆态。本脚本把
# "停 rrd → 等 flock 释放 → 收流 → 必还原 → 验证五模块"收敛成唯一入口：收兵
# 路径由 trap 保证，取证会话中途 Ctrl-C/信号也会还原。
#
# 机制与策略分离：
#   - 机制面（收流/拆帧/seq 记账/wav 落盘）全在 rr3-audio-rec，本脚本不复制
#     任何帧语义，也不解析业务字段；
#   - 本脚本只做维护窗编排：停谁、等什么、何时还原、把哪几行读数带回。
#
# 用法：
#   sudo tools/deploy/audio-rec.sh [--bin <path>] [--dev <tty>] [--baud <n>]
#                                  [--secs <n>] [--out <wav>] [--log <path>]
#                                  [--no-restore] [--dry-run]
#
# 缺省：bin=rr3-audio-rec（PATH 查找）、dev=/dev/ttyAMA0、baud=1500000、
#       secs=1800、out=/tmp/rr3-audio-rec-<unix秒>.wav、log=<out>.log
# 退出码：0 收流正常且还原成功 / 1 前置失败或还原失败 / 2 用法错误；
#         其余＝rr3-audio-rec 自身退出码（0 收兵 / 1 开口握手或 0 分片 /
#         3 看门狗停滞恢复 / 4 重连失败，详见其模块文档）。
set -euo pipefail

BIN=rr3-audio-rec
DEV=/dev/ttyAMA0
BAUD=1500000
SECS=1800
OUT=""
LOG=""
RESTORE=1
DRY=0

usage() {
	sed -n '3,26p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
}

while [ $# -gt 0 ]; do
	case "$1" in
		--bin) BIN=${2:-}; shift 2 ;;
		--dev) DEV=${2:-}; shift 2 ;;
		--baud) BAUD=${2:-}; shift 2 ;;
		--secs) SECS=${2:-}; shift 2 ;;
		--out) OUT=${2:-}; shift 2 ;;
		--log) LOG=${2:-}; shift 2 ;;
		--no-restore) RESTORE=0; shift ;;
		--dry-run) DRY=1; shift ;;
		-h|--help) usage ;;
		*) echo "未知参数: $1（--help 看用法）" >&2; exit 2 ;;
	esac
done

[ -n "$BIN" ] && [ -n "$DEV" ] && [ -n "$BAUD" ] && [ -n "$SECS" ] || usage

if [ "$DRY" -eq 0 ] && [ "$(id -u)" -ne 0 ]; then
	echo "需要 root（systemctl stop/start rrd）；用 sudo 重跑，或先 --dry-run 看编排" >&2
	exit 1
fi

if ! command -v "$BIN" >/dev/null 2>&1 && [ ! -x "$BIN" ]; then
	echo "找不到可执行 ${BIN}：先按文档「产物供给」装到 PATH（本脚本不构建）" >&2
	exit 1
fi

if [ ! -e "$DEV" ]; then
	echo "串口不存在: $DEV" >&2
	exit 1
fi

stamp=$(date +%s)
[ -n "$OUT" ] || OUT="/tmp/rr3-audio-rec-$stamp.wav"
[ -n "$LOG" ] || LOG="$OUT.log"

restore() {
	local rc=$?
	if [ "$RESTORE" -eq 0 ]; then
		echo "[audio-rec] --no-restore：rrd 保持停止（收兵后需人工起链）" >&2
		return "$rc"
	fi
	echo "[audio-rec] 还原：systemctl start rrd" >&2
	if ! systemctl start rrd; then
		echo "[audio-rec] 还原失败：rrd 起不来，立即人工介入" >&2
		return 1
	fi
	i=0
	while [ "$i" -lt 30 ]; do
		if systemctl is-active --quiet rrd; then
			break
		fi
		sleep 0.5
		i=$((i + 1))
	done
	if ! systemctl is-active --quiet rrd; then
		echo "[audio-rec] 还原失败：rrd 未 active" >&2
		return 1
	fi
	if command -v rr3-ctl >/dev/null 2>&1; then
		echo "[audio-rec] 还原后模块态：" >&2
		rr3-ctl status 2>&1 | head -8 >&2 || true
	fi
	return "$rc"
}
trap restore EXIT
trap 'exit 130' INT TERM

echo "[audio-rec] 编排：stop rrd → 等 $DEV 释放 → $BIN $DEV --secs $SECS --baud $BAUD --out $OUT"
if [ "$DRY" -eq 1 ]; then
	echo "[audio-rec] --dry-run：不执行停链/收流（目标文件 ${OUT} / 日志 ${LOG}）"
	trap - EXIT
	exit 0
fi

echo "[audio-rec] 停 rrd（五模块随守护停止）" >&2
systemctl stop rrd

# flock 释放等待：fuser 在位时用它判独占；缺位则按 2s 固定退避（板上最小镜像
# 可能无 psmisc，退避是保守策略，不引入新依赖）。
if command -v fuser >/dev/null 2>&1; then
	i=0
	while [ "$i" -lt 20 ]; do
		if ! fuser -s "$DEV" 2>/dev/null; then
			break
		fi
		sleep 0.5
		i=$((i + 1))
	done
else
	sleep 2
fi

rc=0
set +e
"$BIN" "$DEV" --secs "$SECS" --baud "$BAUD" --out "$OUT" 2>&1 | tee "$LOG"
rc=${PIPESTATUS[0]}
set -e

echo "[audio-rec] 收流退出码=${rc}；wav=${OUT}；日志=${LOG}" >&2
if [ -s "$LOG" ]; then
	grep -E 'gaps=|restarts=|seq|分片|重连|坏帧' "$LOG" | tail -20 >&2 || true
fi
exit "$rc"
