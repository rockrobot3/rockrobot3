#!/usr/bin/env bash
# verify-board-config —— 装机面 config 漂移门（批N38 件二，2026-11-02）。
#
# 为什么需要它：批N37 现场批的 VAD 结构性失效，根因不是代码 bug，而是**仓内 config
# 已重标（10-31 深夜 500/400/500）而板上的装机副本仍是 10-21 旧档（650/700/650）**——
# 10-31 的重标只进了仓，从没铺到部署面；而模块启动只打印"来源路径"，不比内容，
# 所以旧档静默生效、症状表现为"怎么喊都没反应"。
#
# 本脚本把"仓内 config ↔ 装机 config"的比对收敛成**唯一实现**（机制面单点）：
# 逐文件 sha256 判等，缺件即漂移；调用方（build-onboard 报告段 / 板上线 runbook）
# 只决定"漂移之后怎么办"（策略面）——脚本不自动改任何文件。
#
# 用法:
#   verify-board-config.sh --repo <仓根> --remote <user@host> [--root <装机根>]
#   verify-board-config.sh --repo <仓根> --local <装机根>
#   verify-board-config.sh --self-test
#
# 判据:
#   PASS   仓与装机 sha256 逐字节一致
#   DRIFT  两边都存在但内容不同（打印两枚 sha 供对表）
#   MISS   装机缺件（模块开机即读同目录 config，缺件必重启环——按漂移同权）
#   退出码: 0 全等 / 1 有漂移或缺件 / 2 用法错误 / 3 self-test 失败
#
# 装机根缺省 /usr/lib/rockrobot3（两轨同根：deb 与 buildroot 安装位一致的纪律）。
set -euo pipefail

DEFAULT_ROOT=/usr/lib/rockrobot3

usage() {
	sed -n '3,30p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
}

hash_stdin() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum | awk '{print $1}'
	else
		shasum -a 256 | awk '{print $1}'
fi
}

# 逐件比对（实现单点；调用方只管结果）
verify() {
	local repo="$1" mode="$2" target="$3" root="$4"
	local drift=0 checked=0
	local d name f repo_p inst_p a b

	for d in "$repo"/modules/*/; do
		[ -d "$d" ] || continue
		[ -f "$d/config.toml" ] || continue
		name="$(basename "$d")"
		for f in config.toml config-esp32.toml; do
			repo_p="$d$f"
			[ -f "$repo_p" ] || continue
			inst_p="$root/modules/$name/$f"
			checked=$((checked + 1))
			a="$(hash_stdin <"$repo_p")"
			if [ "$mode" = remote ]; then
				if b="$(ssh "$target" "cat '$inst_p'" 2>/dev/null | hash_stdin)" && [ -n "$b" ]; then
					: # 取到
				else
					b=""
				fi
			else
				if [ -f "$inst_p" ]; then b="$(hash_stdin <"$inst_p")"; else b=""; fi
			fi
			if [ -z "$b" ]; then
				printf 'MISS  %s/%s  装机缺件（%s）\n' "$name" "$f" "$inst_p"
				drift=$((drift + 1))
			elif [ "$a" != "$b" ]; then
				printf 'DRIFT %s/%s  仓=%s 装机=%s\n' "$name" "$f" "${a:0:12}" "${b:0:12}"
				drift=$((drift + 1))
			else
				printf 'PASS  %s/%s  sha=%s\n' "$name" "$f" "${a:0:12}"
			fi
		done
	done

	if [ "$checked" -eq 0 ]; then
		echo "仓内未找到任何 modules/*/config.toml（--repo 指错？）" >&2
		return 2
	fi
	printf '\n比对 %d 件：漂移/缺件 %d 件\n' "$checked" "$drift"
	if [ "$drift" -ne 0 ]; then
		echo "结论：装机面与仓内 config 不一致——重装包（dpkg -i）或重新铺镜像后再跑本工具确认"
		return 1
	fi
	echo "结论：装机面 config 与仓内逐字节一致"
	return 0
}

SELFTEST_TMP=""
cleanup_selftest() {
	[ -n "$SELFTEST_TMP" ] && rm -rf "$SELFTEST_TMP"
	return 0
}

self_test() {
	local tmp rc fail=0
	tmp="$(mktemp -d)"
	SELFTEST_TMP="$tmp"
	trap cleanup_selftest EXIT
	mkdir -p "$tmp/repo/modules/rr3-a" "$tmp/inst/modules/rr3-a"
	printf 'rms_start = 650\n' >"$tmp/repo/modules/rr3-a/config.toml"
	cp "$tmp/repo/modules/rr3-a/config.toml" "$tmp/inst/modules/rr3-a/config.toml"

	run_case() { # 名称 期望码
		set +e
		verify "$tmp/repo" local "$tmp/inst" "$tmp/inst" >/dev/null 2>&1
		rc=$?
		set -e
		if [ "$rc" -eq "$2" ]; then
			printf 'PASS  self-test/%s（退出码 %d）\n' "$1" "$rc"
		else
			printf 'FAIL  self-test/%s（期望 %d 实得 %d）\n' "$1" "$2" "$rc"
			fail=1
		fi
	}

	run_case 干净过 0
	printf 'rms_start = 700\n' >"$tmp/inst/modules/rr3-a/config.toml"
	run_case 改一字节红 1
	rm "$tmp/inst/modules/rr3-a/config.toml"
	run_case 缺件红 1

	[ "$fail" -eq 0 ] || return 3
	return 0
}

MODE="" REPO="" TARGET="" ROOT="$DEFAULT_ROOT"
while [ "$#" -gt 0 ]; do
	case "$1" in
	--repo)
		REPO="$2"
		shift 2
		;;
	--remote)
		MODE=remote
		TARGET="$2"
		shift 2
		;;
	--local)
		MODE=local
		TARGET="$2"
		shift 2
		;;
	--root)
		ROOT="$2"
		shift 2
		;;
	--self-test)
		self_test
		exit $?
		;;
	*)
		usage
		;;
	esac
done

if [ -z "$REPO" ] || [ -z "$MODE" ]; then
	usage
fi

if [ "$MODE" = local ]; then
	verify "$REPO" local "$TARGET" "$TARGET"
else
	verify "$REPO" remote "$TARGET" "$ROOT"
fi
