#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 版本化部署包（A10）结构契约的流水线判据（2026-11-27 批168）。
#
# 为什么需要它：`pack-bundle.sh` 是 rk3566 快路轨的交付件，A10 契约此前只写在脚本
# 与文档里——流水线从不真出包，"包长什么样"没有任何机器判据；2026-11-08 真板事故
# 正是包内成员版本偏斜（HEAD rrd + 旧 profile/旧模块）。本脚本把契约变成可执行断言：
# 出包之后、上传之前逐项核对，缺一件即红，且不依赖目标架构可执行（只读 `file`）。
#
# 用法：bundle-contract.sh <bundle.tar.gz> <target-triple> [预期 ELF 架构串]
#       bundle-contract.sh --self-test
# 判据：0 合规 / 1 违约（逐项点名） / 2 用法错误 / 3 self-test 失败
set -euo pipefail

repo=$(git rev-parse --show-toplevel 2>/dev/null) || {
	echo "FAIL  不在 git 仓库内" >&2
	exit 2
}

fail() {
	echo "FAIL  $*" >&2
	exit 1
}

# 临时目录用全局变量 + EXIT trap：trap 在进程退出时执行，函数局部变量那时已出栈，
# 用 local 会在 `set -u` 下报 unbound（批168 自检首跑实测）。
TMPROOT=""
cleanup() {
	[ -n "$TMPROOT" ] && rm -rf "$TMPROOT"
	return 0
}
trap cleanup EXIT

# 顶层名 + BUNDLE_VERSION：包根必须叫 rockrobot3-<commit>，版本行与仓 HEAD、目标
# 三元组、八件组件一致——这三样是"同 commit 出包"的唯一凭据，缺一即版本偏斜旧病。
check_manifest() {
	local dir="$1" commit="$2" want_target="$3" f="$1/BUNDLE_VERSION" comps m
	[ -f "$f" ] || fail "缺 BUNDLE_VERSION（非 A10 版本化包）"
	[ "$(sed -n 's/^commit=//p' "$f")" = "$commit" ] \
		|| fail "BUNDLE_VERSION commit ≠ 仓 HEAD（期望 ${commit}）"
	[ "$(sed -n 's/^target=//p' "$f")" = "$want_target" ] \
		|| fail "BUNDLE_VERSION target ≠ ${want_target}"
	comps="$(sed -n 's/^components=//p' "$f")"
	for m in rrd rr3-ctl rr3-ota-ctl rr3-act rr3-connect rr3-interact rr3-power rr3-sense; do
		case ",$comps," in
		*",$m,"*) ;;
		*) fail "components 缺 $m" ;;
		esac
	done
}

check_bins() {
	local dir="$1" b
	for b in rrd rr3-ctl rr3-ota-ctl; do
		[ -f "$dir/bin/$b" ] || fail "缺 bin/$b"
		[ -x "$dir/bin/$b" ] || fail "bin/$b 无执行位"
	done
}

check_modules() {
	local dir="$1" m
	for m in rr3-act rr3-connect rr3-interact rr3-power rr3-sense; do
		[ -x "$dir/modules/$m/$m" ] || fail "缺 modules/$m/$m 或不可执行"
		[ -f "$dir/modules/$m/module.toml" ] || fail "缺 modules/$m/module.toml"
		[ -f "$dir/modules/$m/config.toml" ] || fail "缺 modules/$m/config.toml"
	done
}

# 数据面按仓内真值逐个对：装配单全量、契约四册、各平台册、各机型 wiring。
# 不写名单字面量——名单的真值在仓里，包少一件就是漏装，多一件不判（向前兼容）。
check_data() {
	local dir="$1" f rel
	for f in "$repo"/profiles/*.toml; do
		rel="profiles/$(basename "$f")"
		[ -f "$dir/$rel" ] || fail "缺 $rel"
	done
	for f in topics slots roles devices; do
		[ -f "$dir/contracts/$f.toml" ] || fail "缺 contracts/$f.toml"
	done
	for f in "$repo"/contracts/platform/*.toml; do
		rel="contracts/platform/$(basename "$f")"
		[ -f "$dir/$rel" ] || fail "缺 $rel"
	done
	for f in "$repo"/hardware/*/wiring.toml; do
		rel="hardware/$(basename "$(dirname "$f")")/wiring.toml"
		[ -f "$dir/$rel" ] || fail "缺 $rel"
	done
}

# 八件二进制全是 ELF64（包不该装进任何脚本占位件）；给了架构串就再钉芯片。
check_elf() {
	local dir="$1" want="$2" b line
	for b in bin/rrd bin/rr3-ctl bin/rr3-ota-ctl modules/rr3-act/rr3-act \
		modules/rr3-connect/rr3-connect modules/rr3-interact/rr3-interact \
		modules/rr3-power/rr3-power modules/rr3-sense/rr3-sense; do
		line="$(file -b "$dir/$b")"
		case "$line" in
		ELF\ 64-bit*) ;;
		*) fail "$b 非 ELF64：$line" ;;
		esac
		if [ -n "$want" ]; then
			case "$line" in
			*"$want"*) ;;
			*) fail "$b 架构不符（期望 ${want}）：$line" ;;
			esac
		fi
	done
}

main_check() {
	local bundle="$1" want_target="$2" want_arch="${3:-}" tmp top
	[ -f "$bundle" ] || fail "包不存在: ${bundle}"
	tmp="$(mktemp -d)"
	TMPROOT="$tmp"
	top="$(tar -tzf "$bundle" | sed -n '1s|/.*||p')"
	case "$top" in
	rockrobot3-*) ;;
	*) fail "顶层目录不是 rockrobot3-<commit>：${top:-空}" ;;
	esac
	tar -xzf "$bundle" -C "$tmp"
	check_manifest "$tmp/$top" "$(git -C "$repo" rev-parse --short=12 HEAD)" "$want_target"
	check_bins "$tmp/$top"
	check_modules "$tmp/$top"
	check_data "$tmp/$top"
	check_elf "$tmp/$top" "$want_arch"
	echo "PASS  bundle A10 契约合规：${top}（target=${want_target} arch=${want_arch:-只验 ELF64}）"
}

# 自检：合成包由仓内数据 + ELF64 头占位件组成（不编译），逐项注入违约判红。
st_build() {
	local stage="$1" m
	mkdir -p "$stage/bin" "$stage/modules"
	cp -R "$repo/profiles" "$repo/contracts" "$repo/hardware" "$stage/"
	for m in rr3-act rr3-connect rr3-interact rr3-power rr3-sense; do
		mkdir -p "$stage/modules/$m"
		cp "$repo/modules/$m/module.toml" "$repo/modules/$m/config.toml" "$stage/modules/$m/"
	done
	for m in bin/rrd bin/rr3-ctl bin/rr3-ota-ctl modules/rr3-act/rr3-act \
		modules/rr3-connect/rr3-connect modules/rr3-interact/rr3-interact \
		modules/rr3-power/rr3-power modules/rr3-sense/rr3-sense; do
		printf '\177ELF\002\001\001\000\000\000\000\000\000\000\000\000\002\000\267\000' >"$stage/$m"
		chmod 0755 "$stage/$m"
	done
	printf 'commit=%s\nbuilt_at=selftest\ntarget=aarch64-unknown-linux-musl\ncomponents=rrd,rr3-ctl,rr3-ota-ctl,rr3-act,rr3-connect,rr3-interact,rr3-power,rr3-sense\nnatives=\n' \
		"$(git -C "$repo" rev-parse --short=12 HEAD)" >"$stage/BUNDLE_VERSION"
}

st_pack() {
	local stage="$1" out="$2"
	tar -C "$(dirname "$stage")" -czf "$out" "$(basename "$stage")"
}

st_case() { # 名称 期望码
	local rc=0
	bash "$0" "$st_bundle" aarch64-unknown-linux-musl >/dev/null 2>&1 || rc=$?
	if [ "$rc" -eq "$2" ]; then
		printf 'PASS  self-test/%s（退出码 %d）\n' "$1" "$rc"
	else
		printf 'FAIL  self-test/%s（期望 %d 实得 %d）\n' "$1" "$2" "$rc"
		st_fail=1
	fi
}

self_test() {
	local tmp
	tmp="$(mktemp -d)"
	TMPROOT="$tmp"
	st_bundle="$tmp/b.tar.gz"
	st_build "$tmp/rockrobot3-selftest"
	st_pack "$tmp/rockrobot3-selftest" "$st_bundle"
	st_case 合规包过 0
	rm "$tmp/rockrobot3-selftest/modules/rr3-sense/config.toml"
	st_pack "$tmp/rockrobot3-selftest" "$st_bundle"
	st_case 模块缺config红 1
	st_build "$tmp/rockrobot3-selftest2"
	printf 'not-an-elf\n' >"$tmp/rockrobot3-selftest2/bin/rrd"
	st_pack "$tmp/rockrobot3-selftest2" "$st_bundle"
	st_case 非ELF件红 1
	st_build "$tmp/rockrobot3-selftest3"
	sed -i.bak 's/^commit=.*/commit=000000000000/' "$tmp/rockrobot3-selftest3/BUNDLE_VERSION"
	rm -f "$tmp/rockrobot3-selftest3/BUNDLE_VERSION.bak"
	st_pack "$tmp/rockrobot3-selftest3" "$st_bundle"
	st_case commit不符红 1
	[ "$st_fail" -eq 0 ] || return 3
	return 0
}

case "${1:-}" in
--self-test)
	st_fail=0
	self_test
	exit $?
	;;
"")
	echo "用法: $0 <bundle.tar.gz> <target-triple> [预期 ELF 架构串]" >&2
	exit 2
	;;
*) main_check "$@" ;;
esac
