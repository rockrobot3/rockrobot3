#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# arm64-deb-check —— 瑞芯微 aarch64 glibc deb 结构门（批230，2026-11-28）。
#
# 为什么需要它：install-smoke 只在 x86_64 上真装（runner 面），distro bundle 是
# aarch64-musl 五模块（brain 需 C 工具链，不在 musl 交叉面）；而 RK3566 distro
# 快路轨的 base 层是 aarch64 glibc deb（六模块含 brain），此前只有真板证据。
# 本门用 apt 交叉 gcc-aarch64-linux-gnu 构建十件 → rr3-pack 出 deb → 只验包结构
# 与 ELF 架构。**不冒充安装**：systemd 跑不进 qemu-user，真装仍归 install-smoke
# 与真板；这里挡的是"包能出但成员/架构/入口不对"这类出包面回归。
#
# 构建集合复用 cross-artifacts.sh（gnu + with-brain）：机制只有一份，本脚本只给
# 目标数据与出包/断言策略。仅 CI 调用（需交叉工具链与 dpkg-deb）。
set -euo pipefail
cd "$(dirname "$0")/../../.." # 仓根

TARGET=aarch64-unknown-linux-gnu
ARCH=arm64
ELF="ARM aarch64"
REL=/tmp/rr3-build/$TARGET/release
HOST=/tmp/rr3-build/release
WORK=/tmp/rr3-arm64-deb
DEB=""
MODULES="act brain connect interact sense power"

die() { echo "FAIL  $*" >&2; exit 1; }

# 宿主前置：交叉 C 工具链（ring/alsa 需要）、dpkg-deb、cargo。
preflight() {
  command -v aarch64-linux-gnu-gcc >/dev/null \
    || die "缺 aarch64-linux-gnu-gcc（apt 包 gcc-aarch64-linux-gnu）"
  command -v aarch64-linux-gnu-ar >/dev/null \
    || die "缺 aarch64-linux-gnu-ar（apt 包 gcc-aarch64-linux-gnu）"
  command -v dpkg-deb >/dev/null || die "缺 dpkg-deb"
  command -v cargo >/dev/null || die "缺 cargo"
}

# 十件产物：rrd + 六模块 + 两工具走 cross-artifacts 单点；rr3-pack 按宿主架构
# 构建（打包器在 runner 上跑，不随目标架构）。
build_all() {
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
  export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
  export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
  bash tools/ci/platform/cross-artifacts.sh "$TARGET" "$ELF" any with-brain
  cargo build -q --locked --release --manifest-path tools/rr3-pack/Cargo.toml
}

# 注入载荷（两轨同源约定：rr3-modules/<name>/{bin,manifest.toml}）。
make_payload() {
  local m
  rm -rf "$WORK/payload"
  mkdir -p "$WORK/payload/rr3-modules"
  cp "$REL/rrd" "$REL/rr3-ctl" "$REL/rr3-ota-ctl" "$WORK/payload/"
  for m in $MODULES; do
    mkdir -p "$WORK/payload/rr3-modules/rr3-$m"
    cp "$REL/rr3-$m" "$WORK/payload/rr3-modules/rr3-$m/bin"
    cp "modules/rr3-$m/module.toml" "$WORK/payload/rr3-modules/rr3-$m/manifest.toml"
  done
}

# 出 deb：版本与仓根 VERSION 同源，架构钉 arm64（不是宿主 amd64）。
pack_deb() {
  local ver
  ver=$(cat VERSION)
  export RR3_DEB_VERSION="$ver"
  DEB="$WORK/rockrobot3_${ver}_${ARCH}.deb"
  "$HOST/rr3-pack" --payload "$WORK/payload" --repo "$PWD" \
    --arch "$ARCH" --out "$DEB"
  [ -f "$DEB" ] || die "rr3-pack 未出包"
}

# control 三字段：包名、架构、版本。
assert_control() {
  local got
  got=$(dpkg-deb -f "$DEB" Package) || die "dpkg-deb 读 control 失败"
  [ "$got" = rockrobot3 ] || die "Package=${got}（期望 rockrobot3）"
  got=$(dpkg-deb -f "$DEB" Architecture)
  [ "$got" = "$ARCH" ] || die "Architecture=${got}（期望 ${ARCH}）"
  got=$(dpkg-deb -f "$DEB" Version)
  [ "$got" = "$(cat VERSION)" ] || die "Version=${got}（期望 $(cat VERSION)）"
}

# 包内清单：三入口、unit、四册、平台册、接线档、六模块各三件（逐件点名）。
assert_tree() {
  local list b m f
  list=$(dpkg-deb -c "$DEB") || die "dpkg-deb -c 失败"
  for b in usr/bin/rrd usr/bin/rr3-ctl usr/bin/rr3-ota-ctl \
    usr/lib/systemd/system/rrd.service; do
    grep -q " ./$b\$" <<<"$list" || die "deb 缺 $b"
  done
  for b in profiles/distro.toml contracts/topics.toml contracts/slots.toml \
    contracts/roles.toml contracts/devices.toml contracts/platform/rk3566.toml \
    hardware/distro/wiring.toml; do
    grep -q " ./usr/lib/rockrobot3/$b\$" <<<"$list" || die "deb 缺 $b"
  done
  for m in $MODULES; do
    for f in rr3-$m module.toml config.toml; do
      grep -q " ./usr/lib/rockrobot3/modules/rr3-$m/$f\$" <<<"$list" \
        || die "deb 缺 modules/rr3-$m/$f"
    done
  done
}

# 解包后断言：unit 的 --profile 指向装机位、postinst 可执行、九件二进制全 aarch64。
assert_content() {
  local tmp m b line
  tmp="$WORK/x"
  rm -rf "$tmp" "$WORK/ctl"
  dpkg-deb -x "$DEB" "$tmp" || die "dpkg-deb -x 失败"
  dpkg-deb -e "$DEB" "$WORK/ctl" || die "dpkg-deb -e 失败"
  [ -x "$WORK/ctl/postinst" ] || die "postinst 缺可执行位"
  grep -q '^ExecStart=.*--profile /usr/lib/rockrobot3/profiles/distro.toml' \
    "$tmp/usr/lib/systemd/system/rrd.service" \
    || die "unit 的 --profile 未指向装机位"
  for b in usr/bin/rrd usr/bin/rr3-ctl usr/bin/rr3-ota-ctl; do
    line=$(file -b "$tmp/$b")
    case "$line" in *"$ELF"*) ;; *) die "$b 非 aarch64 ELF：$line" ;; esac
  done
  for m in $MODULES; do
    b="usr/lib/rockrobot3/modules/rr3-$m/rr3-$m"
    line=$(file -b "$tmp/$b")
    case "$line" in *"$ELF"*) ;; *) die "$b 非 aarch64 ELF：$line" ;; esac
  done
}

main() {
  preflight
  mkdir -p "$WORK"
  build_all
  make_payload
  pack_deb
  assert_control
  assert_tree
  assert_content
  echo "PASS  aarch64 deb 结构：$(dpkg-deb -f "$DEB" Package) $(dpkg-deb -f "$DEB" Version) ${ARCH}；三入口/六模块/四册/平台册/接线/unit/postinst 齐，九件 ELF 全 aarch64（不冒充安装）"
  echo "::notice title=arm64-deb::$(dpkg-deb -f "$DEB" Package) $(dpkg-deb -f "$DEB" Version) ${ARCH}；六模块/四册/平台册/接线/unit/postinst 齐，九件 ELF 全 aarch64（结构门，不冒充安装）"
}

main "$@"
