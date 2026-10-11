#!/usr/bin/env bash
# IPC-T-003 跨架构对拍：x86 与 aarch64 字节布局一致（ADR-0006 后果项）。
#
# **不在门禁步序内**（check.sh 不调它）：它依赖平台条件（Docker / aarch64 交叉目标），
# 塞进门禁等于让无容器环境的机器永远红。登记与步序见 docs/process/conventions.md §2。
#
# 做四件事：
#   1. 宿主跑 `ipct003_dump`，header 行对 `contracts/golden/event-header-128.hex`；
#   2. 已装 `aarch64-unknown-linux-gnu` 目标则交叉 `cargo check`——header.rs 的编译期
#      const 断言随之在 aarch64 目标上求值（不需要链接器）；
#   3. 有 Docker 则 `--platform linux/arm64` 容器真跑 dump，与宿主输出逐行 diff
#      （三方对拍的第三方就是本 golden，第 1 步已对）。没有则 SKIP 并明说——
#      "脚本就绪、双架构真跑待容器环境"，绝不静默当作已验证；
#   4. 已链 esp 工具链（0.0.7 批A'）则 `cargo +esp check --target xtensa-esp32-none-elf
#      -Z build-std=alloc,core`——golden 头 const 对拍（rr-ipc 协议层编译期求值）在该
#      目标成立，即 IPC-T-003 第④路；xtensa 真机运行期 dump 归 0.0.7 批C UART 夹具。
#      未装则 SKIP 并明说。默认在线直连 crates.io（CI 网络正常，build-std 构建依赖
#      现拉即得）；本网络 crates.io CDN 会僵死（0.0.7 方案 §4 批A' 记录），本机跑
#      第④路 export IPCT003_CARGO_OFFLINE=1（缓存已齐后离线求值，缺包先按批A'
#      记录用镜像刷一次缓存）与 IPCT003_CARGO_MIRROR=1（构建依赖走 rsproxy）恢复旧行为。
#
# 用法：bash tools/ci/ipct003.sh [--strict]
#   --strict：缺 Docker/跨架构环境时不放行（退 2）。供有容器环境的一方使用；
#   默认宽容（退 0）但每个 SKIP 都打到 stdout，谁看了都知道哪步没跑。
set -uo pipefail
strict=0
[ "${1:-}" = "--strict" ] && strict=1

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——请在本仓内执行。" >&2
  exit 1
}
cd "$root" || exit 1

manifest=base/runtime/Cargo.toml
golden=contracts/golden/event-header-128.hex
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail=0
skip=0

# 宿主（当前架构）dump
echo "=== [1/4] 宿主 dump ==="
if ! cargo run -q --manifest-path "$manifest" -p rr-ipc --bin ipct003_dump > "$tmp/host.txt"; then
  echo "FAIL  宿主 ipct003_dump 运行失败"
  exit 1
fi
# header 行对 golden（golden 文件单行无换行，printf 补齐后再 diff）
sed -n 's/^header //p' "$tmp/host.txt" > "$tmp/host.header"
printf '%s\n' "$(cat "$golden")" > "$tmp/golden.header"
if diff -q "$tmp/golden.header" "$tmp/host.header" > /dev/null; then
  echo "PASS  宿主 header 行 == contracts/golden/event-header-128.hex"
else
  echo "FAIL  宿主 header 行与 golden 向量不一致"
  diff "$tmp/golden.header" "$tmp/host.header"
  fail=1
fi

# aarch64 交叉编译（const 断言求值；无需链接器）
echo "=== [2/4] aarch64 交叉编译（编译期 const 断言） ==="
if rustup target list --installed 2>/dev/null | grep -q '^aarch64-unknown-linux-gnu$'; then
  if cargo check -q --manifest-path "$manifest" -p rr-ipc \
      --target aarch64-unknown-linux-gnu; then
    echo "PASS  rr-ipc 在 aarch64-unknown-linux-gnu 目标编译通过（const 布局断言已在该目标求值）"
  else
    echo "FAIL  rr-ipc 在 aarch64 目标编译失败（可能缺交叉链接器，见 target 维护者说明）"
    fail=1
  fi
else
  echo "SKIP  未安装 aarch64-unknown-linux-gnu 目标（rustup target add aarch64-unknown-linux-gnu）"
  skip=1
fi

# linux/arm64 容器真跑 + 逐行 diff
echo "=== [3/4] linux/arm64 容器真跑 ==="
if command -v docker > /dev/null 2>&1; then
  # CARGO_TARGET_DIR 指到容器内临时目录：aarch64 产物不落回宿主 target，权限也不串
  if docker run --rm --platform linux/arm64 \
      -v "$root":/w -w /w -e CARGO_TARGET_DIR=/tmp/ipct003-target \
      rust:1-slim \
      cargo run -q --manifest-path base/runtime/Cargo.toml -p rr-ipc --bin ipct003_dump \
      > "$tmp/arm64.txt"; then
    if diff -u "$tmp/host.txt" "$tmp/arm64.txt"; then
      echo "PASS  宿主与 linux/arm64 容器输出逐字节一致（IPC-T-003 成立）"
    else
      echo "FAIL  宿主与 linux/arm64 容器输出不一致——字节布局跨架构漂移"
      fail=1
    fi
  else
    echo "FAIL  linux/arm64 容器内构建/运行失败（网络拉镜像或 rust:1-slim 环境问题）"
    fail=1
  fi
else
  echo "SKIP  本机未装 Docker——双架构真跑待容器环境（docker run --platform linux/arm64 …）"
  skip=1
fi

# xtensa 编译期 golden 对拍（0.0.7 层① 第④路；esp 工具链安装见 0.0.7 方案 §4 批A' 记录）
# 默认在线直连 crates.io；本网络 CDN 僵死时本机跑第④路 export
# IPCT003_CARGO_OFFLINE=1 / IPCT003_CARGO_MIRROR=1 恢复离线+镜像旧行为。
echo "=== [4/4] xtensa 编译期 golden 对拍（esp 工具链） ==="
if rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  extra_cfg=()
  [ -n "${IPCT003_CARGO_OFFLINE:-}" ] && extra_cfg+=(--offline)
  if [ -n "${IPCT003_CARGO_MIRROR:-}" ]; then
    extra_cfg+=(
      --config 'source.crates-io.replace-with="rsproxy-sparse"'
      --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'
    )
  fi
  if cargo +esp check -q --manifest-path "$manifest" -p rr-ipc --lib \
      --target xtensa-esp32-none-elf -Z build-std=alloc,core \
      ${extra_cfg[@]+"${extra_cfg[@]}"}; then
    echo "PASS  rr-ipc 在 xtensa-esp32-none-elf 编译通过——golden 头 const 对拍已在该目标编译期求值（IPC-T-003 第④路）"
  else
    echo "FAIL  rr-ipc 在 xtensa 目标编译失败——层① 契约 parity 破裂"
    fail=1
  fi
else
  echo "SKIP  未安装 esp 工具链（rustup toolchain link esp <dir>，见 0.0.7 方案 §4 批A' 记录）——xtensa 编译期对拍未跑"
  skip=1
fi

echo "=== 汇总 ==="
if [ "$fail" -ne 0 ]; then
  echo "IPC-T-003 对拍存在失败项"
  exit 1
fi
if [ "$skip" -ne 0 ] && [ "$strict" -ne 0 ]; then
  echo "--strict：存在 SKIP，跨架构验证不完整，不放行"
  exit 2
fi
if [ "$skip" -ne 0 ]; then
  echo "IPC-T-003：脚本就绪，跨架构部分有 SKIP（见上），真跑待容器环境"
else
  echo "IPC-T-003：全部通过"
fi
