# SPDX-License-Identifier: Apache-2.0
"""发布面与正式制品面判据（publish.yml / buildroot.yml）。

四腿完整性、merge 三架构、固件资产集、ghcr 托管、job 覆盖契约——这些判据
只在 tag/main 暴露，故与结构面分开成篇，改动聚焦发布策略。
"""

import re

from workflow_common import jobs_of
from workflow_graph import needs_of

# 发布运行面十件（rrd + 六模块 + 三工具）：镜像根按名装这十件，少编任何一件
# 只在装机后才暴露——build 腿的清单按名钉住，增删须同批评审。
PUBLISH_MODULES = ("act", "brain", "connect", "interact", "sense", "power")
PUBLISH_TOOLS = (
    "tools/rrctl/Cargo.toml",
    "base/ota/rr3-ota-ctl/Cargo.toml",
    "tools/rr3-pack/Cargo.toml",
)


def runtime_problems(joined, rel):
    """发布构建清单：rrd + 六模块 + 三工具逐名在场。"""
    bad = []
    if "-p rrd" not in joined:
        bad.append(f"{rel} build 缺 -p rrd（运行面十件之一）")
    m = re.search(r"for m in ([\w ]+); do", joined)
    seen = set(m.group(1).split()) if m else set()
    for mod in PUBLISH_MODULES:
        if mod not in seen:
            bad.append(f"{rel} build 运行面缺模块 {mod}（六模块清单被削）")
    for path in PUBLISH_TOOLS:
        if path not in joined:
            bad.append(f"{rel} build 缺 {path}（运行面工具件被削）")
    return bad


def release_problems(text, rel):
    """发布面完整性：四腿齐；逐腿判据分给下方四个小函数（单函数 <50 行）。"""
    if rel.name != "publish.yml":
        return []
    jobs = dict(jobs_of(text))
    bad = []
    for job in ("build", "build-riscv64", "merge", "firmware"):
        if job not in jobs:
            bad.append(f"{rel} 缺发布 job {job}（发布面被削）")
    for name, check in (
        ("build", build_problems),
        ("build-riscv64", riscv_problems),
        ("merge", merge_problems),
        ("firmware", firmware_problems),
    ):
        block = jobs.get(name)
        if block is not None:
            bad.extend(check(block, rel))
    return bad


def build_problems(block, rel):
    """原生双架构腿：amd64/arm64 矩阵 + 装机面两段冒烟 + 运行面十件。"""
    joined = "\n".join(block)
    bad = []
    for needle in (
        "arch: amd64",
        "arch: arm64",
        "platform: linux/amd64",
        "platform: linux/arm64",
        "ubuntu-24.04-arm",
    ):
        if needle not in joined:
            bad.append(f"{rel} build 矩阵缺 {needle}（原生发布腿缩水）")
    for needle in ("stage-image.sh", "image-smoke.sh file", "image-smoke.sh run"):
        if needle not in joined:
            bad.append(f"{rel} build 缺 {needle}（发布镜像未按装机面验证）")
    bad.extend(runtime_problems(joined, rel))
    return bad


def riscv_problems(block, rel):
    """riscv 腿：构建路径单点复用 + 推送机制二选一（判据是"必须真推送"）。"""
    joined = "\n".join(block)
    bad = []
    if "build-riscv-image.sh" not in joined:
        bad.append(f"{rel} build-riscv64 未复用 build-riscv-image.sh（发布路径分叉）")
    if "docker push" not in joined and "build-riscv-image.sh --push" not in joined:
        bad.append(f"{rel} build-riscv64 未推送（需 docker push 或 build-riscv-image.sh --push）")
    return bad


def merge_problems(block, rel):
    """合并腿：必须等两条构建腿，三架构齐全且合并清单可拉取。"""
    bad = []
    for dep in ("build", "build-riscv64"):
        if dep not in needs_of(block):
            bad.append(f"{rel} merge.needs 缺 {dep}（少一条腿就合并 = 少架构发布）")
    joined = "\n".join(block)
    for arch in ("amd64", "arm64", "riscv64"):
        if f"-{arch}" not in joined:
            bad.append(f"{rel} merge 未合并 {arch} 单架构镜像")
    if "image-smoke.sh file" not in joined:
        bad.append(f"{rel} merge 缺 image-smoke（合并清单未验证可拉取）")
    return bad


def firmware_problems(block, rel):
    """固件腿：双芯片档、ghcr 托管与资产集（双 ELF/bootloader/分区表/说明/sha256）。"""
    joined = "\n".join(block)
    bad = []
    if "rockrobot3-firmware" not in joined:
        bad.append(f"{rel} firmware 未推 rockrobot3-firmware（正式制品必须进 ghcr）")
    if not re.search(r"^\s+packages:\s*write\s*$", joined, re.M):
        bad.append(f"{rel} firmware 缺 packages: write（无法推 ghcr）")
    if "xtensa-esp32s3-espidf" not in joined:
        bad.append(f"{rel} firmware 未构建 S3 目标（双芯片覆盖缺失）")
    if "MCU=esp32s3" not in joined or "--features esp32s3" not in joined:
        bad.append(f"{rel} firmware S3 档缺 MCU/feature（芯片档漂移）")
    # 资产契约（2026-11-22）：ELF 不内嵌 bootloader/分区表，espflash 自带的引导
    # 镜像与 app 的 IDF 版本不保证一致——发布集缺一即红。
    for asset in (
        "rr3-periph-base-esp32-safe-dev.elf",
        "rr3-periph-base-esp32s3-full-dev.elf",
        "rr3-bootloader-esp32-dev.bin",
        "rr3-bootloader-esp32s3-dev.bin",
        "rr3-partitions.csv",
        "FLASHING.md",
        "firmware-sha256.txt",
    ):
        if asset not in joined:
            bad.append(f"{rel} firmware 资产缺 {asset}（发布集不完整）")
    return bad


# 作业覆盖面（按名钉住）：门禁/集成/平台/发布/固件的 job 是覆盖契约的一部分，
# 删除或改名即"某类验证不再跑"——必须同批改本表，不许静默缩水。
REQUIRED_JOBS = {
    # 风格/lint 面并入 conventions（check.sh ①c/①d/④b 单点），不再单列 lint job：
    # 同一批 fmt/clippy 判据只跑一处，避免"CI 绿本地红"的双实现。
    "gate.yml": (
        "conventions", "release", "supply", "alsa", "ota-bundle", "scripts",
    ),
    "integration.yml": (
        "distro-smoke", "ota-sign", "rk3566-onboard", "buildroot-assembly",
        "sim-assemble", "sim-chain", "matrix",
    ),
    "platforms.yml": (
        "cross-artifacts", "riscv-image", "esp32-gates", "autonomy-face",
        "ipct003", "host-windows", "host-macos",
    ),
    "publish.yml": ("build", "build-riscv64", "merge", "firmware"),
    "buildroot.yml": ("dtb-backport", "image", "release"),
    # 观测面同样受覆盖契约约束（2026-11-27 批90）：coverage 非阻断，但"两个 job 都消失"
    # 属于验证面静默停跑——非阻断 ≠ 可以被悄悄摘掉。
    "coverage.yml": ("workspace", "independent"),
}

# 正式制品面（buildroot.yml）：job 名 → 必须进 ghcr 的制品名。
# 与 publish.yml 的镜像/固件同一条纪律：正式发布物只进 release asset 不算托管完成。
GHCR_ARTIFACTS = {
    "buildroot.yml": ("release", ("rockrobot3-ota", "rockrobot3-buildroot")),
}


def release_artifacts_problems(text, rel):
    """正式制品托管面：OTA 与 buildroot 镜像必须由 tag 门作业推 ghcr。"""
    spec = GHCR_ARTIFACTS.get(rel.name)
    if spec is None:
        return []
    job, names = spec
    block = dict(jobs_of(text)).get(job)
    if block is None:
        return [f"{rel} 缺发布 job {job}（正式制品面被删）"]
    joined = "\n".join(block)
    bad = []
    for name in names:
        if name not in joined:
            bad.append(f"{rel} job {job} 未推 {name}（正式制品必须进 ghcr）")
    if "oras push" not in joined:
        bad.append(f"{rel} job {job} 缺 oras push（ghcr 制品未发布）")
    if not re.search(r"^\s+packages:\s*write\s*$", joined, re.M):
        bad.append(f"{rel} job {job} 缺 packages: write（无法推 ghcr）")
    if "startsWith(github.ref, 'refs/tags/v')" not in joined:
        bad.append(f"{rel} job {job} 必须 tag 门（v* 才发布）")
    if "docker/login-action" not in joined:
        bad.append(f"{rel} job {job} 缺 ghcr 登录步骤")
    return bad
