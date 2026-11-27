#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# verify-board-config 的自检册（2026-11-27 批170 从主文件拆出）。
#
# 职责：造临时夹具（仓内/装机侧各一套）、用 ssh 桩把 --remote 也真跑一遍，逐类注入
# 违约并核对退出码。判据机制本身留在 verify-board-config.sh；本册只被它 --self-test
# 源入，不单独执行（{verify, rr3_*} 与 UNIT_DIR/ETC_DIR 均在源入者的作用域内）。
#
# 夹具口径：p 档带 [platform].chip=ck 与平台册；q/r 是档位面负例（只建仓侧，且在
# legacy 全档用例之后才建——否则全档扫描会把它们算成缺件）。
SELFTEST_TMP=""
cleanup_selftest() {
	[ -n "$SELFTEST_TMP" ] && rm -rf "$SELFTEST_TMP"
	return 0
}

# 自检夹具：仓内/装机侧各一套三类齐全资产（模块 config / 装配单 / unit / 板级资产）。
st_prepare() {
	st_dir="$(mktemp -d)"
	SELFTEST_TMP="$st_dir"
	trap cleanup_selftest EXIT
	UNIT_DIR="$st_dir/inst-units"
	ETC_DIR="$st_dir/inst-etc"
	mkdir -p "$st_dir/repo/modules/rr3-a" "$st_dir/inst/modules/rr3-a" \
		"$st_dir/repo/profiles" "$st_dir/inst/profiles" \
		"$st_dir/repo/contracts/platform" "$st_dir/inst/contracts/platform" \
		"$st_dir/repo/hardware/p/axis" "$st_dir/inst/hardware/p/axis" \
		"$st_dir/repo/base/os/distro/units" "$UNIT_DIR" \
		"$st_dir/repo/base/os/distro/boards/p/etc/systemd/system/rrd.service.d" \
		"$st_dir/inst-etc/systemd/system/rrd.service.d"
	printf 'rms_start = 650\n' >"$st_dir/repo/modules/rr3-a/config.toml"
	cp "$st_dir/repo/modules/rr3-a/config.toml" "$st_dir/inst/modules/rr3-a/config.toml"
	printf '[platform]\nchip = "ck"\n[hardware]\nalsa_device = "plughw:CARD=Device"\n' \
		>"$st_dir/repo/profiles/p.toml"
	cp "$st_dir/repo/profiles/p.toml" "$st_dir/inst/profiles/p.toml"
	# 契约四册 + 档位平台册：装机侧与仓内逐字节一致才算过。
	for b in topics slots roles devices; do
		printf '# %s\n' "$b" >"$st_dir/repo/contracts/$b.toml"
		cp "$st_dir/repo/contracts/$b.toml" "$st_dir/inst/contracts/$b.toml"
	done
	printf '# ck\n' >"$st_dir/repo/contracts/platform/ck.toml"
	cp "$st_dir/repo/contracts/platform/ck.toml" "$st_dir/inst/contracts/platform/ck.toml"
	# 装载真值集 hardware 面（wiring + axis）：与装机侧逐字节一致才算过。
	printf '[[wire]]\nconnector = "J0"\n' >"$st_dir/repo/hardware/p/wiring.toml"
	cp "$st_dir/repo/hardware/p/wiring.toml" "$st_dir/inst/hardware/p/wiring.toml"
	printf 'schema = 1\n' >"$st_dir/repo/hardware/p/axis/pan.toml"
	cp "$st_dir/repo/hardware/p/axis/pan.toml" "$st_dir/inst/hardware/p/axis/pan.toml"
	# unit 口径：装机侧 ExecStart 已被 rr3-pack 改写（换 --profile），其余行必须一致。
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /x\n' \
		>"$st_dir/repo/base/os/distro/units/rrd.service"
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /y\n' \
		>"$UNIT_DIR/rrd.service"
	BOARD_REL="base/os/distro/boards/p/etc/systemd/system/rrd.service.d/d.conf"
	printf '[Service]\nEnvironment=RR3_HAL_ALSA_DEVICE=rr3\n' >"$st_dir/repo/$BOARD_REL"
	cp "$st_dir/repo/$BOARD_REL" "$ETC_DIR/systemd/system/rrd.service.d/d.conf"
	# 远端面自检的 ssh 桩（批170）：桩把 `ssh <host> "<cmd>"` 的 cmd 在本机跑；
	# 一次调用记一行，供"单趟往返"回归钉计数（批176 把逐件 ssh 收成一条）。
	# RR3_SSH_FAIL=1 时以 255 退出，验证"不可达按未定处理"而不是整份清单 MISS。
	mkdir -p "$st_dir/bin"
	printf '#!/usr/bin/env bash\n[ "${RR3_SSH_FAIL:-}" = 1 ] && exit 255\nwhile [ "$1" = "-n" ]; do shift; done\nshift\nprintf "%%s\\n" "$*" >>"%s/ssh-calls"\nbash -c "$*"\n' \
		"$st_dir" >"$st_dir/bin/ssh"
	chmod 0755 "$st_dir/bin/ssh"
}

# 远端面单例：ssh 桩进 PATH，远端路径与 unit 口径都真走（--remote 不是只换分支名）。
st_case_remote() { # 名称 期望码
	local rc
	set +e
	PATH="$st_dir/bin:$PATH" verify "$st_dir/repo" remote board "$st_dir/inst" p >/dev/null 2>&1
	rc=$?
	set -e
	if [ "$rc" -eq "$2" ]; then
		printf 'PASS  self-test/%s（退出码 %d）\n' "$1" "$rc"
	else
		printf 'FAIL  self-test/%s（期望 %d 实得 %d）\n' "$1" "$2" "$rc"
		st_fail=1
	fi
}

# 单例判定：跑一次 verify，退出码对期望码。
st_case() { # 名称 期望码 [档位]
	local rc
	set +e
	verify "$st_dir/repo" local "$st_dir/inst" "$st_dir/inst" "${3:-}" >/dev/null 2>&1
	rc=$?
	set -e
	if [ "$rc" -eq "$2" ]; then
		printf 'PASS  self-test/%s（退出码 %d）\n' "$1" "$rc"
	else
		printf 'FAIL  self-test/%s（期望 %d 实得 %d）\n' "$1" "$2" "$rc"
		st_fail=1
	fi
}

# legacy 全档面：全等两例 + 逐类漂移/缺件。每例改完立即还原，夹具保持干净。
st_cases_local() {
	st_case 三类齐全过 0
	st_case unit口径忽略ExecStart 0
	printf 'rms_start = 700\n' >"$st_dir/inst/modules/rr3-a/config.toml"
	st_case 改一字节红 1
	printf 'rms_start = 650\n' >"$st_dir/inst/modules/rr3-a/config.toml"
	printf '[platform]\nchip = "ck"\n[hardware]\nalsa_device = "plughw:CARD=sndrpigooglevoi"\n' \
		>"$st_dir/inst/profiles/p.toml"
	st_case 装配单漂移红 1
	# 还原取仓侧原件而非再抄一份字面量：profile 字段长了以后手抄必然漏（批166 实测）。
	cp "$st_dir/repo/profiles/p.toml" "$st_dir/inst/profiles/p.toml"
	printf '# drift\n' >"$st_dir/inst/contracts/topics.toml"
	st_case 契约册漂移红 1
	cp "$st_dir/repo/contracts/topics.toml" "$st_dir/inst/contracts/topics.toml"
	printf '[[wire]]\nconnector = "JX"\n' >"$st_dir/inst/hardware/p/wiring.toml"
	st_case wiring漂移红 1
	cp "$st_dir/repo/hardware/p/wiring.toml" "$st_dir/inst/hardware/p/wiring.toml"
	printf 'schema = 2\n' >"$st_dir/inst/hardware/p/axis/pan.toml"
	st_case 轴档漂移红 1
	cp "$st_dir/repo/hardware/p/axis/pan.toml" "$st_dir/inst/hardware/p/axis/pan.toml"
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /x\n' \
		>"$UNIT_DIR/rrd.service"
	printf '[Service]\nExecStartPre=-\nExecStart=/usr/bin/rrd --profile /y\n' >"$UNIT_DIR/rrd.service"
	st_case unit非ExecStart行漂移红 1
	rm "$UNIT_DIR/rrd.service"
	st_case unit缺件红 1
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /y\n' \
		>"$UNIT_DIR/rrd.service"
	printf '[Service]\nEnvironment=RR3_HAL_ALSA_DEVICE=hw:9\n' \
		>"$ETC_DIR/systemd/system/rrd.service.d/d.conf"
	st_case 板级资产漂移红 1
	cp "$st_dir/repo/$BOARD_REL" "$ETC_DIR/systemd/system/rrd.service.d/d.conf"
}

# 档位面自洽（批166）：自洽过；档位名/芯片/平台册三处缺失都必须报 2 而不是静默少列。
# 负例装配单只建在仓侧且在全档用例之后建——否则 legacy 全档扫描会把它们算成缺件。
st_cases_profile() {
	printf '# 无 [platform]\n' >"$st_dir/repo/profiles/q.toml"
	printf '[platform]\nchip = "zz"\n' >"$st_dir/repo/profiles/r.toml"
	st_case profile模式三类齐全 0 p
	st_case 档位不存在报2 2 nope
	st_case 档位无chip报2 2 q
	st_case 平台册缺件报2 2 r
}

# 远端面（批170/176）：同一夹具经 ssh 桩走 --remote（档位 p）。用档位模式而非全档：
# guard 负例的 q/r 装配单只在仓侧，全档会把它们算成缺件。
st_cases_remote() {
	st_case_remote 远端全等过 0
	printf 'rms_start = 700\n' >"$st_dir/inst/modules/rr3-a/config.toml"
	st_case_remote 远端改一字节红 1
	printf 'rms_start = 650\n' >"$st_dir/inst/modules/rr3-a/config.toml"
	printf '[Service]\nExecStartPre=-/usr/bin/amixer -c Device sset PCM 100%%\nExecStart=/usr/bin/rrd --profile /z\n' \
		>"$UNIT_DIR/rrd.service"
	st_case_remote 远端unit口径仍过 0
	rm "$st_dir/inst/hardware/p/axis/pan.toml"
	st_case_remote 远端缺件红 1
	cp "$st_dir/repo/hardware/p/axis/pan.toml" "$st_dir/inst/hardware/p/axis/pan.toml"
	st_remote_single_trip
	st_remote_unreachable
}

# 单趟往返回归钉（批176）：整份清单只许起一次 ssh——逐件 ssh 在 Wi-Fi 板上是 16~19
# 次握手；这条用例只在计数上判，防"批处理被悄悄改回逐件"。
st_remote_single_trip() {
	local rc calls
	rm -f "$st_dir/ssh-calls"
	set +e
	PATH="$st_dir/bin:$PATH" verify "$st_dir/repo" remote board "$st_dir/inst" p >/dev/null 2>&1
	rc=$?
	set -e
	calls=$(wc -l <"$st_dir/ssh-calls" 2>/dev/null | tr -d ' ')
	if [ "$rc" -eq 0 ] && [ "$calls" = 1 ]; then
		printf 'PASS  self-test/远端单趟往返（ssh %s 次）\n' "$calls"
	else
		printf 'FAIL  self-test/远端单趟往返（rc=%s ssh=%s，期望 0/1）\n' "$rc" "${calls:-0}"
		st_fail=1
	fi
}

# 不可达（批176）：ssh 255 必须报"未定"（rc 2），不能把整份清单打成 MISS 冒充漂移。
st_remote_unreachable() {
	local rc
	set +e
	RR3_SSH_FAIL=1 PATH="$st_dir/bin:$PATH" \
		verify "$st_dir/repo" remote board "$st_dir/inst" p >/dev/null 2>&1
	rc=$?
	set -e
	if [ "$rc" -eq 2 ]; then
		printf 'PASS  self-test/远端不可达报2（退出码 2）\n'
	else
		printf 'FAIL  self-test/远端不可达报2（期望 2 实得 %d）\n' "$rc"
		st_fail=1
	fi
}

st_cases() {
	st_cases_local
	st_cases_profile
	st_cases_remote
}

self_test() {
	st_fail=0
	st_prepare
	st_cases
	[ "$st_fail" -eq 0 ] || return 3
	return 0
}
