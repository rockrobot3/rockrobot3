# SPDX-License-Identifier: Apache-2.0
# ⑭ 装载门体量基线门步骤体（锚函数块行数 / 读表样板 / 同形函数，基线只降不升）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail 与 prof_src。
# 锚函数读数在 gs_measure，单条判据在 gs_check，基线读取在 gs_pin；编排留在
# rr3_step_14。

# 从基线表取 "<名> = <整数>" 的钉数值（首个匹配）。
gs_pin() {
  sed -n "s/^$1 *= *\([0-9][0-9]*\).*/\1/p" "$gs_base" | head -1
}

# 单条体量判据：实测值对基线钉数；行形状坏 / 回涨即红，下降提示同批下调基线。
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

# 锚函数定位与三项读数。锚缺失即红（装载门被改名/搬走须同批改基线，不许静默）。
gs_measure() {
  gs_fn='pub fn rr3_boot_with_probe('
  gs_file=$(grep -lF "$gs_fn" $prof_src 2>/dev/null | head -1)
  gs_start=$(grep -nF "$gs_fn" "$gs_file" 2>/dev/null | head -1 | cut -d: -f1)
  if [ -z "$gs_start" ]; then
    printf 'FAIL  ⑭ 基线锚函数缺失: 装配单模块中找不到 %s——装载门被改名/搬走须同批改基线，不许静默\n' "$gs_fn"
    gs_fail=1
    return 1
  fi
  gs_end=$(awk -v s="$gs_start" 'NR>=s && /^}/ {print NR; exit}' "$gs_file")
  gs_fn_lines=$((gs_end - gs_start + 1))
  gs_reads=$(awk -v s="$gs_start" -v e="$gs_end" \
    'NR>=s && NR<=e && /read_to_string/' "$gs_file" | wc -l | tr -d ' ')
  # 扫描面 = rr-registry 全树（按内容、不按文件名）：结构拆分后同形小函数可能
  # 住在子模块里，只扫 src/*.rs 会让计数随搬文件"假降"，门也就此失明。
  gs_shapes=$(find base/runtime/rr-registry/src -name '*.rs' -not -path '*/tests/*' -print0 \
    | xargs -0 grep -hoE 'fn rr3_[a-z_0-9]*(from_text|is_empty|contains|check)[a-z_0-9]*\(' \
    | sort -u | wc -l | tr -d ' ')
  return 0
}

rr3_step_14() {
  gs_base=tools/ci/gate-size-baseline.txt
  gs_fail=0
  if [ ! -f "$gs_base" ]; then
    printf 'FAIL  ⑭ 基线文件缺失: %s\n' "$gs_base"
    gs_fail=1
  elif gs_measure; then
    gs_check gate_fn_lines "$gs_fn_lines"
    gs_check read_table_boilerplate "$gs_reads"
    gs_check same_shape_fns "$gs_shapes"
  fi
  if [ "$gs_fail" -ne 0 ]; then
    fail=1
  fi
}
