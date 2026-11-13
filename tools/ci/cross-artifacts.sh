#!/usr/bin/env bash
# 交叉平台静态产物门（多芯片共用一份机制）。
#
# 用法：cross-artifacts.sh <target-triple> <ELF 架构匹配串>
#   cross-artifacts.sh aarch64-unknown-linux-musl 'ARM aarch64'
#   cross-artifacts.sh riscv64gc-unknown-linux-musl 'RISC-V'
#
# 机制与目标分离：构建集合（rrd + rr3-ctl + 五模块）、静态链接断言、产物在树
# 断言都只有一份；各芯片平台的差异只是两段数据——目标三元组与 `file` 的架构串。
# 与 tools/deploy/pack-bundle.sh ① 同源；brain 不在列——ureq+rustls(ring) 需
# 目标 C 工具链，归板上原生构建（pack-bundle 的 ③ 与 build-onboard.sh 同一
# 分工，不在此复制 ring 交叉链）。
#
# 只用 rust-lld + musl 全静态，零外部工具链；不需要真板、不跑打包器。
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

target="${1:-}"
arch="${2:-}"
if [ -z "$target" ] || [ -z "$arch" ]; then
  echo "用法: $0 <target-triple> <file 架构匹配串>" >&2
  exit 1
fi

export RUSTFLAGS="-C linker=rust-lld"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rr3-build}"

cargo build -q --locked --release --target "$target" \
  --manifest-path base/runtime/Cargo.toml -p rrd
cargo build -q --locked --release --target "$target" \
  --manifest-path tools/rrctl/Cargo.toml
for m in act connect interact sense power; do
  cargo build -q --locked --release --target "$target" \
    --manifest-path "modules/rr3-$m/Cargo.toml"
done

rel="$CARGO_TARGET_DIR/$target/release"
for b in rrd rr3-ctl rr3-act rr3-connect rr3-interact rr3-sense rr3-power; do
  if [ ! -x "$rel/$b" ]; then
    echo "FAIL  产物不在树: $rel/$b" >&2
    exit 1
  fi
  if ! file "$rel/$b" | grep -q "ELF 64-bit.*$arch.*statically linked"; then
    echo "FAIL  非静态 $arch 产物: $rel/$b" >&2
    file "$rel/$b" >&2 || true
    exit 1
  fi
done
echo "PASS  $target 静态产物七件（rrd + rr3-ctl + 五模块，$arch）"
