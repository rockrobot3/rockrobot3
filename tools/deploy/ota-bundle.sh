#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# ota-bundle —— OTA 发布制品的组包、签名与自复验（ADR-0028 的机制半边）。
#
# 为什么单点：签名链的顺序（规范名 → manifest → 签名 → 用公钥复验 → 摘要清单）
# 是判据，不是流水线胶水。住在 workflow 内联 shell 里只能靠"打标签→等 CI"往返改，
# 本脚本把它做成可本地复跑的单一实现；workflow 只保留触发面、Secret 注入与
# ghcr 推送/不可变策略。机制与策略分离，和 image-smoke.sh 同一分工。
#
# 用法:
#   ota-bundle.sh --image <rootfs 镜像> --version <x.y.z> --key-file <种子> --out <目录>
#
#   --image    待签镜像（Buildroot 的 rr_a.ext4）；脚本复制为规范名后签规范名
#   --version  发布版本（major.minor.patch，由标签去掉前缀 v 得到）
#   --key-file 32 字节种子的 64 位十六进制文件（ADR-0026 裁定 5；unix 权限 0600）
#   --out      输出目录（必须不存在或为空；不覆盖既有制品）
#
# 产物（四件 + 两清单）:
#   rr3-rootfs-rk3566-<version>.img      规范名镜像
#   rr3-rootfs-rk3566-<version>.img.manifest
#   rr3-rootfs-rk3566-<version>.img.sig
#   keys/<key_id>.pub                    公钥（ADR-0026 安装树路径）
#   checksums.txt                        四件的 SHA-256
#   bundle.txt                           机器可读摘要（key_id / 文件名 / 版本）
#
# 环境:
#   RR3_OTA_CTL   rr3-ota-ctl 可执行文件（默认 PATH 查找；本地可指向 cargo 产物）
#
# 私钥纪律：只作为 --key-file 传给 CLI，本脚本不复制、不打印、不写进任何产物；
# 失败路径也不回显种子内容。
set -euo pipefail

PLATFORM=rk3566
BOARD=radxa-zero3
CTL="${RR3_OTA_CTL:-rr3-ota-ctl}"

usage() {
	sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
}

die() {
	echo "ERROR: $*" >&2
	exit 1
}

# 意外退出必须可归因（批301）：`set -e` 下的裸命令失败不经过 die，只剩退出码；
# OTA 包是发布载荷，卡住时必须能看到失败命令与所在行（与 install-bundle 同款）。
trap 'echo "ERROR: 意外退出：${BASH_COMMAND}（行 ${LINENO}）" >&2' ERR

IMAGE=""
VERSION=""
KEY_FILE=""
OUT=""

while [ $# -gt 0 ]; do
	case "$1" in
	--image | --version | --key-file | --out)
		[ $# -ge 2 ] || die "$1 缺参数值"
		case "$1" in
		--image) IMAGE=$2 ;;
		--version) VERSION=$2 ;;
		--key-file) KEY_FILE=$2 ;;
		--out) OUT=$2 ;;
		esac
		shift 2
		;;
	-h | --help) usage ;;
	*) die "未知参数 $1（用法见 --help）" ;;
	esac
done

[ -n "$IMAGE" ] || die "必须显式传 --image"
[ -n "$VERSION" ] || die "必须显式传 --version"
[ -n "$KEY_FILE" ] || die "必须显式传 --key-file"
[ -n "$OUT" ] || die "必须显式传 --out"

printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' ||
	die "--version 只认 major.minor.patch（实得 '$VERSION'）"
[ -s "$IMAGE" ] || die "镜像不存在或为空: $IMAGE"
[ -s "$KEY_FILE" ] || die "私钥种子文件不存在或为空: $KEY_FILE"
command -v "$CTL" >/dev/null 2>&1 || die "找不到 rr3-ota-ctl（RR3_OTA_CTL=$CTL）"

# 输出目录 fail-closed：既有目录必须为空，避免把上一版制品混进本版 bundle。
if [ -e "$OUT" ]; then
	[ -d "$OUT" ] || die "--out 已存在且不是目录: $OUT"
	[ -z "$(ls -A "$OUT")" ] || die "--out 目录非空，拒绝覆盖既有制品: $OUT"
else
	mkdir -p "$OUT"
fi

IMG_NAME="rr3-rootfs-rk3566-$VERSION.img"
IMG="$OUT/$IMG_NAME"
if [ "$IMAGE" -ef "$IMG" ]; then
	: # 输入已是规范名的同一文件：不复制
else
	cp -- "$IMAGE" "$IMG"
fi
chmod 0644 "$IMG"

# 1) 公钥与 key_id：先写临时名，拿到 key_id 后落到 keys/<key_id>.pub。
pub_line=$("$CTL" keypub --key-file "$KEY_FILE" --out-pub "$OUT/pub.tmp")
KEY_ID=$(printf '%s\n' "$pub_line" | sed -n 's/.*key_id=\([0-9a-f][0-9a-f]*\).*/\1/p' | head -n 1)
[ -n "$KEY_ID" ] || {
	rm -f "$OUT/pub.tmp"
	die "keypub 输出缺少 key_id：$pub_line"
}
mkdir -p "$OUT/keys"
mv -- "$OUT/pub.tmp" "$OUT/keys/$KEY_ID.pub"
chmod 0644 "$OUT/keys/$KEY_ID.pub"

# 2) manifest 与签名：都作用在规范名镜像上，manifest 记录单一路径分量。
"$CTL" manifest --image "$IMG" --platform "$PLATFORM" --board "$BOARD" \
	--version "$VERSION" --key-file "$KEY_FILE" --out "$IMG.manifest" >/dev/null
"$CTL" sign --manifest "$IMG.manifest" --key-file "$KEY_FILE" \
	--sig-out "$IMG.sig" >/dev/null

# 3) 发布前自复验：用刚导出的公钥与镜像本体复跑完整验签链；失败即不出 bundle。
"$CTL" verify --manifest "$IMG.manifest" --sig "$IMG.sig" \
	--pub "$OUT/keys/$KEY_ID.pub" --image "$IMG" \
	--platform "$PLATFORM" --board "$BOARD" >/dev/null

# 4) 摘要清单：相对路径 + 稳定顺序（镜像、manifest、签名、公钥）。
sha_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | awk '{print $1}'
	else
		shasum -a 256 "$1" | awk '{print $1}'
	fi
}
(
	cd "$OUT"
	{
		printf '%s  %s\n' "$(sha_of "$IMG_NAME")" "$IMG_NAME"
		printf '%s  %s\n' "$(sha_of "$IMG_NAME.manifest")" "$IMG_NAME.manifest"
		printf '%s  %s\n' "$(sha_of "$IMG_NAME.sig")" "$IMG_NAME.sig"
		printf '%s  %s\n' "$(sha_of "keys/$KEY_ID.pub")" "keys/$KEY_ID.pub"
	} >checksums.txt
)

cat >"$OUT/bundle.txt" <<EOF
version=$VERSION
platform=$PLATFORM
board=$BOARD
image=$IMG_NAME
manifest=$IMG_NAME.manifest
sig=$IMG_NAME.sig
pub=keys/$KEY_ID.pub
key_id=$KEY_ID
EOF

echo "PASS  OTA bundle 已生成：$OUT"
echo "      image=$IMG_NAME key_id=$KEY_ID"
echo "      checksums.txt 四件（镜像 / manifest / 签名 / 公钥）"
