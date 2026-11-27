#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""release-verify 自检夹具生成器（2026-11-27 批178）。

按场景写一组 OCI manifest JSON：默认五类齐全；`mut` 逐项制造违约，供
release-verify.selftest.sh 用假 oras 喂给巡检器，判据腐化即红。
用法：release-verify.fixture.py <输出目录> [none|distro-sha|index-riscv|fw-boot|attest]
"""

import hashlib
import io
import json
import os
import pathlib
import sys
import tarfile

REPO = pathlib.Path(__file__).resolve().parents[3]
# e_ident 16B + e_type(2) + e_machine(2)=0xb7(AArch64, LE)；file 需报到 "ARM aarch64"。
ELF = b"\x7fELF\x02\x01\x01\x00" + b"\x00" * 8 + b"\x02\x00\xb7\x00"

VER = "0.0.8"
TAG = f"v{VER}"


def layer(title, media="application/octet-stream"):
    dg = "sha256:" + hashlib.sha256(title.encode()).hexdigest()
    return {"mediaType": media, "digest": dg, "annotations": {"org.opencontainers.image.title": title}}


def image_manifest():
    return {"schemaVersion": 2, "layers": [layer("sha256:0", "application/vnd.oci.image.layer.v1.tar+gzip")]}


def index(archs):
    ms = [{"platform": {"architecture": a, "os": "linux"}} for a in archs]
    ms.append(
        {
            "platform": {"architecture": "unknown", "os": "unknown"},
            "annotations": {"vnd.docker.reference.type": "attestation-manifest"},
        }
    )
    return {"schemaVersion": 2, "manifests": ms}


def with_attestations():
    # 真实形状（2026-11-27 匿名实测）：一条 attestation manifest 指向 digest，
    # 证明面在它内部的 in-toto 层里（fixture 由 attest() 单独给出）。
    ms = [{"platform": {"architecture": "unknown", "os": "unknown"},
           "annotations": {"vnd.docker.reference.type": "attestation-manifest"},
           "digest": "sha256:att"}]
    ms.append({"platform": {"architecture": "amd64", "os": "linux"}})
    return {"schemaVersion": 2, "manifests": ms}


def attest(mut):
    preds = ["https://slsa.dev/provenance/v1", "https://spdx.dev/Document"]
    if mut == "attest":
        preds = preds[:1]
    layers = [{"mediaType": "application/vnd.in-toto+json",
               "annotations": {"in-toto.io/predicate-type": p}} for p in preds]
    return {"schemaVersion": 2, "layers": layers}


def firmware(mut):
    titles = [
        "rr3-periph-base-esp32-safe-dev.elf",
        "rr3-periph-base-esp32s3-full-dev.elf",
        "rr3-esp32-safe-merged.bin",
        "rr3-esp32s3-full-merged.bin",
        "rr3-bootloader-esp32-dev.bin",
        "rr3-bootloader-esp32s3-dev.bin",
        "rr3-partitions.csv",
        "FLASHING.md",
        "firmware-sha256.txt",
    ]
    if mut == "fw-boot":
        titles = [t for t in titles if t != "rr3-bootloader-esp32-dev.bin"]
    return {"schemaVersion": 2, "artifactType": "application/vnd.rr3.firmware.v1+json",
            "layers": [layer(t) for t in titles]}


def distro(mut):
    titles = [f"rockrobot3-distro-{VER}-aarch64.tar.gz", "distro-sha256.txt"]
    if mut == "distro-sha":
        titles = titles[:1]
    return {"schemaVersion": 2, "artifactType": "application/vnd.rr3.distro-bundle.v1.tar+gzip",
            "layers": [layer(t) for t in titles]}


def ota():
    titles = [f"rr3-rootfs-rk3566-{VER}.img", f"rr3-rootfs-rk3566-{VER}.img.manifest",
              f"rr3-rootfs-rk3566-{VER}.img.sig", "keys/8570b80b2a0f82c3.pub",
              "checksums.txt", "bundle.txt"]
    return {"schemaVersion": 2, "artifactType": "application/vnd.rr3.ota.bundle.v1+json",
            "layers": [layer(t) for t in titles]}


def buildroot():
    titles = ["rock3-zero3.img", "rk3566-radxa-zero-3w.dtb", "rk3566-radxa-zero-3e.dtb", "checksums.txt"]
    return {"schemaVersion": 2, "artifactType": "application/vnd.rr3.buildroot.image.v1+json",
            "layers": [layer(t) for t in titles]}


def add_bytes(tf, top, name, data, mode=0o644):
    ti = tarfile.TarInfo(f"{top}/{name}")
    ti.size = len(data)
    ti.mode = mode
    tf.addfile(ti, io.BytesIO(data))


def distro_tar(path):
    """合成 A10 合规包：仓内真值树 + ELF 头占位件；commit 用 000… 以覆盖 any 模式。"""
    commit = "000000000000"
    top = f"rockrobot3-{commit}"
    modules = ("rr3-act", "rr3-connect", "rr3-interact", "rr3-power", "rr3-sense")
    with tarfile.open(path, "w:gz") as tf:
        for sub in ("profiles", "contracts", "hardware"):
            for f in (REPO / sub).rglob("*"):
                if f.is_file():
                    add_bytes(tf, top, str(f.relative_to(REPO)), f.read_bytes())
        for m in modules:
            add_bytes(tf, top, f"modules/{m}/{m}", ELF, 0o755)
            for name in ("module.toml", "config.toml"):
                add_bytes(tf, top, f"modules/{m}/{name}", (REPO / "modules" / m / name).read_bytes())
        for b in ("rrd", "rr3-ctl", "rr3-ota-ctl"):
            add_bytes(tf, top, f"bin/{b}", ELF, 0o755)
        add_bytes(
            tf, top, "BUNDLE_VERSION",
            f"commit={commit}\nbuilt_at=fixture\ntarget=aarch64-unknown-linux-musl\n"
            "components=rrd,rr3-ctl,rr3-ota-ctl,rr3-act,rr3-connect,rr3-interact,"
            "rr3-power,rr3-sense\nnatives=\n".encode(),
        )


def main():
    out = sys.argv[1]
    mut = sys.argv[2] if len(sys.argv) > 2 else "none"
    os.makedirs(out, exist_ok=True)
    archs = ["amd64", "arm64"] if mut == "index-riscv" else ["amd64", "arm64", "riscv64"]
    files = {
        "index.json": index(archs),
        "img-amd64.json": with_attestations(),
        "img-arm64.json": with_attestations(),
        "img-riscv64.json": with_attestations(),
        "attest.json": attest(mut),
        "firmware.json": firmware(mut),
        "distro.json": distro(mut),
        "ota.json": ota(),
        "buildroot.json": buildroot(),
    }
    for name, doc in files.items():
        with open(os.path.join(out, name), "w", encoding="utf-8") as f:
            json.dump(doc, f)
    distro_tar(os.path.join(out, f"rockrobot3-distro-{VER}-aarch64.tar.gz"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
