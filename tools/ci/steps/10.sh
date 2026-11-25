# SPDX-License-Identifier: Apache-2.0
# ⑩ 门 d 面板覆盖率对账·基线钉数门（门 10 步骤体：分子 covers 申报 + applies
#    生效路径 / 分母字段级五路并集；基线双行 fail-closed）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail 与 ⑨-a 的 ga_* 临时文件
# （trap 覆盖关系见 gd_tmp_files 注）。六个导出段与四段判据各自成函数，编排留在
# rr3_step_10。

# 临时目录与 trap：本两行 trap 是 ⑨-a trap（tmpout + ga 五文件）的超集，⑨-a 清单
# 必须原样保留在本行内——单独改 ⑨-a 或本步时两处同看。
gd_tmp_files() {
  gd_dir="$(mktemp -d)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  gd_ops="$gd_dir/ops"; gd_pin="$gd_dir/pin"; gd_role="$gd_dir/role"; gd_dom="$gd_dir/dom"
  gd_axis="$gd_dir/axis"; gd_cov="$gd_dir/cov"; gd_cov_raw="$gd_dir/cov_raw"
  gd_sets="$gd_dir/sets"; gd_den="$gd_dir/den"; gd_diff="$gd_dir/diff"
  trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins"; rm -rf "$gd_dir"' EXIT
  trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins"; rm -rf "$gd_dir"; exit 130' INT TERM
}

# 分母五路导出。ops 路 = 件级 assemble.toml ＋ 组合件级 composite.toml（批N4 件五
# 扩面：panel 五限额/自检期望/表情节拍七槽申报在组合件级，消费点在组合件面无原
# 子件对应——两来源 slot 形制相同，并集去重后同账）；axis 路定义点可能在 lib.rs
# 或任一契约族子模块（文件拆分不失锚），逐文件取一次。
gd_den_export() {
  local f
  { grep -hoE 'slot = "[^"]+"' modules/*/atoms/*/assemble.toml
    grep -hoE 'slot = "[^"]+"' modules/*/composite.toml; } \
    | sed 's/.*slot = "/ops\//;s/"$//' | sort -u > "$gd_ops"
  awk '/const RR3_PIN_KEYS/{f=1} f{print} f && /\];/{exit}' $prof_src \
    | grep -oE '"[^"]+"' | tr -d '"' | sed 's/^/pin\//' | sort -u > "$gd_pin"
  gd_pin_decl="$(grep -h -m1 -oE 'RR3_PIN_KEYS: \[&str; [0-9]+\]' $prof_src | grep -oE '[0-9]+\]' | tr -d ']')"
  awk '
    $1 == "requires" {
      line = $0
      while (match(line, /"[^"]+"/)) { print substr(line, RSTART+1, RLENGTH-2); line = substr(line, RSTART+RLENGTH) }
    }
  ' contracts/roles.toml | sed 's/^/role\//' | sort -u > "$gd_role"
  awk '/const RR3_DOMAIN_KEYS/{f=1} f{print} f && /\];/{exit}' $prof_src \
    | grep -oE '"[^"]+"' | tr -d '"' | sed 's/^/domain\//' | sort -u > "$gd_dom"
  gd_dom_decl="$(grep -h -m1 -oE 'RR3_DOMAIN_KEYS: \[&str; [0-9]+\]' $prof_src | grep -oE '[0-9]+\]' | tr -d ']')"
  while IFS= read -r f; do
    awk '/pub const RR3_ACT_PROFILE_FIELDS/{f=1} f{print} f && /^\];/{exit}' "$f"
  done < <(find base/runtime/rr-hal-api/src -name '*.rs' -not -path '*/tests/*' | sort) \
    | grep -oE 'key: "[^"]+"' | sed 's/key: "/axis\//;s/"$//' | sort -u > "$gd_axis"
}

# covers 原串导出（两源同一段扫描逻辑复用，批N13 件二扩入板侧）：
#   host 侧 RR3_PANEL_ROUTES 区段 ＋ 板侧 RR3_BOARD_ROUTES 区段
#   （tools/esp32-periph-base/src/httpd.rs，pub(crate) 声明）——区段内带斜杠
#   且不以 / 开头的字符串：path 以 / 开头被排除，method 无斜杠不匹配；
#   "rr3-act/..." 类 topic 字面量不住本区段。板侧首笔两端点 covers 全空
#   （批N13 件一注记：读的是运行态与注入绑定，不在 ⑩ 分母五路）＝本源贡献零行。
gd_cov_export() {
  { awk '/pub const RR3_PANEL_ROUTES/{f=1} f{print} f && /^\];/{exit}' \
      modules/rr3-interact/src/panel/server.rs
    awk '/pub\(crate\) const RR3_BOARD_ROUTES/{f=1} f{print} f && /^\];/{exit}' \
      tools/esp32-periph-base/src/httpd.rs; } \
    | grep -oE '"[^"/][^"]*/[^"]*"' | tr -d '"' > "$gd_cov_raw"
  awk '/pub const RR3_PANEL_COVER_SETS/{f=1} f{print} f && /\];/{exit}' \
    modules/rr3-interact/src/panel/server.rs | grep -oE '"[^"]+"' | tr -d '"' | sort -u > "$gd_sets"
}

# 扫面前件（键集导出打空属门禁失效，fail-closed，⑨-a 同款）。
gd_precheck() {
  gd_ops_n="$(grep -c . "$gd_ops" || true)"; gd_pin_n="$(grep -c . "$gd_pin" || true)"
  gd_role_n="$(grep -c . "$gd_role" || true)"; gd_dom_n="$(grep -c . "$gd_dom" || true)"
  gd_axis_n="$(grep -c . "$gd_axis" || true)"; gd_sets_n="$(grep -c . "$gd_sets" || true)"
  if [ "$gd_ops_n" -eq 0 ] || [ "$gd_pin_n" -eq 0 ] || [ "$gd_role_n" -eq 0 ] \
     || [ "$gd_dom_n" -eq 0 ] || [ "$gd_axis_n" -eq 0 ] || [ "$gd_sets_n" -eq 0 ] \
     || [ "$gd_pin_decl" -ne "$gd_pin_n" ] || [ "$gd_dom_decl" -ne "$gd_dom_n" ]; then
    printf 'FAIL  ⑩ 扫面前件不成立（ops=%s pin=%s 声明=%s role=%s domain=%s 声明=%s axis=%s 词表=%s）——键集导出打空属门禁失效，不放行\n' \
      "$gd_ops_n" "$gd_pin_n" "${gd_pin_decl:-空}" "$gd_role_n" "$gd_dom_n" "${gd_dom_decl:-空}" "$gd_axis_n" "$gd_sets_n"
    gate_d_fail=1
    return 1
  fi
  printf 'PASS  ⑩ 扫面前件（分母五路：ops %s + pin %s + role %s + domain %s + axis %s；covers 词表 %s 集合）\n' \
    "$gd_ops_n" "$gd_pin_n" "$gd_role_n" "$gd_dom_n" "$gd_axis_n" "$gd_sets_n"
  return 0
}

# covers 形制校验：合法进分子，乱写硬红（申报面错不是账，⑨-a B 段同款）。
gd_cov_shape() {
  local c gd_set gd_rest gd_bad
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
}

# applies 申报面导出（I-20，批N4 件四，全案 §19.3 判据 1）：第四列（生效路径）
# 非空须在 RR3_PANEL_APPLY_MODES 三态词表内；写口（非 GET）动了可配置集
# （covers 非空）则必填——缺失即红。行解析按引号段计：段数 n = 3 +
# covers 元素数（method/path/applies 三段打底），解析出怪行 = 表形制坏，
# fail-closed 当场红。
gd_apply_export() {
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
}

# applies 逐行判据：词表外 / 写口缺失 / 形制坏都计坏；零坏 PASS，有坏硬红。
gd_apply_judge() {
  local r gd_m gd_rest gd_p gd_cn gd_a
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
}

# 差集（分子分母各 sort -u 后 comm；分母已有序）。
gd_diff() {
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
}

# covers ⊆ 分母五路并集对拍执法（批N13 件二，申报制语义的另一半）：
#   分子元素越出分母即硬红——申报面校验只挡词表外形制，挡不住"词表内合法
#   形制、字段却不在五路分母"的越界申报；本位反向 comm（分子−分母）补齐。
#   两源共用（panel 与板侧同账同执法：任何一端申报越界字段都过不了门）。
#   批N21 件四旗标订正：原 `comm -13 cov_sorted den` 打的是 col2（分母−分子），
#   与上行差集同集合——红账 0 时两集合同空潜伏至今，红账非零即双名单假报；
#   订正为 col1（分子−分母）＝越界申报，兑现本位登记语义。
gd_oob() {
  comm -23 "$gd_dir/cov_sorted" "$gd_den" > "$gd_dir/cov_oob"
  gd_oob_n="$(grep -c . "$gd_dir/cov_oob" || true)"
  if grep -q . "$gd_dir/cov_oob"; then
    sed 's/^/X  covers 越界（不在分母五路并集）: /' "$gd_dir/cov_oob"
    printf 'FAIL  ⑩ covers 越界 %s 笔（申报制 ⊆ 分母执法，越界申报硬红不进基线）\n' "$gd_oob_n"
    gate_d_fail=1
  fi
}

# covers 形制坏的收口（放在越界点名之后，输出次序与原段一致）。
gd_cov_verdict() {
  if [ "$gd_cov_bad" -gt 0 ]; then
    printf 'FAIL  ⑩ covers 形制乱写 %s 笔（申报面错不是账，硬红不进基线）\n' "$gd_cov_bad"
    gate_d_fail=1
  fi
}

# 基线双行判据（fail-closed）：行 1 = 红账上限，行 2 = 分母下限（I-19 判据字面）。
gd_baseline() {
  local gd_base gd_b1 gd_b2
  gd_base="tools/ci/baseline/gate-d-baseline.txt"
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
}

rr3_step_10() {
  gate_d_fail=0
  gd_tmp_files
  gd_den_export
  gd_cov_export
  if gd_precheck; then
    gd_cov_shape
    gd_apply_export
    gd_apply_judge
    gd_diff
    gd_oob
    gd_cov_verdict
    gd_baseline
  fi
  rm -rf "$gd_dir"
  if [ "$gate_d_fail" -ne 0 ]; then
    fail=1
  fi
}
