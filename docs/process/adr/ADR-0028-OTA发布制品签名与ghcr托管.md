# ADR-0028：发布制品的签名与 ghcr 托管

- **状态**：已接受（设计裁定；流水线接线随 Buildroot 全量绿回执同批施工）
- **日期**：2026-11-17
- **关联**：[ADR-0026](ADR-0026-OTA镜像签名与验签链.md)（信任模型与 manifest 文法）·
  [ADR-0020](ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)（版本边界）·
  [ADR-0013](ADR-0013-OS双轨-发行版快路与Buildroot保底.md)（Buildroot 出厂轨）·
  [genimage.cfg](../../../base/os/buildroot/board/radxa-zero3/genimage.cfg)（产物集合）·
  [partition-layout.md](../../../base/ota/partition-layout.md)（槽位与写入边界）

## 背景

ADR-0026 已把"制品可签、可验、可拒"做成主机面单点（`rr-ota::sign` +
`rr3-ota-ctl` 九子命令），但发布面仍有三处缺口：

1. **产物不是发布件**：Buildroot 全量构建的整盘镜像与各分区 ext4 只进
   Actions artifact（保留 14 天、需登录、无版本约束），不能承载"正式发布"；
2. **签名的真实载体不存在**：M6 写槽链要求"先验签、后写"，但发布面从未产出一份
   带 manifest 与签名的 rootfs 制品，验签链没有消费对象；
3. **信任根暴露面未约束**：签名若在分支构建上发生，任何能改分支的人都能调用
   私钥；发布桶若允许同名覆盖，已发布件会被静默替换且无法审计。

本 ADR 只裁定"发布件怎么签、放哪里、什么条件下必须失败"，不改变 ADR-0026
的 manifest 文法与验签顺序。

## 决策

### 裁定 1：发布签名只在 `v*` 标签上发生

- 触发面固定为 `push: tags: ["v*"]`；`main`/`dev/**`/PR/`workflow_dispatch`
  **一律不签名**，诊断性全量构建只产出未签名 artifact。
- 版本号 = 标签去掉前缀 `v`；标签必须是 `v<semver>`（`0.0.16` 形态）。
  解析失败即失败，不猜版本、不从 `Cargo.toml` 取第二份版本。
- 同一标签只允许发布一次（裁定 5 的不可变约束）。

### 裁定 2：签名对象 = `rr_a.ext4` 的规范副本

- 签名对象是 **rootfs 分区镜像** `rr_a.ext4`（出厂时 `rr_a`/`rr_b` 同源；
  槽位标签的唯一事实源在 `rr-ota` 常量，genimage 只是物理对应物）。
- 发布前复制为规范名 `rr3-rootfs-rk3566-<version>.img`；manifest 的 `image`
  字段只记这个单一路径分量，`bytes`/`sha256` 由 `rr3-ota-ctl manifest` 计算。
- manifest 的固定字段取值：`kind=rootfs`、`platform=rk3566`、
  `board=radxa-zero3`、`version=<版本>`；不放"任意平台通用"包。
- **不签整盘镜像** `rock3-zero3.img`：整盘包含引导裸区与 GPT，从 offset 0 写入
  会破坏引导链；整盘只作为出厂烧录件托管（裁定 5），不进入 OTA 验签链。
- 以下任一情况即失败：`rr_a.ext4` 缺失/为空、规范名已存在且内容不同、
  标签非 semver、平台/板型不符。

### 裁定 3：密钥载体与 fail-closed

- 私钥来自仓库 Secret `RR3_OTA_SIGNING_KEY`，格式 = 32 字节种子的 64 位十六进制
  （ADR-0026 裁定 5）；公钥由 `keypub` 派生，`key_id` = 公钥 SHA-256 前 8 字节。
- **缺失、空值、非 64 位十六进制一律 job 失败**；没有测试密钥回退，
  没有"跳过签名继续发布"的开关——发版面出现无签名制品就是事故。
- 私钥只写 `$RUNNER_TEMP` 的临时文件：`umask 077`、权限断言 `0600`、
  退出 trap 删除；**不打印、不进 artifact、不进日志**。
- `key_id` 与公钥十六进制写入 job summary，便于对账；私钥任何形态都不出现。

### 裁定 4：先验后发，验不过什么都不推

固定顺序，任一步失败即 job 失败且不推送任何制品（含出厂镜像）：

0. `download-artifact` 取回同一次构建的 `rock3-zero3-image`，
   先用 bundle 内的 `checksums.txt` 复核每个文件的 SHA-256（制品在传输层
   被替换/截断必须在签名前现形）；
1. `keypub`：派生公钥与 `key_id`；
2. `manifest`：对规范名镜像生成 manifest（工具复算 `bytes`/`sha256`）；
3. `sign`：对 manifest 原始字节签名（`manifest.key_id` 必须与私钥派生值一致）；
4. `verify`：用刚写出的公钥与镜像本体做严格验签 + 目标兼容 + 摘要核对；
5. 打包与推送。

`verify` 通过后把公钥写成 `keys/<key_id>.pub`（ADR-0026 裁定 5 的安装树路径），
随 bundle 一起发布。任何人拿到 bundle 都能在干净目录用 `--pub-dir` 复验。

### 裁定 5：ghcr 承载两个不可变 OCI 制品

发布面统一走 `ghcr.io`（与运行镜像 publish 同源凭据 `GITHUB_TOKEN`）：

| 制品 | 内容 | 用途 |
|---|---|---|
| `ghcr.io/rockrobot3/rockrobot3-ota:<version>` | `rr3-rootfs-rk3566-<version>.img`、`.manifest`、`.sig`、`keys/<key_id>.pub`、`checksums.txt` | OTA 写槽链的唯一签名来源（M6 消费） |
| `ghcr.io/rockrobot3/rockrobot3-buildroot:<version>` | `rock3-zero3.img`、两个变体 dtb、`checksums.txt` | 出厂烧录件（maskrom/整盘写入） |

- 用 `oras push` 推送，artifact type 固定；每层带
  `org.opencontainers.image.title`，层名与文件名逐字一致。
- **标签不可变**：推送前先 `oras manifest fetch <ref>`，命中即失败——
  同名覆盖会让"已发布件"被静默替换，审计链断裂。只有"清单不存在"
  （registry 的 404/manifest unknown）才算可用；鉴权失败、网络错误、
  其他状态码一律 fail-closed，不把"读不到"当"不存在"。
- 两个制品同生同灭：任一已存在或任一推送失败即整组失败，不允许只发一半。
- 不打 `latest`：出厂件与 OTA 件的"最新"语义不同，`latest` 归运行镜像
  publish 轨，不在这里制造第二真值。
- 失败语义：登录失败、推送失败、制品读回校验失败均 job 红；已推的一半不回滚
  （OCI 清单最后写入，半成品不可见），但必须在 job 日志点名已完成到哪一步。

### 裁定 6：本版边界

本版**不**交付：

- 设备端下载、断点续传、对侧槽写入与写后回读（M6 板上批）；
- `rr3.ctl.ota` 在线入口与面板 OTA 按钮（M6）；
- U-Boot FIT / FIT 签名 / dm-verity（引导链签名另案）；
- 模块级更新制品的签名格式（`kind` 扩展位，ADR-0026 已留）。

离线手工签名若发生，必须使用同一 `rr3-ota-ctl` 与同一 manifest 文法，
并在 `docs/evidence/` 登记 `key_id` 与完整命令；不得另写签名脚本。

## 备选与否决

| 备选 | 否决理由 |
| --- | --- |
| 只保留 Actions artifact | 14 天保留、需登录下载，不是发布面；也没有版本不可变约束 |
| 分支构建也签名 | 信任根暴露给任何能改分支的人；签名件失去"由发布流程签发"的语义 |
| 签名整盘镜像 | OTA 只应写 rootfs 分区；整盘签名会诱导"从 0 写"的砖路径 |
| Secret 缺失时跳过签名继续发布 | 发版静默降级为无签名件，违反 fail-closed |
| 允许同名标签覆盖 | 已发布件被静默替换，无法回答"线上是哪一份" |
| 用 GitHub Release 资产承载 | 已是固件 ELF 的载体，但 OTA bundle 需要 OCI 层可拉取/可校验；统一 ghcr 更一致 |
| 在签名 job 里重新全量构建 | 重复小时级构建；签名只消费同一次构建的镜像产物 |

## 后果

- **正向**：发布件可追溯、可复验、不可变；M6 有真实签名载体；私钥只在标签
  构建的限定步骤出现，分支与 PR 接触不到。
- **代价**：标签推送会触发一次全量 Buildroot 构建（小时级）；ghcr 增加两份
  制品存储；CI 增加 `oras` 工具依赖。
- **风险**：私钥泄露 = 信任根失效。缓解：Secret 只在签名 job 注入、
  临时文件 0600 + trap 删除、公钥按 `key_id` 分发；轮换密钥只需重新发布公钥，
  不改验签代码。不可变标签防替换，但防不了"持私钥者签发恶意件"——
  这是信任根固有边界，靠离线保管与最小授权控制。

## 验证口径

- **负例（静态/本地可判）**：
  - Secret 未配置 / 空 / 非 64 hex → job 在 `keypub` 前失败；
  - 标签不是 `v<semver>` → 版本解析失败；
  - `rr_a.ext4` 缺失或为空 → 复制/生成 manifest 失败；
  - artifact 内文件与 `checksums.txt` 不符 → 签名前失败；
  - 同名 OCI 标签已存在 → 推送前失败；
  - `verify` 任一项不过 → 不推送任何制品。
- **正例（CI 回执）**：标签推送后 `release` job 绿；`oras manifest fetch` 能读到
  两个制品的清单；bundle 四件齐且层名与文件名一致；在干净目录用
  `rr3-ota-ctl verify --pub-dir keys --image <规范名>` 复验通过。
- **板上**：槽位写入与回滚实录归 M6，不在本 ADR 冒领。

## 落地位置（施工清单）

1. `.github/workflows/buildroot.yml`：`on.push` 增 `tags: ["v*"]`；`image` job 的
   `if` 纳入标签触发；新增 `release` job（`needs: image`，
   `if: startsWith(github.ref, 'refs/tags/v')`）。
2. `release` job 内：`download-artifact` 取 `rock3-zero3-image` → 复核
   `checksums.txt` → 原生构建 `rr3-ota-ctl` → 解 Secret 到 0600 临时文件 →
   规范名复制 → manifest/sign/verify → 组装 bundle → `oras push` 两制品 →
   清理临时私钥 → job summary（`key_id`、制品引用、SHA-256）。
3. `permissions`：`contents: read` + `packages: write`；不申请多余权限。
4. 首次实跑前置：仓库 Secret `RR3_OTA_SIGNING_KEY` 就位；密钥由离线 CSPRNG
   生成（ADR-0026 裁定 5），生成记录登记在 `docs/evidence/`。
5. 文档对表：M6 里程碑行与 `partition-layout.md` 的签名段在本 job 取得绿回执后
   追加实况注记；本 ADR 状态随之改为"已实装"。
