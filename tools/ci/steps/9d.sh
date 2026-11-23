# SPDX-License-Identifier: Apache-2.0
# ⑨-d 生成物一致性步骤体（rr3-codegen --check：真值点 → 生成物 diff 为空；
# 三份 = 板侧 topics_gen.rs / axis_gen.rs + 探针 envelope_gen.py）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail。

rr3_step_9d() {
  if codegen_out="$(cd tools/rr3-codegen && cargo run --locked -q -- --check 2>&1)"; then
    printf '%s\n' "$codegen_out"
    printf 'PASS  ⑨-d 生成物一致（topics_gen.rs / axis_gen.rs / envelope_gen.py 与真值点同步）\n'
  else
    printf '%s\n' "$codegen_out"
    printf 'FAIL  ⑨-d 生成物漂移（topics_gen.rs / axis_gen.rs / envelope_gen.py 之一与真值点不一致；在仓根跑 cd tools/rr3-codegen && cargo run 重生成并审阅）\n'
    fail=1
  fi
}
