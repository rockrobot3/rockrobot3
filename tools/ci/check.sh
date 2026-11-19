#!/usr/bin/env bash
# 规约三检查（方案 D7）。本地与 CI 同一入口，避免"CI 绿本地红"的两套逻辑。
# ① 用 --all-targets：普通 cargo check 根本不编译 #[cfg(test)] 目标，
#    测试代码里的类型错误会一路绿灯（实测：注入 E0308 后 check 退 0，--all-targets 退 1）。
# ①b 跑 workspace 全量测试：②/③ 只点名 rr-registry 的 contract 目标（③ 是它的子集），
#    其余用例——含 rr-ipc 的 envelope 契约守护与 rr-registry 的注册表形状 pin——若不在此步
#    覆盖，门里就没有任何力量行 contracts/README 所称"契约的执行入口"。
#    逐步实测条数只登记在 docs/process/conventions.md §2，本文件不抄数字：
#    测试集每加一条就要改一处注释，抄第二次必然失真。
# 测试步（①b/②/②b/③/⑥）除退出码外还断言"至少一个测试真实通过"：cargo 对打空的过滤器
# 仍以 0 退出（fail-open），只看退出码会在过滤器笔误/测试集改名时静默放行；
# 退出码仍是首要信号（编译错误、错误 --test 目标名照样红）。① 不跑测试，只看得退出码。
# ④ 是独立工程编译步（不是测试步）：modules/、tools/、base/ota 各自成 workspace，①/①b 覆盖不到。
#    例外：tools/esp32-uart-echo 是 ESP32 固件工程（xtensa-esp32-none-elf 目标），编译需 esp fork
#    工具链（cargo +esp）+ Xtensa esp-gcc，宿主工具链 check 必红——它由专用门
#    tools/ci/esp-uart-echo-check.sh 覆盖（工具链缺位时本地 SKIP / CI 即红，编译成功还断言 ELF 出树）。
#    同例：tools/espidf-std-hello（0.0.5 增补轮四 批W1，xtensa-esp32-espidf 目标 + build-std +
#    esp-idf-sys，还须 ESP-IDF 克隆与乐鑫 esp-clang）——专用门 tools/ci/espidf-std-check.sh，
#    SKIP 纪律同上（本地 SKIP / CI 即红）。同例：tools/espidf-sentinel（0.0.7 批D1 层③ 哨兵，同款 espidf 工具链
#    + 主仓 rr-core/rr-ipc path 依赖）——专用门 tools/ci/espidf-sentinel-check.sh（甲面
#    xtensa 构建 = 双证之①），SKIP 纪律同上（本地 SKIP / CI 即红）；其宿主对手件是普通独立工程
#    tools/rr3-sentinel-harness（不在哨兵目录内的原因见其 Cargo.toml 注记），照常进 ④/⑥。
#    同例：tools/esp32-periph-base（0.0.5 增补轮五 批B 外设底座档，同款 espidf
#    工具链，OLED/舵机/UART2 指令面）——专用门 tools/ci/esp32-periph-base-check.sh，
#    SKIP 纪律同上（本地 SKIP / CI 即红）。
#    同例（0.0.5 增补轮六 批N8 件三）：modules/rr3-act、modules/rr3-sense 是
#    **双档工程**——espidf 档专属依赖挂 Cargo.toml target 表（宿主构建不解析），
#    宿主 linux 档照常进本门 ④/⑥，**不**整目录排除（与上述 tools/ 专用工程不同）；
#    espidf 档编译面由专用门 tools/ci/rr3-act-sense-espidf-check.sh 执法（显式
#    --target，SKIP 纪律同上（本地 SKIP / CI 即红）。
# ⑧ 件级判据三门（0.0.5 批K H2）：⑧-a 两腿对拍计数门（slots.toml status 三态 join
#    atom.toml provides，normal 格须 fn parity_leg1_+parity_leg2_ 各 ≥1，C10 stub 反例
#    自证内置）；⑧-b 依赖闭包 grep 门（L0=std::（含 use/extern crate/全限定内联）、
#    L1=+serde_json，非 test 面）；⑧-c 注入面校验门（contract injection 用例显式过滤器封 fail-open）。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内，规约门拒绝执行。" >&2
  exit 1
}
if [ -z "$root" ] || ! cd "$root"; then
  echo "ERROR: 无法进入仓库根：'$root'，规约门拒绝执行。" >&2
  exit 1
fi

# 装配单真值面源码集（B9 起 profile.rs 按 模型/解析/装载 三分）：定义点落在
# 主文件或 profile/ 子模块都算单点——门禁按"内容"执法，不按文件名执法，结构
# 重构（搬文件）不再失锚；测试面（profile/tests/）不计入声明扫描。
prof_src="$(printf '%s\n' base/runtime/rrd/src/profile.rs; find base/runtime/rrd/src/profile -name '*.rs' -not -path '*/tests/*' 2>/dev/null)"
if [ -z "$prof_src" ]; then
  echo "ERROR: 装配单真值面源码集为空（base/runtime/rrd/src/profile* 不存在），规约门拒绝执行。" >&2
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

# 模块语言数据源（读 module.toml `manifest.lang`；schema 必填 + 枚举 rust|python，
# contract 钉死 schema.rs）。⑦-c 件形与 ⑧-a 豁免的**唯一判据数据源**——门内零零件名
# 硬编码（0.0.5 批M MC-10 L-2，承 R-8：architecture §13"⑦-c 仍留两处 if rr3-brain"
# 收编；参数 = module.toml 路径，供 ⑦-c/$mod 与 ⑧-a/$mod_dir 两处共用同一真值）。
mod_lang() {
  grep -m1 -E '^lang[[:space:]]*=' "$1" 2>/dev/null | sed -E 's/.*"([^"]*)".*/\1/'
}

# 步骤体（按族拆）：标题与调用序留在本文件，实现体在 tools/ci/steps/。
. "$root/tools/ci/steps/7.sh"
. "$root/tools/ci/steps/8.sh"
. "$root/tools/ci/steps/9a.sh"
. "$root/tools/ci/steps/9c.sh"
. "$root/tools/ci/steps/9n.sh"
. "$root/tools/ci/steps/9r.sh"
. "$root/tools/ci/steps/9w.sh"
. "$root/tools/ci/steps/9y.sh"
. "$root/tools/ci/steps/9z.sh"
. "$root/tools/ci/steps/10.sh"
. "$root/tools/ci/steps/13.sh"

step "① workspace 编译"
run sh -c 'cd base/runtime && cargo check --locked --all-targets --workspace'

step "①b workspace 全量测试（含未被点名的守护用例）"
run_tests sh -c 'cd base/runtime && cargo test --locked --workspace'

# ①c/①d 与 CI lint job 同源（.github/workflows/ci.yml lint）。此前本地门禁缺
# fmt/clippy 两步，手工折行与 clippy 告警一路入库（9390d28 收 ⑱ 文件漂移、
# 9227249 收 too_many_arguments 即实证）——"CI 绿本地红"的两套逻辑由此补齐。
# fmt 不编译，对 esp 条件工程照跑（CI lint 同口径）；clippy 宿主 stable 对
# esp 专用工程必红，独立工程侧归 ④b 排除、由各自专用门执法（口径同 ④）。
step "①c 代码风格门 rustfmt --check（base/runtime + 全部独立工程，CI lint 同源）"
run sh -c 'cargo fmt --manifest-path base/runtime/Cargo.toml --all -- --check'
while IFS= read -r -d '' manifest; do
  run sh -c "cargo fmt --manifest-path '$manifest' -- --check"
done < <(find modules tools base/ota -name Cargo.toml \
            -not -path "*/target/*" -not -path "base/runtime/*" -print0 | sort -z)

step "①d 静态 lint 门 clippy -D warnings（base/runtime workspace）"
run sh -c 'cargo clippy --locked -q --all-targets --manifest-path base/runtime/Cargo.toml --workspace -- -D warnings'

step "② manifest schema 校验 + 注册表形状 pin"
run_tests sh -c 'cd base/runtime && cargo test --locked -p rr-registry --test contract'
step "②b mkmod 输出即契约用例（方案 P3）"
run_tests sh -c 'cargo test --locked --manifest-path tools/mkmodule/Cargo.toml'

step "③ 主题前缀归属"
run_tests sh -c 'cd base/runtime && cargo test --locked -p rr-registry topics_prefix'

# ④ 仓内所有**独立** Rust 工程编译。①/①b 的作用域是 base/runtime 那一个 workspace，
# 模块 crate 与 tools/、base/ota 各自独立成 workspace，不在其覆盖内；没有 ④ 的话
# 这些目录烂掉无人知晓（批N32 件四起六模块全 Rust，含 rr3-brain）。
step "④ 独立 Rust 工程编译（modules/ + tools/ + base/ota）"
found=0
# 用 NUL 分隔 + 进程替换：for 循环的单词分割会在路径带空格时把一份工程拆成两个，
# 而管道里的 while 在子 shell 中执行，fail 计数带不回来。
while IFS= read -r -d '' manifest; do
  found=1
  if sh -c "cargo check --locked -q --all-targets --manifest-path '$manifest'"; then
    printf 'PASS  cargo check %s\n' "$manifest"
  else
    printf 'FAIL  cargo check %s\n' "$manifest"
    fail=1
  fi
done < <(find modules tools base -name Cargo.toml -not -path "*/target/*" \
                    -not -path "base/runtime/*" -not -path "tools/esp32-uart-echo/*" \
                    -not -path "tools/espidf-std-hello/*" -not -path "tools/espidf-sentinel/*" \
                    -not -path "tools/esp32-periph-base/*" \
                    -print0 | sort -z)
if [ "$found" -eq 0 ]; then
  printf 'FAIL  ④ 未发现任何独立 Rust 工程（find 打空属门禁失效，不放行）\n'
  fail=1
fi

# ④b 独立工程静态 lint（CI lint 同源）：fmt 已在 ①c 全量覆盖（esp 工程 fmt 不
# 编译照跑），此处只补独立工程的 clippy。esp 条件工程宿主 stable clippy 必红，
# 排除口径与 ④ 完全一致，编译执法归各自专用门（SKIP 纪律）。
step "④b 独立工程 clippy -D warnings（CI lint 同源；esp 条件工程归专用门）"
while IFS= read -r -d '' manifest; do
  run sh -c "cargo clippy --locked -q --all-targets --manifest-path '$manifest' -- -D warnings"
done < <(find modules tools base/ota -name Cargo.toml -not -path "*/target/*" \
                    -not -path "base/runtime/*" -not -path "tools/esp32-uart-echo/*" \
                    -not -path "tools/espidf-std-hello/*" -not -path "tools/espidf-sentinel/*" \
                    -not -path "tools/esp32-periph-base/*" \
                    -print0 | sort -z)

# ⑤ 文档门：已跟踪的 md 不得为空、相对链接不得断。
#    空文档判据用 `-s`（字节数为 0 即空），断链判据只取 `](...)` 里的相对路径
#    （跳过 http(s)://、mailto:、以 / 开头的绝对路径与纯锚点 #…）。
#    第三件判据是导航覆盖：仓内每份 README 都要在 docs/README.md 的库地图里有一条链接——
#    地图靠人记得添必然漏，而没进导航的 README 事实上等于不存在。判据与上面两条同口径：
#    逐条点名（`导航漏链`）后整步 FAIL。docs/README.md 自身是导航本身，不参与被检。
#    （0.0.4 批D 曾排除 legacy/ 冻结区；批F 整树退役删除后排除条款撤除，全集进门。）
#    `git ls-files` 显式关掉 core.quotepath：默认 true 时非 ASCII 文件名（如
#    docs/process/dev/0.0.1版本…）会被转义成八进制串，`-s`/`-e` 检的是那个不存在的名字，
#    门禁会在别人机器上凭空报一条假红。
check_docs() {
  local list bad nav f d l
  list="$(mktemp)" || return 1; bad="$(mktemp)" || return 1
  git -c core.quotepath=false ls-files '*.md' > "$list" || true
  if [ ! -s "$list" ]; then
    printf 'FAIL  ⑤ md 列表打空（ls-files 无结果属门禁失效，不放行）\n'
    rm -f "$list" "$bad"; return 1
  fi
  while IFS= read -r f; do
    [ -s "$f" ] || printf '空文档  %s\n' "$f" >> "$bad"
    d="$(dirname "$f")"
    while IFS= read -r l; do
      [ -e "$d/$l" ] || printf '断链    %s -> %s\n' "$f" "$l" >> "$bad"
    done < <(grep -ohE '\]\([^)]+\)' "$f" 2>/dev/null \
             | sed 's/^](//; s/)$//; s/#.*//; s/ .*//' \
             | grep -vE '^(https?:|mailto:|/|$)')
  done < "$list"
  # 导航覆盖：把 docs/README.md 里的相对链接按 docs/ 归一成仓根路径后，逐个 README 对号。
  # 本索引住在 docs/ 下，链接里的 `../` 只可能出现在开头，故一次折叠即够。
  nav="$(mktemp)" || return 1
  while IFS= read -r l; do
    printf '%s\n' "docs/$l" | sed 's#^docs/\./##; s#^docs/\.\./##' >> "$nav"
  done < <(grep -ohE '\]\([^)]+\)' docs/README.md 2>/dev/null \
           | sed 's/^](//; s/)$//; s/#.*//; s/ .*//' \
           | grep -vE '^(https?:|mailto:|/|$)')
  if [ ! -s "$nav" ]; then
    printf 'FAIL  ⑤ 导航链接列表打空（docs/README.md 读不到链接属门禁失效，不放行）\n'
    rm -f "$list" "$bad" "$nav"; return 1
  fi
  while IFS= read -r f; do
    case "$f" in
      *README.md) [ "$f" = "docs/README.md" ] || grep -qxF "$f" "$nav" \
                    || printf '导航漏链  %s\n' "$f" >> "$bad" ;;
    esac
  done < "$list"
  if [ -s "$bad" ]; then cat "$bad"; rm -f "$list" "$bad" "$nav"; return 1; fi
  rm -f "$list" "$bad" "$nav"
}
step "⑤ 文档门（已跟踪 md 不得为空、相对链接不得断、README 不得漏进导航）"
run check_docs

# ⑥ 格格双达标·接线 CI（architecture §13，0.0.5 批E；收口批 E' 扩全量）："双达标"
#    之 tests 侧必须真跑在门禁里，"有测试未接线 = 未达标"。批E 只扫 modules/——
#    深挖发现 tools/sim 的 23 条 #[test] 处于"④ 只 check 编译测试目标、无任何步骤
#    真跑"的死角（重铺五版恰好动了它们钉的面），批E' 起 find 扩为与 ④ 同口径
#    （modules + tools + base 全部独立工程，esp 两工程排除同 ④）；≥1 通过断言与
#    ①b 同口径，0 测试工程在此红（不许"接线了但没测"）。批N32 件四起 rr3-brain
#    也是 Rust 工程，自然进本步扫面（其 Python tests/ 与 CI brain-unittest job
#    已随批N32 件四退役）。
step "⑥ 格格双达标·独立工程测试接线（每件 tests 真跑 ≥1，§13）"
found_mod=0
while IFS= read -r -d '' manifest; do
  found_mod=1
  run_tests sh -c "cargo test --locked -q --manifest-path '$manifest'"
done < <(find modules tools base -name Cargo.toml -not -path "*/target/*" \
                    -not -path "base/runtime/*" -not -path "tools/esp32-uart-echo/*" \
                    -not -path "tools/espidf-std-hello/*" -not -path "tools/espidf-sentinel/*" \
                    -not -path "tools/esp32-periph-base/*" \
                    -print0 | sort -z)
if [ "$found_mod" -eq 0 ]; then
  printf 'FAIL  ⑥ 未发现任何独立 Rust 工程（find 打空属门禁失效，不放行）\n'
  fail=1
fi

# ⑥b 部署面配置漂移门的**工具自检**（批N38 件二；2026-11-16 扩面到装配单与 unit）。
# 真板比对要 ssh 与装机树，CI/本机不可达；可机械判的是**判据本身**——工具用临时夹具跑
# 六例（三类齐全／unit 口径忽略 ExecStart／config 改字节／装配单漂移／unit 非 ExecStart
# 行漂移／unit 缺件），判据腐化即红。漂移的现场比对归板上线 runbook（boards §10 步④）
# 与 build-onboard 报告段。
step "⑥b 部署面配置漂移门·工具自检（verify-board-config --self-test）"
run bash tools/deploy/verify-board-config.sh --self-test

# ⑦ §13 静态断言（0.0.5 批E：零策略载荷 / F⊗F / 五件套 / 入口体量；全 fail-closed，
#    逐条点名后整步 FAIL）。"每格一件·不留空格"的反向断言在 rr-registry 契约测试
#    （② 步仓扫描 repo_atoms_compliant_and_every_slot_uniquely_covered）——查册真值处
#    执法，本步不抄第二份。业务阈值字面量无可靠静态判据（注释里的版本号同为点分数值），
#    执法面 = atom 板无关门 + 描述符/ops 注入纪律 + 评审（§13"判不了 semantic 的留评审"）。
step "⑦ §13 静态断言（零策略载荷/F⊗F/五件套/入口体量）"
rr3_step_7

# ⑧ 件级判据三门（0.0.5 批K H2，architecture §4.0.3 判据② + §13 表；全 fail-closed，
#    上线即全绿——既有步序不得由 FAIL 变 SKIP，新门同理）。步序与条数登记 conventions §2。
# ⑧-a 件级两腿对拍计数门：格两轴读 contracts/slots.toml（C9/C17：豁免数据化，门内无
#     硬编码 if）——`status` 判分母（normal / placeholder / retired 三态），`tier_scope`
#     记档位适用（非空且为 host|rtos|bare 子集，缺/非法即 fail-closed；0.0.5 批M
#     MC-10 L-2 扩读，收 R-8——此前只读 status、tier_scope 门禁不读）。normal 格的落点
#     件须 mech.rs + 模块运行机制实体（composite.rs / dispatch.rs / rpc.rs——0.0.8 W-A
#     拆分后多落点，批N26 件二订正；其实体的 tests/ 子目录同计：单测随结构化拆分搬走
#     时门按内容执法、不按文件名执法）两腿各 ≥1（腿一 fn parity_leg1_*、腿二
#     fn parity_leg2_*，命名即机器锚）；只有腿一记未达标（§4.0.3：否则恒输出 0 的空件
#     照样过门）。件形由 module.toml `lang` 数据位判（rust|python，缺/非法即 fail-closed，
#     不再用 `[ ! -f mech.rs ]` 存在性启发式猜）——lang=python 两件式件（无 atoms/*.rs
#     扫描面）记"不适用"；其 cognition 四格不属本门分母（H1 口径 11 件 = Rust 真件）。
# ⑧-a C10 反例自证：门内建 stub fixture（恒输出 0 的件 + 只有腿一），同一计数函数
#     必须拦下——拦不住则门本身 FAIL（对拍门若放行空件就只是仪式）。
# ⑧-b 件级依赖闭包 grep 门（C7 分级执法）：扫 modules/*/atoms/*/*.rs 的**非 test 面**
#     ——行注释先剥（文档注释要表述门禁规则自身）、#[cfg(test)] 块整块剥（花括号深度
#     剥离；假后端 sim 夹具合法）。L0 = `std::`（含 `use std::` / `extern crate std` /
#     全限定内联三态——L0 与 L1 同为全限定面，仅匹配声明面会被 `std::mem::replace`
#     这类内联路径绕过，活体见批K 深挖收口）；
#     L1 = L0 + `serde_json`（承载面禁入，批I F5 已上移清零——退场迁移清单在册，门认
#     已清零的现状）。`rr_hal::` 引用不进本门执法面（trait 注入与 sim 假后端是批I 定形
#     的合法形状；承载类型违例无 grep 可靠判据，归 §4.0.3 评审条款，收口案例批I 已清）。
# ⑧-c 注入面校验门：rr-registry contract 的 injection 用例**显式过滤器**——② 跑全量
#     对"用例被改名/删除"是 fail-open（还有别的测试兜着），此步封死：过滤器打空即红
#     （run_tests 的 ≥1 断言），注入面校验永远有真值单点在门里。
step "⑧ 件级判据三门（对拍计数门/依赖闭包门/注入面校验门，批K H2）"
rr3_step_8

# —— ⑨-d 生成物一致性门（门 f：重跑生成器 diff 为空；全案 §8/§4.5，批N1 n1-8）
# 真值点 contracts/topics.toml + contracts/face-expressions.toml → 板侧副本
# tools/esp32-periph-base/src/topics_gen.rs（rr3-codegen --check 只比对不写）。
step "⑨-d 生成物一致性（真值点 → 板侧常量表，重跑 rr3-codegen diff 为空）"
if codegen_out="$(cd tools/rr3-codegen && cargo run --locked -q -- --check 2>&1)"; then
  printf '%s\n' "$codegen_out"
  printf 'PASS  ⑨-d 生成物一致（topics_gen.rs：三主题串 + 板侧七态白名单，与 contracts/ 真值点同步）\n'
else
  printf '%s\n' "$codegen_out"
  printf 'FAIL  ⑨-d 生成物漂移（tools/esp32-periph-base/src/topics_gen.rs 与 contracts/ 真值点不一致；在仓根跑 cd tools/rr3-codegen && cargo run 重生成并审阅）\n'
  fail=1
fi

# —— ⑨-e 行程真值单点门（门 g：全案 §8，批N3 件五；读者集合定义随 act A-17）。
# 行程唯一真值点 = 执行轴档案 hardware/<形态>/axis/<instance>.toml 的
# center/lo/hi 三键：host 侧经装配注入（RR3_ACT_AXIS_STATE）进模块，板侧经
# 生成器（rr3-codegen）落 axis_gen.rs 常量表——两条读者路都溯源档案。
# 全仓四字面零命中 fail-closed：旧行程键名（host config 侧，件五② 撤除）与
# 板侧三旧手写行程常量名（件五③ 撤除改生成表）。注释/断言提及字面量名同样
# 红（严格口径——表述一律用"行程键/手写行程字面量"泛称，防回流靠门不靠记性）。
# 豁免（四处，非放水）：
#   docs/ —— status/conventions/全案/ADR 是历史账，记录漂移与裁定，不改史；
#   本步自身 —— 执法文本（grep 模式）必然含字面量，门不自查自身；
#   target/ —— 构建产物不属仓面；
#   .embuild/ —— esp-idf SDK 树（⑨-c/⑨-d 的 ESP32 构建依赖，批N5 件二笔2 补豁免：
#   同 target 同性质的构建产物，几十万文件的全树扫描还让本门在冷缓存下拖数分钟）。
step "⑨-e 行程真值单点（门 g：旧行程键名与板侧旧行程常量名全仓零命中，档案是唯一真值）"
travel_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
grep -rn --exclude-dir=.git --exclude-dir=target --exclude-dir=docs --exclude-dir=.embuild \
     --exclude=check.sh \
     -e 'angle_limit_deg' -e 'AXIS_CENTER_DEG' -e 'PAN_LIMIT_DEG' -e 'TILT_LIMIT_DEG' \
     . > "$travel_hits" 2>/dev/null
if [ -s "$travel_hits" ]; then
  cat "$travel_hits"
  printf 'FAIL  ⑨-e 行程字面零命中被击穿（见上行号）——行程唯一真值点 = hardware/<形态>/axis/*.toml 的 center/lo/hi，出现即读者回流：撤字面量改引注入/生成表\n'
  fail=1
else
  printf 'PASS  ⑨-e 行程真值单点（旧行程键名与板侧旧行程常量名全仓零命中，读者仅档案→装配注入/生成表两路）\n'
fi
rm -f "$travel_hits"

# —— ⑨-w 依赖面禁令（基-35，裁定 12；全案 §24.4 C 组 / architecture §14，0.0.5 增补轮六
#     第 0.5 步建门）。判据（可施工面，窄化已登记 decisions.md 基-35 行）：
#   1) 单元（modules/*/Cargo.toml）必须直依能力面契约 crate rr-hal-api——契约符号的
#      Cargo 依赖面可证，不再靠 rr-hal re-export 的隐性传递；
#   2) 单元 use 面（modules/** 非 test 面一并扫，行注释先剥——文档注释可表述门规则自身，
#      同 ⑦-a 口径）`rr_hal::` 引用仅许**冻结白名单**四类，白名单外（含经 rr-hal re-export
#      引契约符号、crate 根花括号多导 `rr_hal::{`——多导可把契约符号与白名单混进一行，
#      扫描面不可分词，故直接红）即红：
#        rr_hal::backend::*          装配选择函数（返回 Box<dyn 契约 trait>）——装配位
#                                    在单元组合件内是**设计态**（基-35 窄化①裁定
#                                    2026-10-22：不上移，§25.8 #1——Cargo 收益已由
#                                    直依 rr-hal-api 拿到，上移属多抽象；白名单四类
#                                    冻结，条目数由判据 6) 基线钉数执法）；
#        rr_hal::sim::*              sim 承载（单测夹具 + connect 扫描列表运行期真值）；
#        rr_hal::provision_store::*  配网凭据存储面（第 8 能力面 trait 住实现 crate；
#                                    句柄面契约分母 7 冻结，迁移属分母变更须另裁）；
#        rr_hal::Rr3PcmStream        std 侧承载类型（rr-hal-api 定形要点 F1 明文留 rr-hal）。
#   3) 板侧（tools/esp32-periph-base）Cargo 依赖不得出现实现 crate rr-hal / rr-bus / rrd
#      （architecture §14 全量口径；今日零依赖，钉住防漂）。tools/sim 是 CI/无头开发
#      承载工具（工具层），不在单元分母，豁免。
#   4) 单元（modules/*/Cargo.toml）承载面必依契约 crate rr-bus-api 且不得列实现层
#      rr-bus（基-35 窄化②/批N9 件一，§25.8 #2——"单元绑实现 crate"同族的第二次
#      出现，照 rr-hal-api 先例；rr-bus 收窄为语义常量 re-export 层，仅 rrd/tools 消费）；
#   5) 单元 use 面（modules/**，行注释先剥同判据 2 口径）`rr_bus::` 零命中——承载
#      客户端走 rr_bus_api，语义常量直引 rr_core（真值单点，不经 re-export 中介）；
#   6) 白名单条目数基线钉数（基-35 窄化①裁定配套，§25.8 #1 终态"白名单条目数只降
#      不升"机器化）：判据 2) 白名单正则的交替项数对基线文件 tools/ci/gate-w-baseline.txt，
#      count > 基线即红（扩白名单须同批上调基线——diff 即登记）；count < 基线 PASS 并
#      打印"须同批下调基线"提醒；文件缺/坏/非整数即红 fail-closed（同 gate-a 纪律）。
step "⑨-w 依赖面禁令（单元契约面走 rr-hal-api/rr-bus-api + use 面白名单冻结＋条目数基线钉数 + 板侧禁实现 crate，基-35）"
rr3_step_9w

# —— ⑨-n 注入路径唯一（门 n 静态面执法位；全案 §8 门 n / 施工图 Task 8 Step 2，n1-11）。
# 三层判据（实-14）：①装载期判据与 ②反例用例已在（profile.rs 门 n 段 + gate_n_* 用例，
# 经步 ①b cargo test --workspace 进 CI），本步补 ③专属执法步。判据＝[hardware] 段出现
# RR3_PIN_DERIVED_KEYS 任一物理键即红（稳态：物理键一律 [pins] 派生，[hardware] 只留
# 非引脚键 alsa_device/link_ifname——批N3 件六 SPI 词形收口：spi_device 已进派生键集、
# 扫面 8 键，敞口 18 行已结）。键名单从代码常量导出（单真值点，不抄第二份）：解析
# 装配单模块的 RR3_PIN_DERIVED_KEYS 数组（profile/model.rs），声明数量与实解析数量不等即门禁失效。
step "⑨-n 注入路径唯一（[hardware] 禁物理键——[pins] 派生稳态判据静态面，门 n）"
rr3_step_9n
step "⑨-c 电流预算装载拒链（校验⑨ per-instance 扩算 + kind=power 覆盖判据，门 c）"
rr3_step_9c
step "⑨-a 基-39 角色覆盖六表对账·执法（A 未覆盖 body normal 格 / B roles 悬空 / C trait·键差集 / D iface 实现者缺位，门 a）"
rr3_step_9a
step "⑨-r 留痕不进被丢弃的流（六模块生产码 stdout 留痕零命中，门 r）"
rr3_step_9r
step "⑨-z 真值集三方成员判等（单点在位＋判据面/打包面共读＋第二份字面零命中，门 z）"
rr3_step_9z
step "⑨-y 跨平台可编面（base 层 Windows 红 crate 数基线钉数，门 y）"
rr3_step_9y
# —— ⑩ 门 d 面板覆盖率对账·基线钉数门（当前上限 0）（基-19↔本步；interact I-19，批N4 件三；全案 §8
#     门 d / conventions §2 拟步 ⑩）。判据（字段级分母 + covers 申报制分子）：
#       分母 = 五路字段并集（全部自代码常量/契约表导出——单真值点，⑨-a/⑨-n 先例）：
#         ops/<slot>   ← modules/*/atoms/*/assemble.toml 的 ops 槽；
#         pin/<field>  ← 装配单模块 RR3_PIN_KEYS（⑨-a 同款 awk，声明数判等）；
#         role/<req>   ← contracts/roles.toml requires 逐项（⑨-a 同款拆词）；
#         domain/<f>   ← 装配单模块 RR3_DOMAIN_KEYS（声明数判等）；
#         axis/<field> ← rr-hal-api 的 RR3_ACT_PROFILE_FIELDS 十四键（该常量已
#                        按契约族拆进 src/ 子模块，扫描面随内容走、不钉单文件名）。
#       分子 = ∪ 每端点 covers 申报（panel/server.rs RR3_PANEL_ROUTES 第三列 ＋
#       板侧 httpd.rs RR3_BOARD_ROUTES 第三列，批N13 件二扩入两源同扫），
#       元素形制 = <集合>/<单段字段>、集合词表自 RR3_PANEL_COVER_SETS 导出——未申报
#       不计分子（申报制，不是"默认全算"；词表外/缺斜杠/多段 = 申报面错，硬红不进基线）。
#       同表第四列 = applies 生效路径申报（I-20，批N4 件四，全案 §19.3 判据 1）：
#       非空须在 RR3_PANEL_APPLY_MODES 词表（live|reboot|reflash）内；写口（非 GET）
#       动了可配置集（covers 非空）则必填——缺失即红（申报面校验不进红账口径，
#       账 = covers 差集一路；词表外/缺失/行形制坏硬红同 covers）。
#     门 d 数学：置红 = |分母 − 分子|，差集逐条打印（"D "行 = 机器可读红账，
#     ADR-0022 裁定 6.3 开放项的差集产出位）。
#     机器判据（gate-a-baseline 先例，双行 fail-closed）：
#       行 1 红账上限：红账 > 上限 FAIL（回涨必有一笔未登记的新红，先归因再动基线）；
#         红账 < 上限 提醒同批下调（转绿进度，转绿终点 = 差集空，届时本门转红、基线退役）；
#       行 2 分母下限：分母 < 下限 FAIL（I-19 判据字面——红项数不许靠缩分母减少）；
#         分母 > 下限 提醒同批上调（合法扩面必须动基线，有迹可循）。
#     建即红：今日 14 端点读配置者 0（I-15 实况）⇒ 差集 = 全分母，红项随批N4 件九
#     三面扩端点逐批转绿；分母是设计产物，集合定义改动 = 契约级评审。
step "⑩ 门 d 面板覆盖率对账·基线钉数门（当前上限 0）（分子 covers 申报＋applies 生效路径 / 分母字段级五路并集，门 d）"
rr3_step_10

# ⑫ 质量度量门（B11，基线钉数）：重复率硬门槛 3%；圈复杂度、超长生产函数名与
# 生产长睡字面量按 tools/ci/metrics-baseline.json 只降不升（长睡项补"长阻塞主
# 循环"的静态半边：模块/rrd 主循环面 ≥1s 的字面量 sleep 基线 0）。度量核为纯
# Python 标准库脚本（无外网依赖）；python3 缺位按 SKIP 纪律明说不放行静默。
printf '\n=== ⑫ 质量度量门（重复率 / 圈复杂度 / panic 落点 / 函数名长度 / 长睡） ===\n'
if command -v python3 >/dev/null 2>&1; then
  if python3 tools/ci/metrics.py; then
    :
  else
    fail=1
  fi
else
  printf 'SKIP  ⑫ python3 缺位——质量度量门不执行（CI runner 自带 python3）\n'
fi

# ⑬ 仓面卫生门（规则 7-b 的机制化）：追踪面不得含本机开发绝对路径，不得追踪
# 构建目录。白名单策略（.gitignore 默认忽略所有）防的是"无意收进产物"，本门防
# 的是"产物/本机路径已被收进"——两者一前一后，缺一就有漏面。
# 自身与文档中作为反例引用的路径不参与（实现文件按 BASH_SOURCE 推导路径后排除，
# 搬文件不失锚；规约正文与回执存档必须能引用被禁模式来描述规则）。
printf '\n=== ⑬ 仓面卫生门（追踪文件本机路径 / 构建目录） ===\n'
rr3_step_13

# ⑭ 装载门体量基线门（裁-13／基-38；批N8 件六"执法位下轮再论"兑现）：三项块
# 计数各不得超过 tools/ci/gate-size-baseline.txt 钉数（只降不升）。锚函数缺失
# = 基线失锚，红（B7 实证：旧名已改、无读者基线的注记烂掉无人知——失锚不许
# 静默跳过）。计数命令与基线行尾注同源；钉数解析按 "^<名> = <纯整数>"，
# 形状坏照 ⑩ 纪律 fail-closed。
printf '\n=== ⑭ 装载门体量基线（块行数 / 读表样板 / 同形函数） ===\n'
gs_base=tools/ci/gate-size-baseline.txt
gs_fail=0
gs_pin() { sed -n "s/^$1 *= *\([0-9][0-9]*\).*/\1/p" "$gs_base" | head -1; }
if [ ! -f "$gs_base" ]; then
  printf 'FAIL  ⑭ 基线文件缺失: %s\n' "$gs_base"
  gs_fail=1
else
  gs_fn='pub fn rr3_boot_with_probe('
  gs_file=$(grep -lF "$gs_fn" $prof_src 2>/dev/null | head -1)
  gs_start=$(grep -nF "$gs_fn" "$gs_file" 2>/dev/null | head -1 | cut -d: -f1)
  if [ -z "$gs_start" ]; then
    printf 'FAIL  ⑭ 基线锚函数缺失: 装配单模块中找不到 %s——装载门被改名/搬走须同批改基线，不许静默\n' "$gs_fn"
    gs_fail=1
  else
    gs_end=$(awk -v s="$gs_start" 'NR>=s && /^}/ {print NR; exit}' "$gs_file")
    gs_fn_lines=$((gs_end - gs_start + 1))
    gs_reads=$(awk -v s="$gs_start" -v e="$gs_end" \
      'NR>=s && NR<=e && /read_to_string/' "$gs_file" | wc -l | tr -d ' ')
    # 扫描面 = rr-registry 全树（按内容、不按文件名）：结构拆分后同形小函数可能
    # 住在子模块里，只扫 src/*.rs 会让计数随搬文件"假降"，门也就此失明。
    gs_shapes=$(find base/runtime/rr-registry/src -name '*.rs' -not -path '*/tests/*' -print0 \
      | xargs -0 grep -hoE 'fn rr3_[a-z_0-9]*(from_text|is_empty|contains|check)[a-z_0-9]*\(' \
      | sort -u | wc -l | tr -d ' ')
    gs_check() {
      gs_name=$1
      gs_got=$2
      gs_p=$(gs_pin "$gs_name")
      case "$gs_p" in
        ''|*[!0-9]*)
          printf 'FAIL  ⑭ 基线行形状坏: %s 的 "%s" 须为 "名 = 纯整数"（实得 "%s"）\n' \
            "$gs_base" "$gs_name" "${gs_p:-<缺>}"
          gs_fail=1
          ;;
        *)
          if [ "$gs_got" -gt "$gs_p" ]; then
            printf 'FAIL  ⑭ %s 回涨: %s > 钉数 %s（体量纪律只降不升；确须增长走评审并同批动基线）\n' \
              "$gs_name" "$gs_got" "$gs_p"
            gs_fail=1
          elif [ "$gs_got" -lt "$gs_p" ]; then
            printf 'PASS  ⑭ %s %s ≤ 钉数 %s（**下降 %s：须同批下调 %s**）\n' \
              "$gs_name" "$gs_got" "$gs_p" "$((gs_p - gs_got))" "$gs_base"
          else
            printf 'PASS  ⑭ %s %s = 钉数 %s\n' "$gs_name" "$gs_got" "$gs_p"
          fi
          ;;
      esac
    }
    gs_check gate_fn_lines "$gs_fn_lines"
    gs_check read_table_boilerplate "$gs_reads"
    gs_check same_shape_fns "$gs_shapes"
  fi
fi
if [ "$gs_fail" -ne 0 ]; then
  fail=1
fi

# ⑯ 源文件行数门（2026-11-13 立规）：每个 .rs 源文件必须 <600 行。
# 存量超标文件先立低水位基线 tools/ci/gate-lines-baseline.txt（只降不升）：
# 新增超标文件即红；拆完一个就同批下调基线。判据面 = git ls-files（追踪面即
# 执法面），测试文件同样计入——"测试代码不是代码"是自欺。
step "⑯ 源文件行数门（.rs ≥600 行违规，基线只降不升）"
lines_base_file="tools/ci/gate-lines-baseline.txt"
if [ ! -f "$lines_base_file" ]; then
  printf 'FAIL  ⑯ 缺基线文件 %s（fail-closed）\n' "$lines_base_file"
  fail=1
else
  base_n="$(grep -E '^[0-9]+$' "$lines_base_file" | head -1)"
  if [ -z "$base_n" ]; then
    printf 'FAIL  ⑯ 基线文件无整数计数（fail-closed）\n'
    fail=1
  else
    over="$(git -c core.quotepath=false ls-files '*.rs' | while IFS= read -r f; do
      n="$(wc -l < "$f")"
      [ "$n" -ge 600 ] && printf '%s\t%s\n' "$n" "$f"
    done | sort -rn)"
    cnt="$(printf '%s\n' "$over" | grep -c . || true)"
    printf '⑯ 超标文件 %s 个（上限 %s，阈值 600 行）\n' "$cnt" "$base_n"
    if [ "$cnt" -gt "$base_n" ]; then
      printf '%s\n' "$over" | head -10 | sed 's/^/  超标  /'
      printf 'FAIL  ⑯ 超标文件数回涨（%s > %s）：先拆再评审，不许直接上调基线\n' "$cnt" "$base_n"
      fail=1
    elif [ "$cnt" -lt "$base_n" ]; then
      printf 'PASS  ⑯ 超标 %s < 上限 %s（拆完同批下调 %s）\n' "$cnt" "$base_n" "$lines_base_file"
    else
      printf 'PASS  ⑯ 超标 %s = 上限 %s（只降不升）\n' "$cnt" "$base_n"
    fi
  fi
fi

# ⑮ 锁内 I/O 门（续轮二十二）：人工巡检三轮归零后改为机器判据——拦三类已实证
# 回潮：lock 与 log 同行 / 条件位临时 guard 延寿到块体 / 直调会写待落盘日志的
# registry 方法；扫面打空即门禁失效（fail-closed）。
printf '\n=== ⑮ 锁内 I/O 门（lock·log 同行 / 条件位临时 guard / 直调 reg 日志方法） ===\n'
if command -v python3 >/dev/null 2>&1; then
  if ! python3 tools/ci/lockio_audit.py; then
    fail=1
  fi
else
  printf 'SKIP  ⑮ python3 缺位——锁内 I/O 门不执行（CI runner 自带 python3）\n'
fi

printf '\n=== 汇总 ===\n'
if [ "$fail" -eq 0 ]; then
  echo "所有规约检查通过"
else
  echo "存在失败项（见上方 FAIL）"
fi
exit "$fail"
