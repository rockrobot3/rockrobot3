#!/usr/bin/env bash
# OTA 签名/验签演练（ADR-0026，A1c）：CI 用临时测试种子走完整链路，
# 不触碰真实私钥；正例一条 + 负例四条（镜像篡改/清单篡改/错公钥/降级）。
#
# 用法：ota-sign-smoke.sh <rr3-ota-ctl 可执行文件>
# 退出码：0 全部通过；1 任一步失败（文案点名失败对象）。
set -euo pipefail

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

BIN="${1:-}"
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  echo "用法: $0 <rr3-ota-ctl 可执行文件>" >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/rr3-ota-sign.XXXXXX")"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "FAIL  $*" >&2
  exit 1
}

key="$work/sign.key"
pub="$work/sign.pub"
image="$work/rr3-rootfs-smoke.img"
manifest="$work/rr3-ota.manifest"
sig="$work/rr3-ota.sig"

umask 077
printf '%s\n' "$(printf '11%.0s' $(seq 1 32))" > "$key"
chmod 600 "$key"
printf 'rr3-rootfs-smoke' > "$image"

"$BIN" keypub --key-file "$key" --out-pub "$pub" >/dev/null \
  || fail "keypub 失败"
"$BIN" manifest --image "$image" --platform rk3566 --board radxa-zero3 \
  --version 0.0.8 --key-file "$key" --out "$manifest" >/dev/null \
  || fail "manifest 生成失败"
"$BIN" sign --manifest "$manifest" --key-file "$key" --sig-out "$sig" >/dev/null \
  || fail "签名失败"

verify() {
  "$BIN" verify --manifest "$manifest" --sig "$sig" --pub "$pub" \
    --image "$image" --platform rk3566 --board radxa-zero3 "$@"
}

verify --current-version 0.0.7 >/dev/null || fail "正例验签失败"
verify --current-version 0.0.8 --allow-downgrade >/dev/null || fail "显式降级应通过"

expect_fail() {
  local needle="$1"
  shift
  local log="$work/negative.log"
  if "$@" >"$log" 2>&1; then
    fail "负例未被拒（期望含 ${needle}）"
  fi
  grep -qF "$needle" "$log" || fail "负例拒因缺 ${needle}（实得：$(tr '\n' ' ' < "$log")）"
}

bad_image="$work/bad.img"
cp "$image" "$bad_image"
printf 'x' >> "$bad_image"
expect_fail "镜像校验失败" \
  "$BIN" verify --manifest "$manifest" --sig "$sig" --pub "$pub" \
  --image "$bad_image" --platform rk3566 --board radxa-zero3

bad_manifest="$work/bad.manifest"
sed 's/version=0.0.8/version=0.0.9/' "$manifest" > "$bad_manifest"
expect_fail "验签失败" \
  "$BIN" verify --manifest "$bad_manifest" --sig "$sig" --pub "$pub" \
  --image "$image" --platform rk3566 --board radxa-zero3

other_key="$work/other.key"
other_pub="$work/other.pub"
printf '%s\n' "$(printf '22%.0s' $(seq 1 32))" > "$other_key"
chmod 600 "$other_key"
"$BIN" keypub --key-file "$other_key" --out-pub "$other_pub" >/dev/null \
  || fail "第二把 keypub 失败"
expect_fail "key_id 不匹配" \
  "$BIN" verify --manifest "$manifest" --sig "$sig" --pub "$other_pub" \
  --image "$image" --platform rk3566 --board radxa-zero3

expect_fail "拒绝降级" \
  "$BIN" verify --manifest "$manifest" --sig "$sig" --pub "$pub" \
  --image "$image" --platform rk3566 --board radxa-zero3 --current-version 0.0.8

echo "PASS  ota-sign-smoke（keypub→manifest→sign→verify + 4 负例）"
