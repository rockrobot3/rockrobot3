#!/bin/sh
# post-image.sh —— genimage.cfg 的容量参数注入（M1"板上就绪"档，实测=板上）。
#
# base/ota/README.md 明裁：容量不在文档/配置里冻结，"凭想象写下的 size 是最贵的
# 那种错误"。genimage.cfg 因此全部用 @XXX_SIZE@ 占位符，容量在本脚本这一步注入：
#
#   1. 环境变量（RR3_IMG_BOOT / RR3_IMG_RR_A / ... 显式指定时用之）；
#   2. 否则用下方 FALLBACK 的保守缺省——只保证 make 能跑通、结构成立，
#      **不等于**板上定型值；容量定型（随实板分区表复核）属板上收口。
#
# 注入产物写到 $BUILD_DIR/genimage-rock3.cfg（Buildroot post-image 脚本环境提供
# BUILD_DIR），不回写仓内模板——genimage.cfg 在 git 里永远只有占位符。
#
# defconfig 里脚本序：本脚本 → support/scripts/genimage.sh（-c 指向注入产物）。

set -eu

BOARD_DIR="$(dirname "$0")"
OUT="${BUILD_DIR:-/tmp}/genimage-rock3.cfg"

SED_ARGS=""
# $1 = 占位名, $2 = 环境变量名, $3 = 保守缺省（板上定型前不得当作权威值）
inject() {
	eval "val=\${$2:-$3}"
	SED_ARGS="$SED_ARGS -e s|@$1@|$val|g"
}

inject BOOT_SIZE       RR3_IMG_BOOT       256M
inject RR_A_SIZE       RR3_IMG_RR_A       1G
inject RR_B_SIZE       RR3_IMG_RR_B       1G
inject RR_FACTORY_SIZE RR3_IMG_RR_FACTORY 512M
inject RR_STAGING_SIZE RR3_IMG_RR_STAGING 256M
inject RR_DATA_SIZE    RR3_IMG_RR_DATA    1G

# shellcheck disable=SC2086
sed $SED_ARGS "$BOARD_DIR/genimage.cfg" > "$OUT"

# 占位符残留检查：漏注入的 size 会让 genimage 以难懂的报错失败，这里提前点名。
if grep -q '@[A-Z_]*_SIZE@' "$OUT"; then
	echo "ERROR: genimage-rock3.cfg 仍有未注入的容量占位符：" >&2
	grep -n '@[A-Z_]*_SIZE@' "$OUT" >&2
	exit 1
fi

echo "post-image.sh: 容量参数已注入 -> $OUT"
