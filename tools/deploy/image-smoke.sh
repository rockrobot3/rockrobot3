#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# image-smoke —— 发布镜像的两段冒烟（文件面 / 运行面）。
#
# 为什么单点：判据此前只住在 publish.yml 的两段内联 shell 里，本地无法复跑，
# 改一次判据只能靠"打标签→等 CI"往返验证。本脚本把两段收成唯一实现，CI 与
# 本地同源；两段各自独立退出，失败归因不混（容器起不来 / 装机树缺件 /
# 二进制不可执行 / 六模块未起齐 / 出现拒绝 boot）。
#
# 用法:
#   image-smoke.sh file <镜像>   文件面：装机树三件存在 + rrd --help 输出用法
#   image-smoke.sh run  <镜像>   运行面：影子装机根 sim 探测档，六模块真起且零拒绝
#
# 环境:
#   RR3_SMOKE_ALLOW_SKIP=1  无 docker 时打 SKIP 并退 0（本机开发用）。
#                           不设时缺 docker 即红——CI 不得以 SKIP 充绿。
#
# 装配单不能搬单文件到 /tmp：rrd 的 modules-root = 装配单路径的祖父目录
# （profile/boot/load.rs 的定位判据），单文件复制会让模块发现面打空。故运行面
# 复制整棵装机树做影子根，只在副本上改 hal_backend（镜像内原件不动）。
set -euo pipefail
trap 'echo "FAIL  意外退出：${BASH_COMMAND}（行 ${LINENO}；子命令 ${1:-未定}；镜像 ${2:-未定}）" >&2' ERR

usage() {
	sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
}

# docker 缺席时按纪律处理：显式 SKIP（本地）或红（CI）。
rr3_need_docker() {
	if command -v docker >/dev/null 2>&1; then
		return 0
	fi
	if [ "${RR3_SMOKE_ALLOW_SKIP:-0}" = 1 ]; then
		echo "SKIP  未安装 docker，跳过镜像冒烟（RR3_SMOKE_ALLOW_SKIP=1）"
		exit 0
	fi
	echo "::error title=image-smoke::未安装 docker——CI 不得以 SKIP 充绿"
	exit 1
}

# 文件面：三条断言各自独立（缺件 / 不可执行），失败自带现场。
rr3_smoke_file() {
	local img="$1" out rc=0 brief
	if ! docker run --rm --entrypoint sh "$img" -c '
			test -x /usr/bin/rrd || { echo "缺 /usr/bin/rrd"; exit 1; }
			test -f /usr/lib/rockrobot3/contracts/topics.toml || { echo "缺 contracts/topics.toml"; exit 1; }
			test -f /usr/lib/rockrobot3/profiles/distro.toml || { echo "缺 profiles/distro.toml"; exit 1; }
		'; then
		echo "::error title=image-smoke::装机树缺件（镜像内的运行面不完整）"
		exit 1
	fi
	# rrd --help 的退出码是 1（缺装配面属用法错），故判输出而不是退出码。
	out="$(docker run --rm "$img" --help 2>&1)" || rc=$?
	printf '%s\n' "$out" | head -3
	if ! printf '%s' "$out" | grep -q "用法: rrd"; then
		brief="$(printf '%s' "$out" | head -3 | tr '\n' ' ')"
		echo "::error title=image-smoke::镜像内 rrd 未打印用法（rc=${rc}）：${brief:0:600}"
		exit 1
	fi
	echo "PASS  文件面：$img 可执行且装机树完整"
}

# 运行面：sim 探测档真起 rrd，六模块全 spawn、零拒绝。
rr3_smoke_run() {
	local img="$1" rc=0 brief
	docker run --rm --entrypoint sh "$img" -c '
		set -e
		cp -a /usr/lib/rockrobot3 /tmp/rr3root
		sed -i "s/^hal_backend = \"linux\"/hal_backend = \"sim\"/" /tmp/rr3root/profiles/distro.toml
		grep -q "hal_backend = \"sim\"" /tmp/rr3root/profiles/distro.toml
		RR3_DATA_DIR=/tmp RR3_BRAIN_CLOUD=mock RR3_PROBE=sim \
			/usr/bin/rrd --socket /tmp/s.rrd \
				--contracts /tmp/rr3root/contracts \
				--profile /tmp/rr3root/profiles/distro.toml >/tmp/rrd.log 2>&1 &
		pid=$!
		ok=""
		for _ in $(seq 1 45); do
			n=0
			# 镜像的模块集是发布策略（同 stage-image）；此处按镜像应含的全集断言。
			for m in act brain connect interact sense power; do
				grep -q "spawn rr3-$m pid=" /tmp/rrd.log && n=$((n+1))
			done
			[ "$n" -eq 6 ] && ok=1 && break
			sleep 1
		done
		if [ -z "$ok" ]; then
			echo "FAIL 六模块未起齐"
			tail -30 /tmp/rrd.log
			kill $pid 2>/dev/null || true
			exit 1
		fi
		if grep -q "拒绝 boot" /tmp/rrd.log; then
			echo "FAIL 出现拒绝 boot"
			grep -n "拒绝 boot" /tmp/rrd.log | head -5
			tail -10 /tmp/rrd.log
			kill $pid 2>/dev/null || true
			exit 1
		fi
		kill $pid 2>/dev/null || true
		echo "PASS 六模块起齐且零拒绝"
	' >/tmp/img-smoke.log 2>&1 || rc=$?
	if [ "$rc" -ne 0 ]; then
		echo "--- 运行面日志尾部 ---"
		tail -30 /tmp/img-smoke.log || true
		brief="$(tail -12 /tmp/img-smoke.log 2>/dev/null | tr '\n' ' ')"
		echo "::error title=image-run-smoke::未起齐（rc=${rc}）：${brief:0:700}"
		exit 1
	fi
	tail -2 /tmp/img-smoke.log
	echo "PASS  运行面：$img 六模块起齐且零拒绝"
}

[ "$#" -eq 2 ] || usage
case "$1" in
file)
	rr3_need_docker
	rr3_smoke_file "$2"
	;;
run)
	rr3_need_docker
	rr3_smoke_run "$2"
	;;
*) usage ;;
esac
