#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""release-verify 自检夹具生成器（2026-11-27 批178）。

按场景写一组 OCI manifest JSON：默认五类齐全；`mut` 逐项制造违约，供
release-verify.selftest.sh 用假 oras 喂给巡检器，判据腐化即红。
用法：release-verify.fixture.py <输出目录> [none|distro-sha|index-riscv|fw-boot|attest]
"""

import json
import os
import sys

VER = "0.0.8"
TAG = f"v{VER}"


def layer(title, media="application/octet-stream"):
    return {"mediaType": media, "annotations": {"org.opencontainers.image.title": title}}


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


def with_attestations(n):
    ms = [{"platform": {"architecture": "unknown", "os": "unknown"},
           "annotations": {"vnd.docker.reference.type": "attestation-manifest"}} for _ in range(n)]
    ms.append({"platform": {"architecture": "amd64", "os": "linux"}})
    return {"schemaVersion": 2, "manifests": ms}


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


def main():
    out = sys.argv[1]
    mut = sys.argv[2] if len(sys.argv) > 2 else "none"
    os.makedirs(out, exist_ok=True)
    archs = ["amd64", "arm64"] if mut == "index-riscv" else ["amd64", "arm64", "riscv64"]
    n = 1 if mut == "attest" else 2
    files = {
        "index.json": index(archs),
        "img-amd64.json": with_attestations(n),
        "img-arm64.json": with_attestations(n),
        "img-riscv64.json": with_attestations(n),
        "firmware.json": firmware(mut),
        "distro.json": distro(mut),
        "ota.json": ota(),
        "buildroot.json": buildroot(),
    }
    for name, doc in files.items():
        with open(os.path.join(out, name), "w", encoding="utf-8") as f:
            json.dump(doc, f)
    return 0


if __name__ == "__main__":
    sys.exit(main())
