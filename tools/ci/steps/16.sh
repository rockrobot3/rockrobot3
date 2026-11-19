# ⑯ 源文件行数门步骤体（git ls-files 全量 .rs；超标文件数按基线只降不升）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail。

rr3_step_16() {
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
}
