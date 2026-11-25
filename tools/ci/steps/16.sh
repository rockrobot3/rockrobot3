# SPDX-License-Identifier: Apache-2.0
# ⑯ 源文件行数门步骤体（git ls-files 的 .rs/.py/.sh；500 行硬线）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail。
#
# 判据：≥500 = 铁律破坏（2026-11-22 硬线由 600 收紧到 500，存量已清零）。
# 基线键 hard_500，缺键或形状坏 fail-closed；扫面打空亦红（防 glob 失效）。

rr3_step_16() {
  lines_base_file="tools/ci/baseline/gate-lines-baseline.txt"
  if [ ! -f "$lines_base_file" ]; then
    printf 'FAIL  ⑯ 缺基线文件 %s（fail-closed）\n' "$lines_base_file"
    fail=1
    return
  fi
  base_hard="$(sed -nE 's/^hard_500[[:space:]]*=[[:space:]]*([0-9]+)$/\1/p' "$lines_base_file" | head -1)"
  if [ -z "$base_hard" ]; then
    printf 'FAIL  ⑯ 基线缺 hard_500 整数（fail-closed）\n'
    fail=1
    return
  fi

  scanned=0
  hard_cnt=0
  hard_list=()
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    if ! n="$(awk 'END { print NR }' "$f" 2>/dev/null)"; then
      printf 'FAIL  ⑯ 读取失败 %s（fail-closed）\n' "$f"
      fail=1
      continue
    fi
    scanned=$((scanned + 1))
    if [ "$n" -ge 500 ]; then
      hard_cnt=$((hard_cnt + 1))
      hard_list+=("$n  $f")
    fi
  done < <(git -c core.quotepath=false ls-files '*.rs' '*.py' '*.sh')

  if [ "$scanned" -eq 0 ]; then
    printf 'FAIL  ⑯ 扫面打空（git ls-files .rs/.py/.sh 零命中，门禁失效）\n'
    fail=1
    return
  fi

  printf '⑯ 扫面 %s 个源文件：≥500 硬线 %s 个（上限 %s）\n' \
    "$scanned" "$hard_cnt" "$base_hard"

  if [ "$hard_cnt" -gt "$base_hard" ]; then
    printf '%s\n' "${hard_list[@]}" | sort -rn | head -10 | sed 's/^/  硬线超标  /'
    printf 'FAIL  ⑯ ≥500 行文件回涨（%s > %s）：先拆再评审，不许上调基线\n' "$hard_cnt" "$base_hard"
    fail=1
  elif [ "$hard_cnt" -lt "$base_hard" ]; then
    printf 'PASS  ⑯ 硬线超标 %s < 上限 %s（拆完同批下调 %s）\n' "$hard_cnt" "$base_hard" "$lines_base_file"
  else
    printf 'PASS  ⑯ 硬线超标 %s = 上限 %s（铁律：每个源文件 <500 行）\n' "$hard_cnt" "$base_hard"
  fi
}
