# SPDX-License-Identifier: Apache-2.0
"""GitHub 流水线结构审计规则核：并发组 / job 超时 / action 版本矩阵 / 权限与失败显形 /
五芯片覆盖。

判据（文本扫描，不做 YAML 解析——workflow 由本仓独占维护，形状稳定）：
- 每个 workflow 必须有顶层 `concurrency:`（同 ref 重复跑互相踩是历史事故源）；
- 每个 job 必须有 `timeout-minutes:`（挂死不能烧满 6 小时默认额度）；
- `uses:` 的第三方 action 必须落在 ACTION_MATRIX 登记的引用上——版本升级
  走显式提交改本表，与 rust-toolchain 钉版同一纪律；本地 action（`./`）不查。
- 每个 workflow 必须有顶层 `permissions:`（最小权限显式化）：含
  `contents: read`，不得 `write-all`；job 级额外授权另写，不在本门范围。
- 任何 `continue-on-error` 非 false 即红：失败必须显形，不能把红掩成绿
  （漂红巡检与门禁都靠退出码，掩码等于把治理面掏空）。
- platforms.yml 的五芯片矩阵腿（英特尔/AMD x86_64、瑞芯微 aarch64、进迭时空
  riscv64、esp32、esp32s3）缺任一即红，腿内验证脚本缺一即红；riscv-image
  构建门（腿 + 脚本）同样钉住：覆盖缩水不能只靠评审记得。
- publish.yml 的发布面完整性（四腿 / merge 三架构 / 固件资产集）与 buildroot.yml
  的正式制品托管面，判据在 [`workflow_release.py`](workflow_release.py)——只在
  tag 暴露，独立成篇。

入口：`workflows_audit.py`（CLI）与 `workflows_audit_selftest.py`（自检）。
"""

import re

from workflow_common import has_key, jobs_of, step_block, top_block, trigger_block
from workflow_graph import graph_problems
from workflow_release import REQUIRED_JOBS, release_artifacts_problems, release_problems

# action 引用矩阵（升级 = 显式提交改本表；值 = 允许的完整引用）
ACTION_MATRIX = {
    "actions/checkout": {"v7"},
    "actions/upload-artifact": {"v7"},
    "actions/download-artifact": {"v8"},
    "actions/cache": {"v5"},
    "Swatinem/rust-cache": {"v2"},
    "dtolnay/rust-toolchain": {"stable"},
    "docker/setup-qemu-action": {"v4"},
    "docker/setup-buildx-action": {"v4"},
    "docker/login-action": {"v4"},
    "taiki-e/install-action": {"v2"},
    "softprops/action-gh-release": {"v3"},
    "oras-project/setup-oras": {"v2"},
}

USES = re.compile(r"^\s*-?\s*uses:\s*(\S+)")
STEP_USES = re.compile(r"^\s*-\s*uses:\s*(\S+)")

# 工具链基线与作业上限（结构真源 = docs/process/ci-workflows.md；升级走显式提交）
TOOLCHAIN = "1.100.0"
MAX_JOBS = 10
# 跨多清单共写仓根 target-dir 的作业：rust-cache 必须全量缓存共享依赖
CACHE_ALL_JOBS = {"conventions", "sim-assemble", "sim-chain", "matrix"}

# 五芯片验证面（platforms.yml）：每条矩阵腿都是独立开发板环境的回执，
# 删除/改名任何一条即"某芯片不再被流水线验证"——用显式清单钉住。
CHIP_MATRIX = {
    "cross-artifacts": [
        "x86_64-unknown-linux-gnu",       # 英特尔 / AMD
        "aarch64-unknown-linux-musl",     # 瑞芯微 RK3566/RK3588
        "riscv64gc-unknown-linux-gnu",    # 进迭时空 SpacemiT K1/K3
    ],
    "esp32-gates": ["chip: esp32", "chip: esp32s3"],
    # RISC-V 发布镜像构建门：腿与脚本都钉住，删掉即"镜像路径不再被验证"。
    "riscv-image": ["build-riscv-image.sh"],
}

# 矩阵腿的验证步骤（脚本名即判据锚）：腿在而步骤被删，等于覆盖名存实亡。
JOB_STEPS = {
    "cross-artifacts": ["cross-artifacts.sh", "ipct003_dump"],
    "esp32-gates": [
        "periph-base-check.sh",
        "esp-uart-echo-check.sh",
        "espidf-std-check.sh",
        "espidf-sentinel-check.sh",
        "rr3-act-sense-espidf-check.sh",
    ],
}


def step_problems(rel, jobs):
    """工具链钉版与缓存参数：缺参即红（新结构政策）。"""
    bad = []
    for name, block in jobs:
        for i, line in enumerate(block):
            m = STEP_USES.match(line)
            if not m:
                continue
            ref = m.group(1)
            if ref.startswith("dtolnay/rust-toolchain") and not has_key(
                step_block(block, i), "toolchain", TOOLCHAIN
            ):
                bad.append(f"{rel} job {name} 工具链未显式钉 {TOOLCHAIN}（with.toolchain 缺失或漂移）")
            if ref.startswith("Swatinem/rust-cache"):
                sb = step_block(block, i)
                if not has_key(sb, "workspaces"):
                    bad.append(f"{rel} job {name} rust-cache 缺 workspaces（扫仓库根=空转）")
                if name in CACHE_ALL_JOBS and not has_key(sb, "cache-all-crates", "true"):
                    bad.append(f"{rel} job {name} 跨清单构建缺 cache-all-crates: \"true\"")
    return bad


def paths_ignore_problems(text, rel):
    """integration/platforms 的 push/PR 必须跳过纯文档（文档门在 gate）。"""
    if rel.name not in ("integration.yml", "platforms.yml"):
        return []
    lines = text.splitlines()
    bad = []
    for trig in ("push", "pull_request"):
        idx = next((i for i, l in enumerate(lines) if l.rstrip() == f"  {trig}:"), None)
        if idx is None:
            bad.append(f"{rel} 缺 {trig} 触发")
            continue
        joined = "\n".join(trigger_block(lines, idx))
        missing = [k for k in ("paths-ignore", "docs/**", "**.md") if k not in joined]
        if missing:
            bad.append(f"{rel} {trig} 触发缺 {', '.join(missing)}（纯文档应跳过）")
    return bad


def uses_problems(text, where):
    """逐行核对 action 引用（本地 action 跳过）。"""
    bad = []
    for i, line in enumerate(text.splitlines(), 1):
        m = USES.match(line)
        if not m or m.group(1).startswith(("./", "docker://")):
            continue
        ref = m.group(1)
        if "@" not in ref:
            bad.append(f"{where}:{i} action 未钉版本: {ref}")
            continue
        name, ver = ref.rsplit("@", 1)
        allowed = ACTION_MATRIX.get(name)
        if allowed is None:
            bad.append(f"{where}:{i} action 未在矩阵登记: {ref}")
        elif ver not in allowed:
            bad.append(f"{where}:{i} action 版本漂移: {ref}（允许 {'/'.join(sorted(allowed))}）")
    return bad


def trigger_problems(text, rel):
    """触发面：必须有 on:；push 必须收窄到 branches/tags；publish 必须吃 tag。"""
    bad = []
    if not re.search(r"^on:\s*$", text, re.M):
        bad.append(f"{rel} 缺顶层 on: 触发面")
        return bad
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if line.rstrip() != "  push:":
            continue
        block = trigger_block(lines, i)
        if not any(re.match(r"^\s+(branches|tags):", b) for b in block):
            bad.append(f"{rel} push 触发缺 branches/tags 范围（全分支推送会白烧 CI）")
        if rel.name == "publish.yml" and not any(
            re.match(r"^\s+tags:", b) for b in block
        ):
            bad.append(f"{rel} 发布面必须由 tag 触发（push.tags 缺失）")
    return bad


CONTINUE_ON_ERROR = re.compile(r"^\s*continue-on-error:\s*(.*)$")


def policy_problems(text, rel):
    """权限与失败显形：最小权限面 + 不得掩码失败。"""
    bad = []
    for i, line in enumerate(text.splitlines(), 1):
        m = CONTINUE_ON_ERROR.match(line)
        if not m:
            continue
        value = m.group(1).strip().strip('"').strip("'").lower()
        if value != "false":
            bad.append(
                f"{rel}:{i} continue-on-error 掩盖失败（{m.group(1).strip() or '空值'}）——红必须显形"
            )
    if not re.search(r"^permissions:\s*$", text, re.M):
        bad.append(f"{rel} 缺顶层 permissions（最小权限面必须显式）")
        return bad
    joined = "\n".join(top_block(text.splitlines(), "permissions"))
    if "write-all" in joined:
        bad.append(f"{rel} permissions 不得 write-all（按需最小授权）")
    if not re.search(r"^\s+contents:\s*read\s*$", joined, re.M):
        bad.append(f"{rel} permissions 缺 contents: read（checkout 最小面）")
    return bad


def chip_problems(text, rel):
    """五芯片验证面：platforms.yml 的矩阵腿缺一条即红，腿内验证步骤缺一即红。"""
    if rel.name != "platforms.yml":
        return []
    jobs = dict(jobs_of(text))
    bad = []
    for job, needles in CHIP_MATRIX.items():
        block = jobs.get(job)
        if block is None:
            bad.append(f"{rel} 缺 job {job}（五芯片验证面被删）")
            continue
        joined = "\n".join(block)
        for needle in needles:
            if needle not in joined:
                bad.append(f"{rel} job {job} 缺 {needle}（五芯片覆盖不得缩水）")
        for script in JOB_STEPS.get(job, []):
            if script not in joined:
                bad.append(f"{rel} job {job} 缺验证步骤 {script}（矩阵腿名存实亡）")
    return bad


def workflow_problems(path, root):
    text = path.read_text(encoding="utf-8")
    rel = path.relative_to(root)
    bad = uses_problems(text, str(rel))
    bad.extend(trigger_problems(text, rel))
    bad.extend(paths_ignore_problems(text, rel))
    bad.extend(chip_problems(text, rel))
    bad.extend(release_problems(text, rel))
    bad.extend(release_artifacts_problems(text, rel))
    if not re.search(r"^concurrency:\s*$", text, re.M):
        bad.append(f"{rel} 缺顶层 concurrency（同 ref 并发组）")
    if rel.name == "publish.yml" and not re.search(r"cancel-in-progress:\s*false", text):
        bad.append(f"{rel} 发布面必须 cancel-in-progress: false（发布不可中断）")
    jobs = jobs_of(text)
    if not jobs:
        bad.append(f"{rel} 没有可识别的 job")
    names = {n for n, _ in jobs}
    for job in REQUIRED_JOBS.get(rel.name, ()):
        if job not in names:
            bad.append(f"{rel} 缺 job {job}（覆盖契约被削，改表须同批评审）")
    if len(jobs) > MAX_JOBS:
        bad.append(f"{rel} job 定义 {len(jobs)} > 上限 {MAX_JOBS}（塞不下就拆流水线）")
    for name, block in jobs:
        if not any(re.match(r"^\s{4}timeout-minutes:", l) for l in block):
            bad.append(f"{rel} job {name} 缺 timeout-minutes")
    bad.extend(step_problems(rel, jobs))
    bad.extend(graph_problems(rel, jobs))
    bad.extend(policy_problems(text, rel))
    return bad


def audit(root):
    bad = []
    wf_dir = root / ".github" / "workflows"
    files = sorted(wf_dir.glob("*.yml"))
    if not files:
        return ["未找到 .github/workflows/*.yml"]
    for p in files:
        bad.extend(workflow_problems(p, root))
    for p in sorted((root / ".github" / "actions").glob("*/action.yml")):
        bad.extend(uses_problems(p.read_text(encoding="utf-8"), str(p.relative_to(root))))
    return bad
