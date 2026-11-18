#!/usr/bin/env bash
# verify-board-config —— 装机面部署漂移门（批N38 件二，2026-11-02；2026-11-16 扩面）。
#
# 为什么需要它：批N37 现场批的 VAD 结构性失效，根因不是代码 bug，而是**仓内 config
# 已重标（10-31 深夜 500/400/500）而板上的装机副本仍是 10-21 旧档（650/700/650）**——
# 10-31 的重标只进了仓，从没铺到部署面；而模块启动只打印"来源路径"，不比内容，
# 所以旧档静默生效、症状表现为"怎么喊都没反应"。
#
# 比对面（部署面三类，见 rr3_deploy_manifest）：模块 config（含 esp32 档）、
# 装配单 profiles/*.toml、systemd unit（口径忽略 ExecStart= 行——rr3-pack 出包
# 按 --profile 改写该行）。2026-11-16 扩面前只比 7 件 config，而当天两起现场事故
# 恰好落在另两类上：装配单 alsa_device 指向未接入声卡、unit 的 ExecStartPre 未铺。
#
# 本脚本把"仓内真值 ↔ 装机副本"的比对收敛成**唯一实现**（机制面单点）：
# 逐文件 sha256 判等，缺件即漂移；调用方（build-onboard 报告段 / 板上线 runbook）
# 只决定"漂移之后怎么办"（策略面）——脚本不自动改任何文件。
#
# 用法:
#   verify-board-config.sh --repo <仓根> --remote <user@host> [--root <装机根>] [--unit-dir <unit 目录>]
#   verify-board-config.sh --repo <仓根> --local <装机根> [--unit-dir <unit 目录>]
#   verify-board-config.sh --self-test
#
# 判据:
#   PASS   仓与装机 sha256 一致（unit 口径忽略 ExecStart= 行）
#   DRIFT  两边都存在但内容不同（打印两枚 sha 供对表）
#   MISS   装机缺件（模块开机即读同目录 config，缺件必重启环——按漂移同权）
#   退出码: 0 全等 / 1 有漂移或缺件 / 2 用法错误 / 3 self-test 失败
#
# 装机根缺省 /usr/lib/rockrobot3（两轨同根：deb 与 buildroot 安装位一致的纪律），
# unit 目录缺省 /lib/systemd/system。
set -euo pipefail

DEFAULT_ROOT=/usr/lib/rockrobot3
DEFAULT_UNIT_DIR=/lib/systemd/system

usage() {
	sed -n '3,36p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
}

hash_stdin() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum | awk '{print $1}'
	else
		shasum -a 256 | awk '{print $1}'
fi
}

# 部署面清单（机制单点·数据）：每行 `仓内相对路径|装机相对路径|口径`。
# 2026-11-16 扩面：此前只扫 modules/*/config*.toml 七件，而当天两个真实事故都落在
# 漏面上——① profiles/ 的 alsa_device 指向未接入声卡（interact 连崩 5 次降级）；
# ② systemd unit 的 ExecStartPre 从未铺到板上（#24 音量下限空转）。部署面漂移的
# 判据面必须覆盖"决定运行行为的三类文件"，否则门绿而现场坏。
# 口径 full = 逐字节；unit = 忽略 `ExecStart=` 行（rr3-pack 出包按 --profile 名改写
# 该行，逐字节比必然假红——口径差异是契约，不是例外）。
rr3_deploy_manifest() {
	local repo="$1" d f p u name
	for d in "$repo"/modules/*/; do
		[ -f "$d/config.toml" ] || continue
		name="$(basename "$d")"
		for f in config.toml config-esp32.toml; do
			[ -f "$d$f" ] && printf 'modules/%s/%s|modules/%s/%s|full\n' "$name" "$f" "$name" "$f"
		done
	done
	for p in "$repo"/profiles/*.toml; do
		[ -f "$p" ] && printf 'profiles/%s|profiles/%s|full\n' "$(basename "$p")" "$(basename "$p")"
	done
	for u in "$repo"/base/os/distro/units/*.service; do
		[ -f "$u" ] && printf 'base/os/distro/units/%s|units/%s|unit\n' "$(basename "$u")" "$(basename "$u")"
	done
}

# 装机侧路径：unit 住系统目录（非装机根下），其余按装机根拼接。
rr3_inst_path() {
	local root="$1" rel="$2"
	case "$rel" in
	units/*) printf '%s/%s\n' "$UNIT_DIR" "${rel#units/}" ;;
	*) printf '%s/%s\n' "$root" "$rel" ;;
	esac
}

# 取一侧摘要：口径 unit 先滤掉 ExecStart= 行（sed 恒 0 退出，不用 grep 的 1）。
rr3_hash_local() {
	if [ "$2" = unit ]; then sed -e '/^ExecStart=/d' "$1" | hash_stdin; else hash_stdin <"$1"; fi
}

rr3_hash_remote() {
	local target="$1" path="$2" kou="$3"
	# -n：绝不能读 stdin——调用方在 here-doc 里逐行喂清单，ssh 吃掉剩余行会让
	# "比对 16 件"静默变成"比对 2 件"（2026-11-16 实测，门绿而面漏）。
	if [ "$kou" = unit ]; then
		ssh -n "$target" "sed -e '/^ExecStart=/d' '$path'" 2>/dev/null | hash_stdin
	else
		ssh -n "$target" "cat '$path'" 2>/dev/null | hash_stdin
	fi
}

# 逐件比对（实现单点；调用方只管结果）
verify() {
	local repo="$1" mode="$2" target="$3" root="$4"
	local drift=0 checked=0 list repo_rel inst_rel kou repo_p inst_p a b

	list="$(rr3_deploy_manifest "$repo")"
	while IFS='|' read -r repo_rel inst_rel kou; do
		[ -n "$repo_rel" ] || continue
		repo_p="$repo/$repo_rel"
		inst_p="$(rr3_inst_path "$root" "$inst_rel")"
		checked=$((checked + 1))
		a="$(rr3_hash_local "$repo_p" "$kou")"
		if [ "$mode" = remote ]; then
			b="$(rr3_hash_remote "$target" "$inst_p" "$kou")"
		else
			if [ -f "$inst_p" ]; then b="$(rr3_hash_local "$inst_p" "$kou")"; else b=""; fi
		fi
		if [ -z "$b" ]; then
			printf 'MISS  %s  装机缺件（%s）\n' "$repo_rel" "$inst_p"
			drift=$((drift + 1))
		elif [ "$a" != "$b" ]; then
			printf 'DRIFT %s  仓=%s 装机=%s\n' "$repo_rel" "${a:0:12}" "${b:0:12}"
			drift=$((drift + 1))
		else
			printf 'PASS  %s  sha=%s\n' "$repo_rel" "${a:0:12}"
		fi
	done <<EOF
$list
EOF

	if [ "$checked" -eq 0 ]; then
		echo "仓内未找到任何部署面文件（--repo 指错？）" >&2
		return 2
	fi
	printf '\n比对 %d 件：漂移/缺件 %d 件\n' "$checked" "$drift"
	if [ "$drift" -ne 0 ]; then
		echo "结论：装机面与仓内不一致——重装包（dpkg -i）或重新铺镜像后再跑本工具确认"
		return 1
	fi
	echo "结论：装机面（模块 config / 装配单 / systemd unit）与仓内一致"
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
	UNIT_DIR="$tmp/inst-units"
	mkdir -p "$tmp/repo/modules/rr3-a" "$tmp/inst/modules/rr3-a" \
		"$tmp/repo/profiles" "$tmp/inst/profiles" \
		"$tmp/repo/base/os/distro/units" "$UNIT_DIR"
	printf 'rms_start = 650\n' >"$tmp/repo/modules/rr3-a/config.toml"
	cp "$tmp/repo/modules/rr3-a/config.toml" "$tmp/inst/modules/rr3-a/config.toml"
	printf '[hardware]\nalsa_device = "plughw:CARD=Device"\n' >"$tmp/repo/profiles/p.toml"
	cp "$tmp/repo/profiles/p.toml" "$tmp/inst/profiles/p.toml"
	# unit 口径：装机侧 ExecStart 已被 rr3-pack 改写（换 --profile），其余行必须一致。
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /x\n' \
		>"$tmp/repo/base/os/distro/units/rrd.service"
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /y\n' \
		>"$UNIT_DIR/rrd.service"

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

	run_case 三类齐全过 0
	run_case unit口径忽略ExecStart 0
	printf 'rms_start = 700\n' >"$tmp/inst/modules/rr3-a/config.toml"
	run_case 改一字节红 1
	printf 'rms_start = 650\n' >"$tmp/inst/modules/rr3-a/config.toml"
	printf '[hardware]\nalsa_device = "plughw:CARD=sndrpigooglevoi"\n' >"$tmp/inst/profiles/p.toml"
	run_case 装配单漂移红 1
	printf '[hardware]\nalsa_device = "plughw:CARD=Device"\n' >"$tmp/inst/profiles/p.toml"
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /x\n' \
		>"$UNIT_DIR/rrd.service"
	printf '[Service]\nExecStartPre=-\nExecStart=/usr/bin/rrd --profile /y\n' >"$UNIT_DIR/rrd.service"
	run_case unit非ExecStart行漂移红 1
	rm "$UNIT_DIR/rrd.service"
	run_case unit缺件红 1

	[ "$fail" -eq 0 ] || return 3
	return 0
}

MODE="" REPO="" TARGET="" ROOT="$DEFAULT_ROOT" UNIT_DIR="$DEFAULT_UNIT_DIR"
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
	--unit-dir)
		UNIT_DIR="$2"
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
