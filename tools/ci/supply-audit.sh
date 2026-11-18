#!/usr/bin/env bash
# 供应链审计门（2026-11-16 建）：依赖许可证、RustSec 安全公告、来源白名单与
# 通配依赖。判据唯一真源是 `.cargo/deny.toml`；本脚本只做两件事——枚举工程根、
# 逐项调用 cargo-deny 并汇总，不发散任何策略。
#
# 工程根 = `base/runtime`（工作区）+ `modules/`、`tools/`、`base/ota/` 下各自
# 带 Cargo.lock 的独立工程。工作区成员不重复执法（同一棵锁树只审一次）。
#
# 工具链：cargo-deny 由 CI 的 install-action 预编译安装（不占 runner 编译时间）；
# 本机未装时默认 SKIP 并明说——本机只写代码，不承担验证；CI 用 `--strict`
# 把"工具缺位"判红，禁止用 SKIP 冒充通过。
#
# 用法：bash tools/ci/supply-audit.sh [--strict]
set -uo pipefail

strict=0
[ "${1:-}" = "--strict" ] && strict=1

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——请在本仓内执行。" >&2
  exit 1
}
cd "$root" || exit 1

# 判据配置必须在场：缺 .cargo/deny.toml 时 cargo-deny 退回默认策略（宽松），等于静默放行
if [ ! -f .cargo/deny.toml ]; then
  echo "ERROR: 缺 .cargo/deny.toml——供应链判据不在场，拒绝放行。" >&2
  exit 1
fi

if ! command -v cargo-deny > /dev/null 2>&1; then
  if [ "$strict" -ne 0 ]; then
    echo "ERROR: 未安装 cargo-deny——CI 供应链门不放行（cargo install cargo-deny --locked）" >&2
    exit 1
  fi
  echo "SKIP  未安装 cargo-deny——本机跳过供应链审计（CI 以 --strict 执法）"
  exit 0
fi

fail=0
count=0
while IFS= read -r m; do
  d="${m%/Cargo.toml}"
  count=$((count + 1))
  log="$(mktemp)"
  # --locked：审计的就是锁文件解析出的那棵树；锁与 manifest 漂移即红（先同步锁）。
  # 公共选项（--manifest-path / --locked / --config）必须放在子命令之前——放到
  # check 之后会被 clap 当未知参数拒掉，表现为"全部工程根瞬时失败"（首跑实证）。
  if cargo deny --manifest-path "$m" --locked --config "$root/.cargo/deny.toml" \
      check > "$log" 2>&1; then
    echo "PASS  ${d}"
  else
    echo "FAIL  ${d}"
    # 优先回显诊断行；命令级错误（参数/配置）不带 error[...] 形态，退回日志首行，
    # 否则失败只剩 FAIL 一行，根因不可见（首跑踩过）。
    if grep -qE '^error(\[|:)' "$log"; then
      grep -E '^error(\[|:)' "$log" | head -5
    else
      head -3 "$log"
    fi
    fail=1
  fi
  rm -f "$log"
done < <(find base modules tools -name Cargo.toml \
           -not -path "*/target/*" -not -path "base/runtime/*/Cargo.toml" | sort)

if [ "$fail" -ne 0 ]; then
  echo "供应链审计存在失败项（策略见 .cargo/deny.toml：许可证入册改 allow，公告无例外）"
  exit 1
fi
echo "供应链审计通过：${count} 个工程根（策略 .cargo/deny.toml）"
