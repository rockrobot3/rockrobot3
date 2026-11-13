#!/usr/bin/env bash
# 铺镜像根目录（与 deb 装机同一份 rr3-pack stage 树，避免第二套装机布局）。
#
# 用法：stage-image.sh <stage-out> <arch> [version]
#   stage-image.sh /tmp/rr3-img amd64 0.0.6
#
# 前置：调用方已在**目标平台原生**构建好 release 产物（rrd / 六模块 / rr3-ctl /
# rr3-pack）。本脚本不编译、不下载，只做 载荷组装 → rr3-pack --stage-only →
# 把 `*.stage` 树移到 <stage-out>，供 Containerfile 以该目录为构建上下文。
#
# 为什么不用交叉：镜像是"装机面"，必须与目标架构的原生产物一致；amd64 用
# ubuntu-latest、arm64 用 ubuntu-24.04-arm（GitHub 原生 arm64 runner），
# 两边都走同一个 rr3-pack，装机布局只有一份真值。
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

out="${1:-}"
arch="${2:-}"
ver="${3:-0.0.6}"
if [ -z "$out" ] || [ -z "$arch" ]; then
  echo "用法: $0 <stage-out> <arch> [version]" >&2
  exit 1
fi

rel="${CARGO_TARGET_DIR:-/tmp/rr3-build}/release"
for b in rrd rr3-ctl rr3-pack; do
  [ -x "$rel/$b" ] || { echo "缺产物 $rel/$b（先原生 cargo build --release）" >&2; exit 1; }
done
for m in act brain connect interact sense power; do
  [ -x "$rel/rr3-$m" ] || { echo "缺产物 $rel/rr3-$m（先原生 cargo build --release）" >&2; exit 1; }
done

pay="$(mktemp -d)"
trap 'rm -rf "$pay"' EXIT
mkdir -p "$pay/rr3-modules"
cp "$rel/rrd" "$rel/rr3-ctl" "$pay/"
for m in act brain connect interact sense power; do
  mkdir -p "$pay/rr3-modules/rr3-$m"
  cp "$rel/rr3-$m" "$pay/rr3-modules/rr3-$m/bin"
  cp "modules/rr3-$m/module.toml" "$pay/rr3-modules/rr3-$m/manifest.toml"
done

pkgdir="$(mktemp -d)"
pkg="$pkgdir/rr3.deb"
export RR3_DEB_VERSION="$ver"
"$rel/rr3-pack" --payload "$pay" --repo "$root" --arch "$arch" \
  --out "$pkg" --stage-only

stage="$pkg.stage"
[ -d "$stage" ] || { echo "rr3-pack 未产出 stage 树: $stage" >&2; exit 1; }
rm -rf "$out"
cp -a "$stage" "$out"
rm -rf "$stage" "$pkgdir"
echo "PASS  镜像根就位: $out（arch=$arch，version=$ver）"
