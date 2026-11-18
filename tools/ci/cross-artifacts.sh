#!/usr/bin/env bash
# 交叉平台产物门（多芯片共用一份机制）。
#
# 用法：cross-artifacts.sh <target-triple> <ELF 架构匹配串> [static|any]
#   cross-artifacts.sh aarch64-unknown-linux-musl 'ARM aarch64'
#   cross-artifacts.sh x86_64-unknown-linux-gnu 'x86-64' any
#   CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER=riscv64-linux-gnu-gcc \
#     cross-artifacts.sh riscv64gc-unknown-linux-gnu 'RISC-V' any
#
# 机制与目标分离：构建集合（rrd + rr3-ctl + rr3-ota-ctl + 五模块 + 协议契约探针）、产物在树断言、
# ELF 架构断言都只有一份；各芯片平台的差异只是数据——目标三元组、`file` 的架构串、
# 静态断言开关。协议契约探针 `ipct003_dump` 不进产品清单，只供各芯片环境真跑
# 128B 头 golden 对拍（调用方按 native/musl/gnu 三种方式执行）。
# 与 tools/deploy/pack-bundle.sh ① 同源；brain 不在列——ureq+rustls(ring) 需
# 目标 C 工具链，归板上原生构建（pack-bundle 的 ③ 与 build-onboard.sh 同一分工）。
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
if [ -z "$target" ] || [ -z "$arch" ]; then
  echo "用法: $0 <target-triple> <file 架构匹配串> [static|any]" >&2
  exit 1
fi
case "$assert" in
static | any) ;;
*)
  echo "ERROR: 第三参只认 static / any（实得 ${assert}）" >&2
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
cargo build -q --locked --release --target "$target" \
  --manifest-path base/runtime/Cargo.toml -p rr-ipc --bin ipct003_dump

rel="$CARGO_TARGET_DIR/$target/release"
for b in rrd rr3-ctl rr3-ota-ctl rr3-act rr3-connect rr3-interact rr3-sense rr3-power ipct003_dump; do
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
echo "PASS  $target 八件产物＋协议契约探针（${arch}，断言=${assert}）"
