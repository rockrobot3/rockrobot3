# ⑯ 源文件行数门步骤体（git ls-files 的 .rs/.py/.sh；600 硬线 + 500 下一档）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail。
#
# 判据：≥600 = 铁律破坏；≥500 = 下一档目标（只降不升，逐批拆到 0 = <500 行）。
# 基线键 hard_600 / tier_500，缺键或形状坏 fail-closed；扫面打空亦红（防 glob 失效）。

rr3_step_16() {
  lines_base_file="tools/ci/gate-lines-baseline.txt"
  if [ ! -f "$lines_base_file" ]; then
    printf 'FAIL  ⑯ 缺基线文件 %s（fail-closed）\n' "$lines_base_file"
    fail=1
    return
  fi
  base_hard="$(sed -nE 's/^hard_600[[:space:]]*=[[:space:]]*([0-9]+)$/\1/p' "$lines_base_file" | head -1)"
  base_tier="$(sed -nE 's/^tier_500[[:space:]]*=[[:space:]]*([0-9]+)$/\1/p' "$lines_base_file" | head -1)"
  if [ -z "$base_hard" ] || [ -z "$base_tier" ]; then
    printf 'FAIL  ⑯ 基线缺 hard_600/tier_500 整数（fail-closed）\n'
    fail=1
    return
  fi

  scanned=0
  hard_cnt=0
  tier_cnt=0
  hard_list=()
  tier_list=()
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    if ! n="$(wc -l < "$f" 2>/dev/null)"; then
      printf 'FAIL  ⑯ 读取失败 %s（fail-closed）\n' "$f"
      fail=1
      continue
    fi
    scanned=$((scanned + 1))
    if [ "$n" -ge 600 ]; then
      hard_cnt=$((hard_cnt + 1))
      hard_list+=("$n  $f")
    fi
    if [ "$n" -ge 500 ]; then
      tier_cnt=$((tier_cnt + 1))
      tier_list+=("$n  $f")
    fi
  done < <(git -c core.quotepath=false ls-files '*.rs' '*.py' '*.sh')

  if [ "$scanned" -eq 0 ]; then
    printf 'FAIL  ⑯ 扫面打空（git ls-files .rs/.py/.sh 零命中，门禁失效）\n'
    fail=1
    return
  fi

  printf '⑯ 扫面 %s 个源文件：≥600 硬线 %s 个（上限 %s）；≥500 下一档 %s 个（上限 %s）\n' \
    "$scanned" "$hard_cnt" "$base_hard" "$tier_cnt" "$base_tier"

  if [ "$hard_cnt" -gt "$base_hard" ]; then
    printf '%s\n' "${hard_list[@]}" | sort -rn | head -10 | sed 's/^/  硬线超标  /'
    printf 'FAIL  ⑯ ≥600 行文件回涨（%s > %s）：先拆再评审，不许上调基线\n' "$hard_cnt" "$base_hard"
    fail=1
  elif [ "$hard_cnt" -lt "$base_hard" ]; then
    printf 'PASS  ⑯ 硬线超标 %s < 上限 %s（拆完同批下调 %s）\n' "$hard_cnt" "$base_hard" "$lines_base_file"
  else
    printf 'PASS  ⑯ 硬线超标 %s = 上限 %s（只降不升）\n' "$hard_cnt" "$base_hard"
  fi

  if [ "$tier_cnt" -gt "$base_tier" ]; then
    printf '%s\n' "${tier_list[@]}" | sort -rn | head -10 | sed 's/^/  下一档超标  /'
    printf 'FAIL  ⑯ ≥500 行文件回涨（%s > %s）：下一档目标只降不升\n' "$tier_cnt" "$base_tier"
    fail=1
  elif [ "$tier_cnt" -lt "$base_tier" ]; then
    printf 'PASS  ⑯ 下一档 %s < 上限 %s（拆完同批下调 %s）\n' "$tier_cnt" "$base_tier" "$lines_base_file"
  else
    printf 'PASS  ⑯ 下一档 %s = 上限 %s（目标逐批拆到 0 = <500 行）\n' "$tier_cnt" "$base_tier"
  fi
}
