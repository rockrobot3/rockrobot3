# SPDX-License-Identifier: Apache-2.0
# ⑨-a 基-39 角色覆盖六表对账·执法（门 a）。
# 由 check.sh 在启动段 source；函数读写入口的全局 prof_src/fail。
# 键集导出（代码侧/契约侧）与 A/B/C/D 四段执法各自成函数，基线与红账
# 收口在 ga_ledger；编排、临时件清理与 fail 汇总留在 rr3_step_9a。

# 六个导出表 = 六个临时文件；清理 trap 连 check.sh 的 tmpout 一并挂上。
ga_tmp_files() {
  ga_slots="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  ga_roles="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  ga_traits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  ga_desc="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  ga_pins="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  ga_status="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins" "$ga_status"' EXIT
  trap 'rm -f "$tmpout" "$ga_slots" "$ga_roles" "$ga_traits" "$ga_desc" "$ga_pins" "$ga_status"; exit 130' INT TERM
}

# 代码侧键集：装配单 traits/desc 常量与 profile 的 pin 键声明面。
ga_export_code() {
  awk '/pub const RR3_ASSEMBLE_HANDLE_TRAITS/{f=1} f{print} f && /\];/{exit}' \
    base/runtime/rr-registry/src/assemble.rs | grep -oE '"[^"]+"' | tr -d '"' > "$ga_traits"
  awk '/pub const RR3_ASSEMBLE_DESC_KEYS/{f=1} f{print} f && /\];/{exit}' \
    base/runtime/rr-registry/src/assemble.rs | grep -oE '"[^"]+"' | tr -d '"' > "$ga_desc"
  awk '/const RR3_PIN_KEYS/{f=1} f{print} f && /\];/{exit}' \
    $prof_src | grep -oE '"[^"]+"' | tr -d '"' > "$ga_pins"
  decl_pin_n="$(grep -h -m1 -oE 'RR3_PIN_KEYS: \[&str; [0-9]+\]' $prof_src | grep -oE '[0-9]+\]' | tr -d ']')"
  real_pin_n="$(grep -c . "$ga_pins" || true)"
}

# 契约侧键集：slots 的 name+status 两轴逐格（⑧-a 同款 awk 口径）；roles 的
# name/slot/iface/requires/derives 逐条；planned 登记面三列（缺省键 = 空串）。
ga_export_contracts() {
  awk '
    /^\[\[slot\]\]/        { in_s = 1; nm = ""; ly = "" }
    in_s && $1 == "name"   { v = $3; gsub(/[",]/, "", v); nm = v }
    in_s && $1 == "layer"  { v = $3; gsub(/[",]/, "", v); ly = v }
    in_s && $1 == "status" { v = $3; gsub(/[",]/, "", v); print nm "\t" v "\t" ly; in_s = 0 }
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
  awk '
    function flush() { if (nm != "") printf "%s\t%s\t%s\n", nm, st, pu }
    /^\[\[role\]\]/       { flush(); nm = ""; st = ""; pu = ""; next }
    $1 == "name"          { v = $3; gsub(/[",]/, "", v); nm = v }
    $1 == "status"        { v = $3; gsub(/[",]/, "", v); st = v }
    $1 == "planned_until" { v = $3; gsub(/[",]/, "", v); pu = v }
    END { flush() }
  ' contracts/roles.toml > "$ga_status"
}

# 扫面前件：任一键集导出打空即门禁失效，不放行（fail-closed）；
# 前件成立才把 A/B/C 计数清零，后续四段据此累加。
ga_precheck() {
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
    return 1
  fi
  printf 'PASS  ⑨-a 扫面前件（slots %s 格/normal %s × roles %s × traits %s × desc_keys %s × pin_keys %s，全部自契约表/代码常量导出）\n' \
    "$ga_slot_n" "$ga_normal_n" "$ga_roles_n" "$ga_traits_n" "$ga_desc_n" "$real_pin_n"
  ga_a=0; ga_b=0; ga_c=0
  return 0
}

# slot 名单单源导出（循环外一次 fork；打空即红——键集导出打空属门禁失效，
# fail-closed 同扫面前件）。判定用 herestring 同源变量：不得写
# `printf '%s\n' "$(cut ...)" | grep -qxF`——command substitution 在管道左
# 子 shell 内延迟写入，grep -q 会竞态提前退而误判悬空（实-15：完整门禁下间歇
# B=1/2 假红，write error 与红笔一一对应，⑨-a 段独立跑恒绿）。
ga_slot_export() {
  ga_slot_ids="$(cut -f1 "$ga_slots")"
  if [ -z "$ga_slot_ids" ]; then
    printf 'FAIL  ⑨-a slot 名单导出打空——键集导出打空属门禁失效，不放行\n'
    gate_a_fail=1
  fi
}

# B 段：roles 侧悬空（应恒 0；悬空外键任何时候都是错，硬红不进基线）。
ga_dangling() {
  local r_nm r_sl r_if r_rq r_dv k
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
}

# A 段：未覆盖 normal 格（P1 件四口径：只判 layer=body 的格）。
# base 层四族（bus.*/net.online/power.battery|rail|budget）由基座内建承载、
# cognition 层四格是纯软件格——两类按 roles.toml 头注"logic 内建格不进 [pins]"
# 本就没有引脚角色，计入红账会让门 a 永远红（设计原文：logic 内建格覆盖口径
# 归转红收口件另裁，本步即该裁定的落点）。body 层 normal 格 8 个必须全有角色。
ga_uncovered() {
  local s_nm s_st s_ly ga_covered
  ga_covered="$(awk -F'\t' '{print $2}' "$ga_roles" | sort -u)"
  while IFS=$'\t' read -r s_nm s_st s_ly; do
    [ -n "$s_nm" ] || continue
    [ "$s_ly" = "body" ] || continue
    if [ "$s_st" = "normal" ] && ! printf '%s\n' "$ga_covered" | grep -qxF "$s_nm"; then
      printf 'A  normal 格未覆盖: %s（无任何 role.slot 指向）\n' "$s_nm"
      ga_a=$((ga_a + 1))
    fi
  done < "$ga_slots"
}

# C 段：trait 差集 ＋ pin 键差集（未被 roles 引用侧）。
# C 段分母口径（P1 件四裁定，逐条理由在案——不是"把红的藏起来"，是这两类
# 成员按设计不可能被 roles 引用）：
#   trait 侧 Rr3DigitalIn：能力面前置件（模块索引 L-12 / sense S-10"数字输入面"
#     未立项），今天零格位消费者、零 prod 实现——它不是"漏覆盖"，是"还没有格"，
#     计入即永久红；S-10 立项时删本豁免并同步补格—角色。
#   pin 键侧四键永不进 requires：role 是 pin 指向角色的**自指键**（不能由角色
#     要求自己）、instance 只对 per_instance 角色必填、direction 由独立的方
#     向域判据校验、mux_group 明文不归 requires（profile_checks.rs 注记：互斥组
#     由平台 conflicts 表承载）。
ga_unused() {
  local t k ga_trait_exempt ga_pin_exempt ga_used_if ga_used_rq
  ga_trait_exempt="Rr3DigitalIn"
  ga_pin_exempt="role instance direction mux_group"
  ga_used_if="$(awk -F'\t' '{print $3}' "$ga_roles" | sort -u)"
  ga_used_rq="$(awk -F'\t' '{print $4}' "$ga_roles" | tr ' ' '\n' | sort -u)"
  while IFS= read -r t; do
    [ -n "$t" ] || continue
    printf '%s\n' " $ga_trait_exempt " | grep -qF " $t " && continue
    if ! printf '%s\n' "$ga_used_if" | grep -qxF "$t"; then
      printf 'C  trait 未被 roles.iface 引用: %s\n' "$t"
      ga_c=$((ga_c + 1))
    fi
  done < "$ga_traits"
  while IFS= read -r k; do
    [ -n "$k" ] || continue
    printf '%s\n' " $ga_pin_exempt " | grep -qF " $k " && continue
    if ! printf '%s\n' "$ga_used_rq" | grep -qxF "$k"; then
      printf 'C  pin 键未被 roles.requires 引用: %s\n' "$k"
      ga_c=$((ga_c + 1))
    fi
  done < "$ga_pins"
}

# D 段：iface 实现者缺位（裁-17：iface 在册 ≠ 有实现者，应恒 0；硬红不进基线）。
# 先求"零实现 iface"名单（去重一次扫面，循环内零 fork），再逐 role 判豁免：
# status="planned" 且 planned_until 非空 = 豁免成立（终点登记在案）；否则逐角色点名硬红。
ga_dead_iface() {
  local r_nm r_sl r_if r_rq r_dv r_st r_pu s_nm s_st s_pu ga_dead_if
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
}

# 红账收口：四段合计对基线（低水位、只降不升）判红；B/D 段应恒 0，单独硬红。
ga_ledger() {
  local ga_base ga_baseline
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
          printf 'PASS  ⑨-a 红账 %s = 基线 %s（执法态：转绿收口后本门对任何新红即 FAIL）\n' "$ga_total" "$ga_baseline"
        fi
        ;;
    esac
  fi
}

rr3_step_9a() {
  gate_a_fail=0
  ga_tmp_files
  ga_export_code
  ga_export_contracts
  if ga_precheck; then
    ga_slot_export
    ga_dangling
    ga_uncovered
    ga_unused
    ga_dead_iface
    ga_ledger
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
  #     扫面 = 六 Rust 模块 src 全部（act/brain/connect/interact/power/sense；批N32
  #     件四起 brain 是 Rust 并入本扫面，其 Python 扫描段随退役删除；atoms 面另有
  #     ⑦-a 禁词扫面盖着，不在此重复）。Rust 面词边界 grep（防 eprintln! 子串误伤）。
}
