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

printf '\n=== 汇总 ===\n'
if [ "$fail" -eq 0 ]; then
  echo "所有规约检查通过"
else
  echo "存在失败项（见上方 FAIL）"
fi
exit "$fail"
