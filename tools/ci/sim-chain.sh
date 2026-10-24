#!/bin/bash
# sim-chain.sh —— 0.0.6 批F4 全链闭环演练的构建 + 运行入口（CI 与本地同一脚本）。
# 构建 rrd / rr3-ctl / rr3-sim + 全链四模块（sense/interact/brain/act），然后
# `rr3-sim --chain`：采集→意图→判向→转向→表情六真进程 mock 闭环，判据 = 总线
# 观察者取证（vad_start 携 doa → servo/state 落角）+ 三段日志留痕（对话轮完成
# actions=1 / tts talk 挂起 / intent servo 落角）+ denied_publish = 0（§5.2
# 判据 10 的 sim 档，R9 采购前档，不充当真件收口证据）。
set -eu -o pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "== 构建 rrd + rr3-ctl + rr3-sim =="
( cd base/runtime && cargo build -q -p rrd )
cargo build -q --manifest-path tools/rrctl/Cargo.toml
cargo build -q --manifest-path tools/sim/Cargo.toml

echo "== 构建全链四模块 =="
for m in rr3-act rr3-sense rr3-interact rr3-brain; do
        cargo build -q --manifest-path "modules/$m/Cargo.toml"
done

echo "== rr3-sim --chain（采集→意图→判向→转向→表情，六真进程 mock 闭环）=="
exec ./tools/sim/target/debug/rr3-sim --chain
