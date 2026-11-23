#!/usr/bin/env bash
# RISC-V 运行镜像构建门（无 ghcr 推送）：交叉构建 → rr3-pack stage 树 →
# QEMU 构建 linux/riscv64 镜像 → 文件面/运行面两段冒烟。
#
# 为什么先做构建门而不是直接进 publish：发布面只在 tag/main 触发，改判据只能靠
# "打标签→等 CI"往返；本门在每次 push 的 platforms 里真跑镜像路径，先把
# stage/Containerfile/QEMU 三处跑通，发布腿随后只加 login/push/merge（已证形状）。
#
# 前置（platforms 的 riscv-image job 负责装）：rustup target
# riscv64gc-unknown-linux-gnu、apt gcc-riscv64-linux-gnu、qemu binfmt、docker。
# 用法：build-riscv-image.sh [镜像tag]（缺省 rr3-riscv-image:local）
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

tag="${1:-rr3-riscv-image:local}"
ver="${RR3_VER:-$(cat VERSION)}"
target="riscv64gc-unknown-linux-gnu"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rr3-build}"
# 交叉工具链按目标三元组命名（调用方未注入时取 apt 的规范名）。
export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER:-riscv64-linux-gnu-gcc}"
export CC_riscv64_unknown_linux_gnu="${CC_riscv64_unknown_linux_gnu:-riscv64-linux-gnu-gcc}"
export AR_riscv64_unknown_linux_gnu="${AR_riscv64_unknown_linux_gnu:-riscv64-linux-gnu-ar}"

for b in "$CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER" \
         "$CC_riscv64_unknown_linux_gnu" "$AR_riscv64_unknown_linux_gnu"; do
  command -v "$b" >/dev/null || {
    echo "ERROR: 缺交叉工具 ${b}（apt install gcc-riscv64-linux-gnu）" >&2
    exit 1
  }
done

# 1) 产物：复用交叉门（同一构建集合与 ELF 断言；brain 的 ring 需要 CC/AR）。
bash tools/ci/cross-artifacts.sh "$target" RISC-V any with-brain

# 2) 打包器必须是宿主原生（stage-image 会执行它，riscv 二进制在 x86 跑不了）。
cargo build -q --locked --release --manifest-path tools/rr3-pack/Cargo.toml
pack="$CARGO_TARGET_DIR/release/rr3-pack"
[ -x "$pack" ] || { echo "FAIL  宿主 rr3-pack 不在树: $pack" >&2; exit 1; }

# 3) stage 树：CARGO_TARGET_DIR 指到 triple 目录，stage-image 的 rel 即交叉产物面。
RR3_PACK_BIN="$pack" CARGO_TARGET_DIR="$CARGO_TARGET_DIR/$target" \
  bash tools/deploy/stage-image.sh /tmp/rr3-image-riscv riscv64 "$ver"

# 4) 镜像：Containerfile 的 apt-get 层在 QEMU 下执行（binfmt 由 setup-qemu 注册）。
docker build --platform linux/riscv64 \
  --label "org.opencontainers.image.title=rockrobot3" \
  --label "org.opencontainers.image.version=$ver" \
  --label "org.opencontainers.image.revision=$(git rev-parse HEAD)" \
  --label "org.opencontainers.image.source=${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-rockrobot/rockrobot3}" \
  -f tools/deploy/Containerfile -t "$tag" /tmp/rr3-image-riscv

# 5) 两段冒烟与发布面同一脚本（判据不抄第二份）。
bash tools/deploy/image-smoke.sh file "$tag"
bash tools/deploy/image-smoke.sh run "$tag"
echo "PASS  riscv64 运行镜像构建门（${tag}，version=${ver}，三件产物+brain 来自交叉构建）"
