# 上板笔记：RK3566 参考机（Radxa ZERO 3 / 同级核心板）

- **状态**：**待硬件（RK3566 未到货）**。本文件是到货即用清单，不是实测回执；
  真板证据未取得前，任何"已通过"结论都不成立。
- **目标机型**：`profiles/default.toml`（参考机全配）与 `profiles/distro.toml`
  （发行版快路），两者 `chip = "rk3566"`、`tier = "host"`、`hal_backend = "linux"`。
- **平台事实**：`contracts/platform/rk3566.toml` 仍是演练实例，引脚/ADC/互斥
  取值待器件核对；`hardware/default/wiring.toml`、`hardware/distro/wiring.toml`
  同理。
- **关联**：[ADR-0013](../../../../docs/process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md) ·
  [ADR-0015](../../../../docs/process/adr/ADR-0015-板卡策略-rk3566核心板过桥与量产自定制.md) ·
  [partition-layout.md](../../../ota/partition-layout.md) ·
  [buildroot flash-maskrom](../../buildroot/board/radxa-zero3/flash-maskrom.md)

## 0. 到货前静态点验

```bash
bash tools/ci/platform/rk3566-onboard-check.sh          # 数据面齐不齐、是否自相矛盾
bash tools/ci/platform/rk3566-onboard-check.sh --strict # 发布前：轴档案也必须标定
```

静态点验只证明装配数据面自洽，**不替代**真板启动、器件读数与时序证据。

## 1. 镜像/安装二选一

### A. Buildroot 保底轨（出厂镜像）

1. 由 CI `buildroot` 工作流产出的 aarch64 镜像（含 `root=LABEL=rr_a` 的
   `extlinux.conf` 与 `rr3-ota-ctl`）。**2026-11-17 实况**：全量镜像已产出
   （run `44035094772`，artifact `rock3-zero3-image` 265,842,537 B），
   e2fsck/分区标签/rrd/S99rrd/fstab/FIT 内容断言全过；实烧仍待板。
2. 按 [flash-maskrom.md](../../buildroot/board/radxa-zero3/flash-maskrom.md)
   用 rkdeveloptool 进 maskrom 烧录。
3. 串口（默认 1500000 8N1）应出现 U-Boot 与内核日志；登录后确认：

```bash
/usr/bin/rrd --help
/usr/bin/rr3-ctl --help
/usr/bin/rr3-ota-ctl slots
cat /proc/cmdline | tr ' ' '\n' | grep '^root='
```

`root=` 必须是 `LABEL=rr_a` 或 `LABEL=rr_b`；裸设备形态按
[partition-layout.md](../../../ota/partition-layout.md) 的板上核对项处理，不静默放过。

### B. 发行版快路（已有 mainline aarch64 系统）

1. 宿主交叉出包：`bash tools/deploy/pack-arm64.sh`（产物在 `/tmp/rr3-arm64-deploy`）。
2. 板上 `dpkg-deb --build` + `sudo dpkg -i`，或 `tools/deploy/install-bundle.sh`。
3. 确认 unit 带 `--profile`：

```bash
systemctl cat rrd | grep ExecStart
# 期望含 --socket /var/run/rock3-ipc.sock --contracts ... --profile /usr/lib/rockrobot3/profiles/default.toml
```

## 2. 起链与首轮巡检

```bash
sudo systemctl start rrd
journalctl -u rrd -n 80 --no-pager
/usr/bin/rr3-ctl status       # 六模块 running、denied_publish = 0
/usr/bin/rr3-ctl health       # 心跳/看门狗窗口
/usr/bin/rr3-ctl counters     # rrd/boots 与模块计数
/usr/bin/rr3-ctl wiring       # [pins] 派生视图与接线交叉判果
```

失败时按日志首行定位：`拒绝 boot：`（装配/仲裁）、`拒绝启动`（模块缺件/不可执行）、
`装载门`（平台表/接线/能源域）、`Unsupported`（HAL 后端缺件）。

## 3. 真件读数/动作验收（到货后逐项取证）

| 面 | 判据 | 证据落点 |
|---|---|---|
| 舵机 | PWM 中位/两端落角，限位钳位生效 | `servo/state` 流 + 目视 |
| GPIO | LED/继电器电平可控，复位后安全态 | `rr3-ctl counters` + 目视 |
| 电池 | INA219 `0x00=0x399F`，电压/电流符号可信 | 读数 + 万用表对拍 ≤2% |
| 链路 | `net/link` up/ssid/rssi 真值，断链抹除 | 拔网线/关 AP 实录 |
| 能源域 | `armed=false` 直到测点与龄期标定；`armed=true` 必须有证据引用 | `rr3.ctl.wiring` + 证据文件 |
| OTA | `rr3-ota-ctl verify` 通过；未签名包拒 | 本地签名包 + 篡改负例 |
| 长跑 | ≥30min 无挂死、无重启环、计数不倒退 | `rr3-ctl counters` 前后对比 |

## 4. 未闭环项（到货前不得冒领）

1. `hardware/{default,distro}/axis/*.toml` 未标定；当前轴档案缺失只警告。
2. `contracts/platform/rk3566.toml` 的 gpiochip/引脚/ADC/互斥取值待器件核对。
3. `wiring.toml` 的额定/阈值是声明预算位，待万用表与负载实测。
4. M6：U-Boot 回滚链真载体、槽位镜像写入、`fw_env.config` 布局、vfat rename
   断电窗口均未在 RK3566 上验证。
5. Buildroot 实烧/串口 shell/maskrom 回执未取得。

以上任一项未取证，本板只能记"静态点验通过"，不能记"已收口"。
