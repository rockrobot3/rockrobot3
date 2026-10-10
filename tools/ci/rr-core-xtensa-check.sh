#!/usr/bin/env bash
# rr-core 机制核 xtensa 对拍门（0.0.5 第 II 轮 批D⑤，ADR-0018 裁定 1/3/5）。
# rr-core 是 no_std 机制核切片（总线语义 / 模块运行时 / 控制原语 / 安全核）：宿主 std
# 测试面由 check.sh ①/①b 覆盖；本门补**无 OS 裸机面**——在 xtensa-esp32-none-elf（无
# 操作系统目标）上，fork/exec/socket/线程/墙上时钟任何 OS 假设都编译不过，编译成立即
# "机制核可进裸机固件"（无 OS 启动形态）的机械点验。esp fork 工具链（cargo +esp +
# build-std）是平台条件依赖，不进门禁步序，单独覆盖（同 espidf-std-check.sh 先例）。
# 对拍语义：同一份源两个面同判——
#   裸机面：xtensa 编译过，且 rr-ipc golden 头 `const _` 断言随依赖链在该目标编译期
#           重求值（0.0.7 层① 免费资产，批D① 起 rr-core 依赖 rr-ipc 同链生效）；
#   宿主面：cargo test -p rr-core 真跑（golden 形状 / 控制图逐拍一致用例，本门内断言）。
# 纪律：工具链缺位 = SKIP（退出 0 并明说，同 espidf-std-check.sh 口径）；
#       工具链在位但编译失败 = FAIL；tripwire（全局分配器 / std 逃逸）命中 = FAIL；
#       宿主测试 0 通过 = FAIL（fail-open 封死，同 check.sh run_tests 口径）。
# rr-ipc 零外部依赖、rr-core 仅依赖 rr-ipc，本门全程无网络。
# 0.0.5 批I C8（门查门）：新机制核面 rr-hal-api 同批进本门三处点位（tripwire 扫面 +
# xtensa 裸机面 + 宿主面对拍）——门清单不与新面同步即门禁失效。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
cd "$root/base/runtime" || { echo "ERROR: 无法进入 base/runtime" >&2; exit 1; }

export PATH="$HOME/.cargo/bin:$PATH"

# ① SKIP 判据：esp fork 工具链（rustup alias esp → esp-1.90.0.0，0.0.7 批A'）
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  echo "SKIP  esp 工具链未安装（rustup toolchain list 无 'esp'）——机制核 xtensa 对拍门不执行，安装方式见 0.0.7 方案批A'"
  exit 0
fi
# build-std 依赖 rust-src 源码树（esp fork 的 rust-src 不走 rustup component 注册，
# 判据用 sysroot 内源码目录，同 espidf-std-check.sh）
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
  echo "SKIP  esp 工具链 rustc 不可用（rustc +esp 失败）——机制核 xtensa 对拍门不执行"
  exit 0
}
if [ ! -d "$esp_sysroot/lib/rustlib/src/rust" ]; then
  echo "SKIP  esp 工具链缺 rust-src 源码树（$esp_sysroot/lib/rustlib/src/rust 不在，build-std 依赖）——机制核 xtensa 对拍门不执行"
  exit 0
fi

fail=0

# ② 机制核 tripwire：不得假设全局分配器（裁定 3——内存策略是平台四类供给之一，机制核
#    不得自设 #[global_allocator]）；不得 extern crate std（no_std 面失真、对拍不成立）。
tripwire=0
while IFS= read -r hit; do
  printf 'FAIL  机制核 tripwire 命中: %s\n' "$hit"
  tripwire=1
done < <(grep -RnE 'global_allocator|extern crate std' rr-core/src rr-hal-api/src --include='*.rs')
if [ "$tripwire" -eq 0 ]; then
  echo "PASS  机制核 tripwire：无全局分配器假设、无 std 逃逸（裁定 3/5 静态面）"
else
  fail=1
fi

# ③ 裸机面对拍：xtensa-esp32-none-elf 编译 = 无 OS 启动形态点验；rr-ipc golden 头
#    const 断言随依赖链在该目标编译期重求值 = xtensa 编译期 golden 对拍（0.0.7 第④路机制）。
#    0.0.5 批J G1/C6：transport 拆门后 rr-ipc 的总线窄缝 Rr3ByteStream + 帧缓冲
#    Rr3FrameBuf + conn blanket impl 随依赖链同面编译——"窄缝在 none 目标可编译"在此出数。
if cargo +esp check -q -Zbuild-std=core,alloc -p rr-core -p rr-hal-api --target xtensa-esp32-none-elf; then
  echo "PASS  机制核切片（总线语义/运行时/控制/安全核 + 能力面契约 rr-hal-api）在 xtensa-esp32-none-elf 编译成立；rr-ipc 总线窄缝 Rr3ByteStream + 帧缓冲 + conn blanket 同面编译，golden 头 const 断言已在该目标重求值"
else
  echo "FAIL  机制核面（rr-core/rr-hal-api）xtensa-esp32-none-elf 编译失败（no_std 面被破坏）" >&2
  fail=1
fi

# ④ 宿主面对拍：同一份源的 std 测试面真跑（golden 形状 / 逐拍一致），≥1 通过才算数
hostout="$(mktemp)" || { echo "ERROR: mktemp 失败，无法捕获宿主面输出。" >&2; exit 1; }
trap 'rm -f "$hostout"' EXIT
if cargo test -p rr-core -p rr-hal-api 2>&1 | tee "$hostout"; then
  if grep -qE '^running [1-9][0-9]* tests?' "$hostout" \
     && grep -qE '^test result: ok\. [1-9][0-9]* passed' "$hostout"; then
    echo "PASS  机制核宿主面对拍（rr-core + rr-hal-api）：golden 形状 / 控制图逐拍一致用例全绿"
  else
    echo "FAIL  机制核宿主测试 0 通过（过滤器打空或测试集丢失），视为门禁失效" >&2
    fail=1
  fi
else
  echo "FAIL  机制核宿主面对拍失败" >&2
  fail=1
fi

printf '\n=== 汇总 ===\n'
if [ "$fail" -eq 0 ]; then
  echo "机制核 xtensa 对拍门全绿（tripwire + 裸机面 + 宿主面）"
else
  echo "存在失败项（见上方 FAIL）"
fi
exit "$fail"
