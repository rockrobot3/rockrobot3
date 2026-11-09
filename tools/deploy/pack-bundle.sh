#!/usr/bin/env bash
# 版本化部署包（A10）：把同一 commit 的二进制、装配单、契约、模块清单/配置打成
# 一个单元，杜绝"只换 rrd 不换 profile/模块"的版本偏斜（2026-11-08 真板实录：
# HEAD rrd + 旧 profile/旧模块 → 装载拒启 + 描述符键不认 → 模块崩溃循环）。
#
# 用法：bash tools/deploy/pack-bundle.sh [target-triple] [out.tar.gz]
# 前置：先跑 bash tools/ci/arm64-cross-check.sh（产物在 /tmp/rr3-build）。
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
. "$ROOT/tools/ci/cargo-artifact.sh"

die() { echo "FAIL  $*" >&2; exit 1; }

B="$STAGE/rockrobot3-$COMMIT"
mkdir -p "$B/bin" "$B/profiles" "$B/contracts" "$B/hardware" "$B/modules"

for name in rrd rr3-ctl; do
  src=$( cd "$ROOT/base/runtime" && rr3_cargo_artifact "$TARGET" release "$name" ) \
    || die "$name 产物定位失败"
  [ -f "$src" ] || die "$name 未构建: $src（先跑 bash tools/ci/arm64-cross-check.sh）"
  install -m755 "$src" "$B/bin/$name"
done

NATIVE_DIR=${RR3_BUNDLE_NATIVE_DIR:-}
natives=""
for m in rr3-act rr3-connect rr3-interact rr3-power rr3-sense; do
  src=""
  if [ -n "$NATIVE_DIR" ]; then
    for cand in "$NATIVE_DIR/$m/$m" "$NATIVE_DIR/$m/bin"; do
      if [ -x "$cand" ]; then
        src=$cand
        natives="$natives$m,"
        break
      fi
    done
  fi
  if [ -z "$src" ]; then
    src=$( cd "$ROOT/modules/$m" && rr3_cargo_artifact "$TARGET" release "$m" ) \
      || die "$m 产物定位失败"
    [ -f "$src" ] || die "$m 未构建: $src（先跑 bash tools/ci/arm64-cross-check.sh）"
  fi
  mkdir -p "$B/modules/$m"
  install -m755 "$src" "$B/modules/$m/$m"
  cp "$ROOT/modules/$m/module.toml" "$B/modules/$m/"
  for cfg in "$ROOT/modules/$m"/config*.toml; do
    [ -f "$cfg" ] && cp "$cfg" "$B/modules/$m/"
  done
done

cp -R "$ROOT/profiles/." "$B/profiles/"
cp -R "$ROOT/contracts/." "$B/contracts/"
cp -R "$ROOT/hardware/." "$B/hardware/"

cat > "$B/BUNDLE_VERSION" <<EOF
commit=$COMMIT
built_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
target=$TARGET
components=rrd,rr3-ctl,rr3-act,rr3-connect,rr3-interact,rr3-power,rr3-sense
natives=${natives%,}
EOF

tar -C "$STAGE" -czf "$OUT" "rockrobot3-$COMMIT"

echo "PASS  部署包: $OUT"
echo "      版本: commit=$COMMIT target=$TARGET native=${natives:-无}"
