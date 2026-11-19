# ⑨-r 留痕不进被丢弃的流（门 r）。
# 由 check.sh 在启动段 source；函数读写入口的全局 prof_src/fail。

rr3_step_9r() {
  gate_r_fail=0
  gr_rs="$(grep -rnE '(^|[^A-Za-z_])(println|print)!\(' \
    modules/rr3-act/src modules/rr3-brain/src modules/rr3-connect/src \
    modules/rr3-interact/src modules/rr3-power/src modules/rr3-sense/src \
    --include='*.rs' 2>/dev/null || true)"
  if [ -n "$gr_rs" ]; then
    gate_r_fail=1
    printf '%s\n' "$gr_rs"
    printf 'FAIL  ⑨-r 模块生产码 stdout 留痕命中（见上行号）——stdout 被 supervisor 接成 Stdio::null()，留痕按设计不可达：改 eprintln!（stderr 落 per-unit 日志 <socket>.log/<unit>.log，门 r 定形批N4 件八）\n'
  fi
  if [ "$gate_r_fail" -ne 0 ]; then
    fail=1
  else
    printf 'PASS  ⑨-r 留痕不进被丢弃的流（六 Rust 模块 stdout 宏全零命中；stderr 通道 = per-unit 日志 <socket>.log/<unit>.log，append 保留重启历史）\n'
  fi

  # —— ⑨-z 真值集三方成员判等（门 z 静态面执法位；基-40 / 模块设计索引 §14.7，批N8
  #     件一）。病根（自检五腿全绿而机器起不来）：三份"该带哪些文件"清单不同源——
  #     装载面 rr3_truth_set（七 files+一 dir）vs 判据面 RR3_CONTRACT_FILES
  #     （曾只四册）vs 打包面手抄。修法定形（件一 b2cc925/bda7fda）：成员名单点生成
  #     （profile.rs rr3_truth_set_member_names ← RR3_CONTRACT_BOOKS 字面唯一份），
  #     三面共读；成员集相等由判据保证（单测 truth_set_members_and_boot_locations_
  #     are_the_same_set 判等实体＋运行时 truth_set 自检逐枚点名），不再靠注释声称。
  #     本门四条执法（静态面，逐条点名，任一缺失即红）：
  #       1) 单点在位：装配单模块（profile/boot.rs）定义 pub const RR3_CONTRACT_BOOKS（字面唯一份）；
  #       2) 判据面共读：contracts.rs 的 RR3_CONTRACT_FILES 定义必须引用该常量
  #          （判据面再现字面数组 = 第二份清单 = 红）；
  #       3) 打包面共读：rr3-pack（src/ 全树，含拆分后的 pack/ 子模块）必须调 rrd::profile::rr3_truth_set 照单
  #          收口（打包面自带清单 = 块-15 同病 = 红）；
  #       4) 第二份字面零命中：全仓 .rs 产码不得再现四册并列清单字面（单点定义行
  #          与 tests/ 判据面豁免——判据钉四册点名是判据不是清单）。
  #     运行时半边（非空差逐枚点名）住 rrd 自检 truth_set check 与 boot 记账
  #     fail-closed，本门只执法静态面——两半合成 ⑨-z 全貌。
}
