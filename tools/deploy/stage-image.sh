#!/usr/bin/env bash
# 铺镜像根目录（与 deb 装机同一份 rr3-pack stage 树，避免第二套装机布局）。
#
# 用法：stage-image.sh <stage-out> <arch> [version]
#   stage-image.sh /tmp/rr3-img amd64 0.0.6
#
# 前置：调用方已在**目标平台**构建好 release 产物（rrd / 六模块 / rr3-ctl /
# rr3-ota-ctl）；交叉腿把 CARGO_TARGET_DIR 指到 `<triple>` 目录，并用
# RR3_PACK_BIN 指向宿主原生 rr3-pack（打包器必须能在当前宿主执行）。
# 本脚本不编译、不下载，只做 载荷组装 → rr3-pack --stage-only →
# 把 `*.stage` 树移到 <stage-out>，供 Containerfile 以该目录为构建上下文。
#
# 发布面仍坚持原生：amd64 用 ubuntu-latest、arm64 用 ubuntu-24.04-arm，两边走
# 同一个 rr3-pack，装机布局只有一份真值；riscv64 的**构建门**（platforms 的
# riscv-image 腿）在 x86 宿主交叉构建后用 QEMU 真跑镜像，仅为提前验证镜像路径，
# 不冒充原生发布腿。
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
pack_bin="${RR3_PACK_BIN:-$rel/rr3-pack}"
for b in rrd rr3-ctl rr3-ota-ctl; do
  [ -x "$rel/$b" ] || { echo "缺产物 $rel/${b}（先原生 cargo build --release）" >&2; exit 1; }
done
[ -x "$pack_bin" ] || {
  echo "缺打包器 ${pack_bin}（交叉腿用 RR3_PACK_BIN 指向宿主原生 rr3-pack）" >&2
  exit 1
}
for m in act brain connect interact sense power; do
  [ -x "$rel/rr3-$m" ] || { echo "缺产物 $rel/rr3-${m}（先原生 cargo build --release）" >&2; exit 1; }
done

pay="$(mktemp -d)"
trap 'rm -rf "$pay"' EXIT
mkdir -p "$pay/rr3-modules"
cp "$rel/rrd" "$rel/rr3-ctl" "$rel/rr3-ota-ctl" "$pay/"
for m in act brain connect interact sense power; do
  mkdir -p "$pay/rr3-modules/rr3-$m"
  cp "$rel/rr3-$m" "$pay/rr3-modules/rr3-$m/bin"
  cp "modules/rr3-$m/module.toml" "$pay/rr3-modules/rr3-$m/manifest.toml"
done

pkgdir="$(mktemp -d)"
pkg="$pkgdir/rr3.deb"
export RR3_DEB_VERSION="$ver"
"$pack_bin" --payload "$pay" --repo "$root" --arch "$arch" \
  --out "$pkg" --stage-only

stage="$pkg.stage"
[ -d "$stage" ] || { echo "rr3-pack 未产出 stage 树: $stage" >&2; exit 1; }
rm -rf "$out"
cp -a "$stage" "$out"
rm -rf "$stage" "$pkgdir"
echo "PASS  镜像根就位: ${out}（arch=${arch}，version=${ver}）"
