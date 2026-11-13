#!/usr/bin/env bash
# 目标三元组编译面门（cargo check，不链接）。
#
# 用法：host-check-target.sh <target-triple>
#
# 用途：目标平台在 runner 上缺可用的 C 运行时/链接器时（典型：riscv64-musl 无
# musl 交叉工具链，rust-lld 找不到 -lc / -lgcc_s），"能链接出 ELF"这一条不可得；
# 但"代码面能否为该平台编译"仍可判——cargo check 做类型/借用检查与代码生成到
# metadata，不产 ELF、不调用链接器。本门如实只声称编译面，不冒充"可执行"。
#
# 与 cross-artifacts.sh 的分工：能链接的目标走那边（构建+ELF+执行冒烟）；
# 不能链接的目标走本门，并在 CI 矩阵里显式标 mode=check。
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

target="${1:-}"
if [ -z "$target" ]; then
  echo "用法: $0 <target-triple>" >&2
  exit 1
fi

cargo check -q --locked --target "$target" \
  --manifest-path base/runtime/Cargo.toml --workspace
for m in act connect interact sense power; do
  cargo check -q --locked --target "$target" --manifest-path "modules/rr3-$m/Cargo.toml"
done
cargo check -q --locked --target "$target" --manifest-path tools/rrctl/Cargo.toml
echo "PASS  $target 编译面（cargo check，无链接；brain 因 ring 需目标 C 工具链不在列）"
