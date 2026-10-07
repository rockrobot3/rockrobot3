# 实测笔记：树莓派 64-bit（Pi 4B / Pi 5 · Raspberry Pi OS Bookworm）

- **状态**：已实测收口（批D 2026-10-05 六条判据全过，§6 已回填；批F1 2026-10-06
  四项验证过，§8 已回填；socket 权限修复敞口见 §7）
- **板卡**：Pi 4B / Pi 5（0.0.6 方案 Q2 裁定：手头已有、型号不锁——payload 对
  Layer 0 无感正是验证目标）
- **OS**：Raspberry Pi OS Bookworm 64-bit（Debian 12 系，与 CI distro-smoke 的
  `rust:bookworm` 同系 glibc；systemd 在位；自带 dpkg-deb）
- **关联**：[0.0.6 方案](../../../../docs/process/dev/0.0.6版本洛克三号树莓派验证方案.md)（§5.2 验收判据 / §5.3 已知边界）·
  [ADR-0013](../../../../docs/process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md)（快路轨：发行版现成镜像 + payload 层安装）·
  [distro README](../README.md)（boards/ 一板一文件纪律）

本板验证的命题：**同一 payload、不改一行产品代码，装进 Layer 0 完全不同的板**
（RK3566 → BCM，跨 SoC 厂商）——跨平台快速部署的实测证明。

## 0. 镜像与准备

1. Raspberry Pi Imager 烧写 **Raspberry Pi OS Lite (64-bit)**（Bookworm；
   Lite 版无桌面，payload 不需要 GUI）。烧写时在 Imager 高级设置里预配：
   主机名、SSH 开启、用户名密码、Wi-Fi（无有线时）。
2. 板首次启动后 `ssh <user>@<主机名>` 可达即备妥。**不需要装 Rust、不需要装
   任何编译工具**——产物在宿主交叉编译，板上只做安装。

## 1. 宿主出包（macOS 开发机）

```bash
bash tools/deploy/pack-arm64.sh
# 产物：/tmp/rr3-arm64-deploy/rockrobot3_0.0.6_arm64.deb.stage（staging 布局树）
# 脚本内含 aarch64 静态 ELF 体检与 entry 装机绝对化断言，绿了才往下走
```

交叉口径：`aarch64-unknown-linux-musl` 静态 + rust-lld（零外部工具链；产物全
静态，板上零依赖——ADR-0013 裁定 2"静态二进制集"精神，登记见 conventions §2
0.0.6 批B）。

## 2. 传板真装

```bash
scp -r /tmp/rr3-arm64-deploy/rockrobot3_0.0.6_arm64.deb.stage <user>@<板>:~/stage
ssh <user>@<板>
cd ~ && dpkg-deb --build stage ./rockrobot3_0.0.6_arm64.deb   # 板上出包（dpkg-deb 自带）
sudo dpkg -i ./rockrobot3_0.0.6_arm64.deb                      # postinst 只 enable 不 start
```

装机位（与 `rrd.service` 同一组路径）：`/usr/bin/{rrd,rr3-ctl}` +
`/usr/lib/rockrobot3/{profiles,contracts,modules}` + `/usr/lib/systemd/system/rrd.service`。

## 3. 起链与巡检

```bash
sudo systemctl start rrd
journalctl -u rrd -n 50 --no-pager     # 或 /usr/bin/rr3-ctl logs
rr3-ctl status                          # spawn 计数 / denied_publish = 0
rr3-ctl health                          # 各模块心跳
rr3-ctl counters                        # rrd/boots 等真值键
sudo reboot                             # 判据 6：重启自恢复（systemd enable 已由 postinst 做）
```

## 4. 验收判据（0.0.6 方案 §5.2，批D 逐条核对）

1. `dpkg -i` 成功，装机位断言同 distro-smoke ⑤；
2. `systemctl start rrd` → 五模块 spawn 齐一张日志 + `拒绝启动 rr3-brain：缺格
   cognition.dialog` 恰一条 + 0 拒绝 boot；
3. `rr3-ctl status/health/counters` 实查回显正常；
4. `systemctl enable` + `reboot` → 重启后链路自恢复；
5. **全程零产品代码改动**（只新增出包脚本与本文档）——跨平台判据本体；
6. §6 实测回填完成。

## 5. 已知边界（不装不知道）

- **probe 仍 sim 后端**：`rr3_probe_default` 未接线 linux 探测（0.0.2 D2 预留），
  缺格判定只由装配单缺 binding 表达——本板 boot 全绿**不构成**"真探测通过"证据。
- **`RR3_HAL_BACKEND` 板上不设**：走平台默认 sim（musl 交叉产物不带 alsa
  feature）；显式设 `linux` 得 `Unsupported` 显式回错、模块探活退出属诚实行为
  不是缺陷。
- **真硬件批次归属**：ALSA 采集 / NetworkManager / 电池计的板上实装是独立批次
  （M5 板上段）；本板运行时各模块均为 sim 后端，观测到的音频/电池/链路数据是
  模拟源，不是真传感器。
- **`rr3-brain` 批D 时不在包内**：0.0.2 快路 payload 边界的诚实表达（boot 日志
  缺格点名同批是断言点，不是故障）；**批F1 起随 deb 装机、cognition 四格绑定进
  boot**（六模块全 running 实测见 §8）。

## 6. 实测回填（批D 2026-10-05 实测）

| 项 | 实测 |
|---|---|
| 部署日期 / 板型 | 2026-10-05 / 树莓派（aarch64，内核 `rpi-v8`，Pi 4B/5 手头板） |
| OS 与内核版本 | **Debian 13 (trixie)**（比 §0 预期 Bookworm 新一个大版本）；`6.12.47+rpt-rpi-v8`——musl 静态产物零 glibc 依赖，跨版本无影响 |
| deb 出包与安装 | 宿主 `pack-arm64.sh` staging（23 文件 / 12MB）→ 传板 → 板上 `dpkg-deb --build` + `dpkg -i` 成功，版本 0.0.6，postinst 自动 enable（multi-user.target.wants symlink） |
| boot 巡检 | 五模块全 `running`（sense/power/act/connect/interact），`denied_publish = 0`；缺格点名 **四条**（`cognition.dialog/asr/llm/tts`——§5.2 原预期"dialog 恰一条"改为四格全点名，faces 保留 brain 声明故四格全缺，语义一致）+ brain manifest 读取失败一条（包内无 brain，诚实边界双现） |
| rrctl 三查 | status/health：五模块 running、心跳 1000/2000ms 全在窗口内；counters：`rrd/boots = 1` |
| 重启恢复 | `sudo reboot` 后 rrd 自启 active，五模块全新 running（task 重编号），denied_publish = 0 |
| 偏离与备注 | boots 缺陷已随 0.0.6 批E' 修复并**真板复验通过**（unit 注 `RR3_DATA_DIR` + postinst 建目录：升级安装 restart 后 `/var/lib/rockrobot3/counters.json` 落盘 `{"boots":1}`，reboot 自恢复后 `{"boots":2}` 跨重启累积，`rr3-ctl counters` 回显 `rrd/boots = 2`）；socket 权限现状补录见 §7。巡检用户 `pi`（Imager 预配），主机名 `spharx` |

**验收判据 §4 六条：全过**（判据 3 经 sudo 路径；判据 2 缺格点名条数按上表口径修正）。

## 7. 已知现状：ctl socket 权限（R6 补录，0.0.6 批E'）

- **现状**：`/var/run/rock3-ipc.sock = srwxr-xr-x root:root`——rrd 以 root 跑
  （systemd 系统服务），bind() 继承 root umask `022`，other 无 w 位；UDS connect
  需要写权限，故非 root 用户的 `rr3-ctl` 连接报 Permission denied。
- **运维口径（修复前）**：板上巡检走 `sudo rr3-ctl …`（root 可连）；本板批D 六判据
  即按此完成。
- **影响面**：仅运维面（rr3-ctl / 直连总线的调试客户端）；rrd ↔ 模块链路不受影响
  （模块由 rrd spawn、经 `RR3_BUS_PATH` 以 root 同权连接）。
- **归属**：修复（rrd bind 后 fchmod 放开组写，或 service 声明运行时目录权限，
  配套 unit 加 `SupplementaryGroups` / 专用组）归后续独立小批（阶段二收尾件）；
  本节只补录现状不修复（0.0.5 方案 R6 裁定）。
- **修复状态（2026-10-07 批F5b，ADR-0010 修订 1）**：已实装"socket 建后
  chgrp/chmod 属主组"路线——rrd bind 后 `chmod 0o660` + `chgrp rockrobot3`
  （postinst 幂等建组，运维用户装机现场 `sudo usermod -aG rockrobot3 pi` 入组；
  组不在位留痕跳过不拒启）。**真机复验留板上线**：非 root `rr3-ctl status`
  直连不再 Permission denied，随判据 14/15 运维链验收同跑；复验通过前板上
  巡检仍按 `sudo rr3-ctl …` 口径。

## 8. 实测回填：批F1 板上验证（2026-10-06）

| 项 | 实测 |
|---|---|
| 板联网 | 房间 WiFi（NetworkManager keyfile 首启注入：initramfs 前置 cpio 段 + `rdinit=/wifi-hook.sh` 写 keyfile 后 `exec /init` 交还正常启动——USB RNDIS/CDC 路线在 macOS 端驱动废 + 串口无 getty，均判死弃用）；bootfs 注入脚手架**已于 2026-10-07 拆除并复验**：config/cmdline 还原备份、initramfs 剥钩子段（cmp 一致）→ 冷启动**无脚手架自主联网**（WXYYBG wlan0 activated，keyfile 持久化生效）+ 六模块 running + boots 15→16 累积 |
| 验证① counters/重启回归 | 含 brain 新 deb 装机 → counters 跨 rrd 重启自增（11→14）→ `sudo reboot` 后 `{"boots":15}` **跨整机重启累积**（批E' 修复健康复验）；`rrd.service` enabled，冷启动六模块全新 running |
| 验证② brain 随 deb 进 boot | `profiles/distro.toml` cognition 四格绑定 + payload 补 pkg 树后：六模块全 `running`（`rr3-brain` task=7，`python3 -m rr3_brain` 真进程，shim `RR3_BRAIN_CONFIG`/`PYTHONPATH` 双装机绝对注入）；`denied_publish = 0`；reboot 冷启动复验同绿 |
| 验证③ build-onboard.sh 实测 | 全链 exit 0：①板上依赖门（apt `rustc 1.85.1` ≥1.82 门过 + `libasound2-dev` 1.2.14 + pkg-config）→ ②rsync 源码同步 → ③`cargo fetch --locked`（crates.io 直连，sense/interact 各自 manifest）→ ④板上 release 构建 `--features alsa` 两件（1m58s）→ ⑤产物归位 payload → ⑥动态 ELF aarch64 + ldd 含 libasound 断言。音频件换装验收仍归批F5a（洞 M，修订版 6 拆分），本批只证构建能力 |
| 验证④ 麦克风 | **USB 麦阵列未采购**（R22 修订版 6 定案：PS3 Eye 或同级 ≥2 麦多通道 UAC 阵列；I2S HAT 存档备选）——真麦采集/DoA/换装验收按 R14 口径诚实标注：sense 板上运行仍是 sim 音源，多通道下混 mono 的真件验收归批F4/F5a |
| 真板暴露缺陷（同批结清） | ①`cp -R src dst`（dst 不存在）BSD 下摊平包树——brain pkg 装机层级丢一截，两组装脚本改"dst 已存在 + 尾斜杠"；②shim 缺 `RR3_BRAIN_CONFIG` 注入——装机布局多一层 `pkg/`，brain config 回退按"包上一级"指到虚位，rr3-pack shim 补 config 位注入 + 单测同步；③build-onboard.sh ③ 段在仓库根 `cargo fetch`——分立 workspace 无根 manifest，改对两个构建 manifest 各自 fetch；④rr-hal `linux.rs` `errno()` 按 Option 解构（alsa-gated 代码从未被宿主编译踩过，R13 musl×alsa 互斥），改裸 i32 |
| 环境备注 | 板上 apt 源 deb.debian.org 直连 ≈57KB/s 且 HTTP method 空转卡死、TUNA pool 403，切 USTC 后正常；rustup 官方通道同慢不可用，apt rustc 1.85.1 达板上构建门 |

**批F1 四项验证：全过**（③ 的构建门依赖板上装 rustc/libasound2-dev，与 §0 "板上
不装工具链"的批D 口径不同——那是纯安装轨；板上构建轨的依赖门由脚本 ① 点名）。

## 9. 实测回填：批F4 板上功能链闭环演练（2026-10-07）

**演练载体**：`rr3-sim --chain`（批F4 新增模式，方案 §5.2 判据 10 sim 档）——六真
进程全链（rrd boot spawn rr3-sense + 外部 rr3-interact/rr3-brain/rr3-act + 演练自带
总线观察者 rr3-obs），mock 云（R4）下无人工干预跑通"采集→意图→判向→转向→表情"
一轮以上。前置：板上 debug 构建六件（rrd / rr3-ctl / rr3-sim / act / sense /
interact；**板上首次构建 rrd**，aarch64 原生约数分钟），系统实例（§8 deb 装机六模块
running）不停机并存——演练用独立 socket，仅面板端口需让位（见缺陷②）。

**回执摘录（判据 10 全要素）**：

```
=== 总线取证（判向 → 转向） ===
doa=-60°/left（注入 -60° ±2° 容差内）→ servo=45°（词表左档）
=== 全链留痕（进程内日志） ===
brain: 对话轮完成: trace=1 user='向左看' reply='（mock 回复）向左看' tts_pcm_ms=960 played_ms=720 actions=1
interact: tts: 12 字符播了 720ms（talk 挂起 idle）（表情机 talk 自动挂起，sim 档无显示件）
act: intent: trace=1 servo angle=45，state#1 已发
=== rr3-ctl status 巡检 ===
六模块全 running（sense/obs/interact/act/brain）+ denied_publish = 0
```

**真板暴露缺陷（同批结清）**：

1. **rr-hal sim burst 门控多通道折半**（产品缺陷）：门控按交错样点序号 `idx/rate`
   算偶秒——双通道下 1s 音频 = 32000 交错样点，窗口折半成 500ms 有声/500ms 静音，
   短于 sense VAD `silence_tail_ms=700ms` → **offset 永不触发**（演练 60s 只见一次
   onset，brain 起不了轮）。单测只钉过 mono（ch=1 时帧序号 == 交错序号，碰巧正确）、
   e2e 只验过 sine+DOA，多通道 burst 首次上板即暴露。修：门控改按**帧序号**
   `idx/声道数/rate`（mono 逐点不变，golden 向量安全）+ 双通道 burst 回归测试钉住。
2. **演练实例面板端口冲突**：板上 deb 系统实例的 rr3-interact 占缺省 127.0.0.1:8080，
   演练实例面板绑定失败拖垮全链（--brain 演练同款中招，CI 上无系统实例故未暴露）。
   修：演练统一注入 `RR3_PANEL_ADDR=127.0.0.1:18080`（--brain/--chain 共享常量）。

**R9 档位诚实标注**：本演练全链 sim/mock（采集=sim burst 注入口、意图=mock 云、
判向=sim 方位注入、转向=sim 舵机、表情=sim 状态机）——**采购前档，不充当真件收口
证据**；真件档（真麦 DoA / 真舵机 / MAX7219）随采购到位后批验收。
