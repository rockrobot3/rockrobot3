#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 版本化部署包（A10）：把同一 commit 的二进制、装配单、契约、模块清单/配置打成
# 一个单元，杜绝"只换 rrd 不换 profile/模块"的版本偏斜（2026-11-08 真板实录：
# HEAD rrd + 旧 profile/旧模块 → 装载拒启 + 描述符键不认 → 模块崩溃循环）。
#
# 用法：bash tools/deploy/pack-bundle.sh [target-triple] [out.tar.gz]
# 前置：先跑 bash tools/ci/platform/arm64-cross-check.sh（产物在 /tmp/rr3-build）。
# 产物：<out>.tar.gz，内含 rockrobot3-<commit>/{bin,modules,profiles,contracts,
#       hardware,BUNDLE_VERSION}；BUNDLE_VERSION 记录 commit/target/构建时间。
#
# 音频件例外（R13）：ALSA 与 musl 全静态结构性互斥，交叉件在宿主档必落 placeholder
# ——rr3-interact 播放面探活会 exit(1)（2026-11-09 真板实录）。设
# `RR3_BUNDLE_NATIVE_DIR=<dir>` 可把板上原生构建件覆盖进包：目录形状接受
# `<dir>/<模块名>/<模块名>` 或 `<dir>/<模块名>/bin`（后者即 build-onboard 的
# payload 布局）。覆盖件写入 BUNDLE_VERSION 的 natives 行，装机后仍可追溯。
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TARGET=${1:-aarch64-unknown-linux-musl}
COMMIT=$(git -C "$ROOT" rev-parse --short=12 HEAD)
OUT=${2:-/tmp/rockrobot3-$COMMIT-$TARGET.tar.gz}
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/rr3-bundle.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT

# 产物定位共享单点（与烧录/镜像门同源）：target-dir 在源码区外，不得写死
. "$ROOT/tools/ci/lib/cargo-artifact.sh"
# 模块清单取仓内真值（modules/rr3-*/module.toml），不再在脚本里抄名字。
. "$ROOT/tools/ci/lib/runtime-members.sh"

die() { echo "FAIL  $*" >&2; exit 1; }

mods=$(rr3_modules "$ROOT") || die "仓内没有任何 modules/rr3-*/module.toml（模块清单真值缺席）"

B="$STAGE/rockrobot3-$COMMIT"
mkdir -p "$B/bin" "$B/profiles" "$B/contracts" "$B/hardware" "$B/modules"

for name in rrd rr3-ctl; do
  src=$( cd "$ROOT/base/runtime" && rr3_cargo_artifact "$TARGET" release "$name" ) \
    || die "$name 产物定位失败"
  [ -f "$src" ] || die "$name 未构建: ${src}（先跑 bash tools/ci/platform/arm64-cross-check.sh）"
  install -m755 "$src" "$B/bin/$name"
done
src=$( cd "$ROOT/base/ota/rr3-ota-ctl" && rr3_cargo_artifact "$TARGET" release rr3-ota-ctl ) \
  || die "rr3-ota-ctl 产物定位失败"
[ -f "$src" ] || die "rr3-ota-ctl 未构建: ${src}（先跑 bash tools/ci/platform/arm64-cross-check.sh）"
install -m755 "$src" "$B/bin/rr3-ota-ctl"

NATIVE_DIR=${RR3_BUNDLE_NATIVE_DIR:-}
natives=""
components="rrd,rr3-ctl,rr3-ota-ctl"
for m in $mods; do
  # musl 交叉例外：brain 的 ring 需目标 C 工具链，musl/rust-lld 腿不带；归板上原生
  # 构建（与 cross-artifacts 第四参、pack-arm64 的同一分工）。新模块默认随包。
  if [ "$m" = brain ]; then
    continue
  fi
  name="rr3-$m"
  src=""
  if [ -n "$NATIVE_DIR" ]; then
    for cand in "$NATIVE_DIR/$name/$name" "$NATIVE_DIR/$name/bin"; do
      if [ -x "$cand" ]; then
        src=$cand
        natives="$natives$name,"
        break
      fi
    done
  fi
  if [ -z "$src" ]; then
    src=$( cd "$ROOT/modules/$name" && rr3_cargo_artifact "$TARGET" release "$name" ) \
      || die "$name 产物定位失败"
    [ -f "$src" ] || die "$name 未构建: ${src}（先跑 bash tools/ci/platform/arm64-cross-check.sh）"
  fi
  mkdir -p "$B/modules/$name"
  install -m755 "$src" "$B/modules/$name/$name"
  cp "$ROOT/modules/$name/module.toml" "$B/modules/$name/"
  for cfg in "$ROOT/modules/$name"/config*.toml; do
    [ -f "$cfg" ] && cp "$cfg" "$B/modules/$name/"
  done
  components="$components,$name"
done

cp -R "$ROOT/profiles/." "$B/profiles/"
cp -R "$ROOT/contracts/." "$B/contracts/"
cp -R "$ROOT/hardware/." "$B/hardware/"

cat > "$B/BUNDLE_VERSION" <<EOF
commit=$COMMIT
built_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
target=$TARGET
components=$components
natives=${natives%,}
EOF

tar -C "$STAGE" -czf "$OUT" "rockrobot3-$COMMIT"

echo "PASS  部署包: $OUT"
echo "      版本: commit=$COMMIT target=$TARGET native=${natives:-无}"
