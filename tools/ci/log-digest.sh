#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# log-digest —— 构建失败日志的单点归因（CI 注解机制，2026-11-17 立）。
#
# 为什么单点：buildroot 轨的失败面（stage-build / br-defconfig / br-uboot /
# br-build / ota-build）此前各自写一份 grep 与末段打印，噪音过滤口径不一致，
# 同一类日志在不同 step 给出的归因不同。本脚本把"信号提取 → 噪音过滤 →
# 单行注解 → 末段回显"做成唯一实现；workflow 只保留标题、日志路径与回显行数
# （策略），不再复制正则。
#
# 用法：
#   log-digest.sh --title <注解标题> --log <日志路径> [选项]
#   log-digest.sh --self-test
#
# 选项：
#   --tail N     回显末 N 行到 stdout（缺省 8；0 = 只出注解）
#   --max N      注解消息上限（字符，缺省 900；GitHub 过长注解会被截断）
#   --sig RE     覆盖信号正则（缺省见 SIG_DEFAULT）
#   --extra RE   追加信号正则（与缺省取并集）
#   --head       优先取最早的 12 条信号（缺省取最后 12 条）
#   --phases     并入 Buildroot 的 `>>>` 阶段推进行（最多 3 条）
#   --note TXT   追加固定上下文（如 dtc 版本），不参与日志解析
#   --plain      只打印消息、不加 `::error` 前缀（自检用，避免假注解）
#
# 退出码：0 = 已给出归因（调用方负责 exit 1）；2 = 用法错误。
set -uo pipefail

# 信号 = 失败的第一现场；噪音 = 含 signal 关键字却不指向失败的宿主工具输出。
# pyerrors.h 是 2026-11-17 实证的污染源：host-python 的路径命中 "error" 关键字。
SIG_DEFAULT='error|fatal|failed|failure|no space|no such file|not found|cannot|undefined|bad value|no rule|stop\.|binman:|too old|non-zero'
NOISE_DEFAULT='\(ignored\)|broken pipe|makeinfo is (too old|missing)|pyerrors\.h'

title=""
log=""
tail_n=8
max=900
sig=""
extra=""
phases=0
head_first=0
note=""
plain=0

die() {
	echo "log-digest: $*" >&2
	exit 2
}

usage() {
	sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
}

# 注解必须是单行：多行会被 GitHub 拆成逐条注解并只保留前 10 条，真因被挤掉。
oneline() {
	tr '\n' ' ' | tr -s ' ' | sed 's/^ *//; s/ *$//'
}

digest() {
	local hits="" msg=""
	if [ ! -e "$log" ]; then
		# ${log} 必须带花括号：紧跟多字节标点时 $log（ 会被当成变量名的一部分。
		msg="日志缺失：${log}（命令可能在写日志前就失败）"
	else
		# 是否调用方显式给了 --sig（决定随后用大小写敏感还是缺省的不敏感）
		local caller_sig=0
		[ -n "$sig" ] && caller_sig=1
		if [ -n "$sig" ] && [ -n "$extra" ]; then
			sig="$sig|$extra"
		elif [ -z "$sig" ]; then
			sig=$SIG_DEFAULT${extra:+|$extra}
		fi
		# 调用方给 --sig 时大小写敏感：-i 会让 `^test .* FAILED` 命中
		# "test result: ok. 0 failed"，真 FAIL 行被正常测试尾巴挤出 900 字符上限
		# （2026-11-24 gate 注解实测）。缺省 SIG_DEFAULT 仍是大小写不敏感。
		local sflags="-aiE"
		[ "$caller_sig" -eq 1 ] && sflags="-aE"
		if [ "$head_first" -eq 1 ]; then
			hits=$(grep "$sflags" "$sig" "$log" | grep -aviE "$NOISE_DEFAULT" | head -12 | oneline)
		else
			hits=$(grep "$sflags" "$sig" "$log" | grep -aviE "$NOISE_DEFAULT" | tail -12 | oneline)
		fi
		if [ -z "$hits" ]; then
			hits="无信号匹配，末段：$(tail -3 "$log" 2>/dev/null | oneline)"
		fi
		if [ "$phases" -eq 1 ]; then
			local ph
			ph=$(grep -aE '^>>> ' "$log" | tail -3 | oneline)
			[ -n "$ph" ] && hits="phase: $ph | sig: $hits"
		fi
		[ -n "$note" ] && hits="$note | $hits"
		msg=$hits
	fi
	# % 是 GitHub 注解的转义前缀；不转义时 %0A 会被当换行吃掉后续内容。
	msg=${msg//'%'/'%25'}
	msg=${msg:0:$max}
	if [ "$plain" -eq 1 ]; then
		printf '%s\n' "$msg"
	else
		printf '::error title=%s::%s\n' "$title" "$msg"
	fi
	if [ "$tail_n" -gt 0 ] && [ -s "$log" ]; then
		printf '%s\n' "---- 末 $tail_n 行 ----"
		tail -n "$tail_n" "$log"
	fi
}

# 自检夹具：一条含真信号、噪音与 phase 行的构建日志。
st_fixture() {
	cat >"$ST_TMP/build.log" <<'EOF'
>>> host-python3 3.11.8 Extracting
make[2]: *** [Makefile:1270: all] Error 1 (ignored)
Makeinfo is too old (4.13); documentation will not be built
/usr/include/python3.11/pyerrors.h scanned by keyword grep
error: could not compile `rr3-act` due to previous error
make: *** [package/pkg-generic.mk:283: /tmp/br-out/build/uboot-2024.10] Error 2
EOF
}

# 正例：真信号进注解；噪音两行被滤掉；phase 行并入
st_positive() {
	local out
	out=$(bash "$0" --title t --log "$ST_TMP/build.log" --phases --plain --tail 0)
	case "$out" in
	"phase: >>> host-python3"*) ;;
	*) echo "FAIL  自检: phase 行未并入: $out"; st_fail=1 ;;
	esac
	case "$out" in
	*"Error 2"*) ;;
	*) echo "FAIL  自检: 真信号缺失: $out"; st_fail=1 ;;
	esac
	case "$out" in
	*pyerrors.h* | *"(ignored)"* | *"Makeinfo"*)
		echo "FAIL  自检: 噪音未被滤掉: $out"
		st_fail=1
		;;
	esac
}

# 负例 1：日志缺失也必须给归因，不能静默
st_neg_missing() {
	local out
	out=$(bash "$0" --title t --log "$ST_TMP/nope.log" --plain --tail 0)
	case "$out" in
	*"日志缺失"*) ;;
	*) echo "FAIL  自检: 缺日志未点名: $out"; st_fail=1 ;;
	esac
}

# 负例 2：--max 必须截断（--tail 0：回显会带出真信号，破坏"截断"断言）
st_neg_max() {
	local out
	out=$(bash "$0" --title t --log "$ST_TMP/build.log" --plain --max 40 --tail 0)
	case "$out" in
	*"Error 2"*) echo "FAIL  自检: --max 未截断: $out"; st_fail=1 ;;
	esac
}

# 负例 3：末尾回显确实在 stdout（tail 1 = make 的 Error 2 行）
st_neg_tail() {
	local out
	out=$(bash "$0" --title t --log "$ST_TMP/build.log" --plain --tail 1)
	case "$out" in
	*"末 1 行"*) ;;
	*) echo "FAIL  自检: 末段回显缺失: $out"; st_fail=1 ;;
	esac
}

# 负例 4：--head 优先取最早信号（供给类日志：命令级错误在前、逐项 FAIL 在后；
# 条目多于 12 条时 tail 会把命令级错误挤出注解）
st_neg_head() {
	local out i
	{
		echo "error: unknown flag --strict"
		for i in $(seq 1 14); do echo "FAIL  modules/m$i"; done
	} >"$ST_TMP/head.log"
	out=$(bash "$0" --title t --log "$ST_TMP/head.log" --sig '^error(\[|:)|^FAIL' \
		--head --plain --tail 0)
	case "$out" in
	*"unknown flag"*) ;;
	*) echo "FAIL  自检: --head 未取到首条信号: $out"; st_fail=1 ;;
	esac
	case "$out" in
	*"m14"*)
		echo "FAIL  自检: --head 取到了尾部信号: $out"
		st_fail=1
		;;
	esac
}

self_test() {
	ST_TMP=$(mktemp -d) || die "mktemp 失败"
	trap 'rm -rf "$ST_TMP"' EXIT
	st_fail=0
	st_fixture
	st_positive
	st_neg_missing
	st_neg_max
	st_neg_tail
	st_neg_head
	[ "$st_fail" -eq 0 ] && echo "PASS  log-digest 自检（正例 + 四条负例）"
	return "$st_fail"
}

[ "${1:-}" = "--self-test" ] && { self_test; exit $?; }

while [ $# -gt 0 ]; do
	case "$1" in
	--title | --log | --tail | --max | --sig | --extra | --note)
		[ $# -ge 2 ] || die "$1 缺参数值"
		case "$1" in
		--title) title=$2 ;;
		--log) log=$2 ;;
		--tail) tail_n=$2 ;;
		--max) max=$2 ;;
		--sig) sig=$2 ;;
		--extra) extra=$2 ;;
		--note) note=$2 ;;
		esac
		shift 2
		;;
	--phases) phases=1; shift ;;
	--head) head_first=1; shift ;;
	--plain) plain=1; shift ;;
	-h | --help) usage ;;
	*) die "未知参数 $1（用法见 --help）" ;;
	esac
done

[ -n "$title" ] || die "必须传 --title"
[ -n "$log" ] || die "必须传 --log"
printf '%s' "$tail_n" | grep -Eq '^[0-9]+$' || die "--tail 只认非负整数"
printf '%s' "$max" | grep -Eq '^[0-9]+$' || die "--max 只认正整数"

digest
