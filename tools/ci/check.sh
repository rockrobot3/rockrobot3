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
#    tools/ci/esp-uart-echo-check.sh 覆盖（工具链缺位时 SKIP 明说，编译成功还断言 ELF 出树）。
#    同例：tools/espidf-std-hello（0.0.5 增补轮四 批W1，xtensa-esp32-espidf 目标 + build-std +
#    esp-idf-sys，还须 ESP-IDF 克隆与乐鑫 esp-clang）——专用门 tools/ci/espidf-std-check.sh，
#    SKIP 纪律同上。同例：tools/espidf-sentinel（0.0.7 批D1 层③ 哨兵，同款 espidf 工具链
#    + 主仓 rr-core/rr-ipc path 依赖）——专用门 tools/ci/espidf-sentinel-check.sh（甲面
#    xtensa 构建 = 双证之①），SKIP 纪律同上；其宿主对手件是普通独立工程
#    tools/rr3-sentinel-harness（不在哨兵目录内的原因见其 Cargo.toml 注记），照常进 ④/⑥。
#    同例：tools/esp32-periph-base（0.0.5 增补轮五 批B 外设底座档，同款 espidf
#    工具链，OLED/舵机/UART2 指令面）——专用门 tools/ci/esp32-periph-base-check.sh，
#    SKIP 纪律同上。
#    同例（0.0.5 增补轮六 批N8 件三）：modules/rr3-act、modules/rr3-sense 是
#    **双档工程**——espidf 档专属依赖挂 Cargo.toml target 表（宿主构建不解析），
#    宿主 linux 档照常进本门 ④/⑥，**不**整目录排除（与上述 tools/ 专用工程不同）；
#    espidf 档编译面由专用门 tools/ci/rr3-act-sense-espidf-check.sh 执法（显式
#    --target，SKIP 纪律同上）。
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
                    -not -path "base/runtime/*" -not -path "tools/esp32-uart-echo/*" \
                    -not -path "tools/espidf-std-hello/*" -not -path "tools/espidf-sentinel/*" \
                    -not -path "tools/esp32-periph-base/*" \
                    -print0 | sort -z)
if [ "$found" -eq 0 ]; then
  printf 'FAIL  ④ 未发现任何独立 Rust 工程（find 打空属门禁失效，不放行）\n'
  fail=1
fi

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
#    ①b 同口径，0 测试工程在此红（不许"接线了但没测"）。brain 是 Python（无
#    Cargo.toml，find 自然跳过）：其 tests/ 由 CI brain-unittest job 覆盖（R30 单
#    真源 .github/workflows/ci.yml——atomgit 无流水线功能，CI 改走 GitHub Actions），
#    接线事实登记于 conventions §2，不以本步 SKIP 充数。
step "⑥ 格格双达标·独立工程测试接线（每件 tests 真跑 ≥1，§13）"
found_mod=0
while IFS= read -r -d '' manifest; do
  found_mod=1
  run_tests sh -c "cargo test -q --manifest-path '$manifest'"
done < <(find modules tools base -name Cargo.toml -not -path "*/target/*" \
                    -not -path "base/runtime/*" -not -path "tools/esp32-uart-echo/*" \
                    -not -path "tools/espidf-std-hello/*" -not -path "tools/espidf-sentinel/*" \
                    -not -path "tools/esp32-periph-base/*" \
                    -print0 | sort -z)
if [ "$found_mod" -eq 0 ]; then
  printf 'FAIL  ⑥ 未发现任何独立 Rust 工程（find 打空属门禁失效，不放行）\n'
  fail=1
fi

# ⑦ §13 静态断言（0.0.5 批E：零策略载荷 / F⊗F / 五件套 / 入口体量；全 fail-closed，
#    逐条点名后整步 FAIL）。"每格一件·不留空格"的反向断言在 rr-registry 契约测试
#    （② 步仓扫描 repo_atoms_compliant_and_every_slot_uniquely_covered）——查册真值处
#    执法，本步不抄第二份。业务阈值字面量无可靠静态判据（注释里的版本号同为点分数值），
#    执法面 = atom 板无关门 + 描述符/ops 注入纪律 + 评审（§13"判不了 semantic 的留评审"）。
step "⑦ §13 静态断言（零策略载荷/F⊗F/五件套/入口体量）"
static_fail=0
# ⑦-a 机制核零策略载荷：厂商/产品/芯片/器件名与 vendor/product 条件分支命中即红。
#     机制核目录 = no_std 可达面：rr-mech 全部（0.0.5 批M 控制原语叶子件，自
#     rr-core 下沉）+ rr-core 全部 + rr-hal-api 能力面契约层全部
#     （0.0.5 批I C8：新机制核面同批进本扫面，门查门）+ rr-ipc 字节契约层 protocol/
#     + 传输抽象 transport/mod.rs + 原子件面 modules/*/atoms/*/*.rs（批K H2/L-6：
#     SURFACE 常量与 println 同在载荷面——件自述串是线上字节，板名藏进去即违约）；
#     具体传输实现（uds/shm/tcp/uart）是平台供给，不在载荷面。
#     行注释先剥离（`// …` 含 `//!`）再查：文档注释常需表述门禁规则自身，自指不违例。
#     禁词是 rock3 自有清单（同 atom.rs 板无关门的厂商/产品段，另加中文板名），不搬外部模板。
#     自家产品标识（rock3/洛克/RR3_ 前缀）不进禁词——机制自身的运行时身份（如
#     rock3-ipc.sock 契约冻结路径）不是策略载荷，门禁禁的是**外部**板卡厂商与器件。
policy_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
# 扫描面唯一真值（0.0.5 批M MC-10 L-4）：⑦-a 执法与 ⑦-e 自指判等共用此数组；文档两处口径
# （conventions §2 ⑦ 行、tools/ci/README.md ⑦ 行）向它看齐——改面只改这里一处，⑦-e 拦住漏同步。
mech_surfaces=(
  base/runtime/rr-mech/src/*.rs
  base/runtime/rr-core/src/*.rs
  base/runtime/rr-hal-api/src/*.rs
  base/runtime/rr-ipc/src/protocol/*.rs
  base/runtime/rr-ipc/src/transport/mod.rs
  modules/*/atoms/*/*.rs
)
for f in ${mech_surfaces[@]}; do
  [ -f "$f" ] || continue
  sed 's://.*$::' "$f" \
    | grep -inE 'rockchip|raspberry|broadcom|espressif|allwinner|sunxi|amlogic|rk35|rk33|rv11|cm4|esp32|ina219|max7219|tp4056|es8388|sg90|cp2102|mpu6050|树莓派|if.*(vendor|product)' \
    | sed "s|^|$f:|" >> "$policy_hits"
done
if [ -s "$policy_hits" ]; then
  cat "$policy_hits"
  printf 'FAIL  ⑦-a 机制核零策略载荷命中（厂商/产品名或 vendor 条件分支，见上行号）\n'
  static_fail=1
else
  printf 'PASS  ⑦-a 机制核零策略载荷（rr-mech + rr-core + rr-hal-api + rr-ipc 契约层/传输抽象 + atoms 件面，禁词零命中）\n'
fi
# ⑦-b F⊗F 非法 + 禁薄胶水存在性：composite.toml [assemble].atoms 的每个成员必须解析到
#     本模块 atoms/<名>/atom.toml——⊗ 链只能装原子件，把组合件/面当被装配方（F⊗F/C⊗C）
#     或写不存在的名即红（§13：面间只 ⊙）。atoms 行缺失/形状畸形同红（fail-closed）。
for c in modules/*/composite.toml; do
  mod_dir="$(dirname "$c")"; mod="$(basename "$mod_dir")"
  line="$(grep -E '^atoms *= *\[' "$c" || true)"
  if [ -z "$line" ]; then
    printf 'FAIL  ⑦-b F⊗F/装配成员: %s 缺 [assemble] atoms 行（fail-closed，不猜形状）\n' "$c"
    static_fail=1; continue
  fi
  members="$(printf '%s\n' "$line" | grep -oE '"[^"]+"' | tr -d '"' || true)"
  if [ -z "$members" ]; then
    printf 'FAIL  ⑦-b F⊗F/装配成员: %s atoms 行无可解析成员\n' "$c"
    static_fail=1; continue
  fi
  for name in $members; do
    if [ ! -f "$mod_dir/atoms/$name/atom.toml" ]; then
      printf 'FAIL  ⑦-b F⊗F/装配成员: %s 成员 "%s" 不是本模块原子件（atoms/%s/atom.toml 不在）——⊗ 链只装原子件，面间只 ⊙\n' "$c" "$name" "$name"
      static_fail=1
    fi
  done
done
[ "$static_fail" -eq 0 ] && printf 'PASS  ⑦-b F⊗F/装配成员存在性（全部 composite.toml 的 ⊗ 链成员皆本模块原子件）\n'
# —— 模块语言数据源（读 module.toml `manifest.lang`；schema 必填 + 枚举 rust|python，
#    contract 钉死 schema.rs）。⑦-c 件形与 ⑧-a 豁免的**唯一判据数据源**——门内零零件名
#    硬编码（0.0.5 批M MC-10 L-2，承 R-8：architecture §13"⑦-c 仍留两处 if rr3-brain"
#    收编；参数 = module.toml 路径，供 ⑦-c/$mod 与 ⑧-a/$mod_dir 两处共用同一真值）。
mod_lang() {
  grep -m1 -E '^lang[[:space:]]*=' "$1" 2>/dev/null | sed -E 's/.*"([^"]*)".*/\1/'
}
# ⑦-c 五件套齐全（§13 计数断言，承 ②b mkmod 产物形状）：Rust 原子件五件 = atom.toml +
#     <名>.rs（入口）+ mech.rs（实现）+ assemble.toml（装配清单）+ private.rs（唯一私有头）；
#     Python 件为两件式（atom.toml + assemble.toml，机制本体 .py 原位——conventions
#     §2 批C⑤ 在案，属登记豁免非 SKIP）。件形由 module.toml `lang` 数据位选（不再按
#     零件名 `if` 猜）。组合件实体 = composite.toml + 入口 + 实现（禁薄胶水存在性）。
for d in modules/*/atoms/*/; do
  mod="$(basename "$(dirname "$(dirname "$d")")")"; name="$(basename "$d")"
  lang="$(mod_lang "modules/$mod/module.toml")"
  case "$lang" in
    rust)   five="atom.toml assemble.toml private.rs mech.rs $name.rs" ;;
    python) five="atom.toml assemble.toml" ;;
    *)
      printf 'FAIL  ⑦-c 五件套: %s 模块 %s 的 lang=%s（module.toml manifest.lang 白名单 rust|python，缺/非法即 fail-closed）\n' "$d" "$mod" "${lang:-<空>}"
      static_fail=1; continue
      ;;
  esac
  for f in $five; do
    if [ ! -f "$d$f" ]; then
      printf 'FAIL  ⑦-c 五件套: %s 缺 %s\n' "$d" "$f"
      static_fail=1
    fi
  done
done
for c in modules/*/composite.toml; do
  mod_dir="$(dirname "$c")"; mod="$(basename "$mod_dir")"
  lang="$(mod_lang "$mod_dir/module.toml")"
  case "$lang" in
    rust)   req="src/main.rs src/composite.rs" ;;
    # Python 组合件实体：包目录 = 模块名破折号转下划线（naming.md §3 目录名 == 进程名 ==
    # manifest.name；pyproject `packages.find` include `"<包>*"` 同源），不硬编码零件名。
    python) pkg="$(printf '%s' "$mod" | tr '-' '_')"; req="$pkg/__main__.py $pkg/composite.py" ;;
    *)
      printf 'FAIL  ⑦-c 五件套(组合件): %s 模块 %s 的 lang=%s（缺/非法即 fail-closed）\n' "$c" "$mod" "${lang:-<空>}"
      static_fail=1; continue
      ;;
  esac
  for f in $req; do
    if [ ! -f "$mod_dir/$f" ]; then
      printf 'FAIL  ⑦-c 五件套(组合件): %s 缺 %s\n' "$c" "$f"
      static_fail=1
    fi
  done
done
[ "$static_fail" -eq 0 ] && printf 'PASS  ⑦-c 五件套齐全（全部原子件与组合件实体逐件计数过）\n'
# ⑦-d 入口体量（§13：装配入口 ≤80 行；批E 起 fail-closed——六入口现况 16–18 行）。
#     入口路径按 module.toml `lang` 数据位推导（L-2：此前硬编码 modules/rr3-brain/…）。
for mt in modules/*/module.toml; do
  mod_dir="$(dirname "$mt")"; mod="$(basename "$mod_dir")"
  lang="$(mod_lang "$mt")"
  case "$lang" in
    rust)   e="$mod_dir/src/main.rs" ;;
    python) pkg="$(printf '%s' "$mod" | tr '-' '_')"; e="$mod_dir/$pkg/__main__.py" ;;
    *) continue ;;  # lang 缺/非法已由 ⑦-c 逐件点名，此处不重复扰民
  esac
  [ -f "$e" ] || continue
  lines="$(wc -l < "$e" | tr -d ' ')"
  if [ "$lines" -gt 80 ]; then
    printf 'FAIL  ⑦-d 入口体量: %s %s 行 > 80（瘦入口纪律，逻辑下沉组合件/原子件）\n' "$e" "$lines"
    static_fail=1
  fi
done
[ "$static_fail" -eq 0 ] && printf 'PASS  ⑦-d 入口体量（全部装配入口 ≤80 行）\n'
# ⑦-e 门禁清单自指判等（0.0.5 批M MC-10 L-4，architecture §13"门禁清单同步"行）：
#     ⑦-a 扫面在代码里有一个真值（上面的 mech_surfaces 数组）；文档另有 conventions §2 ⑦ 行
#     与 tools/ci/README.md ⑦ 行两处抄写。抄写靠人记得同步必然漂移——实测批M 起 `rr-mech` 进了
#     扫面，两处文档行均未列（"文档宣称有门、门实际没扫"的制度自己也得被扫）。本件从数组归一
#     出面名集 A（`base/runtime/<crate>/…` → `<crate>`；`modules/*/atoms/…` → `atoms`），逐名
#     在两份文档的 `^| ⑦ |` 行里查找，缺任一即 FAIL 点名；并断言 rr-core-xtensa-check.sh 的
#     `-p rr-*` 目标集 ⊆ A（机制核 crate 面与静态扫面同步）。方向单向：代码是唯一真值，
#     文档向代码看齐，门不由代码迁就文档。
mech_surfaces_names=""
for s in "${mech_surfaces[@]}"; do
  case "$s" in
    base/runtime/*)    n="$(printf '%s' "$s" | awk -F/ '{print $3}')" ;;
    modules/*/atoms/*) n="atoms" ;;
    *) printf 'FAIL  ⑦-e 自指判等: 扫面条目 %s 无归属规则（fail-closed，不猜）\n' "$s"; static_fail=1; continue ;;
  esac
  case " $mech_surfaces_names " in
    *" $n "*) ;;
    *) mech_surfaces_names="${mech_surfaces_names:+$mech_surfaces_names }$n" ;;
  esac
done
if [ -z "$mech_surfaces_names" ]; then
  printf 'FAIL  ⑦-e 自指判等: 扫面名集归一出空（门禁失效，不放行）\n'
  static_fail=1
fi
conv_row="$(grep -m1 -E '^\| ⑦ \|' docs/process/conventions.md || true)"
ci_row="$(grep -m1 -E '^\| ⑦ \|' tools/ci/README.md || true)"
if [ -z "$conv_row" ] || [ -z "$ci_row" ]; then
  printf 'FAIL  ⑦-e 自指判等: 文档 ⑦ 行读不到（conventions=%s README=%s；门禁失效，不放行）\n' \
         "$([ -n "$conv_row" ] && printf 有 || printf 空)" "$([ -n "$ci_row" ] && printf 有 || printf 空)"
  static_fail=1
else
  for n in $mech_surfaces_names; do
    case "$conv_row" in *"$n"*) ;; *) printf 'FAIL  ⑦-e 自指判等: conventions §2 ⑦ 行未列扫面名 "%s"（文档向代码看齐）\n' "$n"; static_fail=1 ;; esac
    case "$ci_row"   in *"$n"*) ;; *) printf 'FAIL  ⑦-e 自指判等: tools/ci/README.md ⑦ 行未列扫面名 "%s"（文档向代码看齐）\n' "$n"; static_fail=1 ;; esac
  done
fi
xtensa_targets="$(grep -oE '\-p rr-[a-z-]+' tools/ci/rr-core-xtensa-check.sh | awk '{print $2}' | sort -u)"
if [ -z "$xtensa_targets" ]; then
  printf 'FAIL  ⑦-e 自指判等: rr-core-xtensa-check.sh 的 -p rr-* 目标集打空（门禁失效，不放行）\n'
  static_fail=1
else
  for t in $xtensa_targets; do
    case " $mech_surfaces_names " in
      *" $t "*) ;;
      *) printf 'FAIL  ⑦-e 自指判等: xtensa 门目标 "%s" 不在静态扫面集 A（机制核面不同步）\n' "$t"; static_fail=1 ;;
    esac
  done
fi
[ "$static_fail" -eq 0 ] && printf 'PASS  ⑦-e 门禁清单自指判等（扫面名集 A 逐名在 conventions §2 与 tools/ci/README.md ⑦ 行齐 + xtensa 门目标集 ⊆ A）\n'
rm -f "$policy_hits"
if [ "$static_fail" -ne 0 ]; then
  fail=1
fi

# ⑧ 件级判据三门（0.0.5 批K H2，architecture §4.0.3 判据② + §13 表；全 fail-closed，
#    上线即全绿——既有步序不得由 FAIL 变 SKIP，新门同理）。步序与条数登记 conventions §2。
# ⑧-a 件级两腿对拍计数门：格两轴读 contracts/slots.toml（C9/C17：豁免数据化，门内无
#     硬编码 if）——`status` 判分母（normal / placeholder / retired 三态），`tier_scope`
#     记档位适用（非空且为 host|rtos|bare 子集，缺/非法即 fail-closed；0.0.5 批M
#     MC-10 L-2 扩读，收 R-8——此前只读 status、tier_scope 门禁不读）。normal 格的落点
#     件须 mech.rs + 模块 composite.rs 两腿各 ≥1（腿一 fn parity_leg1_*、腿二
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
gate_fail=0

# —— 两腿对拍计数函数（参数：<slots.toml> <modules根目录>；抽成函数供 C10 fixture
#    复用——自证跑的必须是同一份逻辑，另抄一份就失去了自证意义）。返回 0=全过、
#    1=有 FAIL、2=门自身失效（枚举打空等）。
parity_gate() {
  local slots_toml="$1" mods_root="$2" atom_tomls rc=0
  atom_tomls="$(find "$mods_root" -path '*/atoms/*/atom.toml' -not -path '*/target/*' 2>/dev/null | sort)"
  if [ -z "$atom_tomls" ]; then
    echo "ERROR: parity_gate atom.toml 枚举打空（门禁失效，不放行）" >&2
    return 2
  fi
  local atom_toml atom_dir atom_name mod_dir slot status tier_scope lang t leg1 leg2
  while IFS= read -r atom_toml; do
    atom_dir="$(dirname "$atom_toml")"
    atom_name="$(basename "$atom_dir")"
    mod_dir="$(dirname "$(dirname "$atom_dir")")"
    # [provides] 段内的格名（段边界内取引号串；形状畸形归 atom.schema 门，此处不猜）
    for slot in $(awk '/^\[provides\]/{p=1;next} /^\[/{p=0} p' "$atom_toml" \
                    | grep -oE '"[a-z][a-z0-9.]*"' | tr -d '"'); do
      status="$(awk -v want="$slot" '
        /^\[\[slot\]\]/  { in_s = 1; cur = "" }
        in_s && $1 == "name"   { v = $3; gsub(/[",]/, "", v); cur = v }
        in_s && $1 == "status" { v = $3; gsub(/[",]/, "", v); if (cur == want) { print v; exit } }
      ' "$slots_toml")"
      if [ -z "$status" ]; then
        printf 'FAIL  ⑧-a %s→%s: 提供格在 slots.toml 无 status 记录（格表缺失即红，C9 门只读这一处）\n' "$atom_name" "$slot"
        rc=1
        continue
      fi
      # 第二轴 tier_scope（L-2 扩读）：非空 + 档名在 host|rtos|bare 白名单内，缺/非法即红。
      tier_scope="$(awk -v want="$slot" '
        /^\[\[slot\]\]/  { in_s = 1; cur = "" }
        in_s && $1 == "name"       { v = $3; gsub(/[",]/, "", v); cur = v }
        in_s && $1 == "tier_scope" {
          if (cur == want) {
            v = $0; out = ""
            while (match(v, /"[^"]+"/)) { out = out substr(v, RSTART + 1, RLENGTH - 2) " "; v = substr(v, RSTART + RLENGTH) }
            print out; exit
          }
        }
      ' "$slots_toml")"
      if [ -z "$tier_scope" ]; then
        printf 'FAIL  ⑧-a %s→%s: 提供格在 slots.toml 缺 tier_scope 登记（两轴其一缺失即红，L-2 扩读）\n' "$atom_name" "$slot"
        rc=1
        continue
      fi
      for t in $tier_scope; do
        case "$t" in
          host|rtos|bare) ;;
          *) printf 'FAIL  ⑧-a %s→%s: tier_scope 含非法档 "%s"（白名单 host|rtos|bare）\n' "$atom_name" "$slot" "$t"; rc=1 ;;
        esac
      done
      if [ "$status" = "retired" ]; then
        printf 'PASS  ⑧-a %s→%s: retired 退出分母（号位保留、义务解除；tier_scope=%s）\n' "$atom_name" "$slot" "$tier_scope"
        continue
      fi
      if [ "$status" = "placeholder" ]; then
        printf 'PASS  ⑧-a %s→%s: placeholder 记不适用（占位 ≠ 通过，登记填实触发条件；tier_scope=%s）\n' "$atom_name" "$slot" "$tier_scope"
        continue
      fi
      lang="$(mod_lang "$mod_dir/module.toml")"
      if [ "$lang" = "python" ]; then
        # Python 两件式件（lang=python：atom.toml + assemble.toml，机制本体 .py 原位）——
        # 无 Rust 扫描面，记不适用（判据 = module.toml 数据位，非 `[ ! -f mech.rs ]` 启发式）
        printf 'PASS  ⑧-a %s→%s: normal 格但 lang=python 两件式件，记不适用（tier_scope=%s）\n' "$atom_name" "$slot" "$tier_scope"
        continue
      fi
      if [ "$lang" != "rust" ]; then
        printf 'FAIL  ⑧-a %s→%s: module.toml lang=%s（白名单 rust|python，缺/非法即 fail-closed）\n' "$atom_name" "$slot" "${lang:-<空>}"
        rc=1
        continue
      fi
      # 件级精确归属：对拍函数名须带本件名前缀（fn parity_leg1_<件名>_*）——
      # 模块级粗扫会让同模块异件互充数（battery 没写、charger 写了也能过），
      # 与 C10"不许空件充数"同理。atom.name 与目录名一致性已由批E 断言钉死。
      leg1="$(cat "$atom_dir/mech.rs" "$mod_dir/src/composite.rs" 2>/dev/null \
                | grep -cE "fn parity_leg1_${atom_name}_[a-z0-9_]+" || true)"
      leg2="$(cat "$atom_dir/mech.rs" "$mod_dir/src/composite.rs" 2>/dev/null \
                | grep -cE "fn parity_leg2_${atom_name}_[a-z0-9_]+" || true)"
      leg1="${leg1:-0}"; leg2="${leg2:-0}"
      if [ "$leg1" -ge 1 ] && [ "$leg2" -ge 1 ]; then
        printf 'PASS  ⑧-a %s→%s: 两腿齐（leg1=%s leg2=%s，扫描 mech.rs + composite.rs；lang=%s tier_scope=%s）\n' "$atom_name" "$slot" "$leg1" "$leg2" "$lang" "$tier_scope"
      else
        printf 'FAIL  ⑧-a %s→%s: normal 格两腿不齐（leg1=%s leg2=%s）——只有腿一记未达标（§4.0.3 判据②；lang=%s tier_scope=%s）\n' "$atom_name" "$slot" "$leg1" "$leg2" "$lang" "$tier_scope"
        rc=1
      fi
    done
  done <<< "$atom_tomls"
  return $rc
}

# —— ⑧-a C10 反例自证（先于正仓跑：门先证明自己拦得住空件）
c10_dir="$(mktemp -d)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
mkdir -p "$c10_dir/contracts" "$c10_dir/modules/rr3-stub/atoms/stub" "$c10_dir/modules/rr3-stub/src"
cat > "$c10_dir/contracts/slots.toml" <<'EOF'
schema_version = 2
[[slot]]
name = "stub.slot"
layer = "body"
status = "normal"
tier_scope = ["host", "rtos", "bare"]
EOF
# 件形判据是 module.toml `lang`（L-2）：fixture 须给数据位，否则会先被 lang 门（而非
# 对拍计数）拦下——自证要证明的是"对拍计数拦得住空件"，不能让旁门替它挡刀。
cat > "$c10_dir/modules/rr3-stub/module.toml" <<'EOF'
[manifest]
name = "rr3-stub"
lang = "rust"
EOF
cat > "$c10_dir/modules/rr3-stub/atoms/stub/atom.toml" <<'EOF'
schema_version = 1

[atom]
name = "stub"
version = "0.0.1"

[provides]
slots = ["stub.slot"]

[uses]
slots = []
EOF
cat > "$c10_dir/modules/rr3-stub/atoms/stub/mech.rs" <<'EOF'
//! stub 反例件（C10 fixture）：恒输出 0 的机制 + 只有腿一的对拍。
fn rr3_stub_body() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn parity_leg1_stub_is_deterministic() {
        assert_eq!(super::rr3_stub_body(), 0);
    }
}
EOF
: > "$c10_dir/modules/rr3-stub/src/composite.rs"
c10_out="$(parity_gate "$c10_dir/contracts/slots.toml" "$c10_dir/modules" 2>&1)" && c10_rc=0 || c10_rc=$?
# 自证判据不止"退非零"：必须是**对拍计数**（两腿不齐）拦下的，不能被 lang/tier_scope/
# status 等旁门替它挡刀——否则门红归因失真，自证就失去了意义。
if [ "$c10_rc" -eq 0 ] || ! printf '%s' "$c10_out" | grep -q 'normal 格两腿不齐'; then
  printf 'FAIL  ⑧-a C10 反例自证: stub 件（恒输出0+只有腿一）未被**对拍计数**拦下（rc=%s）——门对空件发奖即门禁失效\n' "$c10_rc"
  printf '%s\n' "$c10_out"
  gate_fail=1
else
  printf 'PASS  ⑧-a C10 反例自证: stub 反例被拦（恒输出0+只有腿一 → 对拍计数记未达标，门红）\n'
fi
rm -rf "$c10_dir"
# —— ⑧-a 正仓计数
parity_gate contracts/slots.toml modules || gate_fail=1

# —— ⑧-b 件级依赖闭包 grep 门（L0/L1，C7 分级）
atom_rs=0
for f in modules/*/atoms/*/*.rs; do [ -f "$f" ] && atom_rs=$((atom_rs + 1)); done
if [ "$atom_rs" -eq 0 ]; then
  printf 'FAIL  ⑧-b atom 源文件枚举打空（门禁失效，不放行）\n'
  gate_fail=1
else
  closure_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  awk '
    { line = $0; sub(/\/\/.*$/, "", line) }
    strip {
      n = gsub(/{/, "{", line); m = gsub(/}/, "}", line); depth += n - m
      if (depth <= 0 && n + m > 0) strip = 0
      next
    }
    line ~ /^[[:space:]]*#\[cfg\(test\)\]/ { strip = 1; depth = 0; next }
    line ~ /std::|extern crate std|use serde_json|serde_json::/ { print FILENAME ":" FNR ":" $0 }
  ' modules/*/atoms/*/*.rs >> "$closure_hits"
  if [ -s "$closure_hits" ]; then
    cat "$closure_hits"
    printf 'FAIL  ⑧-b 依赖闭包命中（L0=std::（含声明与全限定内联）；L1=+serde_json；非 test 面，见上行号）\n'
    gate_fail=1
  else
    printf 'PASS  ⑧-b 件级依赖闭包（%s 份 atom 源非 test 面，L0/L1 禁词零命中）\n' "$atom_rs"
  fi
  rm -f "$closure_hits"
fi

# —— ⑧-c 注入面校验门（显式过滤器封改名/删除 fail-open；run_tests 内部已置全局 fail）
if [ "$gate_fail" -ne 0 ]; then
  fail=1
fi
run_tests sh -c 'cd base/runtime && cargo test -p rr-registry --test contract injection'

# —— ⑨-d 生成物一致性门（门 f：重跑生成器 diff 为空；全案 §8/§4.5，批N1 n1-8）
# 真值点 contracts/topics.toml + contracts/face-expressions.toml → 板侧副本
# tools/esp32-periph-base/src/topics_gen.rs（rr3-codegen --check 只比对不写）。
step "⑨-d 生成物一致性（真值点 → 板侧常量表，重跑 rr3-codegen diff 为空）"
if codegen_out="$(cd tools/rr3-codegen && cargo run -q -- --check 2>&1)"; then
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
#                                    今日在单元组合件内是结构事实，Cargo 级拆边需装配
#                                    上移（敞口在册，落地后本行同步收紧为 Cargo 禁令）；
#        rr_hal::sim::*              sim 承载（单测夹具 + connect 扫描列表运行期真值）；
#        rr_hal::provision_store::*  配网凭据存储面（第 8 能力面 trait 住实现 crate；
#                                    句柄面契约分母 7 冻结，迁移属分母变更须另裁）；
#        rr_hal::Rr3PcmStream        std 侧承载类型（rr-hal-api 定形要点 F1 明文留 rr-hal）。
#   3) 板侧（tools/esp32-periph-base）Cargo 依赖不得出现实现 crate rr-hal / rr-bus / rrd
#      （architecture §14 全量口径；今日零依赖，钉住防漂）。tools/sim 是 CI/无头开发
#      承载工具（工具层），不在单元分母，豁免。
step "⑨-w 依赖面禁令（单元契约面走 rr-hal-api + use 面白名单冻结 + 板侧禁实现 crate，基-35）"
dep_fail=0
dep_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
# 1) rr-hal-api 必依（逐单元点名；枚举打空即门禁失效，与 ④/⑥ 同口径）
unit_manifests="$(find modules -maxdepth 2 -name Cargo.toml -not -path '*/target/*' | sort)"
if [ -z "$unit_manifests" ]; then
  printf 'FAIL  ⑨-w 单元 Cargo.toml 枚举打空（门禁失效，不放行）\n'
  dep_fail=1
fi
for c in $unit_manifests; do
  if grep -q 'rr-hal-api' "$c"; then
    printf 'PASS  ⑨-w %s 直依 rr-hal-api（契约依赖面在 Cargo 清单可证）\n' "$c"
  else
    printf 'FAIL  ⑨-w %s 未声明 rr-hal-api——单元契约符号须直依能力面 crate，不经实现 crate re-export（基-35）\n' "$c"
    dep_fail=1
  fi
done
# 2) use 面白名单（modules/** 全部 .rs；行注释先剥；白名单外逐行点名）
unit_rs=0
while IFS= read -r -d '' f; do
  unit_rs=$((unit_rs + 1))
  sed 's://.*$::' "$f" \
    | grep -nE 'rr_hal::' \
    | grep -vE 'rr_hal::(backend::|sim|provision_store|Rr3PcmStream)' \
    | sed "s|^|$f:|" >> "$dep_hits"
done < <(find modules -name '*.rs' -not -path '*/target/*' -print0 | sort -z)
if [ "$unit_rs" -eq 0 ]; then
  printf 'FAIL  ⑨-w 单元 .rs 枚举打空（门禁失效，不放行）\n'
  dep_fail=1
elif [ -s "$dep_hits" ]; then
  cat "$dep_hits"
  printf 'FAIL  ⑨-w 依赖面白名单外命中（契约符号须走 rr_hal_api；白名单=backend 装配/sim/provision_store/Rr3PcmStream 四类冻结，扩名单须登记，见上行号）\n'
  dep_fail=1
else
  printf 'PASS  ⑨-w 单元 use 面白名单（%s 份 .rs 全扫，rr_hal:: 引用皆装配/承载白名单四类）\n' "$unit_rs"
fi
rm -f "$dep_hits"
# 3) 板侧禁实现 crate（rr-hal/rr-bus/rrd；用 `名 =` 形状匹配依赖键，不误伤 rr-hal-api）
board_dep="$(grep -E '^[[:space:]]*(rr-hal|rr-bus|rrd)[[:space:]]*=' tools/esp32-periph-base/Cargo.toml 2>/dev/null || true)"
if [ -n "$board_dep" ]; then
  printf '%s\n' "$board_dep"
  printf 'FAIL  ⑨-w 板侧 Cargo 出现实现 crate 依赖（rr-hal/rr-bus/rrd，architecture §14 全量口径）\n'
  dep_fail=1
else
  printf 'PASS  ⑨-w 板侧零实现 crate 依赖（esp32-periph-base，rr-hal/rr-bus/rrd 三件钉住）\n'
fi
if [ "$dep_fail" -ne 0 ]; then
  fail=1
fi

# —— ⑨-n 注入路径唯一（门 n 静态面执法位；全案 §8 门 n / 施工图 Task 8 Step 2，n1-11）。
# 三层判据（实-14）：①装载期判据与 ②反例用例已在（profile.rs 门 n 段 + gate_n_* 用例，
# 经步 ①b cargo test --workspace 进 CI），本步补 ③专属执法步。判据＝[hardware] 段出现
# RR3_PIN_DERIVED_KEYS 任一物理键即红（稳态：物理键一律 [pins] 派生，[hardware] 只留
# 非引脚键 alsa_device/link_ifname——批N3 件六 SPI 词形收口：spi_device 已进派生键集、
# 扫面 8 键，敞口 18 行已结）。键名单从代码常量导出（单真值点，不抄第二份）：解析
# rrd/src/profile.rs 的 RR3_PIN_DERIVED_KEYS 数组，声明数量与实解析数量不等即门禁失效。
step "⑨-n 注入路径唯一（[hardware] 禁物理键——[pins] 派生稳态判据静态面，门 n）"
gate_n_fail=0
keys_file="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
awk '/const RR3_PIN_DERIVED_KEYS/{f=1} f{print} f && /\];/{exit}' base/runtime/rrd/src/profile.rs \
  | grep -oE '"[^"]+"' | tr -d '"' > "$keys_file"
decl_n="$(grep -m1 -oE 'RR3_PIN_DERIVED_KEYS: \[&str; [0-9]+\]' base/runtime/rrd/src/profile.rs | grep -oE '[0-9]+\]' | tr -d ']')"
real_n="$(grep -c . "$keys_file" || true)"
prof_n="$(find profiles -maxdepth 1 -name '*.toml' 2>/dev/null | wc -l | tr -d ' ')"
if [ -z "$decl_n" ] || [ "$real_n" -eq 0 ] || [ "$decl_n" -ne "$real_n" ] || [ "$prof_n" -eq 0 ]; then
  printf 'FAIL  ⑨-n 扫面前件不成立（键名单声明数=%s 实解析数=%s，profiles 份数=%s）——门禁失效，不放行\n' \
    "${decl_n:-空}" "$real_n" "$prof_n"
  gate_n_fail=1
else
  printf 'PASS  ⑨-n 扫面前件（键名单自代码常量 RR3_PIN_DERIVED_KEYS 导出 %s 键 × profiles %s 份）\n' "$real_n" "$prof_n"
fi
if [ "$gate_n_fail" -eq 0 ]; then
  hits_file="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  key_alt="$(tr '\n' '|' < "$keys_file")"; key_alt="${key_alt%|}"
  for f in profiles/*.toml; do
    awk -v alt="$key_alt" '
      /^[[:space:]]*#/ {next}                       # TOML 注释先剥（键名住注释是合法表述，同 ⑨-w 口径）
      /^\[/ {in_hw = ($0 ~ /^\[hardware\]/); next}  # 段界：进 [hardware]，遇下一节头（含 [[…]]）即出
      in_hw && $0 ~ "^(" alt ")[[:space:]]*=" {print FILENAME ":" FNR ": " $0}
    ' "$f" >> "$hits_file"
  done
  if [ -s "$hits_file" ]; then
    cat "$hits_file"
    printf 'FAIL  ⑨-n [hardware] 出现物理键（门 n 稳态判据：物理键一律 [pins] 派生；迁移期判据见 profile.rs 门 n 段）\n'
    gate_n_fail=1
  else
    printf 'PASS  ⑨-n [hardware] 零物理键（%s 份 profile 全扫，[hardware] 只留非引脚键）\n' "$prof_n"
  fi
  rm -f "$hits_file"
fi
rm -f "$keys_file"
if [ "$gate_n_fail" -ne 0 ]; then
  fail=1
fi

# —— ⑨-c 电流预算装载拒链（校验⑨ 专属执法步，门 c；全案 §5 ⑨ 行 / §16.4 计数，
#    批N2 本体 件一）。三层判据（实-14）：①装载期判据（rrd rr3_demand_budget_check
#    ＋ rr3_demand_declared_expand per-instance 扩算 ＋ rr3_demand_kind_power_gaps
#    kind=power 覆盖判据，接 boot 链致命汇合点）与 ②反例用例（demand_budget_check_nine
#    并发三态 ＋ demand_budget_chain_* 装载拒链三用例）经 ①b 全量进 CI，本步补 ③专属
#    执法步——显式过滤器 demand_budget 封 fail-open（过滤器打空或用例改名即 0 通过，
#    run_tests 直接红；新拒链用例命名须带 demand_budget 前缀进本步扫面）。
step "⑨-c 电流预算装载拒链（校验⑨ per-instance 扩算 + kind=power 覆盖判据，门 c）"
run_tests sh -c 'cd base/runtime && cargo test -p rrd --lib demand_budget'

# —— ⑨-a 基-39 角色覆盖六表对账·报告模式（门 a 第 ③ 层执法位——报告不置红，转红随
#     基-39 全刀落完；全案 §8 门 a / §24.1 第 5 条 + 裁-6，批N2 件六）。
#     四腿键集全部自代码常量/契约表导出（单真值点，不抄第二份；⑨-n 先例）：
#       slots ← contracts/slots.toml；roles ← contracts/roles.toml；
#       TRAITS/DESC_KEYS ← rr-registry/src/assemble.rs 两在册表；
#       PIN_KEYS ← rrd/src/profile.rs RR3_PIN_KEYS（声明数/实解析数判等）。
#     报告三段（裁定配套 (a)：走 stdout 可见面，不走被 Stdio::null() 吞掉的面）：
#       A 段 = 未覆盖 normal 格（status=normal 且无任何 role.slot 指向）；
#       B 段 = roles 侧悬空（slot/iface/requires/derives 任一不在对方真值集——**应恒 0**，
#              悬空外键是错不是账，>0 即硬红、不进基线）；
#       C 段 = TRAITS 未被 roles.iface 引用差集 ＋ RR3_PIN_KEYS 未被 roles.requires 引用差集；
#       D 段 = iface 实现者缺位（裁-17：iface 在册 ≠ 有实现者——role.iface 须 ≥1 个
#              prod 实现（`impl <iface> for` 文本命中，扫 base/modules/tools 产码、
#              排除 /tests/，口径同 ⑨-z 门；占位后端 Rr3UnavailableSource 是发布面
#              产码"能力缺失显式回错"，命中算数），否则该角色须显式标 status="planned"
#              且 planned_until 非空（终点登记）——**应恒 0**，硬红不进基线；
#              sensor.env/imu 两角色 planned 终点 = D9 器件到货转自动可判）。
#     门 a 数学（§24.1 第 5 条 + 裁-17）：置红 ＝ A + B + C + D。
#     机器判据（裁定配套 (c)）：低水位基线文件 tools/ci/gate-a-baseline.txt——
#       count > 基线即 FAIL（防红账回涨）；文件缺/坏/非整数即 FAIL（fail-closed）；
#       count < 基线 PASS 并打印"须同批下调基线"提醒（转红终点＝基-39 全覆盖 count=0）。
#     六表之第 5 表（件 handles[].desc_key × DESC_KEYS）已由 ② parse 层执法；第 6 表
#     （平台 role→具名构造）归门 a 转红收口件——本步如实不判、不抄第二份。
#     TRAITS/DESC_KEYS 是 &[&str] 切片无常量 N，提取打空的护栏＝B 段全悬空硬红＋扫面前件四查。
step "⑨-a 基-39 角色覆盖六表对账·报告模式（A 未覆盖 normal 格 / B roles 悬空 / C trait·键差集 / D iface 实现者缺位，门 a）"
gate_a_fail=0
ga_slots="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
ga_roles="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
ga_traits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
ga_desc="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
ga_pins="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
ga_status="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins" "$ga_status"' EXIT
trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins" "$ga_status"; exit 130' INT TERM
awk '/pub const RR3_ASSEMBLE_HANDLE_TRAITS/{f=1} f{print} f && /\];/{exit}' \
  base/runtime/rr-registry/src/assemble.rs | grep -oE '"[^"]+"' | tr -d '"' > "$ga_traits"
awk '/pub const RR3_ASSEMBLE_DESC_KEYS/{f=1} f{print} f && /\];/{exit}' \
  base/runtime/rr-registry/src/assemble.rs | grep -oE '"[^"]+"' | tr -d '"' > "$ga_desc"
awk '/const RR3_PIN_KEYS/{f=1} f{print} f && /\];/{exit}' \
  base/runtime/rrd/src/profile.rs | grep -oE '"[^"]+"' | tr -d '"' > "$ga_pins"
decl_pin_n="$(grep -m1 -oE 'RR3_PIN_KEYS: \[&str; [0-9]+\]' base/runtime/rrd/src/profile.rs | grep -oE '[0-9]+\]' | tr -d ']')"
real_pin_n="$(grep -c . "$ga_pins" || true)"
# slots：name+status 两轴逐格（⑧-a 同款 awk 口径）；roles：name/slot/iface/requires/derives 逐条
awk '
  /^\[\[slot\]\]/        { in_s = 1; nm = "" }
  in_s && $1 == "name"   { v = $3; gsub(/[",]/, "", v); nm = v }
  in_s && $1 == "status" { v = $3; gsub(/[",]/, "", v); print nm "\t" v; in_s = 0 }
' contracts/slots.toml > "$ga_slots"
awk '
  function flush() { if (nm != "") printf "%s\t%s\t%s\t%s\t%s\n", nm, sl, ifc, rq, dv }
  /^\[\[role\]\]/    { flush(); nm = ""; sl = ""; ifc = ""; rq = ""; dv = ""; next }
  $1 == "name"       { v = $3; gsub(/[",]/, "", v); nm = v }
  $1 == "slot"       { v = $3; gsub(/[",]/, "", v); sl = v }
  $1 == "iface"      { v = $3; gsub(/[",]/, "", v); ifc = v }
  $1 == "requires"   { line = $0; rq = ""
                       while (match(line, /"[^"]+"/)) { rq = rq substr(line, RSTART+1, RLENGTH-2) " "; line = substr(line, RSTART+RLENGTH) } }
  $1 == "derives"    { line = $0; dv = ""
                       while (match(line, /"[^"]+"/)) { dv = dv substr(line, RSTART+1, RLENGTH-2) " "; line = substr(line, RSTART+RLENGTH) } }
  END { flush() }
' contracts/roles.toml > "$ga_roles"
# planned 登记面（D 段豁免判据对象）：name/status/planned_until 三列，缺省键 = 空串
awk '
  function flush() { if (nm != "") printf "%s\t%s\t%s\n", nm, st, pu }
  /^\[\[role\]\]/       { flush(); nm = ""; st = ""; pu = ""; next }
  $1 == "name"          { v = $3; gsub(/[",]/, "", v); nm = v }
  $1 == "status"        { v = $3; gsub(/[",]/, "", v); st = v }
  $1 == "planned_until" { v = $3; gsub(/[",]/, "", v); pu = v }
  END { flush() }
' contracts/roles.toml > "$ga_status"
ga_slot_n="$(wc -l < "$ga_slots" | tr -d ' ')"
ga_normal_n="$(awk -F'\t' '$2 == "normal"' "$ga_slots" | wc -l | tr -d ' ')"
ga_roles_n="$(grep -c . "$ga_roles" || true)"
ga_traits_n="$(grep -c . "$ga_traits" || true)"
ga_desc_n="$(grep -c . "$ga_desc" || true)"
if [ -z "$ga_slot_n" ] || [ "$ga_slot_n" -eq 0 ] || [ "$ga_normal_n" -eq 0 ] \
   || [ -z "$ga_roles_n" ] || [ "$ga_roles_n" -eq 0 ] \
   || [ -z "$ga_traits_n" ] || [ "$ga_traits_n" -eq 0 ] \
   || [ -z "$ga_desc_n" ] || [ "$ga_desc_n" -eq 0 ] \
   || [ -z "$decl_pin_n" ] || [ -z "$real_pin_n" ] || [ "$real_pin_n" -eq 0 ] || [ "$decl_pin_n" -ne "$real_pin_n" ]; then
  printf 'FAIL  ⑨-a 扫面前件不成立（slots=%s/normal=%s roles=%s traits=%s desc_keys=%s pin_keys 声明=%s 实解析=%s）——键集导出打空属门禁失效，不放行\n' \
    "${ga_slot_n:-空}" "${ga_normal_n:-空}" "${ga_roles_n:-空}" "${ga_traits_n:-空}" "${ga_desc_n:-空}" "${decl_pin_n:-空}" "${real_pin_n:-空}"
  gate_a_fail=1
else
  printf 'PASS  ⑨-a 扫面前件（slots %s 格/normal %s × roles %s × traits %s × desc_keys %s × pin_keys %s，全部自契约表/代码常量导出）\n' \
    "$ga_slot_n" "$ga_normal_n" "$ga_roles_n" "$ga_traits_n" "$ga_desc_n" "$real_pin_n"
  ga_a=0; ga_b=0; ga_c=0
  # slot 名单单源导出（循环外一次 fork；打空即红——键集导出打空属门禁失效，fail-closed 同扫面前件）。
  # 判定用 herestring 同源变量：不得写 `printf '%s\n' "$(cut ...)" | grep -qxF`——
  # command substitution 在管道左子 shell 内延迟写入，grep -q 会竞态提前退而误判悬空
  # （实-15：完整门禁下间歇 B=1/2 假红，write error 与红笔一一对应，⑨-a 段独立跑恒绿）。
  ga_slot_ids="$(cut -f1 "$ga_slots")"
  if [ -z "$ga_slot_ids" ]; then
    printf 'FAIL  ⑨-a slot 名单导出打空——键集导出打空属门禁失效，不放行\n'
    gate_a_fail=1
  fi
  # —— B 段：roles 侧悬空（应恒 0；悬空外键任何时候都是错，硬红不进基线）
  while IFS=$'\t' read -r r_nm r_sl r_if r_rq r_dv; do
    [ -n "$r_nm" ] || continue
    if ! grep -qxF "$r_sl" <<<"$ga_slot_ids"; then
      printf 'B  role %s 的 slot 悬空: %s（slots.toml 无此格）\n' "$r_nm" "$r_sl"
      ga_b=$((ga_b + 1))
    fi
    if ! grep -qxF "$r_if" "$ga_traits"; then
      printf 'B  role %s 的 iface 悬空: %s（RR3_ASSEMBLE_HANDLE_TRAITS 无此名）\n' "$r_nm" "$r_if"
      ga_b=$((ga_b + 1))
    fi
    for k in $r_rq; do
      if ! grep -qxF "$k" "$ga_pins"; then
        printf 'B  role %s 的 requires 悬空: %s（RR3_PIN_KEYS 无此键）\n' "$r_nm" "$k"
        ga_b=$((ga_b + 1))
      fi
    done
    for k in $r_dv; do
      if ! grep -qxF "$k" "$ga_desc"; then
        printf 'B  role %s 的 derives 悬空: %s（RR3_ASSEMBLE_DESC_KEYS 无此键）\n' "$r_nm" "$k"
        ga_b=$((ga_b + 1))
      fi
    done
  done < "$ga_roles"
  # —— A 段：未覆盖 normal 格
  ga_covered="$(awk -F'\t' '{print $2}' "$ga_roles" | sort -u)"
  while IFS=$'\t' read -r s_nm s_st; do
    [ -n "$s_nm" ] || continue
    if [ "$s_st" = "normal" ] && ! printf '%s\n' "$ga_covered" | grep -qxF "$s_nm"; then
      printf 'A  normal 格未覆盖: %s（无任何 role.slot 指向）\n' "$s_nm"
      ga_a=$((ga_a + 1))
    fi
  done < "$ga_slots"
  # —— C 段：trait 差集 ＋ pin 键差集（未被 roles 引用侧）
  ga_used_if="$(awk -F'\t' '{print $3}' "$ga_roles" | sort -u)"
  ga_used_rq="$(awk -F'\t' '{print $4}' "$ga_roles" | tr ' ' '\n' | sort -u)"
  while IFS= read -r t; do
    [ -n "$t" ] || continue
    if ! printf '%s\n' "$ga_used_if" | grep -qxF "$t"; then
      printf 'C  trait 未被 roles.iface 引用: %s\n' "$t"
      ga_c=$((ga_c + 1))
    fi
  done < "$ga_traits"
  while IFS= read -r k; do
    [ -n "$k" ] || continue
    if ! printf '%s\n' "$ga_used_rq" | grep -qxF "$k"; then
      printf 'C  pin 键未被 roles.requires 引用: %s\n' "$k"
      ga_c=$((ga_c + 1))
    fi
  done < "$ga_pins"
  # —— D 段：iface 实现者缺位（裁-17：iface 在册 ≠ 有实现者，应恒 0；硬红不进基线）。
  # 先求"零实现 iface"名单（去重一次扫面，循环内零 fork），再逐 role 判豁免：
  # status="planned" 且 planned_until 非空 = 豁免成立（终点登记在案）；否则逐角色点名硬红。
  ga_dead_if="$(awk -F'\t' '{print $3}' "$ga_roles" | sort -u | while IFS= read -r t; do
    [ -n "$t" ] || continue
    if [ -z "$(grep -rn --include='*.rs' "impl ${t} for" base modules tools 2>/dev/null | grep -v '/tests/' || true)" ]; then
      printf '%s\n' "$t"
    fi
  done)"
  ga_d=0
  while IFS=$'\t' read -r r_nm r_sl r_if r_rq r_dv; do
    [ -n "$r_nm" ] || continue
    printf '%s\n' "$ga_dead_if" | grep -qxF "$r_if" || continue
    r_st=""; r_pu=""
    while IFS=$'\t' read -r s_nm s_st s_pu; do
      if [ "$s_nm" = "$r_nm" ]; then r_st="$s_st"; r_pu="$s_pu"; fi
    done < "$ga_status"
    if [ "$r_st" = "planned" ] && [ -n "$r_pu" ]; then
      continue  # 豁免成立：planned 且终点在案（D9 器件到货转自动可判）
    fi
    printf 'D  role %s 的 iface 实现者缺位: %s（裁-17：iface 在册 ≠ 有实现者——落 prod 实现者，或显式标 status="planned" 并给非空 planned_until 终点）\n' "$r_nm" "$r_if"
    ga_d=$((ga_d + 1))
  done < "$ga_roles"
  ga_total=$((ga_a + ga_b + ga_c + ga_d))
  printf '门 a 数学（全案 §24.1 第 5 条 + 裁-17）：置红 = 未覆盖 normal 格(A=%s) + roles 悬空(B=%s) + TRAITS/PIN_KEYS 差集(C=%s) + iface 实现者缺位(D=%s) = %s\n' \
    "$ga_a" "$ga_b" "$ga_c" "$ga_d" "$ga_total"
  if [ "$ga_b" -gt 0 ]; then
    printf 'FAIL  ⑨-a roles 侧悬空 %s 笔（B 段应恒 0——悬空外键是错不是账，硬红不进基线）\n' "$ga_b"
    gate_a_fail=1
  fi
  if [ "$ga_d" -gt 0 ]; then
    printf 'FAIL  ⑨-a iface 实现者缺位 %s 笔（D 段应恒 0——iface 在册 ≠ 有实现者，硬红不进基线，裁-17）\n' "$ga_d"
    gate_a_fail=1
  fi
  ga_base="tools/ci/gate-a-baseline.txt"
  if [ ! -r "$ga_base" ]; then
    printf 'FAIL  ⑨-a 基线文件不可读: %s（裁定配套(c) fail-closed——低水位基线缺位即红，不放行）\n' "$ga_base"
    gate_a_fail=1
  else
    ga_baseline="$(grep -vE '^[[:space:]]*(#|$)' "$ga_base" | head -1 | tr -d '[:space:]')"
    case "$ga_baseline" in
      ''|*[!0-9]*)
        printf 'FAIL  ⑨-a 基线文件形状坏: %s（首个非注释行非纯整数: "%s"）——fail-closed，不放行\n' "$ga_base" "${ga_baseline:-<空>}"
        gate_a_fail=1
        ;;
      *)
        if [ "$ga_total" -gt "$ga_baseline" ]; then
          printf 'FAIL  ⑨-a 红账回涨: %s > 基线 %s（"红条数单调不增"被破——回涨必有一笔未登记的新红，先归因再动基线）\n' "$ga_total" "$ga_baseline"
          gate_a_fail=1
        elif [ "$ga_total" -lt "$ga_baseline" ]; then
          printf 'PASS  ⑨-a 红账 %s ≤ 基线 %s（**红账下降 %s 笔：须同批下调 tools/ci/gate-a-baseline.txt；转红终点 = 基-39 全覆盖 count=0，届时本门转红、基线文件退役**）\n' \
            "$ga_total" "$ga_baseline" "$((ga_baseline - ga_total))"
        else
          printf 'PASS  ⑨-a 红账 %s = 基线 %s（报告模式：A/C 已知红不置红，转红随基-39 全刀落完）\n' "$ga_total" "$ga_baseline"
        fi
        ;;
    esac
  fi
fi
rm -f "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins"
if [ "$gate_a_fail" -ne 0 ]; then
  fail=1
fi

# —— ⑨-r 留痕不进被丢弃的流（门 r 静态面执法位；全案 §8 门 r / §20 流-1，批N4 件八）。
#     机制定形（件八笔1 07b998d）：supervisor 对子进程 stderr 接 per-unit 日志文件
#     <socket>.log/<unit>.log（append），stdout 仍 null——stdout 是唯一被丢弃的流，
#     故六模块生产码 stdout 留痕零命中；stderr（eprintln! / print(file=sys.stderr)）
#     是声明过的通道，不在禁列。二选一裁定 per-unit 日志文件（上报窄缝否决：观测面
#     不得依赖被观测面总线健康度；§17 同轨判据"一次重启后仍能取到崩溃前的最后 N 行"
#     内存环做不到，文件天然满足；总线连接前的早期留痕只有文件通道可达）。
#     扫面 = 五 Rust 模块 src 全部（act/connect/interact/power/sense）+ brain Python
#     源（六模块生产码；atoms 面另有 ⑦-a 禁词扫面盖着，不在此重复）。brain 原不在
#     门 r "45 处"实测口径内（该口径只数五 Rust 模块），件八扩面同批登记全案 §8
#     门 r 行对表注记。Rust 面词边界 grep（防 eprintln! 子串误伤）；Python 面按 AST
#     判（多行 print 调用按行 grep 会误报起始行），python3 缺失/解析崩 = 门禁失效不放行
#     （⑨-n 扫面前件同一纪律）。
step "⑨-r 留痕不进被丢弃的流（六模块生产码 stdout 留痕零命中，门 r）"
gate_r_fail=0
gr_rs="$(grep -rnE '(^|[^A-Za-z_])(println|print)!\(' \
  modules/rr3-act/src modules/rr3-connect/src modules/rr3-interact/src \
  modules/rr3-power/src modules/rr3-sense/src \
  --include='*.rs' 2>/dev/null || true)"
if [ -n "$gr_rs" ]; then
  gate_r_fail=1
  printf '%s\n' "$gr_rs"
  printf 'FAIL  ⑨-r 模块生产码 stdout 留痕命中（见上行号）——stdout 被 supervisor 接成 Stdio::null()，留痕按设计不可达：改 eprintln!（stderr 落 per-unit 日志 <socket>.log/<unit>.log，门 r 定形批N4 件八）\n'
fi
gr_py="$(python3 - modules/rr3-brain/rr3_brain 2>/dev/null <<'PYEOF'
import ast, pathlib, sys
hits = []
for f in sorted(pathlib.Path(sys.argv[1]).glob("*.py")):
    tree = ast.parse(f.read_text(encoding="utf-8"))
    for node in ast.walk(tree):
        if (isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
                and node.func.id == "print"
                and not any(kw.arg == "file" for kw in node.keywords)):
            hits.append(f"{f}:{node.lineno}")
print("\n".join(hits))
PYEOF
)"
gr_py_rc=$?
if [ "$gr_py_rc" -ne 0 ]; then
  gate_r_fail=1
  printf 'FAIL  ⑨-r Python 扫面前件不成立（python3 缺失或 AST 解析崩）——门禁失效，不放行\n'
elif [ -n "$gr_py" ]; then
  gate_r_fail=1
  printf '%s\n' "$gr_py"
  printf 'FAIL  ⑨-r 模块生产码 stdout print 命中（见上行号）——stdout 被 supervisor 接成 null，留痕按设计不可达：print(..., file=sys.stderr)（stderr 落 per-unit 日志，门 r 定形批N4 件八）\n'
fi
if [ "$gate_r_fail" -ne 0 ]; then
  fail=1
else
  printf 'PASS  ⑨-r 留痕不进被丢弃的流（五 Rust 模块 stdout 宏 + brain Python stdout print 全零命中；stderr 通道 = per-unit 日志 <socket>.log/<unit>.log，append 保留重启历史）\n'
fi

# —— ⑨-z 真值集三方成员判等（门 z 静态面执法位；基-40 / 模块设计索引 §14.7，批N8
#     件一）。病根（自检五腿全绿而机器起不来）：三份"该带哪些文件"清单不同源——
#     装载面 rr3_runtime_truth_set（七 files+一 dir）vs 判据面 RR3_CONTRACT_FILES
#     （曾只四册）vs 打包面手抄。修法定形（件一 b2cc925/bda7fda）：成员名单点生成
#     （profile.rs rr3_truth_set_member_names ← RR3_CONTRACT_BOOKS 字面唯一份），
#     三面共读；成员集相等由判据保证（单测 truth_set_members_and_boot_locations_
#     are_the_same_set 判等实体＋运行时 truth_set 自检逐枚点名），不再靠注释声称。
#     本门四条执法（静态面，逐条点名，任一缺失即红）：
#       1) 单点在位：profile.rs 定义 pub const RR3_CONTRACT_BOOKS（字面唯一份）；
#       2) 判据面共读：contracts.rs 的 RR3_CONTRACT_FILES 定义必须引用该常量
#          （判据面再现字面数组 = 第二份清单 = 红）；
#       3) 打包面共读：rr3-pack 必须调 rrd::profile::rr3_runtime_truth_set 照单
#          收口（打包面自带清单 = 块-15 同病 = 红）；
#       4) 第二份字面零命中：全仓 .rs 产码不得再现四册并列清单字面（单点定义行
#          与 tests/ 判据面豁免——判据钉四册点名是判据不是清单）。
#     运行时半边（非空差逐枚点名）住 rrd 自检 truth_set check 与 boot 记账
#     fail-closed，本门只执法静态面——两半合成 ⑨-z 全貌。
step "⑨-z 真值集三方成员判等（单点在位＋判据面/打包面共读＋第二份字面零命中，门 z）"
gate_z_fail=0
if ! grep -q 'pub const RR3_CONTRACT_BOOKS' base/runtime/rrd/src/profile.rs; then
  gate_z_fail=1
  printf 'FAIL  ⑨-z 契约四册名单单点缺席（base/runtime/rrd/src/profile.rs 须定义 pub const RR3_CONTRACT_BOOKS——字面唯一份，基-40）\n'
fi
if ! grep -q 'pub const RR3_CONTRACT_FILES: \[&str; 4\] = crate::profile::RR3_CONTRACT_BOOKS' base/runtime/rrd/src/contracts.rs; then
  gate_z_fail=1
  printf 'FAIL  ⑨-z 判据面清单未共读单点（base/runtime/rrd/src/contracts.rs 的 RR3_CONTRACT_FILES 须引用 crate::profile::RR3_CONTRACT_BOOKS——自检判据面不得自带第二份字面，基-40）\n'
fi
if ! grep -q 'rrd::profile::rr3_runtime_truth_set(' tools/rr3-pack/src/main.rs; then
  gate_z_fail=1
  printf 'FAIL  ⑨-z 打包面未共读真值点（tools/rr3-pack 须调 rrd::profile::rr3_runtime_truth_set 照单收口——打包面自带清单即块-15 同病，基-25/实-12）\n'
fi
gz_lit="$(grep -rn --include='*.rs' '"topics.toml", "slots.toml", "roles.toml", "devices.toml"' \
  base modules tools 2>/dev/null \
  | grep -v '/tests/' | grep -v 'pub const RR3_CONTRACT_BOOKS' || true)"
if [ -n "$gz_lit" ]; then
  gate_z_fail=1
  printf '%s\n' "$gz_lit"
  printf 'FAIL  ⑨-z 第二份四册清单字面命中（见上行号）——成员名单点在 profile.rs RR3_CONTRACT_BOOKS，消费方一律引用常量，手抄清单即成员集漂移温床（基-40）\n'
fi
if [ "$gate_z_fail" -ne 0 ]; then
  fail=1
else
  printf 'PASS  ⑨-z 真值集三方成员判等（单点 RR3_CONTRACT_BOOKS 在位；判据面/打包面共读；产码第二份字面零命中；成员集判等实体 = profile.rs 单测 truth_set_members_and_boot_locations_are_the_same_set）\n'
fi

# —— ⑨-y 跨平台可编面（base 层 Windows 红 crate 数基线钉数，门 y；裁-19/基-33 /
#    §25.8 #9，批N8 件六）。判据：cargo check --target x86_64-pc-windows-msvc
#    （check 不链接，宿主无需 MSVC）--keep-going 尽量多编，base/runtime 九成员的
#    **红 crate 数**（按 crate 名去重——同 crate lib/bin 双单元红只计一）不得超过
#    tools/ci/gate-win-baseline.txt 的钉数（只降不升）。
#    fail-closed 三查：基线文件缺/坏/首个非注释行非纯整数即红（gate-a 同款）；
#    Windows target 未装即红（附安装命令，**不设 SKIP**——裁-10 同族理由：能跑的
#    机器越少，漂移越没人发现，ci.yml conventions job 同批装 target）；count < 基线
#    PASS 并打印"须同批下调基线"提醒。转绿终点 = 基-33 拆修全落 count=0，届时本门
#    转红、基线退役。
step "⑨-y 跨平台可编面（base 层 Windows 红 crate 数基线钉数，门 y）"
gate_y_fail=0
gy_log="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
if rustup target list --installed 2>/dev/null | grep -q '^x86_64-pc-windows-msvc$'; then
  gy_red=0
  if ! cargo check --locked --manifest-path base/runtime/Cargo.toml \
         --target x86_64-pc-windows-msvc --keep-going >"$gy_log" 2>&1; then
    gy_red="$(grep -oE 'could not compile `[a-z0-9_-]+`' "$gy_log" | sort -u | wc -l | tr -d ' ')"
  fi
  gy_b="$(grep -vE '^[[:space:]]*(#|$)' tools/ci/gate-win-baseline.txt 2>/dev/null | sed -n '1p' | tr -d '[:space:]')"
  case "$gy_b" in
    ''|*[!0-9]*)
      printf 'FAIL  ⑨-y 基线文件形状坏: tools/ci/gate-win-baseline.txt（首个非注释行须为纯整数，实得"%s"）——fail-closed，不放行\n' "${gy_b:-<缺文件或空>}"
      gate_y_fail=1
      ;;
    *)
      if [ "$gy_red" -gt "$gy_b" ]; then
        printf 'FAIL  ⑨-y Windows 红 crate 回涨: %s > 基线 %s（"只降不升"被破——新红 crate 必有一笔未归因，先归因再动基线；红名单见上 cargo 输出）\n' \
          "$gy_red" "$gy_b"
        grep -oE 'could not compile `[a-z0-9_-]+`' "$gy_log" | sort -u | sed 's/^/      红 crate: /'
        gate_y_fail=1
      elif [ "$gy_red" -lt "$gy_b" ]; then
        printf 'PASS  ⑨-y Windows 红 crate %s < 基线 %s（**红账下降 %s：须同批下调 tools/ci/gate-win-baseline.txt**；转绿终点 = 基-33 拆修全落 count=0）\n' \
          "$gy_red" "$gy_b" "$((gy_b - gy_red))"
      else
        printf 'PASS  ⑨-y Windows 红 crate %s = 基线 %s（只降不升在账；唯一红 = 基-33 断点面，拆修随后）\n' \
          "$gy_red" "$gy_b"
      fi
      ;;
  esac
else
  printf 'FAIL  ⑨-y Windows target 未安装（rustup target add x86_64-pc-windows-msvc）——fail-closed 不设 SKIP（能跑的机器越少，漂移越没人发现）\n'
  gate_y_fail=1
fi
rm -f "$gy_log"
if [ "$gate_y_fail" -ne 0 ]; then
  fail=1
fi

# —— ⑩ 门 d 面板覆盖率对账·基线钉数门（当前上限 0）（基-19↔本步；interact I-19，批N4 件三；全案 §8
#     门 d / conventions §2 拟步 ⑩）。判据（字段级分母 + covers 申报制分子）：
#       分母 = 五路字段并集（全部自代码常量/契约表导出——单真值点，⑨-a/⑨-n 先例）：
#         ops/<slot>   ← modules/*/atoms/*/assemble.toml 的 ops 槽；
#         pin/<field>  ← rrd/src/profile.rs RR3_PIN_KEYS（⑨-a 同款 awk，声明数判等）；
#         role/<req>   ← contracts/roles.toml requires 逐项（⑨-a 同款拆词）；
#         domain/<f>   ← rrd/src/profile.rs RR3_DOMAIN_KEYS（声明数判等）；
#         axis/<field> ← rr-hal-api lib.rs RR3_ACT_PROFILE_FIELDS 十四键。
#       分子 = ∪ 每端点 covers 申报（panel/server.rs RR3_PANEL_ROUTES 第三列），
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
gate_d_fail=0
gd_dir="$(mktemp -d)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
gd_ops="$gd_dir/ops"; gd_pin="$gd_dir/pin"; gd_role="$gd_dir/role"; gd_dom="$gd_dir/dom"
gd_axis="$gd_dir/axis"; gd_cov="$gd_dir/cov"; gd_cov_raw="$gd_dir/cov_raw"
gd_sets="$gd_dir/sets"; gd_den="$gd_dir/den"; gd_diff="$gd_dir/diff"
# bash 的 trap 后设覆盖先设：本两行是 ⑨-a trap（tmpout + ga 五文件）的超集，⑨-a 清单
# 必须原样保留在本行内——单独改 ⑨-a 或本步时两处同看。
trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins"; rm -rf "$gd_dir"' EXIT
trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins"; rm -rf "$gd_dir"; exit 130' INT TERM
# —— 分母五路导出
# ops 路 = 件级 assemble.toml ＋ 组合件级 composite.toml（批N4 件五扩面：panel
# 五限额/自检期望/表情节拍七槽申报在组合件级，消费点在组合件面无原子件对应——
# 两来源 slot 形制相同，并集去重后同账）
{ grep -hoE 'slot = "[^"]+"' modules/*/atoms/*/assemble.toml
  grep -hoE 'slot = "[^"]+"' modules/*/composite.toml; } \
  | sed 's/.*slot = "/ops\//;s/"$//' | sort -u > "$gd_ops"
awk '/const RR3_PIN_KEYS/{f=1} f{print} f && /\];/{exit}' base/runtime/rrd/src/profile.rs \
  | grep -oE '"[^"]+"' | tr -d '"' | sed 's/^/pin\//' | sort -u > "$gd_pin"
gd_pin_decl="$(grep -m1 -oE 'RR3_PIN_KEYS: \[&str; [0-9]+\]' base/runtime/rrd/src/profile.rs | grep -oE '[0-9]+\]' | tr -d ']')"
awk '
  $1 == "requires" {
    line = $0
    while (match(line, /"[^"]+"/)) { print substr(line, RSTART+1, RLENGTH-2); line = substr(line, RSTART+RLENGTH) }
  }
' contracts/roles.toml | sed 's/^/role\//' | sort -u > "$gd_role"
awk '/const RR3_DOMAIN_KEYS/{f=1} f{print} f && /\];/{exit}' base/runtime/rrd/src/profile.rs \
  | grep -oE '"[^"]+"' | tr -d '"' | sed 's/^/domain\//' | sort -u > "$gd_dom"
gd_dom_decl="$(grep -m1 -oE 'RR3_DOMAIN_KEYS: \[&str; [0-9]+\]' base/runtime/rrd/src/profile.rs | grep -oE '[0-9]+\]' | tr -d ']')"
awk '/pub const RR3_ACT_PROFILE_FIELDS/{f=1} f{print} f && /^\];/{exit}' base/runtime/rr-hal-api/src/lib.rs \
  | grep -oE 'key: "[^"]+"' | sed 's/key: "/axis\//;s/"$//' | sort -u > "$gd_axis"
# —— covers 原串导出（RR3_PANEL_ROUTES 区段内带斜杠且不以 / 开头的字符串：
#     path 以 / 开头被排除，method 无斜杠不匹配；"rr3-act/..." 类 topic 字面量
#     不住本区段）＋ 词表导出
awk '/pub const RR3_PANEL_ROUTES/{f=1} f{print} f && /^\];/{exit}' \
  modules/rr3-interact/src/panel/server.rs \
  | grep -oE '"[^"/][^"]*/[^"]*"' | tr -d '"' > "$gd_cov_raw"
awk '/pub const RR3_PANEL_COVER_SETS/{f=1} f{print} f && /\];/{exit}' \
  modules/rr3-interact/src/panel/server.rs | grep -oE '"[^"]+"' | tr -d '"' | sort -u > "$gd_sets"
# —— 扫面前件（键集导出打空属门禁失效，fail-closed，⑨-a 同款）
gd_ops_n="$(grep -c . "$gd_ops" || true)"; gd_pin_n="$(grep -c . "$gd_pin" || true)"
gd_role_n="$(grep -c . "$gd_role" || true)"; gd_dom_n="$(grep -c . "$gd_dom" || true)"
gd_axis_n="$(grep -c . "$gd_axis" || true)"; gd_sets_n="$(grep -c . "$gd_sets" || true)"
if [ "$gd_ops_n" -eq 0 ] || [ "$gd_pin_n" -eq 0 ] || [ "$gd_role_n" -eq 0 ] \
   || [ "$gd_dom_n" -eq 0 ] || [ "$gd_axis_n" -eq 0 ] || [ "$gd_sets_n" -eq 0 ] \
   || [ "$gd_pin_decl" -ne "$gd_pin_n" ] || [ "$gd_dom_decl" -ne "$gd_dom_n" ]; then
  printf 'FAIL  ⑩ 扫面前件不成立（ops=%s pin=%s 声明=%s role=%s domain=%s 声明=%s axis=%s 词表=%s）——键集导出打空属门禁失效，不放行\n' \
    "$gd_ops_n" "$gd_pin_n" "${gd_pin_decl:-空}" "$gd_role_n" "$gd_dom_n" "${gd_dom_decl:-空}" "$gd_axis_n" "$gd_sets_n"
  gate_d_fail=1
else
  printf 'PASS  ⑩ 扫面前件（分母五路：ops %s + pin %s + role %s + domain %s + axis %s；covers 词表 %s 集合）\n' \
    "$gd_ops_n" "$gd_pin_n" "$gd_role_n" "$gd_dom_n" "$gd_axis_n" "$gd_sets_n"
  # —— covers 形制校验：合法进分子，乱写硬红（申报面错不是账，⑨-a B 段同款）
  gd_cov_bad=0
  : > "$gd_cov"
  while IFS= read -r c; do
    [ -n "$c" ] || continue
    gd_set="${c%%/*}"; gd_rest="${c#*/}"
    gd_bad=0
    case "$gd_rest" in
      ''|*/*) gd_bad=1 ;;   # 字段段空或多段
    esac
    grep -qxF "$gd_set" "$gd_sets" || gd_bad=1   # 集合不在词表
    if [ "$gd_bad" -ne 0 ]; then
      printf 'X  covers 形制乱写: %s（应为 <RR3_PANEL_COVER_SETS 词表集合>/<单段字段>）\n' "$c"
      gd_cov_bad=$((gd_cov_bad + 1))
    else
      printf '%s\n' "$c" >> "$gd_cov"
    fi
  done < "$gd_cov_raw"
  gd_cov_n="$(grep -c . "$gd_cov" || true)"
  # —— applies 申报面校验（I-20，批N4 件四，全案 §19.3 判据 1）：第四列（生效路径）
  #     非空须在 RR3_PANEL_APPLY_MODES 三态词表内；写口（非 GET）动了可配置集
  #     （covers 非空）则必填——缺失即红。行解析按引号段计：段数 n = 3 +
  #     covers 元素数（method/path/applies 三段打底），解析出怪行 = 表形制坏，
  #     fail-closed 当场红。
  gd_apply_sets="$gd_dir/apply_sets"; gd_apply_rows="$gd_dir/apply_rows"
  awk '/pub const RR3_PANEL_APPLY_MODES/{f=1} f{print} f && /\];/{exit}' \
    modules/rr3-interact/src/panel/server.rs | grep -oE '"[^"]+"' | tr -d '"' | sort -u > "$gd_apply_sets"
  awk '/pub const RR3_PANEL_ROUTES/{f=1;next} f && /^\];/{exit} f {
    n = 0; line = $0
    while (match(line, /"[^"]*"/)) {
      n++; val[n] = substr(line, RSTART+1, RLENGTH-2); line = substr(line, RSTART+RLENGTH)
    }
    if (n >= 3) printf "%s|%s|%d|%s\n", val[1], val[2], n - 3, val[n]
    else printf "MALFORMED|%s\n", $0
  }' modules/rr3-interact/src/panel/server.rs > "$gd_apply_rows"
  gd_apply_sets_n="$(grep -c . "$gd_apply_sets" || true)"
  gd_apply_n="$(grep -c . "$gd_apply_rows" || true)"
  gd_apply_declared=0; gd_apply_bad=0
  if [ "$gd_apply_sets_n" -eq 0 ]; then
    printf 'X  applies 词表打空（RR3_PANEL_APPLY_MODES 读不到属门禁失效，不放行）\n'
    gd_apply_bad=$((gd_apply_bad + 1))
  fi
  while IFS= read -r r; do
    case "$r" in
      MALFORMED*)
        printf 'X  applies 行解析失败（RR3_PANEL_ROUTES 行形制坏）: %s\n' "$r"
        gd_apply_bad=$((gd_apply_bad + 1)); continue ;;
    esac
    gd_m="${r%%|*}"; gd_rest="${r#*|}"
    gd_p="${gd_rest%%|*}"; gd_rest="${gd_rest#*|}"
    gd_cn="${gd_rest%%|*}"; gd_a="${gd_rest#*|}"
    if [ -n "$gd_a" ]; then
      if grep -qxF "$gd_a" "$gd_apply_sets"; then
        gd_apply_declared=$((gd_apply_declared + 1))
      else
        printf 'X  applies 词表外: %s %s -> %s（词表真值 = RR3_PANEL_APPLY_MODES 单真值点）\n' \
          "$gd_m" "$gd_p" "$gd_a"
        gd_apply_bad=$((gd_apply_bad + 1))
      fi
    elif [ "$gd_m" != "GET" ] && [ "$gd_cn" -gt 0 ]; then
      printf 'X  applies 缺失: %s %s 写口动了可配置集（covers %s 项）却未申报生效路径（I-20 缺失即红）\n' \
        "$gd_m" "$gd_p" "$gd_cn"
      gd_apply_bad=$((gd_apply_bad + 1))
    fi
  done < "$gd_apply_rows"
  if [ "$gd_apply_bad" -gt 0 ]; then
    printf 'FAIL  ⑩ applies 申报面 %s 笔坏（词表外/缺失/形制坏，申报面错不是账，硬红不进基线）\n' "$gd_apply_bad"
    gate_d_fail=1
  else
    printf 'PASS  ⑩ applies 申报面（行 %s 全过词表/必填判据，申报 %s 笔，词表 %s 态）\n' \
      "$gd_apply_n" "$gd_apply_declared" "$gd_apply_sets_n"
  fi
  # —— 差集（分子分母各 sort -u 后 comm；分母已有序）
  cat "$gd_ops" "$gd_pin" "$gd_role" "$gd_dom" "$gd_axis" | sort -u > "$gd_den"
  sort -u "$gd_cov" > "$gd_dir/cov_sorted"
  comm -23 "$gd_den" "$gd_dir/cov_sorted" > "$gd_diff"
  gd_den_n="$(grep -c . "$gd_den" || true)"
  gd_red_n="$(grep -c . "$gd_diff" || true)"
  if grep -q . "$gd_diff"; then
    sed 's/^/D  /' "$gd_diff"
  fi
  printf '门 d 数学（全案 §8 / I-19）：分母 %s − covers 申报 %s = 差集（红账）%s\n' \
    "$gd_den_n" "$gd_cov_n" "$gd_red_n"
  if [ "$gd_cov_bad" -gt 0 ]; then
    printf 'FAIL  ⑩ covers 形制乱写 %s 笔（申报面错不是账，硬红不进基线）\n' "$gd_cov_bad"
    gate_d_fail=1
  fi
  # —— 基线双行判据（fail-closed）
  gd_base="tools/ci/gate-d-baseline.txt"
  if [ ! -r "$gd_base" ]; then
    printf 'FAIL  ⑩ 基线文件不可读: %s（裁定配套(c) fail-closed——低水位基线缺位即红，不放行）\n' "$gd_base"
    gate_d_fail=1
  else
    gd_b1="$(grep -vE '^[[:space:]]*(#|$)' "$gd_base" | sed -n '1p' | tr -d '[:space:]')"
    gd_b2="$(grep -vE '^[[:space:]]*(#|$)' "$gd_base" | sed -n '2p' | tr -d '[:space:]')"
    case "$gd_b1$gd_b2" in
      ''|*[!0-9]*)
        printf 'FAIL  ⑩ 基线文件形状坏: %s（前两非注释行须各为纯整数，实得行1="%s" 行2="%s"）——fail-closed，不放行\n' \
          "$gd_base" "${gd_b1:-<空>}" "${gd_b2:-<空>}"
        gate_d_fail=1
        ;;
      *)
        if [ "$gd_red_n" -gt "$gd_b1" ]; then
          printf 'FAIL  ⑩ 红账回涨: %s > 上限 %s（"红条数单调不增"被破——回涨必有一笔未登记的新红，先归因再动基线）\n' \
            "$gd_red_n" "$gd_b1"
          gate_d_fail=1
        elif [ "$gd_red_n" -lt "$gd_b1" ]; then
          printf 'PASS  ⑩ 红账 %s ≤ 上限 %s（**红账下降 %s 笔：须同批下调 %s 行 1；转绿终点 = 差集空，届时本门转红、基线退役**）\n' \
            "$gd_red_n" "$gd_b1" "$((gd_b1 - gd_red_n))" "$gd_base"
        else
          printf 'PASS  ⑩ 红账 %s = 上限 %s（基线钉数门·当前上限 0：建即红如实在账，红项随批N4 件九扩端点逐批转绿）\n' \
            "$gd_red_n" "$gd_b1"
        fi
        if [ "$gd_den_n" -lt "$gd_b2" ]; then
          printf 'FAIL  ⑩ 分母缩水: %s < 下限 %s（I-19 判据字面——红项数不许靠缩分母减少；缩分母 = 集合定义改动，走契约级评审并同批动基线）\n' \
            "$gd_den_n" "$gd_b2"
          gate_d_fail=1
        elif [ "$gd_den_n" -gt "$gd_b2" ]; then
          printf 'PASS  ⑩ 分母 %s ≥ 下限 %s（**分母扩面 %s 笔：须同批上调 %s 行 2**）\n' \
            "$gd_den_n" "$gd_b2" "$((gd_den_n - gd_b2))" "$gd_base"
        else
          printf 'PASS  ⑩ 分母 %s = 下限 %s（字段级分母无缩水）\n' "$gd_den_n" "$gd_b2"
        fi
        ;;
    esac
  fi
fi
rm -rf "$gd_dir"
if [ "$gate_d_fail" -ne 0 ]; then
  fail=1
fi

printf '\n=== 汇总 ===\n'
if [ "$fail" -eq 0 ]; then
  echo "所有规约检查通过"
else
  echo "存在失败项（见上方 FAIL）"
fi
exit "$fail"
