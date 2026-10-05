# 实测笔记：树莓派 64-bit（Pi 4B / Pi 5 · Raspberry Pi OS Bookworm）

- **状态**：已实测收口（批D 2026-10-05，六条判据全过，§6 已回填；socket 权限
  修复敞口见 §6 备注）
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
- **`rr3-brain` 不在包内**：distro.toml 认知层四格缺位是快路 payload 边界的
  诚实表达（boot 日志缺格点名同批是断言点，不是故障）。

## 6. 实测回填（批D 2026-10-05 实测）

| 项 | 实测 |
|---|---|
| 部署日期 / 板型 | 2026-10-05 / 树莓派（aarch64，内核 `rpi-v8`，Pi 4B/5 手头板） |
| OS 与内核版本 | **Debian 13 (trixie)**（比 §0 预期 Bookworm 新一个大版本）；`6.12.47+rpt-rpi-v8`——musl 静态产物零 glibc 依赖，跨版本无影响 |
| deb 出包与安装 | 宿主 `pack-arm64.sh` staging（23 文件 / 12MB）→ 传板 → 板上 `dpkg-deb --build` + `dpkg -i` 成功，版本 0.0.6，postinst 自动 enable（multi-user.target.wants symlink） |
| boot 巡检 | 五模块全 `running`（sense/power/act/connect/interact），`denied_publish = 0`；缺格点名 **四条**（`cognition.dialog/asr/llm/tts`——§5.2 原预期"dialog 恰一条"改为四格全点名，faces 保留 brain 声明故四格全缺，语义一致）+ brain manifest 读取失败一条（包内无 brain，诚实边界双现） |
| rrctl 三查 | status/health：五模块 running、心跳 1000/2000ms 全在窗口内；counters：`rrd/boots = 1` |
| 重启恢复 | `sudo reboot` 后 rrd 自启 active，五模块全新 running（task 重编号），denied_publish = 0 |
| 偏离与备注 | boots 缺陷已随 0.0.6 批E' 修复（unit 注 `RR3_DATA_DIR` + postinst 建目录）；socket 权限现状补录见 §7。巡检用户 `pi`（Imager 预配），主机名 `spharx` |

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
