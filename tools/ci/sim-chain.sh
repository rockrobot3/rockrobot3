#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
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
( cd base/runtime && cargo build --locked -q -p rrd )
cargo build --locked -q --manifest-path tools/rrctl/Cargo.toml
cargo build --locked -q --manifest-path tools/sim/Cargo.toml

echo "== 构建全链四模块 =="
for m in rr3-act rr3-sense rr3-interact rr3-brain; do
        cargo build --locked -q --manifest-path "modules/$m/Cargo.toml"
done

echo "== rr3-sim --chain（采集→意图→判向→转向→表情，六真进程 mock 闭环）=="
# 取证后稳态段：缺省 hold 60s，逐拍采 health 判"四模块末拍 running 且心跳峰值
# 不触看门狗"（实时性基线；RR3_SIM_HOLD_MS=0 关闭，其它值按毫秒覆写）。
hold_ms="${RR3_SIM_HOLD_MS:-60000}"
args=(--chain)
if [ "$hold_ms" != "0" ]; then
        args+=(--hold-ms "$hold_ms")
fi
# 产物落点随仓根 target-dir（源码区外）：cargo run 复用同一 target-dir，
# 不写死 `<crate>/target`（写死会因产物不在源码区而误报缺件）。
#
# 稳态数值回传（续轮二十八）：job 日志匿名 403，check-run annotation 是唯一
# 无需凭据的取数通道——把 `稳态摘要：` 那行打成 notice/error 注解，CI 的
# since/峰值/写超时/慢写/落盘峰值从此可匿名读取；本地跑同样输出该行，无副作用。
log="${TMPDIR:-/tmp}/rr3-sim-chain.log"
# 落盘慢写抖动重跑（2026-11-21 实测：共享 runner 单次 ≥50ms 写盘偶发一次，
# 与产品链路无关）。只对这一类失败重跑一拍；其它任何失败立即红，不放宽判据。
rc=0
for attempt in 1 2; do
        rc=0
        cargo run --locked -q --manifest-path tools/sim/Cargo.toml -- "${args[@]}" 2>&1 | tee "$log" || rc=$?
        [ "$rc" -eq 0 ] && break
        if [ "$attempt" -eq 1 ] && grep -Eq 'log_sink_slow=[1-9]' "$log"; then
                echo "::warning title=sim-chain::首次稳态失败为落盘慢写抖动（log_sink_slow>0），重跑一拍"
                sleep 2
                continue
        fi
        break
done
summary=$(grep '^稳态摘要：' "$log" | tail -1 || true)
if [ "$rc" -ne 0 ]; then
        reason=$(grep -E '^(rr3-sim|rr3-sim --chain)' "$log" | tail -1 || true)
        echo "::error title=sim-chain::${reason:-退出码 ${rc}（详见日志）}"
        [ -n "$summary" ] && echo "::notice title=sim-chain 稳态（失败拍）::${summary}"
        exit "$rc"
fi
[ -n "$summary" ] && echo "::notice title=sim-chain 稳态::${summary}"
exit 0
