#!/usr/bin/env bash
# 规约三检查（方案 D7）。本地与 CI 同一入口，避免"CI 绿本地红"的两套逻辑。
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"
fail=0

step() { printf '\n=== %s ===\n' "$1"; }
run() {
  if "$@"; then
    printf 'PASS  %s\n' "$*"
  else
    printf 'FAIL  %s\n' "$*"
    fail=1
  fi
}

step "① workspace 编译"
run sh -c 'cd base/runtime && cargo check --workspace'

step "② manifest schema 校验"
run sh -c 'cd base/runtime && cargo test -p rr-registry --test contract'
step "②b mkmod 输出即契约用例（方案 P3）"
run sh -c 'cargo test --manifest-path tools/mkmodule/Cargo.toml'

step "③ 主题前缀归属"
run sh -c 'cd base/runtime && cargo test -p rr-registry topics_prefix'

printf '\n=== 汇总 ===\n'
if [ "$fail" -eq 0 ]; then
  echo "所有规约检查通过"
else
  echo "存在失败项（见上方 FAIL）"
fi
exit "$fail"
