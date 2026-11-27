#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 发布面齐套巡检（只读，批178）：一个 tag 的五类制品是否都在 ghcr、成员与证明面是否合规。
#
# 为什么需要它：发版后"东西齐不齐"此前靠人按 runbook B 逐条 curl/imagetools，判据散在
# 文档里、命名靠记忆（2026-11-27 核对时就发现 status 里 OTA 标签写法与 buildroot.yml
# 实推的 `v<VER>` 不一致）。本脚本把发布集收成一张数据表，逐项只读核对并打印 digest，
# 不发任何写请求；发布面本身仍由 publish.yml / buildroot.yml 产出（机制与策略分离）。
#
# 用法：release-verify.sh --tag vX.Y.Z|auto [--repo owner/name] [--classes image,firmware,distro,ota,buildroot]
#                             [--with-signature] [--deep]
#       release-verify.sh --self-test
# 判据：0 所选类别全齐且成员/证明面合规 / 1 有缺件或不合规（逐项点名）
#       2 用法错误，或取数失败（凭据/网络/包不可见——"看不到"不等于"没有"，未定）
# 依赖：oras（按 docker 登录凭据读私有包）、python3；本脚本不登录、不推送。
set -uo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null) || {
	echo "FAIL  不在 git 仓库内" >&2
	exit 2
}

TAG=""
OWNER="rockrobot3"
CLASSES="image,firmware,distro,ota,buildroot"
# 签名身份约束与 runbook B.1 同源（只认 publish.yml 在正式 semver tag 上的 keyless 身份）。
SIGN=0
DEEP=0
SIG_ID_RE='^https://github\.com/rockrobot3/rockrobot3/\.github/workflows/publish\.yml@refs/tags/v[0-9]+\.[0-9]+\.[0-9]+$'
SIG_ISSUER='https://token.actions.githubusercontent.com'
WORK=""
FAILS=0
FETCH_FAILS=0
# 回执用 digest 表（批192）：B.2 第 3 项要求五类 digest，人工抄五条易错；巡检时顺手记。
DIGESTS=""

# 类别过滤：publish 与 buildroot 两条流水线各自只产一半，tag 窗口不能用"全齐"卡
# 对方的产物（buildroot 是小时级，晚于 publish）。默认五类，操作者用全量口径。
has_class() {
	case ",${CLASSES}," in
	*",$1,"*) return 0 ;;
	esac
	return 1
}

usage() {
	sed -n '3,18p' "$0" | sed 's/^# \{0,1\}//'
	exit 2
}

# 计数型判据：每件只打印一行，FAILS 汇总，避免 set -e 在第一件就中断后面的核对。
ok() { printf 'PASS  %s\n' "$*"; }

record_digest() { # <类别> <ref> <digest>
	DIGESTS="${DIGESTS}$(printf '%-9s %-62s %s\n' "$1" "$2" "${3:-未取到}")"$'\n'
}
bad() {
	printf 'FAIL  %s\n' "$*"
	FAILS=$((FAILS + 1))
}

# 取回 manifest 到文件。失败再分两种（批186 实测必要）：
#   not found / manifest unknown / 404 ⇒ **缺件**（结论明确，退 1）
#   401/403/网络/其它                 ⇒ **未定**（凭据或可见性，退 2）
# 混成一种会把"还没发"读成"看不到"（v0.0.8 的 OTA/buildroot 正是前者）。
fetch() { # <ref> <out>
	local ref="$1" out="$2" err
	err=$(oras manifest fetch "$ref" 2>&1 >"$out")
	if [ $? -eq 0 ] && [ -s "$out" ]; then
		ok "取回 ${ref}"
		return 0
	fi
	if printf '%s' "$err" | grep -qiE 'not found|manifest unknown|404'; then
		bad "缺件 ${ref}"
		return 1
	fi
	FETCH_FAILS=$((FETCH_FAILS + 1))
	printf 'FAIL  取数失败 %s：%s\n' "$ref" "$(printf '%s' "$err" | head -n 1)"
	return 1
}

# 层标题集合（oras push 用 org.opencontainers.image.title 记文件名）。
titles_of() {
	python3 - "$1" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
for l in m.get("layers", []):
    t = (l.get("annotations") or {}).get("org.opencontainers.image.title")
    if t:
        print(t)
PY
}

archs_of() {
	python3 - "$1" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
for e in m.get("manifests", []):
    a = (e.get("platform") or {}).get("architecture")
    if a and a != "unknown":
        print(a)
PY
}

# buildx 的证明面形状（2026-11-27 批185 匿名实测）：单架构索引里只有**一条**
# attestation manifest，SLSA provenance 与 SPDX SBOM 是它内部的两枚 in-toto 层——
# 不是"两条 attestation manifest"（旧文档口径与真实形状不符）。判据据此按层判。
attest_digest() {
	python3 - "$1" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
for e in m.get("manifests", []):
    if (e.get("annotations") or {}).get("vnd.docker.reference.type") == "attestation-manifest":
        print(e.get("digest", ""))
        break
PY
}

# 层标题 → digest（深检拉层用）。
title_digest() { # <manifest.json> <title>
	python3 - "$1" "$2" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
for l in m.get("layers", []):
    if (l.get("annotations") or {}).get("org.opencontainers.image.title") == sys.argv[2]:
        print(l.get("digest", ""))
        break
PY
}

predicates_of() {
	python3 - "$1" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
for l in m.get("layers", []):
    p = (l.get("annotations") or {}).get("in-toto.io/predicate-type")
    if p:
        print(p)
PY
}

# 标签指向的 digest：按 descriptor 取（authoritative），不走"本地重算哈希"。
ref_digest() {
	oras manifest fetch --descriptor "$1" 2>/dev/null \
		| python3 -c 'import json,sys; print(json.load(sys.stdin).get("digest",""))' 2>/dev/null
}

# cosign keyless 验签（批188）：身份约束是安全判据，不做可配置放宽。
# 未启用 --with-signature 时零成本跳过；启用而本机缺 cosign 由调用前 gate 拒绝。
verify_sig() { # <标签> <仓（owner/name）> <digest>
	[ "$SIGN" -eq 1 ] || return 0
	local label="$1" repo="$2" dg="$3" err
	if [ -z "$dg" ]; then
		bad "${label} 无 digest，无法验签"
		return 1
	fi
	err=$(cosign verify "ghcr.io/${repo}@${dg}" \
		--certificate-identity-regexp "$SIG_ID_RE" \
		--certificate-oidc-issuer "$SIG_ISSUER" 2>&1 >/dev/null)
	if [ $? -eq 0 ]; then
		ok "${label} 签名验证通过（keyless 身份约束）"
		return 0
	fi
	bad "${label} 签名验证失败：$(printf '%s' "$err" | head -n 1)"
	return 1
}

# 精确成员判据：required 逐个在层标题里找，缺一即 FAIL（点名缺哪件）。
need_titles() { # <标签> <json> <标题...>
	local label="$1" json="$2" t have miss=""
	have=$(titles_of "$json")
	shift 2
	for t in "$@"; do
		printf '%s\n' "$have" | grep -qxF "$t" || miss="$miss $t"
	done
	if [ -n "$miss" ]; then
		bad "${label} 缺层：${miss# }"
	else
		ok "${label} 成员齐（$# 件）"
	fi
}

# 镜像索引：三架构齐全；每条单架构清单带一条 attestation，内有两枚 in-toto 谓词。
check_image() { # <镜像仓> <tag>
	local img="$1" tag="$2" d="$WORK/index.json" a f dg af preds miss
	fetch "${img}:${tag}" "$d" || return 1
	record_digest 镜像 "${img}:${tag}" "$(ref_digest "${img}:${tag}")"
	local archs want="amd64 arm64 riscv64" miss=""
	archs=$(archs_of "$d")
	for a in $want; do
		printf '%s\n' "$archs" | grep -qx "$a" || miss="$miss $a"
	done
	if [ -n "$miss" ]; then
		bad "镜像索引缺架构：${miss# }"
	else
		ok "镜像索引三架构齐（amd64/arm64/riscv64）"
	fi
	verify_sig 镜像总账 "${OWNER}/rockrobot3" "$(ref_digest "${img}:${tag}")"
	for a in $want; do
		f="$WORK/img-$a.json"
		if ! fetch "${img}:${tag}-${a}" "$f"; then
			continue
		fi
		dg=$(attest_digest "$f")
		if [ -z "$dg" ]; then
			bad "镜像 ${a} 缺 attestation manifest"
			continue
		fi
		af="$WORK/att-$a.json"
		fetch "${img}@${dg}" "$af" || continue
		preds=$(predicates_of "$af")
		miss=""
		for pred in https://slsa.dev/provenance/v1 https://spdx.dev/Document; do
			printf '%s\n' "$preds" | grep -qxF "$pred" || miss="$miss $pred"
		done
		if [ -n "$miss" ]; then
			bad "镜像 ${a} 证明面缺：${miss# }"
		else
			ok "镜像 ${a} 证明面=SLSA provenance + SPDX SBOM"
		fi
	done
}

# 固件与 distro：成员清单是发布合同的一部分（与 publish.yml 的资产集逐名对齐）。
check_firmware() {
	local ref="ghcr.io/${OWNER}/rockrobot3-firmware:${TAG}" d="$WORK/firmware.json"
	fetch "$ref" "$d" || return 1
	record_digest 固件 "$ref" "$(ref_digest "$ref")"
	need_titles 固件 "$d" \
		rr3-periph-base-esp32-safe-dev.elf \
		rr3-periph-base-esp32s3-full-dev.elf \
		rr3-esp32-safe-merged.bin \
		rr3-esp32s3-full-merged.bin \
		rr3-bootloader-esp32-dev.bin \
		rr3-bootloader-esp32s3-dev.bin \
		rr3-partitions.csv FLASHING.md firmware-sha256.txt
	verify_sig 固件 "${OWNER}/rockrobot3-firmware" "$(ref_digest "$ref")"
}

check_distro() {
	local ref="ghcr.io/${OWNER}/rockrobot3-distro:${TAG}" d="$WORK/distro.json" ver="${TAG#v}" t
	fetch "$ref" "$d" || return 1
	record_digest distro "$ref" "$(ref_digest "$ref")"
	need_titles distro "$d" "rockrobot3-distro-${ver}-aarch64.tar.gz" distro-sha256.txt
	t=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1],encoding="utf-8")).get("artifactType",""))' "$d")
	if [ "$t" = "application/vnd.rr3.distro-bundle.v1.tar+gzip" ]; then
		ok "distro artifactType=${t}"
	else
		bad "distro artifactType 不符：${t:-空}"
	fi
	verify_sig distro "${OWNER}/rockrobot3-distro" "$(ref_digest "$ref")"
	deep_distro "$ref" "$d" "rockrobot3-distro-${ver}-aarch64.tar.gz"
}

# 深检（批201，--deep）：把已发布的 tar 拉回本地再过 A10 契约——层名对只证明"有这个名字"，
# 不证明"内容还能装"。已发布包的提交是 tag 当时的，故契约按 `any` 跳 commit 锚（tag 真伪
# 由 cosign 签名背书），其余成员判据一条不松。
deep_distro() { # <ref> <manifest.json> <tar 层标题>
	[ "$DEEP" -eq 1 ] || return 0
	local dg dir tar
	dg=$(title_digest "$2" "$3")
	if [ -z "$dg" ]; then
		bad "distro 深检：清单里没有 ${3} 层"
		return 1
	fi
	dir="$WORK/deep-distro"
	mkdir -p "$dir"
	if ! oras pull "$1" -o "$dir" >/dev/null 2>"$WORK/pull.err"; then
		FETCH_FAILS=$((FETCH_FAILS + 1))
		printf 'FAIL  取数失败 %s（深检拉层）：%s\n' "$1" "$(head -n 1 "$WORK/pull.err")"
		return 1
	fi
	tar="$dir/$3"
	if [ ! -f "$tar" ]; then
		bad "distro 深检：拉回缺 ${3}"
		return 1
	fi
	if bash "$root/tools/ci/platform/bundle-contract.sh" "$tar" \
		aarch64-unknown-linux-musl 'ARM aarch64' any >/dev/null 2>&1; then
		ok "distro 深检：发布包过 A10 契约（${3}）"
		return 0
	fi
	bad "distro 深检：发布包不合 A10 契约（${3}）"
	return 1
}

# OTA：签名链与密钥必须随包（keys/*.pub 名称带 key_id，按形状判）。
check_ota() {
	local ref="ghcr.io/${OWNER}/rockrobot3-ota:${TAG}" d="$WORK/ota.json" ver="${TAG#v}" have
	fetch "$ref" "$d" || return 1
	record_digest OTA "$ref" "$(ref_digest "$ref")"
	need_titles OTA "$d" "rr3-rootfs-rk3566-${ver}.img" "rr3-rootfs-rk3566-${ver}.img.manifest" \
		"rr3-rootfs-rk3566-${ver}.img.sig" checksums.txt bundle.txt
	have=$(titles_of "$d")
	if printf '%s\n' "$have" | grep -qE '^keys/[^/]+\.pub$'; then
		ok "OTA 含公钥层（keys/<id>.pub）"
	else
		bad "OTA 缺公钥层（keys/<id>.pub）"
	fi
}

check_buildroot() {
	local ref="ghcr.io/${OWNER}/rockrobot3-buildroot:${TAG}" d="$WORK/buildroot.json"
	fetch "$ref" "$d" || return 1
	record_digest buildroot "$ref" "$(ref_digest "$ref")"
	need_titles buildroot "$d" rock3-zero3.img rk3566-radxa-zero-3w.dtb \
		rk3566-radxa-zero-3e.dtb checksums.txt
}

rr3_run() {
	WORK=$(mktemp -d)
	trap 'rm -rf "$WORK"' EXIT
	has_class image && check_image "ghcr.io/${OWNER}/rockrobot3" "$TAG"
	has_class firmware && check_firmware
	has_class distro && check_distro
	has_class ota && check_ota
	has_class buildroot && check_buildroot
	printf '\n== 制品 digest（回执直接引用）==\n%s\n' "$DIGESTS"
	# "看不到"与"没有"必须分开：鉴权/网络失败时打"不齐"会把操作者引去查制品，
	# 而真正要修的是凭据。取数失败只要有一处，整份结论就是未定（退出 2）。
	if [ "$FETCH_FAILS" -ne 0 ]; then
		echo "结论：取数失败 ${FETCH_FAILS} 处（凭据/网络/包不可见），另有不合规 ${FAILS} 处——未定（tag=${TAG}）"
		return 2
	fi
	if [ "$FAILS" -ne 0 ]; then
		echo "结论：发布面不齐/不合规 ${FAILS} 处（tag=${TAG}）"
		return 1
	fi
	echo "结论：发布面齐套（classes=${CLASSES}，tag=${TAG}）"
	return 0
}

# --tag auto（批189）：取 registry 上最新的正式 semver tag。周程不能用仓根 VERSION
# 直接拼 tag——"VERSION 已 bump、tag 未推"是发布前的正常窗口，按 VERSION 会周程常红；
# 取已发布的最新 tag 才是"公开交付面现在是什么"的正确问题。
rr3_resolve_tag() {
	[ "$TAG" = auto ] || return 0
	local found
	found=$(oras repo tags "ghcr.io/${OWNER}/rockrobot3" 2>/dev/null \
		| grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+' | sort -V | tail -n 1)
	if [ -z "$found" ]; then
		echo "FAIL  --tag auto 未能从 registry 取到正式 tag（网络/可见性）" >&2
		return 2
	fi
	TAG="$found"
	ok "tag auto → ${TAG}"
	return 0
}

while [ $# -gt 0 ]; do
	case "$1" in
	--tag) TAG="${2:?--tag 缺参数}"; shift 2 ;;
	--repo) OWNER="${2:?--repo 缺参数}"; OWNER="${OWNER%%/*}"; shift 2 ;;
	--classes) CLASSES="${2:?--classes 缺参数}"; shift 2 ;;
	--with-signature) SIGN=1; shift ;;
	--deep) DEEP=1; shift ;;
	--self-test)
		# 自检以假 oras 喂夹具，判据腐化即红（见文件末自检段）。
		. "$(dirname "$0")/release-verify.selftest.sh"
		rr3_rv_self_test
		exit $?
		;;
	*) usage ;;
	esac
done
[ -n "$TAG" ] || usage
# 类别名只认五值：拼错时若静默跳过全部判据，巡检会"全绿"而一件没看（F-2 同型）。
case "$CLASSES" in
"" | *, | ,* | *,,* | *[!a-z,]*)
	echo "FAIL  --classes 只认 image,firmware,distro,ota,buildroot（实得 ${CLASSES}）" >&2
	exit 2
	;;
esac
for c in ${CLASSES//,/ }; do
	case " image firmware distro ota buildroot " in
	*" $c "*) ;;
	*) echo "FAIL  --classes 含未知类别 ${c}" >&2; exit 2 ;;
	esac
done
command -v oras >/dev/null || { echo "FAIL  本机无 oras（发布面取数依赖它）" >&2; exit 2; }
rr3_resolve_tag || exit $?
printf '%s' "$TAG" | grep -qE '^v[0-9]+\.[0-9]+\.[0-9]+$' \
	|| { echo "FAIL  --tag 只认正式版 vX.Y.Z 或 auto（实得 ${TAG}）" >&2; exit 2; }
if [ "$SIGN" -eq 1 ]; then
	command -v cosign >/dev/null || {
		echo "FAIL  --with-signature 需要 cosign（keyless 验签不可跳过）" >&2
		exit 2
	}
fi
rr3_run
