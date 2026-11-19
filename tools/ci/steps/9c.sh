# ⑨-c 电流预算装载拒链（门 c）。
# 由 check.sh 在启动段 source；函数读写入口的全局 prof_src/fail。

rr3_step_9c() {
  run_tests sh -c 'cd base/runtime && cargo test --locked -p rrd --lib demand_budget'

  # —— ⑨-a 基-39 角色覆盖六表对账·执法（门 a 第 ③ 层执法位——2026-11-12 P1 审计件四
  #     起由"报告模式"转执法：红账 21→0 连带基线归零，此后任何新红即 FAIL；全案 §8
  #     门 a / §24.1 第 5 条 + 裁-6，批N2 件六建步、P1 件四转执法）。
  #     四腿键集全部自代码常量/契约表导出（单真值点，不抄第二份；⑨-n 先例）：
  #       slots ← contracts/slots.toml；roles ← contracts/roles.toml；
  #       TRAITS/DESC_KEYS ← rr-registry/src/assemble.rs 两在册表；
  #       PIN_KEYS ← 装配单模块 RR3_PIN_KEYS（profile/model.rs）（声明数/实解析数判等）。
  #     报告三段（裁定配套 (a)：走 stdout 可见面，不走被 Stdio::null() 吞掉的面）：
  #       A 段 = 未覆盖 normal 格（status=normal 且无任何 role.slot 指向）；
  #       B 段 = roles 侧悬空（slot/iface/requires/derives 任一不在对方真值集——**应恒 0**，
  #              悬空外键是错不是账，>0 即硬红、不进基线）；
  #       C 段 = TRAITS 未被 roles.iface 引用差集 ＋ RR3_PIN_KEYS 未被 roles.requires 引用差集；
  #       D 段 = iface 实现者缺位（裁-17：iface 在册 ≠ 有实现者——role.iface 须 ≥1 个
  #              prod 实现（`impl <iface> for` 文本命中，扫 base/modules/tools 产码、
  #              排除 /tests/，口径同 ⑨-z 门；占位后端 Rr3UnavailableSource 是发布面
  #              产码"能力缺失显式回错"，命中算数），否则该角色须显式标 status="planned"
  #              且 planned_until 非空（终点登记）——**应恒 0**，硬红不进基线；
  #              sensor.env/imu 两角色 planned 终点 = D9 器件到货转自动可判）。
  #     门 a 数学（§24.1 第 5 条 + 裁-17）：置红 ＝ A + B + C + D。
  #     机器判据（裁定配套 (c)）：低水位基线文件 tools/ci/gate-a-baseline.txt——
  #       count > 基线即 FAIL（防红账回涨）；文件缺/坏/非整数即 FAIL（fail-closed）；
  #       count < 基线 PASS 并打印"须同批下调基线"提醒。
  #       **2026-11-12 转执法**：基线已降至 0（基-39 转绿收口件落地），故 count>0 即
  #       FAIL；文件保留为低水位锚（0），不再随批下调。
  #     A 段分母口径（P1 件四裁定）：只判 layer=body 的 normal 格；base 内建四族与
  #       cognition 软件四格无 [pins] 角色域，不计入（设计原文"logic 内建格覆盖口径
  #       归转红收口件另裁"即本步）。C 段分母口径同批：Rr3DigitalIn（S-10 未立项的
  #       能力面前置件）与四枚永不进 requires 的 pin 键豁免，理由见 C 段内注。
  #     六表之第 5 表（件 handles[].desc_key × DESC_KEYS）已由 ② parse 层执法；第 6 表
  #     （平台 role→具名构造）归门 a 转红收口件——本步如实不判、不抄第二份。
  #     TRAITS/DESC_KEYS 是 &[&str] 切片无常量 N，提取打空的护栏＝B 段全悬空硬红＋扫面前件四查。
}
