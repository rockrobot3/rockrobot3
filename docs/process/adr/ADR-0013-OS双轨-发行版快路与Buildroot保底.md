# ADR-0013：OS 双轨 —— 发行版快路 + Buildroot 保底，同一 payload 双矩阵执法

- **状态**：已接受（OS 层设计裁定；`base/os/{distro,buildroot}` 双目录、payload 打包器与 CI 双矩阵为规划落点，实体归后续批次，实况见 status.md）
- **日期**：2026-10-01
- **关联**：[ADR-0001](ADR-0001-平台选型-linux66-buildroot-rk3566.md)（主路线由本文修订，Buildroot 降为保底轨）· [ADR-0012](ADR-0012-能力格坐标系与装配单.md)（硬件探测与装配链）· [ADR-0007](ADR-0007-控制面分层与模块级升级.md)（模块级升级/双轨升级）· [architecture.md](../../architecture/architecture.md)

## 背景

ADR-0001 把 Buildroot 定为主路线。当时的目标是"构建链完全自持、镜像可控"，但泛化
目标（快速部署到不同板卡、不同机器人）后来被完整提出（ADR-0012 背景同源），Buildroot
作为唯一路线暴露出结构性矛盾：

1. **每板一个 defconfig**——换一块板 = 重走一遍 M1 的 DTS/defconfig/补丁回移全流程，
   "快速部署"退化为"每板数周"；
2. **上游白拿为零**——发行版（Debian/Armbian/Alpine）已经为几百块板维护着内核、
   U-Boot、Wi-Fi 固件，Buildroot 路线把这份维护全部自扛；
3. **板卡支持面与机器人价值无关**——用户买到的是机器人能力，不是我们的内核补丁。

同时用户三条决策约束本 ADR：**①自研 OS 镜像可以不用，但自有的是保底（双轨）；
②快速部署不是不开发，而是最大程度减少开发量；③快速部署不代表要舍弃稳定性**。

## 决策

### 裁定 1：三层模型 —— 板卡支持白拿上游，rock3 只做 payload

```
Layer 2  模块层   rr3-sense / rr3-act / … / rr3-brain
                 分发格式 = OCI 容器（裁定 4），rrd 编排
Layer 1  底座 payload（rock3 本体 = 本项目真正的交付物）
                 rrd + rr-hal + contracts + 安装器（deb/静态二进制集）
Layer 0  板卡支持（白拿上游，不自研）
                 Debian / Armbian / Alpine 已有的内核、U-Boot、固件
                 + Buildroot 保底轨（裁定 3）
```

- **rock3 的交付物是 Layer 1 payload**：一个可以在"任何能跑 Linux 6.6 mainline 的
  aarch64 环境"上安装的自足安装单元。板卡差异是 Layer 0 的事，谁的板谁支持
  （瑞芯微经 Debian/Armbian 主线、全志经 mainline、其他 SoC 经各自发行版支持面）。
- payload 内部不含"怎么起 Linux"的知识——那属于 Layer 0；payload 只含
  "Linux 起来之后的一切"。

### 裁定 2：快路 —— 发行版 + payload 安装包

- **快路定义**：一块已能跑标准发行版（Debian / Armbian / Alpine 任一）的板，
  装 payload、写装配单（ADR-0012）、`rrd` 起链路——部署完成。
- payload 以 **deb 包为主形态**（Alpine 环境经 apk 拆装或静态二进制集兜底，
  形态细节随实体落盘登记，不阻塞本裁定）：systemd 单元、`/var/run/rock3-ipc.sock`
  路径约定、A/B 槽位 CLI（`rr3-ota-ctl`）随包安装。
- **开发量最小化不等于零开发**：payload 本体（rrd/rr-hal/contracts/安装器）是
  rock3 既有的正款开发，不因双轨增加；双轨真正新增的只是**打包器**与 CI 矩阵，
  每板数周的 DTS/defconfig 工作量被移出 rock3 的账本。

### 裁定 3：保底 —— Buildroot 降为量产冻结轨，资产不删

- **Buildroot 不废**：M1 已建成的 `base/os/`（defconfig、genimage.cfg、补丁回移、
  maskrom 自烧录文档）全部保留，角色从"主路线"改为"保底轨"。
- **保底轨的定位**：量产冻结镜像（无发行版变数、逐字节可复现）、发行版支持不到的
  板的兜底、以及 maskrom 自烧录（硬性目标）的最深控制层。
- **目录**：`base/os/` 拆为 `base/os/buildroot/`（现内容平移）与 `base/os/distro/`
  （快路打包脚本与说明）。同一 payload、两个轨道各出一份安装产物。

### 裁定 4：OCI = 模块打包/分发格式，不是编排层

- **采纳**：模块以 OCI 镜像分发（`rr3-sense`、`rr3-brain` 等打为 OCI 镜像，
  经 registry 拉取/换版本）。brain（Python）受益最大——依赖冻结进镜像，
  "Python 环境漂移"这一类问题随格式消解。
- **拒绝作为编排层**：rrd 保持模块生命周期的**唯一所有者**（ADR-0007 模块级升级、
  ADR-0009 boot 仲裁的既有裁定不变）。容器运行时（如 runc）只是 rrd 手里的
  一种 spawn 后端——与"直接 spawn 二进制"同列，rrd 调它起进程、喂狗、记账、
  重启，**容器不接管生命周期**。编排权进容器运行时即 F⊗F（ADR-0012 裁定 4
  同款越界）。
- 推论：OCI 轨下 A/B OTA（整机镜像，保底轨）与模块级升级（OCI 镜像换 tag，
  ADR-0007 双轨升级）各管各层，互不替代。

### 裁定 5：CI 双矩阵执法 —— 同一 payload，两轨都绿才算绿

- **双矩阵**：CI 里同一 payload 产物在两个轨道各验证一遍——快路（发行版容器 +
  deb 安装 + rrd 冒烟）与保底（Buildroot 镜像构建 + 镜像内冒烟）。
- **稳定性不因快路打折**：快路引入的发行版变数（glibc 版本、systemd 差异）由
  双矩阵暴露在 CI 层，不带病上板。fast 只是不重做 Layer 0，payload 自身的
  行为在两轨下必须逐字节同源（契约测试与冒烟用例同一套）。
- 矩阵落地方式（runner 配置、容器镜像）随实体落盘登记，本文档只裁
  "双轨必须同批验证"这一执法语义。

### 裁定 6：硬件探测归装配链，OS 轨无关

板卡差异在运行期由 ADR-0012 裁定 3 的装配链消化：rrd 按 profile + 硬件探测
（`/proc/device-tree`、`/sys`、ALSA 枚举）匹配格位，缺格 Degraded。**该链路
对快路/保底两轨完全同构**——payload 不感知自己装在哪条轨上，OS 轨差异止步
于 Layer 0。

## 明确不做（本轮裁定边界）

1. **Yocto**：ADR-0001 否决维持有效，双轨不含第三轨。
2. **自建发行版**：不 fork Debian/Armbian；快路只做"往标准发行版上装 payload"。
3. **容器编排运行时**（K8s/Docker Compose/systemd-container-as-orchestrator）：
   rrd 唯一生命周期所有者，见裁定 4。
4. **跨机分发/多机编排**：单机单 rrd（ADR-0009 不做清单第 1 条同款边界），
   传输层 trait（ADR-0008）为将来分布式预留，本轮不裁。
5. **payload 的 apk/rpm 形态**：随实体落盘评估，本轮只冻结 deb 主形态与
   "静态二进制集兜底"原则。

## 备选方案否决理由

- **Buildroot 维持唯一主路线（现状）**：裁定 1 背景三条结构性矛盾无解——每板
  defconfig、上游白拿为零、板卡支持面与机器人价值无关；泛化目标（适配其他板卡）
  在此路线下永远要以"数周/板"计。
- **发行版唯一路线、删 Buildroot**：违背决策约束 ①——自有的镜像是保底：量产
  需要逐字节可复现，maskrom 自烧录需要最深控制层，M1 资产不删也不该删。
- **OCI 兼作编排层（容器即模块运行时管理器）**：生命周期所有权分裂——rrd 的
  喂狗/记账/重启/升级全部要经容器运行时中转，故障域叠加，且与 ADR-0007/0009
  既有裁定冲突。
- **双轨两套 payload（各写一份安装逻辑）**：两份真值必然漂移；双轨的全部意义
  就是"同一 payload、不同 Layer 0"。

## 后果

- 正面：换板成本从"数周/板（defconfig+DTS+补丁回移）"降为"装发行版 + 装 payload
  + 写装配单"；上游内核/固件维护白拿；Buildroot 保底保住量产可复现与 maskrom
  自烧录；brain 依赖漂移随 OCI 格式消解。
- 负面：CI 从单矩阵变双矩阵（构建时长与维护面增加）；快路接受发行版变数，
  需双矩阵执法兜底；`base/os/` 目录拆分连带文档/链接一批变更（随实体落盘同批做）。
- 修订记录：**ADR-0001 的"Buildroot 主路线"裁定由本文修订**——内核选择
  （Linux 6.6 mainline）、SoC 首选（RK3566）、maskrom 自烧录硬性目标全部不变，
  仅构建系统主次对调。
- 落点登记：`base/os/{distro,buildroot}` 目录拆分、payload 打包器、CI 双矩阵、
  OCI 分发的 registry 选型均**未随本 ADR 落盘**，归后续批次（实况唯一落在
  status.md）；实体不得与本裁定冲突。
