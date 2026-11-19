#!/usr/bin/env bash
# 交叉平台产物门（多芯片共用一份机制）。
#
# 用法：cross-artifacts.sh <target-triple> <ELF 架构匹配串> [static|any] [with-brain]
#   cross-artifacts.sh aarch64-unknown-linux-musl 'ARM aarch64'
#   cross-artifacts.sh x86_64-unknown-linux-gnu 'x86-64' any with-brain
#   CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER=riscv64-linux-gnu-gcc \
#     CC_riscv64_unknown_linux_gnu=riscv64-linux-gnu-gcc \
#     cross-artifacts.sh riscv64gc-unknown-linux-gnu 'RISC-V' any with-brain
#
# 机制与目标分离：构建集合（rrd + rr3-ctl + rr3-ota-ctl + 五模块 + 协议契约探针）、
# 产物在树断言、ELF 架构断言都只有一份；各芯片平台的差异只是数据——目标三元组、
# `file` 的架构串、静态断言开关、是否带 brain。协议契约探针 `ipct003_dump` 不进
# 产品清单，只供各芯片环境真跑 128B 头 golden 对拍（调用方按 native/musl/gnu 三种方式执行）。
# brain 只在第四参 with-brain 时构建：ureq+rustls(ring) 需目标 C 工具链（CC/AR
# 由调用方按三元组注入）；有 C 工具链的腿（原生 x86_64、apt 交叉 riscv64）带上，
# musl/rust-lld 腿无目标 C 工具链，保持不构建（与 pack-bundle 的 ③ / build-onboard
# 板上原生构建同一分工）。
#
# 构建器：cargo；musl 目标加 rust-lld（承载全静态、零外部工具链），gnu 目标
# 用系统/交叉链接器（调用方经 CARGO_TARGET_<TRIPLE>_LINKER 注入）。
# 第三参 static（缺省）断言全静态；any 跳过该断言（gnu 目标动态链接）。
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

target="${1:-}"
arch="${2:-}"
assert="${3:-static}"
with_brain="${4:-}"
if [ -z "$target" ] || [ -z "$arch" ]; then
  echo "用法: $0 <target-triple> <file 架构匹配串> [static|any] [with-brain]" >&2
  exit 1
fi
case "$assert" in
static | any) ;;
*)
  echo "ERROR: 第三参只认 static / any（实得 ${assert}）" >&2
  exit 1
  ;;
esac
case "$with_brain" in
"" | with-brain) ;;
*)
  echo "ERROR: 第四参只认 with-brain（实得 ${with_brain}）" >&2
  exit 1
  ;;
esac

# rust-lld 只对 musl 目标启用：它让 musl 全静态链接零外部工具链；gnu 目标
# （原生腿与 riscv64 交叉腿）走系统/交叉链接器，否则找不到 crt 对象与 libc
# （2026-11-16 首跑实证：x86_64 原生腿被 rust-lld 判红）。
case "$target" in
*musl*) export RUSTFLAGS="-C linker=rust-lld" ;;
esac
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rr3-build}"

cargo build -q --locked --release --target "$target" \
  --manifest-path base/runtime/Cargo.toml -p rrd
cargo build -q --locked --release --target "$target" \
  --manifest-path tools/rrctl/Cargo.toml
for m in act connect interact sense power; do
  cargo build -q --locked --release --target "$target" \
    --manifest-path "modules/rr3-$m/Cargo.toml"
done
cargo build -q --locked --release --target "$target" \
  --manifest-path base/ota/rr3-ota-ctl/Cargo.toml
if [ "$with_brain" = with-brain ]; then
  cargo build -q --locked --release --target "$target" \
    --manifest-path modules/rr3-brain/Cargo.toml
fi
cargo build -q --locked --release --target "$target" \
  --manifest-path base/runtime/Cargo.toml -p rr-ipc --bin ipct003_dump

rel="$CARGO_TARGET_DIR/$target/release"
products="rrd rr3-ctl rr3-ota-ctl rr3-act rr3-connect rr3-interact rr3-sense rr3-power ipct003_dump"
if [ "$with_brain" = with-brain ]; then
  products="$products rr3-brain"
fi
for b in $products; do
  if [ ! -x "$rel/$b" ]; then
    echo "FAIL  产物不在树: $rel/$b" >&2
    exit 1
  fi
  line="$(file "$rel/$b")"
  if ! printf '%s\n' "$line" | grep -q "ELF 64-bit.*$arch"; then
    echo "FAIL  ELF 架构不符（期望 ${arch}）: $rel/$b" >&2
    printf '%s\n' "$line" >&2
    exit 1
  fi
  if [ "$assert" = static ] && ! printf '%s\n' "$line" | grep -q "statically linked"; then
    echo "FAIL  非静态产物: $rel/$b" >&2
    printf '%s\n' "$line" >&2
    exit 1
  fi
done
echo "PASS  $target 产物全在树＋架构匹配（${arch}，断言=${assert}，brain=${with_brain:-不带}）"
