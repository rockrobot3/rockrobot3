# base/os/distro —— 发行版快路（ADR-0013 裁定 3）

OS 双轨的**快路**：不自己构建 OS，用现成发行版（Armbian / Debian / 发行版镜像）
直接启动，rock3 以 **payload 层**形态安装上去——rrd、rr3-\* 模块二进制、systemd
单元、配置与 profile，全部按"包"交付，不碰内核与引导链。

与 `../buildroot/`（保底轨，见其 patches/README.md）的关系：

| | distro 快路 | buildroot 保底 |
|---|---|---|
| OS 来源 | 发行版现成镜像 | 全量 make 自建 |
| 自烧录 | 随发行版（SD / USB / 网络启动） | maskrom 不拆机（rkdeveloptool） |
| 定位 | 开发与功能收口先行 | M1 终态：可复现、可出厂 |

**本目录要交出的文件**（有没有落、算哪一档属实况，唯一落点
[`docs/status.md`](../../../docs/status.md)，本文件不记进度）：

- `payload/` —— payload 打包器与打包脚本：把 `base/runtime` 交叉产物
  （rrd、rr3-ctl、`rr3-modules/<name>/{bin,manifest.toml}`）与 systemd 单元
  打成 .deb（或 OCI image，分发裁定见 ADR-0013）
- `units/` —— systemd 单元（rrd.service 等）
- `boards/` —— 各发行版在各板卡上的已验证笔记（镜像来源、内核版本、
  ALSA/UART 可用性），一板一文件

模块层与 OS 层的边界语义见 [ADR-0013](../../../docs/process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md)；
机型选择（哪些格位、哪个 profile）不落在本层——那是 `profiles/`（ADR-0012 装配单）的活。
