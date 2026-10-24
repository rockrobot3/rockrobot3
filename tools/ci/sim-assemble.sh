#!/bin/bash
# sim-assemble.sh —— D9 装配演练的构建 + 运行入口（CI 与本地同一脚本）。
# 构建 rrd / rr3-ctl / rr3-sim / 六个 Rust 模块，然后 `rr3-sim --profile
# profiles/default.toml`：装配单驱动 rrd 装配链正门 boot 全部 face 模块，
# 判据 = 六模块真进程全 running + `denied_publish = 0`（D9 验收）。
set -eu -o pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "== 构建 rrd + rr3-ctl + rr3-sim =="
( cd base/runtime && cargo build -q -p rrd )
cargo build -q --manifest-path tools/rrctl/Cargo.toml
cargo build -q --manifest-path tools/sim/Cargo.toml

echo "== 构建 6 个 Rust 模块 =="
for m in rr3-act rr3-sense rr3-power rr3-connect rr3-interact rr3-brain; do
        cargo build -q --manifest-path "modules/$m/Cargo.toml"
done

echo "== rr3-sim --profile（装配单驱动 boot，六模块真进程）=="
exec ./tools/sim/target/debug/rr3-sim --profile profiles/default.toml
