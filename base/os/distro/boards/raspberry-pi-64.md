# 实测笔记：树莓派 64-bit（Pi 4B / Pi 5 · Raspberry Pi OS Bookworm）

- **状态**：步骤已立（批C）；§6 实测回填区待批D 板上演练后填写
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

## 6. 实测回填（批D 填写，未测项留空不预填）

| 项 | 实测 |
|---|---|
| 部署日期 / 板型 | （批D） |
| OS 与内核版本 | `uname -a`： |
| deb 出包与安装 | `dpkg -i` 退出码 / 版本： |
| boot 巡检 | spawn 五模块 / 缺格点名 / denied： |
| rrctl 三查 | status / health / counters 摘录： |
| 重启恢复 | reboot 后 status 回显： |
| 偏离与备注 | （无偏离则写"无"） |
