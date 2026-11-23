# SPDX-License-Identifier: Apache-2.0
# ④b 独立工程 clippy 步骤体（排除口径与 ④ 完全一致，esp 条件工程归专用门）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail via run。

rr3_step_4b() {
  while IFS= read -r -d '' manifest; do
    run sh -c "cargo clippy --locked -q --all-targets --manifest-path '$manifest' -- -D warnings"
  done < <(find modules tools base/ota -name Cargo.toml -not -path "*/target/*" \
                      -not -path "base/runtime/*" -not -path "tools/esp32-uart-echo/*" \
                      -not -path "tools/espidf-std-hello/*" -not -path "tools/espidf-sentinel/*" \
                      -not -path "tools/esp32-periph-base/*" \
                      -print0 | sort -z)
}
