#!/usr/bin/env bash
# aarch64-musl 静态产物构建门（树莓派 / 瑞芯微部署面，CI 可跑）。
#
# 与 tools/deploy/pack-arm64.sh ① 同源：rust-lld + musl 全静态，产物零外部
# 工具链依赖；但本门只验"可构建 + 真静态 aarch64"，不需要真板、不跑打包器。
# brain 不在列——ureq+rustls(ring) 需 aarch64 C 工具链，归板上原生构建
# （pack-arm64 的 ③ 与 build-onboard.sh 同一分工，勿在此复制 ring 交叉链）。
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

TARGET=aarch64-unknown-linux-musl
export RUSTFLAGS="-C linker=rust-lld"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rr3-build}"

cargo build -q --locked --release --target "$TARGET" \
  --manifest-path base/runtime/Cargo.toml -p rrd
cargo build -q --locked --release --target "$TARGET" \
  --manifest-path tools/rrctl/Cargo.toml
for m in act connect interact sense power; do
  cargo build -q --locked --release --target "$TARGET" \
    --manifest-path "modules/rr3-$m/Cargo.toml"
done

rel="$CARGO_TARGET_DIR/$TARGET/release"
for b in rrd rr3-ctl rr3-act rr3-connect rr3-interact rr3-sense rr3-power; do
  if [ ! -x "$rel/$b" ]; then
    echo "FAIL  产物不在树: $rel/$b" >&2
    exit 1
  fi
  if ! file "$rel/$b" | grep -q 'ELF 64-bit.*ARM aarch64.*statically linked'; then
    echo "FAIL  非静态 aarch64 产物: $rel/$b" >&2
    file "$rel/$b" >&2 || true
    exit 1
  fi
done
echo "PASS  aarch64-musl 静态产物七件（rrd + rr3-ctl + 五模块）"
