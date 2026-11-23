# SPDX-License-Identifier: Apache-2.0
# ⑨-e 行程真值单点步骤体（门 g：旧行程键名与板侧旧行程常量名全仓零命中；
#  docs/target/.embuild 与含字面量的本文件自身豁免）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail。

rr3_step_9e() {
  local self
  self="$(basename "${BASH_SOURCE[0]}")"
  travel_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  grep -rn --exclude-dir=.git --exclude-dir=target --exclude-dir=docs --exclude-dir=.embuild \
       --exclude="$self" \
       -e 'angle_limit_deg' -e 'AXIS_CENTER_DEG' -e 'PAN_LIMIT_DEG' -e 'TILT_LIMIT_DEG' \
       . > "$travel_hits" 2>/dev/null
  if [ -s "$travel_hits" ]; then
    cat "$travel_hits"
    printf 'FAIL  ⑨-e 行程字面零命中被击穿（见上行号）——行程唯一真值点 = hardware/<形态>/axis/*.toml 的 center/lo/hi，出现即读者回流：撤字面量改引注入/生成表\n'
    fail=1
  else
    printf 'PASS  ⑨-e 行程真值单点（旧行程键名与板侧旧行程常量名全仓零命中，读者仅档案→装配注入/生成表两路）\n'
  fi
  rm -f "$travel_hits"
}
