#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 目标 ISA 单测最小面（2026-11-24）：rr-ipc / rr-registry 的测试二进制在交叉
# 目标上构建并按平台方式真跑——"能构建 + 能打印用法 + 128B 头对拍"不能证明
# 语义断言在目标 ABI 上成立。
#
# 用法：cross-tests.sh <target-triple> <native|musl|gnu> [容器平台串]
#   cross-tests.sh aarch64-unknown-linux-musl musl linux/arm64
#   cross-tests.sh riscv64gc-unknown-linux-gnu gnu
#
# 机制与平台分离：构建集合（两 crate 的测试目标）与三种执行方式只有一份；
# 平台差异只是数据（三元组、执行方式、容器平台）。链接器环境由调用方注入
# （musl 走 rust-lld，gnu 走 apt 交叉，与 cross-artifacts.sh 同口径）。
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
target="${1:-}"
mode="${2:-}"
platform="${3:-}"
if [ -z "$target" ] || [ -z "$mode" ]; then
  echo "用法: $0 <target-triple> <native|musl|gnu> [容器平台串]" >&2
  exit 1
fi
case "$mode" in
native | musl | gnu) ;;
*)
  echo "ERROR: 第二参只认 native / musl / gnu（实得 ${mode}）" >&2
  exit 1
  ;;
esac
case "$target" in
*musl*) export RUSTFLAGS="-C linker=rust-lld" ;;
esac
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rr3-build}"
cd "$root"

json="/tmp/rr3-cross-tests-${target}.jsonl"
build_log="/tmp/rr3-cross-tests-${target}.build.log"
if ! cargo test -q --locked --no-run --target "$target" \
  --manifest-path base/runtime/Cargo.toml -p rr-ipc -p rr-registry \
  --message-format=json >"$json" 2>"$build_log"; then
  errs="$(grep -E '^error' "$build_log" | head -3 | tr '\n' ' ')"
  echo "::error title=isa-tests::${target} 测试二进制构建失败: ${errs:0:800}"
  exit 1
fi

# 从 cargo 的 JSON 事件里收集可执行测试二进制（包归属按 package_id 过滤）。
bins="$(python3 - "$json" <<'PY'
import json
import sys

out = set()
with open(sys.argv[1], encoding="utf-8") as fh:
    for line in fh:
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("reason") != "compiler-artifact" or not ev.get("executable"):
            continue
        pkg = ev.get("package_id", "")
        if "rr-ipc" in pkg or "rr-registry" in pkg:
            out.add(ev["executable"])
for path in sorted(out):
    print(path)
PY
)"
if [ -z "$bins" ]; then
  echo "FAIL  未收集到测试二进制（构建面异常）" >&2
  exit 1
fi

# 三种执行方式：原生直跑 / musl 进目标架构容器 / gnu 走 qemu 用户态。
# gnu 腿不注册 binfmt，`uart_excl_cross_rej` 的 self-exec（current_exe 再执行）
# 在跨架构下无法成立，显式跳过并点名；该用例的 flock 排他语义由宿主 gate 与
# musl 腿（binfmt 在）覆盖，不是静默丢测。
run_one() {
  case "$mode" in
  native) "$1" ;;
  musl)
    docker run --rm --platform "$platform" \
      -v "$root:$root:ro" \
      -v "$CARGO_TARGET_DIR:$CARGO_TARGET_DIR:ro" \
      -w "$root" alpine:3.20 "$1"
    ;;
  gnu) qemu-riscv64 -L /usr/riscv64-linux-gnu "$1" --skip uart_excl_cross_rej ;;
  esac
}
if [ "$mode" = gnu ]; then
  echo "NOTE  gnu 腿跳过 uart_excl_cross_rej（跨架构 self-exec 需 binfmt；musl 腿与宿主 gate 已覆盖）"
fi

fail=0
n=0
for b in $bins; do
  n=$((n + 1))
  if run_one "$b" >/tmp/rr3-cross-test-one.log 2>&1; then
    echo "PASS  $(basename "$b") 在 $target 执行通过"
  else
    errs="$(tail -8 /tmp/rr3-cross-test-one.log | tr '\n' ' ' | tr -s ' ')"
    echo "FAIL  $(basename "$b") 在 $target 执行失败: ${errs:0:600}" >&2
    echo "::error title=isa-tests::$(basename "$b") 在 ${target} 失败: ${errs:0:800}"
    fail=1
  fi
done
if [ "$fail" -eq 0 ]; then
  echo "PASS  目标 ISA 单测 $n 件全过（$target, $mode）"
fi
exit "$fail"
