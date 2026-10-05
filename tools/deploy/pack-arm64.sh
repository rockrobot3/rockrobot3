#!/usr/bin/env bash
# pack-arm64 —— 真板部署出包链（0.0.6 批B，方案 Q3 裁定：宿主交叉 + 板上出包）。
#
# 三步：①交叉编译（musl 静态 + rust-lld，零外部工具链——macOS 本机无 aarch64 glibc
# sysroot，brew 又是 Tier 3 无 bottle；musl std 自带完整 libc，产物全静态，树上
# 零依赖直接跑，即 ADR-0013 裁定 2"静态二进制集"的精神）；②注入产物装配
# （两轨同源约定 rr3-modules/<name>/{bin,manifest.toml}，与 CI distro-smoke ③ 同一口径，
# 不互抄装配）；③rr3-pack --stage-only（staging deb 布局树，rr3-pack 本机跑）。
#
# 真装不在这里：staging 树 scp 到板 → 板上 dpkg-deb --build + dpkg -i（Raspberry Pi OS
# 自带 dpkg-deb）→ 巡检——步骤见 base/os/distro/boards/raspberry-pi-64.md。
# 与 CI distro-smoke 的分工：那边容器内 x86 本机构建真装 deb（结构验证），
# 本脚本宿主交叉出 arm64 staging（真板部署用）——两者共用 rr3-pack 与注入约定。
set -euo pipefail

cd "$(dirname "$0")/../.." # 仓根

TARGET=aarch64-unknown-linux-musl
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rr3-arm64-target}"
REL="$CARGO_TARGET_DIR/$TARGET/release"
OUT="${1:-/tmp/rr3-arm64-deploy}"
PAY="$OUT/payload"
RR3_PACK="$CARGO_TARGET_DIR/release/rr3-pack" # 打包器本身按宿主架构跑

echo "== ① 交叉编译（$TARGET 静态，rust-lld）=="
export RUSTFLAGS="-C linker=rust-lld"
cargo build -q --locked --release --manifest-path base/runtime/Cargo.toml --target "$TARGET" -p rrd
cargo build -q --locked --release --manifest-path tools/rrctl/Cargo.toml --target "$TARGET"
for m in act connect interact sense power; do
  cargo build -q --locked --release --manifest-path "modules/rr3-$m/Cargo.toml" --target "$TARGET"
done
unset RUSTFLAGS
cargo build -q --locked --release --manifest-path tools/rr3-pack/Cargo.toml # 打包器按宿主跑

echo "== ② 组装注入产物（两轨同源约定）=="
rm -rf "$PAY"
mkdir -p "$PAY/rr3-modules"
cp "$REL/rrd" "$REL/rr3-ctl" "$PAY/"
for m in act connect interact sense power; do
  mkdir -p "$PAY/rr3-modules/rr3-$m"
  cp "$REL/rr3-$m" "$PAY/rr3-modules/rr3-$m/bin"
  cp "modules/rr3-$m/module.toml" "$PAY/rr3-modules/rr3-$m/manifest.toml"
done

echo "== ③ rr3-pack --stage-only（staging deb 布局树）=="
mkdir -p "$OUT"
"$RR3_PACK" --payload "$PAY" --repo "$(pwd)" --arch arm64 \
  --out "$OUT/rockrobot3_0.0.6_arm64.deb" --stage-only

STAGE="$OUT/rockrobot3_0.0.6_arm64.deb.stage"
echo "== ④ staging 布局断言（同 distro-smoke ⑤ 的 staging 版）=="
test -x "$STAGE/usr/bin/rrd"
test -x "$STAGE/usr/bin/rr3-ctl"
test -f "$STAGE/usr/lib/systemd/system/rrd.service"
test -f "$STAGE/usr/lib/rockrobot3/profiles/distro.toml"
test -f "$STAGE/usr/lib/rockrobot3/contracts/topics.toml"
test -f "$STAGE/usr/lib/rockrobot3/contracts/slots.toml"
for m in act connect interact sense power; do
  test -x "$STAGE/usr/lib/rockrobot3/modules/rr3-$m/rr3-$m"
  test -f "$STAGE/usr/lib/rockrobot3/modules/rr3-$m/module.toml"
  test -f "$STAGE/usr/lib/rockrobot3/modules/rr3-$m/config.toml"
done
grep -q "/usr/lib/rockrobot3/modules/rr3-act/rr3-act" \
  "$STAGE/usr/lib/rockrobot3/modules/rr3-act/module.toml" # entry 已装机绝对化
file "$STAGE/usr/bin/rrd" | grep -q "ELF 64-bit.*ARM aarch64.*statically linked" # 真板产物体检

echo "staging 树就绪: $STAGE"
echo "传板真装三步: scp -r '$STAGE' 板:~/stage && 板上 dpkg-deb --build ~/stage ./rockrobot3_0.0.6_arm64.deb && sudo dpkg -i ./rockrobot3_0.0.6_arm64.deb"
