#!/usr/bin/env bash
# 规约三检查（方案 D7）。本地与 CI 同一入口，避免"CI 绿本地红"的两套逻辑。
# ① 用 --all-targets：普通 cargo check 根本不编译 #[cfg(test)] 目标，
#    测试代码里的类型错误会一路绿灯（实测：注入 E0308 后 check 退 0，--all-targets 退 1）。
# ①b 跑 workspace 全量测试：②/③ 只点名 rr-registry 的 contract 目标（③ 是它的子集），
#    其余用例——含 rr-ipc 的 envelope 契约守护与 rr-registry 的注册表形状 pin——若不在此步
#    覆盖，门里就没有任何力量行 contracts/README 所称"契约的执行入口"。
#    逐步实测条数只登记在 docs/process/conventions.md §2，本文件不抄数字：
#    测试集每加一条就要改一处注释，抄第二次必然失真。
# 测试步（①b/②/②b/③）除退出码外还断言"至少一个测试真实通过"：cargo 对打空的过滤器
# 仍以 0 退出（fail-open），只看退出码会在过滤器笔误/测试集改名时静默放行；
# 退出码仍是首要信号（编译错误、错误 --test 目标名照样红）。① 不跑测试，只看得退出码。
# ④ 是独立工程编译步（不是测试步）：modules/、tools/、base/ota 各自成 workspace，①/①b 覆盖不到。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内，规约门拒绝执行。" >&2
  exit 1
}
if [ -z "$root" ] || ! cd "$root"; then
  echo "ERROR: 无法进入仓库根：'$root'，规约门拒绝执行。" >&2
  exit 1
fi

fail=0
tmpout="$(mktemp)" || { echo "ERROR: mktemp 失败，无法捕获步骤输出。" >&2; exit 1; }
trap 'rm -f "$tmpout"' EXIT
trap 'rm -f "$tmpout"; exit 130' INT TERM

step() { printf '\n=== %s ===\n' "$1"; }

# 只看退出码的步骤（供 ① 使用）。
run() {
  if "$@"; then
    printf 'PASS  %s\n' "$*"
  else
    printf 'FAIL  %s\n' "$*"
    fail=1
  fi
}

# 跑测试的步骤：cargo 退出码为首要信号；退出码 0 时还须同时出现行首的
# "running N tests"（N≥1）与 "test result: ok. N passed"（N≥1）才判 PASS，
# 封死空过滤器/测试集丢失、以及仅凭一行仿造文本蒙混过关的 fail-open。
# 全量输出经 tee 实时流给操作者，同时落盘临时文件供 grep，不吞日志。
run_tests() {
  if ! "$@" 2>&1 | tee "$tmpout"; then
    printf 'FAIL  %s\n' "$*"
    fail=1
    return
  fi
  if ! grep -qE '^running [1-9][0-9]* tests?' "$tmpout" \
     || ! grep -qE '^test result: ok\. [1-9][0-9]* passed' "$tmpout"; then
    printf 'FAIL  %s ——命令成功但 0 个测试通过（过滤器打空或测试集丢失），视为门禁失效\n' "$*"
    fail=1
    return
  fi
  printf 'PASS  %s\n' "$*"
}

step "① workspace 编译"
run sh -c 'cd base/runtime && cargo check --all-targets --workspace'

step "①b workspace 全量测试（含未被点名的守护用例）"
run_tests sh -c 'cd base/runtime && cargo test --workspace'

step "② manifest schema 校验 + 注册表形状 pin"
run_tests sh -c 'cd base/runtime && cargo test -p rr-registry --test contract'
step "②b mkmod 输出即契约用例（方案 P3）"
run_tests sh -c 'cargo test --manifest-path tools/mkmodule/Cargo.toml'

step "③ 主题前缀归属"
run_tests sh -c 'cd base/runtime && cargo test -p rr-registry topics_prefix'

# ④ 仓内所有**独立** Rust 工程编译。①/①b 的作用域是 base/runtime 那一个 workspace，
# 模块 crate 与 tools/、base/ota 各自独立成 workspace，不在其覆盖内；没有 ④ 的话
# 这些目录烂掉无人知晓。没有 Cargo.toml 的模块目录（rr3-brain 是 Python）自然跳过，
# 其语法检查属人工检查点。
step "④ 独立 Rust 工程编译（modules/ + tools/ + base/ota）"
found=0
# 用 NUL 分隔 + 进程替换：for 循环的单词分割会在路径带空格时把一份工程拆成两个，
# 而管道里的 while 在子 shell 中执行，fail 计数带不回来。
while IFS= read -r -d '' manifest; do
  found=1
  if sh -c "cargo check -q --all-targets --manifest-path '$manifest'"; then
    printf 'PASS  cargo check %s\n' "$manifest"
  else
    printf 'FAIL  cargo check %s\n' "$manifest"
    fail=1
  fi
done < <(find modules tools base -name Cargo.toml -not -path "*/target/*" \
                    -not -path "base/runtime/*" -print0 | sort -z)
if [ "$found" -eq 0 ]; then
  printf 'FAIL  ④ 未发现任何独立 Rust 工程（find 打空属门禁失效，不放行）\n'
  fail=1
fi

printf '\n=== 汇总 ===\n'
if [ "$fail" -eq 0 ]; then
  echo "所有规约检查通过"
else
  echo "存在失败项（见上方 FAIL）"
fi
exit "$fail"
