#!/usr/bin/env bash
# 自治健康面集成腿（W3-3/W4-2，2026-11-19）：真跑监督核状态迁移，不依赖 ESP 工具链。
#
# 判据载体：`rr-autonomy` 单测 + `tests/health_face.rs`（30 s 模拟：稳态零超时 →
# 单任务停摆判死 → 复位闸门去抖 → 心跳恢复）。双芯片构建面归 platforms 的
# esp32-gates；本腿只跑可执行的机制面，失败即红（无 SKIP 分支）。
set -euo pipefail

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/rr3-autonomy-face}"
cd "$root"

cargo test -q --locked --manifest-path base/runtime/Cargo.toml -p rr-autonomy --all-targets

echo "PASS  自治健康面集成腿：rr-autonomy 单测 + health_face（30 s 模拟）"
