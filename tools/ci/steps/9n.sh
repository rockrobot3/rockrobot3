# ⑨-n 注入路径唯一（门 n）。
# 由 check.sh 在启动段 source；函数读写入口的全局 prof_src/fail。

rr3_step_9n() {
  gate_n_fail=0
  keys_file="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  awk '/const RR3_PIN_DERIVED_KEYS/{f=1} f{print} f && /\];/{exit}' $prof_src \
    | grep -oE '"[^"]+"' | tr -d '"' > "$keys_file"
  decl_n="$(grep -h -m1 -oE 'RR3_PIN_DERIVED_KEYS: \[&str; [0-9]+\]' $prof_src | grep -oE '[0-9]+\]' | tr -d ']')"
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
      printf 'FAIL  ⑨-n [hardware] 出现物理键（门 n 稳态判据：物理键一律 [pins] 派生；迁移期判据见 profile/model.rs 门 n 段）\n'
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
}
