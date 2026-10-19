# docs/evidence —— 证据号命名空间（容器）

真板对拍/实测证据的唯一住址（brain B-15，批N4 前置；D11 `armed=true` 界面门与
`severe_mv`/`current_ceil_ma` 安全写引用的真值点容器）。

**规则唯一落点 = [conventions.md §6.1](../process/conventions.md)**（本 README 不复制规则，
引用不复制）。要点速览，以 §6.1 为准：

- 文件形制：`docs/evidence/<形态>/<编号>.md`（形态目录名与文件内 `form` 一致）；
- 编号：形态目录内四位零填充递增（0001 起），**一次对拍一份文件，编号不复用**；
- frontmatter 六必填键：`form` / `slot_or_role` / `method` / `conclusion` / `date` / `recorder`
  （形态/格或角色/观测方法/结论/日期/取证人）；
- 引用形制：`<形态>/<编号>`（如 `对拍/0001`）；写面与装载期对出现的引用 fail-closed
  （校验器唯一实现住 `rr-ipc::evidence`）。

## 模板（新证据按此起文件）

```markdown
+++
form = "对拍"
slot_or_role = "power.rail"
method = "万用表实测与面板读数对拍"
conclusion = "severe_mv=3300 与实测一致"
date = "2026-10-19"
recorder = "lei_huang"
+++

（正文：观测原始数据、两腿对拍记录、可复核的命令与读数。）
```

本目录当前零证据文件——证据随 0.0.6/0.0.7 验证窗产出；本命名空间只规定
"结论长什么样、住哪儿、怎么被引用"，不代替验证窗产出任何板上结论。
