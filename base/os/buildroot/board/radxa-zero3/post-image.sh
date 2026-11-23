#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# post-image.sh —— genimage.cfg 的容量参数注入（M1"板上就绪"档，实测=板上）。
#
# base/ota/README.md 明裁：容量不在文档/配置里冻结，"凭想象写下的 size 是最贵的
# 那种错误"。genimage.cfg 因此全部用 @XXX_SIZE@ 占位符，容量在本脚本这一步注入：
#
#   1. 环境变量（RR3_IMG_BOOT / RR3_IMG_RR_A / ... 显式指定时用之）；
#   2. 否则用下方 FALLBACK 的保守缺省——只保证 make 能跑通、结构成立，
#      **不等于**板上定型值；容量定型（随实板分区表复核）属板上收口。
#
# 同时注入 @TARGET_DIR@：genimage.sh 的 --rootpath 是空临时目录，ext4 段必须用
# srcpath 指向真实 target 目录才装得进 rootfs（否则生成空文件系统）。
#
# 注入产物写到 $BUILD_DIR/genimage-rock3.cfg（Buildroot post-image 脚本环境提供
# BUILD_DIR 与 TARGET_DIR），不回写仓内模板——genimage.cfg 在 git 里只有占位符。
#
# defconfig 里脚本序：本脚本 → support/scripts/genimage.sh（-c 指向注入产物）。

set -eu

BOARD_DIR="$(dirname "$0")"
OUT="${BUILD_DIR:-/tmp}/genimage-rock3.cfg"
if [ ! -d "$(dirname "$OUT")" ]; then
	echo "ERROR: 输出目录不存在: $(dirname "$OUT")（BUILD_DIR=${BUILD_DIR:-<未设>}；Buildroot post-image 会提供）" >&2
	exit 1
fi

SED_ARGS=""
# $1 = 占位名, $2 = 环境变量名, $3 = 保守缺省（板上定型前不得当作权威值）
# 不用 eval 间接展开：环境变量值进 eval 等于把构建环境的字符串当代码执行
# （值含 ; $() 空格即改脚本行为）。printenv 只取值不求值；容量形制再过一道
# 白名单（<数字>[KMG]）——非法即点名拒，不把坏值揉进 sed 表达式。
inject() {
	val="$(printenv "$2" 2>/dev/null || true)"
	[ -n "$val" ] || val="$3"
	if ! printf '%s' "$val" | grep -Eq '^[0-9]+[KMG]$'; then
		echo "ERROR: \$$2 形制非法: '$val'（只认 <数字>[KMG]，如 256M / 1G）" >&2
		exit 1
	fi
	SED_ARGS="$SED_ARGS -e s|@$1@|$val|g"
}

inject BOOT_SIZE       RR3_IMG_BOOT       256M
inject RR_A_SIZE       RR3_IMG_RR_A       1G
inject RR_B_SIZE       RR3_IMG_RR_B       1G
inject RR_FACTORY_SIZE RR3_IMG_RR_FACTORY 512M
inject RR_STAGING_SIZE RR3_IMG_RR_STAGING 256M
inject RR_DATA_SIZE    RR3_IMG_RR_DATA    1G

# target 目录绝对路径注入（srcpath）。路径形制严格白名单：它是 sed 替换值，
# 含 | & \ 空格或换行都会改变替换语义；Buildroot 的 O= 路径固定形如
# /tmp/br-out/target，不符合即点名拒，不把构建环境字符串当代码。
TARGET="$(printenv TARGET_DIR 2>/dev/null || true)"
[ -n "$TARGET" ] || {
	echo "ERROR: TARGET_DIR 未设（Buildroot post-image 会提供）" >&2
	exit 1
}
[ -d "$TARGET" ] || {
	echo "ERROR: TARGET_DIR 不是目录: $TARGET" >&2
	exit 1
}
if ! printf '%s' "$TARGET" | grep -Eq '^[A-Za-z0-9_./-]+$'; then
	echo "ERROR: TARGET_DIR 形制非法（只认 [A-Za-z0-9_./-]）：'$TARGET'" >&2
	exit 1
fi
SED_ARGS="$SED_ARGS -e s|@TARGET_DIR@|$TARGET|g"

# shellcheck disable=SC2086
sed $SED_ARGS "$BOARD_DIR/genimage.cfg" > "$OUT"

# 占位符残留检查：漏注入会让 genimage 以难懂的报错失败（或悄悄生成空 rootfs），
# 这里提前点名全部未注入占位符。
if grep -q '@[A-Z_]*@' "$OUT"; then
	echo "ERROR: genimage-rock3.cfg 仍有未注入的占位符：" >&2
	grep -n '@[A-Z_]*@' "$OUT" >&2
	exit 1
fi

echo "post-image.sh: 容量参数已注入 -> $OUT"
