# base/ota —— OTA 槽位 CLI

本目录只有一个工程：

    rr3-ota-ctl/   ← bin rr3-ota-ctl（独立 Rust 工程）

分区集合、两条更新轨道、切换与回滚链、断电安全约束见
[docs/design/分区布局与OTA槽位.md](../../docs/design/分区布局与OTA槽位.md)。

标签的**唯一事实源在代码里**：`base/runtime/rr-ota/src/lib.rs` 的
`Rr3Slot::rr3_slot_label` 与 `RR3_OTA_SLOT_NAMES` / `RR3_OTA_STAGING_PARTITION`。
文档与 CLI 都引用那份常量而不重抄，所以"文档说 rr_a、代码用 rr_x"这种分家
在结构上不可能发生；`cargo test -p rr-ota` 的用例把标签、对合律与
genimage.cfg / extlinux.conf 真工件逐字钉死（条数只登记在
[`docs/process/conventions.md`](../../docs/process/conventions.md) §2）。

    cargo run -q --manifest-path base/ota/rr3-ota-ctl/Cargo.toml --bin rr3-ota-ctl -- slots
    # rr_a peer=rr_b / rr_b peer=rr_a / rr_factory peer=rr_factory / rr_staging (模块级暂存，无对侧)

0.0.3 批D 起，写侧的**引导配置改写**主机面已实装（判定与改写逻辑在
`rr-ota::bootconf`，base/runtime workspace 内全量跑测试；CLI 只做参数解析
与落盘，`--bootconf` 必传，不缺省碰任何路径）：

    rr3-ota-ctl switch rr_b --bootconf /boot/extlinux/extlinux.conf
    # 引导配置已改写：root=LABEL=rr_a → rr_b（持久切换，重启后生效）
    rr3-ota-ctl factory --bootconf /boot/extlinux/extlinux.conf
    # 一次性出厂引导；rr_factory 分区本体不写

边界（诚实口径）：本批交付的是**切槽的"翻标志"一步**——改 extlinux.conf 的
root=LABEL 标签（原子落盘）。槽位镜像写入（往 rr_b 落整镜像）随升级下载链
另批落地；U-Boot 环境变量写侧（bootcount / bootlimit / altbootcmd 自动回滚链）
与块设备/挂载探测仍是**板上件**，未实装、不 claim——语义与板上核对项清单见
[分区布局与OTA槽位.md](../../docs/design/分区布局与OTA槽位.md) 增补段。在实板验证通过之前，这条
命令链具备"切过去"的能力，尚不具备"失败自动回得来"的保障。

容量不在本目录冻结（本机无 Buildroot 与板子，凭想象写下的 size 是最贵的那种错误）。

## OTA 制品签名（ADR-0026，主机面已实装）

升级制品必须带签字清单；未签名、签名不符、目标平台/板型不符、镜像摘要不符
一律在写入前拒。签名是分离式的：签名覆盖 manifest 原始字节，manifest 内的
`bytes`/`sha256` 约束镜像本体。

    # 1. 私钥只放离线/CI Secret：32 字节种子的 64 位十六进制，unix 权限必须 0600
    rr3-ota-ctl keypub --key-file sign.key --out-pub ota/keys/<key_id>.pub

    # 2. 生成 manifest（sha256/bytes/key_id 由工具计算）
    rr3-ota-ctl manifest --image rr3-rootfs-rk3566-0.0.8.img \
        --platform rk3566 --board radxa-zero3 --version 0.0.8 \
        --key-file sign.key --out rr3-ota.manifest

    # 3. 签名（manifest.key_id 必须与私钥派生值一致）
    rr3-ota-ctl sign --manifest rr3-ota.manifest --key-file sign.key \
        --sig-out rr3-ota.sig

    # 4. 写入前验签：目标兼容 + 可选降级门 + 镜像摘要核对
    rr3-ota-ctl verify --manifest rr3-ota.manifest --sig rr3-ota.sig \
        --pub-dir ota/keys --image rr3-rootfs-rk3566-0.0.8.img \
        --platform rk3566 --board radxa-zero3 --current-version 0.0.7

    # 5. 验签后装到普通文件 staging 目标（块设备/槽位写入归 M6）
    rr3-ota-ctl install --manifest rr3-ota.manifest --sig rr3-ota.sig \
        --pub-dir ota/keys --image rr3-rootfs-rk3566-0.0.8.img \
        --target /var/lib/rr3/staging/rr3-rootfs.img --confirm \
        --platform rk3566 --board radxa-zero3 --current-version 0.0.7

`--pub-dir` 按 manifest 的 `key_id` 查 `<key_id>.pub`；单文件场景用 `--pub`，
两者必须二选一。

边界：`verify` 只读，不写任何槽位；`install` 只接受**普通文件**目标，
先验签、再临时件复核、fsync 临时件、rename、fsync 父目录；块设备/槽位写入、`rr3.ctl.ota`
在线入口与 U-Boot 回滚链仍归 M6 板上批。公钥按 `ota/keys/<key_id>.pub`
随安装树只读分发，私钥永不进镜像、仓库与设备。文法与信任模型见
[ADR-0026](../../docs/process/adr/ADR-0026-OTA镜像签名与验签链.md)。
