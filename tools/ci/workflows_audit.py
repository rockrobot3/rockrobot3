#!/usr/bin/env python3
"""GitHub 流水线结构审计：并发组 / job 超时 / action 版本矩阵 / 权限与失败显形 /
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
- publish.yml 的发布面完整性：build / build-riscv64 / merge / firmware 四腿
  必须在；merge.needs 必须等两条构建腿、合并命令必须含三架构；firmware 必须
  带 packages: write 且推 rockrobot3-firmware——发布面缺腿只在 tag 暴露太晚。
- buildroot.yml 的正式制品面：release 腿必须 tag 门、登录 ghcr、带
  packages: write，并以 oras 推 rockrobot3-ota / rockrobot3-buildroot——
  正式制品只发 release asset 不进 ghcr 等于托管面残缺。

用法：
    workflows_audit.py [--root <仓根>]
    workflows_audit.py --self-test

退出码：0 = 通过；1 = 有缺口或自检失败。
"""

import argparse
import pathlib
import re
import sys

# action 引用矩阵（升级 = 显式提交改本表；值 = 允许的完整引用）
ACTION_MATRIX = {
    "actions/checkout": {"v7"},
    "actions/upload-artifact": {"v7"},
    "actions/download-artifact": {"v8"},
    "actions/cache": {"v4"},
    "Swatinem/rust-cache": {"v2"},
    "dtolnay/rust-toolchain": {"stable"},
    "docker/setup-qemu-action": {"v4"},
    "docker/setup-buildx-action": {"v3"},
    "docker/login-action": {"v3"},
    "taiki-e/install-action": {"v2"},
    "softprops/action-gh-release": {"v2"},
    "oras-project/setup-oras": {"v2"},
}

USES = re.compile(r"^\s*-?\s*uses:\s*(\S+)")
JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")
STEP_USES = re.compile(r"^\s*-\s*uses:\s*(\S+)")

# 工具链基线与作业上限（结构真源 = docs/process/ci-workflows.md；升级走显式提交）
TOOLCHAIN = "1.100.0"
MAX_JOBS = 10
# 跨多清单共写仓根 target-dir 的作业：rust-cache 必须全量缓存共享依赖
CACHE_ALL_JOBS = {"conventions", "lint", "sim-assemble", "sim-chain", "matrix"}

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


def step_block(lines, i):
    """第 i 行（`- uses:`）之后的步骤块（缩进更深的行）。"""
    indent = len(lines[i]) - len(lines[i].lstrip())
    out = []
    for line in lines[i + 1 :]:
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        out.append(line)
    return out


def has_key(block, key, value=None):
    """块内 `key:` 是否存在（给了 value 则值必须相等，去引号）。"""
    for line in block:
        m = re.match(rf"^\s+{re.escape(key)}:\s*(.*)$", line)
        if not m:
            continue
        got = m.group(1).strip().strip('"').strip("'")
        if value is None or got == value:
            return True
    return False


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


def jobs_of(text):
    """返回 [(job 名, 块行)]——jobs: 之后的 2 空格键为 job 边界。"""
    jobs, name, buf = [], None, []
    in_jobs = False
    for line in text.splitlines():
        if re.match(r"^jobs:\s*$", line):
            in_jobs, name, buf = True, None, []
            continue
        if in_jobs and re.match(r"^\S", line):
            if name:
                jobs.append((name, buf))
            in_jobs, name, buf = False, None, []
            continue
        if not in_jobs:
            continue
        m = JOB.match(line)
        if m:
            if name:
                jobs.append((name, buf))
            name, buf = m.group(1), []
        elif name:
            buf.append(line)
    if name:
        jobs.append((name, buf))
    return jobs


def trigger_block(lines, i):
    """`  push:` 之后的子块（缩进 ≥4 的行）。"""
    out = []
    for line in lines[i + 1 :]:
        if line.strip() and not line.startswith("    "):
            break
        out.append(line)
    return out


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


def needs_of(block):
    """job 块的 needs 列表（支持内联 [a, b] / 标量 / 多行 - a）。"""
    for idx, line in enumerate(block):
        m = re.match(r"^\s{4}needs:\s*(.*)$", line)
        if not m:
            continue
        rest = m.group(1).strip()
        if rest.startswith("["):
            return [x.strip().strip("'\"") for x in rest.strip("[]").split(",") if x.strip()]
        if rest:
            return [rest.strip("'\"")]
        out = []
        for nxt in block[idx + 1 :]:
            m2 = re.match(r"^\s{6}-\s*(\S+)", nxt)
            if not m2:
                break
            out.append(m2.group(1).strip("'\""))
        return out
    return []


def graph_problems(rel, jobs):
    """needs 图：引用必须存在，且不得成环。"""
    names = {n for n, _ in jobs}
    graph = {n: needs_of(b) for n, b in jobs}
    bad = []
    for n, deps in graph.items():
        for d in deps:
            if d not in names:
                bad.append(f"{rel} job {n} needs 指向不存在的 job {d}")
    if cycle := graph_cycle(graph):
        bad.append(f"{rel} needs 成环: {' -> '.join(cycle)}")
    return bad


CONTINUE_ON_ERROR = re.compile(r"^\s*continue-on-error:\s*(.*)$")


def top_block(lines, key):
    """顶层键（缩进 0）之后的子块。"""
    idx = next((i for i, l in enumerate(lines) if l.rstrip() == f"{key}:"), None)
    if idx is None:
        return []
    out = []
    for line in lines[idx + 1 :]:
        if line.strip() and not line.startswith("  "):
            break
        out.append(line)
    return out


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


def graph_cycle(graph):
    state, stack = {}, []

    def dfs(n):
        if state.get(n) == 1:
            return stack[stack.index(n) :] + [n]
        if state.get(n) == 2:
            return None
        state[n] = 1
        stack.append(n)
        for d in graph.get(n, []):
            if found := dfs(d):
                return found
        stack.pop()
        state[n] = 2
        return None

    for n in graph:
        if found := dfs(n):
            return found
    return None


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


def release_problems(text, rel):
    """发布面完整性：四腿齐、merge 等两条构建腿并覆盖三架构、固件进 ghcr。"""
    if rel.name != "publish.yml":
        return []
    jobs = dict(jobs_of(text))
    bad = []
    for job in ("build", "build-riscv64", "merge", "firmware"):
        if job not in jobs:
            bad.append(f"{rel} 缺发布 job {job}（发布面被削）")
    merge = jobs.get("merge")
    if merge is not None:
        needs = needs_of(merge)
        for dep in ("build", "build-riscv64"):
            if dep not in needs:
                bad.append(f"{rel} merge.needs 缺 {dep}（少一条腿就合并 = 少架构发布）")
        joined = "\n".join(merge)
        for arch in ("amd64", "arm64", "riscv64"):
            if f"-{arch}" not in joined:
                bad.append(f"{rel} merge 未合并 {arch} 单架构镜像")
    firmware = jobs.get("firmware")
    if firmware is not None:
        joined = "\n".join(firmware)
        if "rockrobot3-firmware" not in joined:
            bad.append(f"{rel} firmware 未推 rockrobot3-firmware（正式制品必须进 ghcr）")
        if not re.search(r"^\s+packages:\s*write\s*$", joined, re.M):
            bad.append(f"{rel} firmware 缺 packages: write（无法推 ghcr）")
        if "xtensa-esp32s3-espidf" not in joined:
            bad.append(f"{rel} firmware 未构建 S3 目标（双芯片覆盖缺失）")
        if "rr3-periph-base-esp32s3-dev.elf" not in joined:
            bad.append(f"{rel} firmware 缺 S3 ELF 资产/推送（双芯片覆盖缺失）")
    return bad


# 作业覆盖面（按名钉住）：门禁/集成/平台/发布/固件的 job 是覆盖契约的一部分，
# 删除或改名即"某类验证不再跑"——必须同批改本表，不许静默缩水。
REQUIRED_JOBS = {
    "gate.yml": (
        "conventions", "release", "lint", "supply", "alsa", "ota-bundle", "scripts",
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


def self_test():
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        wf = root / ".github" / "workflows"
        wf.mkdir(parents=True)
        (wf / "good.yml").write_text(
            "name: good\non:\n  push:\n    branches: [main]\n"
            "permissions:\n  contents: read\n"
            "concurrency:\n  group: g\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - uses: actions/checkout@v7\n"
            "      - uses: dtolnay/rust-toolchain@stable\n"
            '        with:\n          toolchain: "1.100.0"\n'
            "      - uses: Swatinem/rust-cache@v2\n"
            "        with:\n          workspaces: base/runtime\n",
            encoding="utf-8",
        )
        (wf / "bad.yml").write_text(
            "name: bad\non:\n  push:\njobs:\n  b:\n    runs-on: ubuntu-latest\n"
            "    needs: [nope]\n"
            "    steps:\n      - uses: actions/checkout@v5\n"
            "      - uses: dtolnay/rust-toolchain@stable\n"
            "      - uses: Swatinem/rust-cache@v2\n"
            "      - run: echo hi\n        continue-on-error: true\n",
            encoding="utf-8",
        )
        (wf / "noperm.yml").write_text(
            "name: noperm\non:\n  push:\n    branches: [main]\n"
            "concurrency:\n  group: n\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - uses: actions/checkout@v7\n",
            encoding="utf-8",
        )
        (wf / "integration.yml").write_text(
            "name: integ\non:\n  push:\n    branches: [main]\n  pull_request:\n"
            "concurrency:\n  group: i\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - uses: actions/checkout@v7\n",
            encoding="utf-8",
        )
        (wf / "publish.yml").write_text(
            "name: pub\non:\n  push:\n    tags: [\"v*\"]\n"
            "concurrency:\n  group: p\n  cancel-in-progress: true\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - uses: actions/checkout@v7\n"
            "  firmware:\n    if: startsWith(github.ref, 'refs/tags/v')\n"
            "    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: echo classic\n",
            encoding="utf-8",
        )
        many = "".join(
            f"  j{i}:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            for i in range(MAX_JOBS + 1)
        )
        (wf / "big.yml").write_text(
            "name: big\non:\n  push:\n    branches: [main]\n"
            "concurrency:\n  group: b\njobs:\n" + many,
            encoding="utf-8",
        )
        (wf / "cycle.yml").write_text(
            "name: cyc\non:\n  push:\n    branches: [main]\n"
            "concurrency:\n  group: c\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    needs: [b]\n"
            "  b:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    needs: [a]\n",
            encoding="utf-8",
        )
        (wf / "platforms.yml").write_text(
            "name: platforms\non:\n  push:\n    branches: [main]\n"
            "permissions:\n  contents: read\n"
            "concurrency:\n  group: p\njobs:\n"
            "  cross-artifacts:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: echo x86_64-unknown-linux-gnu\n",
            encoding="utf-8",
        )
        (wf / "gate.yml").write_text(
            "name: g\non:\n  push:\n    branches: [main]\n"
            "permissions:\n  contents: read\n"
            "concurrency:\n  group: g\njobs:\n"
            "  lint:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - uses: actions/checkout@v7\n",
            encoding="utf-8",
        )
        (wf / "buildroot.yml").write_text(
            "name: br\non:\n  push:\n    tags: [\"v*\"]\n"
            "permissions:\n  contents: read\n"
            "concurrency:\n  group: br\njobs:\n"
            "  release:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    if: startsWith(github.ref, 'refs/tags/v')\n"
            "    permissions:\n      contents: read\n      packages: write\n"
            "    steps:\n      - run: oras push ghcr.io/o/rockrobot3-ota:1\n",
            encoding="utf-8",
        )
        bad = audit(root)

    cases = [
        ("缺 concurrency 被抓", any("concurrency" in b for b in bad)),
        ("缺 timeout 被抓", any("timeout-minutes" in b for b in bad)),
        ("版本漂移被抓", any("版本漂移" in b for b in bad)),
        ("needs 悬空被抓", any("不存在的 job" in b for b in bad)),
        ("needs 成环被抓", any("成环" in b for b in bad)),
        ("push 无范围被抓", any("branches/tags" in b for b in bad)),
        ("工具链未钉版被抓", any("工具链未显式钉" in b for b in bad)),
        ("缓存缺 workspaces 被抓", any("缺 workspaces" in b for b in bad)),
        ("paths-ignore 缺失被抓", any("paths-ignore" in b for b in bad)),
        ("publish 可取消被抓", any("cancel-in-progress: false" in b for b in bad)),
        ("job 超限被抓", any("> 上限" in b for b in bad)),
        ("continue-on-error 被抓", any("continue-on-error" in b for b in bad)),
        ("缺 permissions 被抓", any("缺顶层 permissions" in b for b in bad)),
        ("芯片矩阵缩水被抓", any("五芯片覆盖不得缩水" in b for b in bad)),
        ("芯片 job 缺失被抓", any("缺 job esp32-gates" in b for b in bad)),
        ("芯片验证步骤缺失被抓", any("验证步骤" in b for b in bad)),
        ("riscv 镜像腿缺失被抓", any("缺 job riscv-image" in b for b in bad)),
        ("发布面缺腿被抓", any("缺发布 job" in b for b in bad)),
        ("门禁 job 缩水被抓", any("缺 job conventions" in b for b in bad)),
        ("固件缺 S3 被抓", any("未构建 S3 目标" in b for b in bad)),
        ("制品面缺名被抓", any("未推 rockrobot3-buildroot" in b for b in bad)),
        ("好例零误报", not any("good.yml" in b for b in bad)),
    ]
    for name, ok in cases:
        print(("PASS  " if ok else "FAIL  ") + name)
    return 0 if all(ok for _, ok in cases) else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    bad = audit(pathlib.Path(a.root))
    for b in bad:
        print("FAIL  " + b)
    if bad:
        print(f"FAIL  流水线结构缺口 {len(bad)} 处（修法见 tools/ci/README.md）")
        return 1
    print(
        "PASS  流水线结构审计通过（并发组 / job 超时 / action 版本矩阵 / 权限与失败显形 / "
        "五芯片覆盖 / 发布面完整性 / 制品托管）"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
