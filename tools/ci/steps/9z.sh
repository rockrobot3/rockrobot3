# ⑨-z 真值集三方成员判等（门 z）。
# 由 check.sh 在启动段 source；函数读写入口的全局 prof_src/fail。

rr3_step_9z() {
  gate_z_fail=0
  if ! printf '%s\n' "$prof_src" | xargs grep -q 'pub const RR3_CONTRACT_BOOKS'; then
    gate_z_fail=1
    printf 'FAIL  ⑨-z 契约四册名单单点缺席（装配单模块须定义 pub const RR3_CONTRACT_BOOKS（profile/boot.rs）——字面唯一份，基-40）\n'
  fi
  if ! grep -q 'pub const RR3_CONTRACT_FILES: \[&str; 4\] = crate::profile::RR3_CONTRACT_BOOKS' base/runtime/rrd/src/contracts.rs; then
    gate_z_fail=1
    printf 'FAIL  ⑨-z 判据面清单未共读单点（base/runtime/rrd/src/contracts.rs 的 RR3_CONTRACT_FILES 须引用 crate::profile::RR3_CONTRACT_BOOKS——自检判据面不得自带第二份字面，基-40）\n'
  fi
  if ! grep -rq 'rrd::profile::rr3_truth_set(' tools/rr3-pack/src; then
    gate_z_fail=1
    printf 'FAIL  ⑨-z 打包面未共读真值点（tools/rr3-pack 须调 rrd::profile::rr3_truth_set 照单收口——打包面自带清单即块-15 同病，基-25/实-12）\n'
  fi
  gz_lit="$(grep -rn --include='*.rs' '"topics.toml", "slots.toml", "roles.toml", "devices.toml"' \
    base modules tools 2>/dev/null \
    | grep -v '/tests/' | grep -v 'pub const RR3_CONTRACT_BOOKS' || true)"
  if [ -n "$gz_lit" ]; then
    gate_z_fail=1
    printf '%s\n' "$gz_lit"
    printf 'FAIL  ⑨-z 第二份四册清单字面命中（见上行号）——成员名单点在 profile.rs RR3_CONTRACT_BOOKS，消费方一律引用常量，手抄清单即成员集漂移温床（基-40）\n'
  fi
  if [ "$gate_z_fail" -ne 0 ]; then
    fail=1
  else
    printf 'PASS  ⑨-z 真值集三方成员判等（单点 RR3_CONTRACT_BOOKS 在位；判据面/打包面共读；产码第二份字面零命中；成员集判等实体 = profile.rs 单测 truth_set_members_and_boot_locations_are_the_same_set）\n'
  fi

  # —— ⑨-y 跨平台可编面（base 层 Windows 红 crate 数基线钉数，门 y；裁-19/基-33 /
  #    §25.8 #9，批N8 件六）。判据：cargo check --target x86_64-pc-windows-msvc
  #    （check 不链接，宿主无需 MSVC）--keep-going 尽量多编，base/runtime 十成员的
  #    **红 crate 数**（按 crate 名去重——同 crate lib/bin 双单元红只计一）不得超过
  #    tools/ci/gate-win-baseline.txt 的钉数（只降不升）。
  #    fail-closed 三查：基线文件缺/坏/首个非注释行非纯整数即红（gate-a 同款）；
  #    Windows target 未装即红（附安装命令，**不设 SKIP**——裁-10 同族理由：能跑的
  #    机器越少，漂移越没人发现，ci.yml conventions job 同批装 target）；count < 基线
  #    PASS 并打印"须同批下调基线"提醒。转绿终点 = 基-33 拆修全落 count=0，届时本门
  #    转红、基线退役。
}
