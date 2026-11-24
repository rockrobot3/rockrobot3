# SPDX-License-Identifier: Apache-2.0
# ⑦ §13 静态断言（门 ⑦ 步骤体：⑦-a 零策略载荷 / ⑦-b F⊗F / ⑦-c 五件套 /
#    ⑦-d 入口体量 / ⑦-e 门禁清单自指判等；全 fail-closed，逐条点名后整步 FAIL）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail，并调用入口的共享助手 mod_lang。
# 结构：rr3_step_7 只做编排（建立扫描面唯一真值 mech_surfaces、按序调五门、收口置 fail）；
#   各门体在 rr3_7a_policy / rr3_7b_ff / rr3_7c_five（→ 7c_atoms + 7c_composites）/
#   rr3_7d_entry / rr3_7e_selfref（→ 7e_names + 7e_docs + 7e_xtensa）里，逐门一件。

rr3_step_7() {
  static_fail=0
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
  rr3_7a_policy
  rr3_7b_ff
  rr3_7c_five
  rr3_7d_entry
  rr3_7e_selfref
  if [ "$static_fail" -ne 0 ]; then
    fail=1
  fi
}

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
rr3_7a_policy() {
  local f policy_hits
  policy_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
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
  rm -f "$policy_hits"
}

# ⑦-b F⊗F 非法 + 禁薄胶水存在性：composite.toml [assemble].atoms 的每个成员必须解析到
#     本模块 atoms/<名>/atom.toml——⊗ 链只能装原子件，把组合件/面当被装配方（F⊗F/C⊗C）
#     或写不存在的名即红（§13：面间只 ⊙）。atoms 行缺失/形状畸形同红（fail-closed）。
rr3_7b_ff() {
  local c mod_dir mod line members name
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
}

# ⑦-c 五件套齐全（§13 计数断言，承 ②b mkmod 产物形状）：原子件与组合件实体分别计数，
#     全过后统一放行。
rr3_7c_five() {
  rr3_7c_atoms
  rr3_7c_composites
  [ "$static_fail" -eq 0 ] && printf 'PASS  ⑦-c 五件套齐全（全部原子件与组合件实体逐件计数过）\n'
}

# ⑦-c 原子件计数：Rust 五件 = atom.toml + <名>.rs（入口）+ mech.rs（实现）+
#     assemble.toml（装配清单）+ private.rs（唯一私有头）；Python 件为两件式
#     （atom.toml + assemble.toml，机制本体 .py 原位——conventions §2 批C⑤ 在案，
#     属登记豁免非 SKIP）。件形由 module.toml `lang` 数据位选（不再按零件名 `if` 猜）。
rr3_7c_atoms() {
  local d mod name lang five f
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
}

# ⑦-c 组合件实体计数：rust 组合件必在 src/main.rs，运行机制实体 src/composite.rs 与
#     拆分后 src/dispatch.rs 至少其一（过渡期双态可判）；Python 组合件 = 包目录
#     （模块名破折号转下划线，naming.md §3 同源）的 __main__.py + composite.py。
rr3_7c_composites() {
  local c mod_dir mod lang alt req pkg f alt_ok
  for c in modules/*/composite.toml; do
    mod_dir="$(dirname "$c")"; mod="$(basename "$mod_dir")"
    lang="$(mod_lang "$mod_dir/module.toml")"
    alt=""
    case "$lang" in
      # rust 组合件实体（批N26 件二订正，0.0.8 W-A 批1 起）：装配入口 src/main.rs 必在；
      # 运行机制实体 src/composite.rs（五件套②）与拆分后 src/dispatch.rs（0.0.8 方案 §4
      # 各批目标树）至少其一——过渡期未拆模块持 composite.rs、已拆模块持 dispatch.rs，双态可判。
      rust)   req="src/main.rs"; alt="src/composite.rs src/dispatch.rs" ;;
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
    # 运行机制实体至少其一（批N26 件二，0.0.8 W-A 批1 起：composite.rs / dispatch.rs；
    # alt 非空才判——python 组合件两件全必在，req 已覆盖）
    if [ -n "$alt" ]; then
      alt_ok=0
      for f in $alt; do [ -f "$mod_dir/$f" ] && alt_ok=1; done
      if [ "$alt_ok" -eq 0 ]; then
        printf 'FAIL  ⑦-c 五件套(组合件): %s 缺运行机制实体（%s 至少其一）\n' "$mod" "$alt"
        static_fail=1
      fi
    fi
  done
}

# ⑦-d 入口体量（§13：装配入口 ≤80 行；批E 起 fail-closed——六入口现况 16–18 行）。
#     入口路径按 module.toml `lang` 数据位推导（L-2：此前硬编码 modules/rr3-brain/…）。
rr3_7d_entry() {
  local mt mod_dir mod lang e lines pkg
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
}

# ⑦-e 门禁清单自指判等（0.0.5 批M MC-10 L-4，architecture §13"门禁清单同步"行）：
#     代码里的扫描面真值（mech_surfaces）与两处文档抄写、xtensa 门目标集三方对齐，
#     逐名点名后统一放行。
rr3_7e_selfref() {
  rr3_7e_names
  rr3_7e_docs
  rr3_7e_xtensa
  [ "$static_fail" -eq 0 ] && printf 'PASS  ⑦-e 门禁清单自指判等（扫面名集 A 逐名在 conventions §2 与 tools/ci/README.md ⑦ 行齐 + xtensa 门目标集 ⊆ A）\n'
}

# ⑦-e 扫面名归一：从 mech_surfaces 归一出面名集 A（`base/runtime/<crate>/…` → `<crate>`；
#     `modules/*/atoms/…` → `atoms`）；无归属规则或归一出空即红（fail-closed，不猜）。
#     方向单向：代码是唯一真值，文档向代码看齐，门不由代码迁就文档。
rr3_7e_names() {
  local s n
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
}

# ⑦-e 文档两处逐名核对：conventions §2 ⑦ 行与 tools/ci/README.md ⑦ 行须列全面名集 A；
#     任一行读不到即门禁失效红，缺名逐条点名。
rr3_7e_docs() {
  local conv_row ci_row n
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
}

# ⑦-e xtensa 门对齐：rr-core-xtensa-check.sh 的 `-p rr-*` 目标集非空且 ⊆ A
#     （机制核 crate 面与静态扫面同步）；打空属门禁失效。
rr3_7e_xtensa() {
  local xtensa_targets t
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
}
