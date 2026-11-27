#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# verify-board-config —— 装机面部署漂移门（批N38 件二，2026-11-02；2026-11-16 扩面）。
#
# 为什么需要它：批N37 现场批的 VAD 结构性失效，根因不是代码 bug，而是**仓内 config
# 已重标（10-31 深夜 500/400/500）而板上的装机副本仍是 10-21 旧档（650/700/650）**——
# 10-31 的重标只进了仓，从没铺到部署面；而模块启动只打印"来源路径"，不比内容，
# 所以旧档静默生效、症状表现为"怎么喊都没反应"。
#
# 比对面（部署面四类，见 rr3_deploy_manifest）：模块 config（含 esp32/pi4b 形态档）、
# 装配单 profiles/*.toml、systemd unit（口径忽略 ExecStart= 行）、板级资产
# （base/os/distro/boards/<profile>/etc/** → 装机 /etc/**，2026-11-22 收编后新增）。
# 2026-11-16 扩面前只比 7 件 config，而当天两起现场事故恰好落在另两类上：装配单
# alsa_device 指向未接入声卡、unit 的 ExecStartPre 未铺。
#
# 本脚本把"仓内真值 ↔ 装机副本"的比对收敛成**唯一实现**（机制面单点）：
# 逐文件 sha256 判等，缺件即漂移；调用方（build-onboard 报告段 / 板上线 runbook）
# 只决定"漂移之后怎么办"（策略面）——脚本不自动改任何文件。
#
# 用法:
#   verify-board-config.sh --repo <仓根> --remote <user@host> [--root <装机根>] [--unit-dir <unit 目录>] [--etc-dir <etc 目录>] [--profile <机型>]
#   verify-board-config.sh --repo <仓根> --local <装机根> [--unit-dir <unit 目录>] [--etc-dir <etc 目录>] [--profile <机型>]
#   verify-board-config.sh --self-test
#
# 判据:
#   PASS   仓与装机 sha256 一致（unit 口径忽略 ExecStart= 行）
#   DRIFT  两边都存在但内容不同（打印两枚 sha 供对表）
#   MISS   装机缺件（模块开机即读同目录 config，缺件必重启环——按漂移同权）
#   退出码: 0 全等 / 1 有漂移或缺件 / 2 用法错误或远端不可达（未定）/ 3 self-test 失败
#
# 装机根缺省 /usr/lib/rockrobot3（两轨同根：deb 与 buildroot 安装位一致的纪律），
# unit 目录缺省 /lib/systemd/system，etc 目录缺省 /etc。
# --profile 只比该机型装配单与其板级资产（deb 按机型只装一份 profiles/<name>.toml）；
# 不传则扫全部装配单（legacy/多档镜像自测）。
set -euo pipefail

DEFAULT_ROOT=/usr/lib/rockrobot3
DEFAULT_UNIT_DIR=/lib/systemd/system
DEFAULT_ETC_DIR=/etc

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
	local repo="$1" profile="${2:-}"
	rr3_manifest_modules "$repo"
	rr3_manifest_boards "$repo" "$profile"
	rr3_manifest_profiles "$repo" "$profile"
	rr3_manifest_contracts "$repo" "$profile"
	rr3_manifest_units "$repo"
	rr3_manifest_hardware "$repo" "$profile"
}

# 档位 → 平台册名（[platform].chip）：派生单点。rr3_manifest_contracts 与
# rr3_profile_guard 都从这里取值，避免两处 sed 各自漂移。
rr3_chip_of() {
	sed -n '/^\[platform\]/,/^\[/s/^chip[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
		"$1/profiles/$2.toml" | head -n 1
}

# 档位面自洽（2026-11-27 批166）：--profile 的装配单不存在、或不带 [platform].chip、
# 或该芯片的平台册不在仓内时，rr3_deploy_manifest 会**静默少列**相应成员——漂移门
# 照样打绿，而没比的正是该档最要紧的几件。这里前置判不自洽，退出码 2 与用法错误同权。
rr3_profile_guard() {
	local repo="$1" profile="$2" chip
	[ -f "$repo/profiles/$profile.toml" ] || {
		printf '档位不存在：profiles/%s.toml（--profile 拼写？）\n' "$profile" >&2
		return 1
	}
	chip="$(rr3_chip_of "$repo" "$profile")"
	[ -n "$chip" ] || {
		printf '档位 profiles/%s.toml 无 [platform].chip——平台册无从对上\n' "$profile" >&2
		return 1
	}
	[ -f "$repo/contracts/platform/$chip.toml" ] || {
		printf '平台册缺件：contracts/platform/%s.toml（档位 %s 的 chip）\n' "$chip" "$profile" >&2
		return 1
	}
	return 0
}

# 契约册与平台事实表：rr3-pack 随包装机、rrd 装载期直接读——漂了同样拒 boot。
# 四册名单与 rrd 代码里的 RR3_CONTRACT_BOOKS 同集（真值在代码；本表是部署侧
# 对账面，名单变化须同批改两处）。平台册按档位 [platform].chip 选一；无 --profile
# 的 legacy/多档模式不猜芯片，只比四册（平台册按 MISS 计会误报）。
rr3_manifest_contracts() {
	local repo="$1" profile="$2" b f chip=""
	for b in topics slots roles devices; do
		[ -f "$repo/contracts/$b.toml" ] \
			&& printf 'contracts/%s.toml|contracts/%s.toml|full\n' "$b" "$b"
	done
	[ -n "$profile" ] || return 0
	[ -f "$repo/profiles/$profile.toml" ] || return 0
	chip="$(rr3_chip_of "$repo" "$profile")"
	[ -n "$chip" ] || return 0
	f="$repo/contracts/platform/$chip.toml"
	[ -f "$f" ] && printf 'contracts/platform/%s.toml|contracts/platform/%s.toml|full\n' "$chip" "$chip"
	return 0
}

# 模块 config：每模块三份候选（默认 / esp32 档 / pi4b 档），存在即比。
rr3_manifest_modules() {
	local repo="$1" d f name
	for d in "$repo"/modules/*/; do
		[ -f "$d/config.toml" ] || continue
		name="$(basename "$d")"
		for f in config.toml config-esp32.toml config-pi4b.toml; do
			[ -f "$d$f" ] && printf 'modules/%s/%s|modules/%s/%s|full\n' "$name" "$f" "$name" "$f"
		done
	done
}

# 板级资产：boards/<profile>/etc/** 是装机 /etc/** 的源（仓内唯一真值）。
# 指定 --profile 时只扫该档；未指定（legacy/self-test）扫全部。夹具没有
# boards/ 时 find 打空，合法跳过。
rr3_manifest_boards() {
	local repo="$1" profile="$2" board_root f rel
	board_root="$repo/base/os/distro/boards"
	[ -n "$profile" ] && board_root="$board_root/$profile"
	[ -d "$board_root" ] || return 0
	find "$board_root" -type f -path '*/etc/*' 2>/dev/null |
		while IFS= read -r f; do
			rel="${f#"$repo"/base/os/distro/boards/}"
			printf 'base/os/distro/boards/%s|%s|full\n' "$rel" "${rel#*/}"
		done
}

rr3_manifest_profiles() {
	local repo="$1" profile="$2" p
	if [ -n "$profile" ]; then
		p="$repo/profiles/$profile.toml"
		[ -f "$p" ] && printf 'profiles/%s.toml|profiles/%s.toml|full\n' "$profile" "$profile"
		return 0
	fi
	for p in "$repo"/profiles/*.toml; do
		[ -f "$p" ] && printf 'profiles/%s|profiles/%s|full\n' "$(basename "$p")" "$(basename "$p")"
	done
}

rr3_manifest_units() {
	local repo="$1" u
	for u in "$repo"/base/os/distro/units/*.service; do
		[ -f "$u" ] && printf 'base/os/distro/units/%s|units/%s|unit\n' "$(basename "$u")" "$(basename "$u")"
	done
}

# 装载真值集的 hardware 面（wiring + axis）：rr3-pack 随包落
# <装机根>/hardware/<stem>/…，rrd 装载期直接消费——它漂了 boot 就拒，
# 与模块 config 同权必须比对（2026-11-27 批149：此前漏扫，门绿而 wiring 可漂）。
rr3_manifest_hardware() {
	local repo="$1" profile="$2" hw name f
	if [ -n "$profile" ]; then
		hw="$repo/hardware/$profile"
		rr3_manifest_hw_one "$profile" "$hw"
		return 0
	fi
	for hw in "$repo"/hardware/*/; do
		name="$(basename "$hw")"
		rr3_manifest_hw_one "$name" "$hw"
	done
}

# 单档 hardware 两条目：wiring.toml 与 axis/*.toml。
rr3_manifest_hw_one() {
	local name="$1" hw="$2" f
	[ -f "$hw/wiring.toml" ] \
		&& printf 'hardware/%s/wiring.toml|hardware/%s/wiring.toml|full\n' "$name" "$name"
	for f in "$hw"/axis/*.toml; do
		[ -f "$f" ] && printf 'hardware/%s/axis/%s|hardware/%s/axis/%s|full\n' \
			"$name" "$(basename "$f")" "$name" "$(basename "$f")"
	done
}

# 装机侧路径：unit 住系统目录（非装机根下），板级资产住 /etc 目录（--etc-dir
# 可重定位，供 self-test/staging 用），其余按装机根拼接。
rr3_inst_path() {
	local root="$1" rel="$2"
	case "$rel" in
	units/*) printf '%s/%s\n' "$UNIT_DIR" "${rel#units/}" ;;
	etc/*) printf '%s/%s\n' "$ETC_DIR" "${rel#etc/}" ;;
	/*) printf '%s\n' "$rel" ;;
	*) printf '%s/%s\n' "$root" "$rel" ;;
	esac
}

# 取一侧摘要：口径 unit 先滤掉 ExecStart= 行（sed 恒 0 退出，不用 grep 的 1）。
rr3_hash_local() {
	if [ "$2" = unit ]; then sed -e '/^ExecStart=/d' "$1" | hash_stdin; else hash_stdin <"$1"; fi
}

# 远端摘要脚本（批176）：一条 ssh 取回整份清单。此前逐件起 ssh——单档 16~19 次
# 握手，Wi-Fi 板上每次都要重新认证，核验要等半分钟；改为一趟。摘要在远端算（与
# 本地同一 sha 口径），只回传 `<装机路径>\t<sha>`，缺件回 `<路径>\tMISS`。
rr3_remote_script() {
	local root="$1" list="$2" inst_p kou
	printf '%s\n' "h() { if command -v sha256sum >/dev/null 2>&1; then sha256sum | awk '{print \$1}'; else shasum -a 256 | awk '{print \$1}'; fi; }"
	while IFS='|' read -r repo_rel inst_rel kou; do
		[ -n "$repo_rel" ] || continue
		inst_p="$(rr3_inst_path "$root" "$inst_rel")"
		if [ "$kou" = unit ]; then
			printf "if [ -f '%s' ]; then printf '%%s\\t%%s\\n' '%s' \"\$(sed -e '/^ExecStart=/d' '%s' | h)\"; else printf '%%s\\tMISS\\n' '%s'; fi\n" \
				"$inst_p" "$inst_p" "$inst_p" "$inst_p"
		else
			printf "if [ -f '%s' ]; then printf '%%s\\t%%s\\n' '%s' \"\$(h < '%s')\"; else printf '%%s\\tMISS\\n' '%s'; fi\n" \
				"$inst_p" "$inst_p" "$inst_p" "$inst_p"
		fi
	done <<EOF
$list
EOF
}

# 一次取回；255 = ssh 传输层失败（不可达/认证失败），调用方按"未定"处理，
# 不把整份清单打成 MISS（那会把"网不通"读成"板上全漂"）。
rr3_remote_fetch() {
	local target="$1" root="$2" list="$3" out="$4" rc=0
	rr3_remote_script "$root" "$list" | ssh "$target" 'bash -s' >"$out" 2>/dev/null || rc=$?
	[ "$rc" -eq 0 ] && return 0
	[ "$rc" -eq 255 ] && return 255
	return 1
}

# 从取回表里按路径取摘要（无此行 = 远端没报，按缺件计）。
rr3_remote_lookup() {
	awk -F'\t' -v p="$2" '$1 == p { print $2; exit }' "$1"
}

# 逐件比对（实现单点；调用方只管结果）
verify() {
	local repo="$1" mode="$2" target="$3" root="$4" profile="${5:-}"
	local drift=0 checked=0 list repo_rel inst_rel kou repo_p inst_p a b digests=""

	if [ -n "$profile" ]; then
		rr3_profile_guard "$repo" "$profile" || return 2
	fi
	list="$(rr3_deploy_manifest "$repo" "$profile")"
	if [ "$mode" = remote ]; then
		digests="$(mktemp)" || return 2
		if ! rr3_remote_fetch "$target" "$root" "$list" "$digests"; then
			rm -f "$digests"
			echo "远端不可达：${target}（ssh 传输失败）——无法判定漂移，按未定处理" >&2
			return 2
		fi
	fi
	while IFS='|' read -r repo_rel inst_rel kou; do
		[ -n "$repo_rel" ] || continue
		repo_p="$repo/$repo_rel"
		inst_p="$(rr3_inst_path "$root" "$inst_rel")"
		checked=$((checked + 1))
		a="$(rr3_hash_local "$repo_p" "$kou")"
		if [ "$mode" = remote ]; then
			b="$(rr3_remote_lookup "$digests" "$inst_p")"
		else
			if [ -f "$inst_p" ]; then b="$(rr3_hash_local "$inst_p" "$kou")"; else b=""; fi
		fi
		if [ -z "$b" ] || [ "$b" = MISS ]; then
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

	[ -n "$digests" ] && rm -f "$digests"
	if [ "$checked" -eq 0 ]; then
		echo "仓内未找到任何部署面文件（--repo 指错？）" >&2
		return 2
	fi
	printf '\n比对 %d 件：漂移/缺件 %d 件\n' "$checked" "$drift"
	if [ "$drift" -ne 0 ]; then
		echo "结论：装机面与仓内不一致——重装包（dpkg -i）或重新铺镜像后再跑本工具确认"
		return 1
	fi
	echo "结论：装机面（模块 config / 装配单 / 契约册 / systemd unit / 板级资产 / hardware 真值集）与仓内一致"
	return 0
}

MODE="" REPO="" TARGET="" ROOT="$DEFAULT_ROOT" UNIT_DIR="$DEFAULT_UNIT_DIR" \
	ETC_DIR="$DEFAULT_ETC_DIR" PROFILE=""
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
	--etc-dir)
		ETC_DIR="$2"
		shift 2
		;;
	--profile)
		PROFILE="$2"
		shift 2
		;;
	--self-test)
		# 夹具与用例按职责独立成册（2026-11-27 批170 拆出）：主文件只留判据机制，
		# 自检册以同一 shell 源入（verify/rr3_* 与 UNIT_DIR/ETC_DIR 都在作用域内）。
		. "$(dirname "$0")/verify-board-config.selftest.sh"
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
	verify "$REPO" local "$TARGET" "$TARGET" "$PROFILE"
else
	verify "$REPO" remote "$TARGET" "$ROOT" "$PROFILE"
fi
