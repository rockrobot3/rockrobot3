# ⑥ 独立工程测试接线步骤体（find 与 ④ 同口径；0 工程即门禁失效）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail via run_tests。

rr3_step_6() {
  found_mod=0
  while IFS= read -r -d '' manifest; do
    found_mod=1
    run_tests sh -c "cargo test --locked -q --manifest-path '$manifest'"
  done < <(find modules tools base -name Cargo.toml -not -path "*/target/*" \
                      -not -path "base/runtime/*" -not -path "tools/esp32-uart-echo/*" \
                      -not -path "tools/espidf-std-hello/*" -not -path "tools/espidf-sentinel/*" \
                      -not -path "tools/esp32-periph-base/*" \
                      -print0 | sort -z)
  if [ "$found_mod" -eq 0 ]; then
    printf 'FAIL  ⑥ 未发现任何独立 Rust 工程（find 打空属门禁失效，不放行）\n'
    fail=1
  fi
}
