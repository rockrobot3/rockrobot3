#!/usr/bin/env bash
# SKIP 纪律单点（机制）：本地缺工具链 = SKIP（exit 0），CI 内缺工具链 = FAIL（exit 1）。
#
# 为什么 CI 内不能 SKIP：esp 专用门是 check.sh ④/⑥ 显式排除面的唯一编译执法位；
# 门一 SKIP，这些工程就变成"零覆盖但流水线绿"。SKIP 只在本地开发机有意义。
#
# 用法：
#   . "$root/tools/ci/skip.sh"
#   rr3_skip "esp 工具链未安装（rustup toolchain list 无 'esp'）——xxx 门不执行"
#
# CI 判据：GitHub Actions 注入 CI=true；本地可用 RR3_CI_STRICT=1 模拟 CI 严格口径。
rr3_skip() {
  local msg="$1"
  if [ "${CI:-}" = "true" ] || [ "${RR3_CI_STRICT:-}" = "1" ]; then
    printf '::error title=SKIP::CI 内 SKIP 即红：%s\n' "$msg"
    printf 'FAIL  %s（CI 内 SKIP 即红：缺的是门禁覆盖，不是环境噪音）\n' "$msg" >&2
    exit 1
  fi
  printf 'SKIP  %s\n' "$msg"
  exit 0
}

# 自检：本地口径退 0，CI 口径退 1；直接执行本文件时生效（被 source 时不跑）。
if [ "${BASH_SOURCE[0]}" = "$0" ] && [ "${1:-}" = "--self-test" ]; then
  ok=1
  if ( unset CI RR3_CI_STRICT; rr3_skip "self-test-local" >/dev/null 2>&1 ); then
    echo "PASS  skip 自检：本地缺件 = SKIP（exit 0）"
  else
    echo "FAIL  skip 自检：本地缺件未按 SKIP 退出"
    ok=0
  fi
  if ( CI=true rr3_skip "self-test-ci" >/dev/null 2>&1 ); then
    echo "FAIL  skip 自检：CI 内缺件未按 FAIL 退出"
    ok=0
  else
    echo "PASS  skip 自检：CI 内缺件 = FAIL（exit 1）"
  fi
  exit $(( ok ? 0 : 1 ))
fi
