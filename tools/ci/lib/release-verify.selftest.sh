#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# release-verify 自检册（2026-11-27 批178，从主文件拆出）。
#
# 判据：用假 oras 喂 manifest 夹具（release-verify.fixture.py 生成），逐项注入违约，
# 核对巡检器的退出码与点名；「全齐」一例同时防误报。夹具与假件都只在临时目录里。

rv_lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
rv_main="$rv_lib/release-verify.sh"

# 假 oras：只认 `oras manifest fetch <ref>`，按 ref 末段点名夹具文件。
rv_stub() {
	local dir="$1"
	printf '%s\n' '#!/usr/bin/env bash' \
		'[ "${RR3_ORAS_FAIL:-}" = 1 ] && { echo "401 Unauthorized" >&2; exit 1; }' \
		'ref="${3:-}"' \
		'case "$ref" in' \
		'  *rockrobot3-firmware:*) f=firmware.json ;;' \
		'  *rockrobot3-distro:*) f=distro.json ;;' \
		'  *rockrobot3-ota:*) f=ota.json ;;' \
		'  *rockrobot3-buildroot:*) f=buildroot.json ;;' \
		'  *rockrobot3:v0.0.8-amd64) f=img-amd64.json ;;' \
		'  *rockrobot3:v0.0.8-arm64) f=img-arm64.json ;;' \
		'  *rockrobot3:v0.0.8-riscv64) f=img-riscv64.json ;;' \
		'  *@sha256:att*) f=attest.json ;;' \
		'  *rockrobot3:v0.0.8) f=index.json ;;' \
		'  *) echo "404 not found $ref" >&2; exit 1 ;;' \
		'esac' \
		'[ -f "$FIX/$f" ] || { echo "404 not found $ref" >&2; exit 1; }' \
		'cat "$FIX/$f"' >"$dir/bin/oras"
	chmod 0755 "$dir/bin/oras"
}

# 单例：变异夹具 → 跑巡检器 → 期望码 + 期望点名片段。
rv_case() { # 名称 期望码 变异 期望片段
	local d out rc
	d=$(mktemp -d)
	mkdir -p "$d/bin"
	rv_stub "$d"
	python3 "$rv_lib/release-verify.fixture.py" "$d/fix" "$3" >/dev/null || {
		printf 'FAIL  self-test/%s（夹具生成失败）\n' "$1"
		rm -rf "$d"
		return 1
	}
	out=$(FIX="$d/fix" PATH="$d/bin:$PATH" bash "$rv_main" --tag v0.0.8 --repo test 2>&1)
	rc=$?
	rm -rf "$d"
	if [ "$rc" -ne "$2" ]; then
		printf 'FAIL  self-test/%s（期望 %d 实得 %d）\n' "$1" "$2" "$rc"
		return 1
	fi
	if [ -n "$4" ] && ! printf '%s' "$out" | grep -qF "$4"; then
		printf 'FAIL  self-test/%s（未点名：%s）\n' "$1" "$4"
		return 1
	fi
	printf 'PASS  self-test/%s（退出码 %d）\n' "$1" "$rc"
	return 0
}

# 类别过滤：只核 firmware 时，distro/镜像夹具缺失不应拖红——publish 与 buildroot
# 两条流水线各产一半，tag 窗口不能用"全齐"卡对方尚未完成的产物。
rv_class_case() {
	local d out rc
	d=$(mktemp -d)
	mkdir -p "$d/bin"
	rv_stub "$d"
	python3 "$rv_lib/release-verify.fixture.py" "$d/fix" none >/dev/null
	rm -f "$d/fix/distro.json" "$d/fix/index.json"
	out=$(FIX="$d/fix" PATH="$d/bin:$PATH" bash "$rv_main" --tag v0.0.8 --classes firmware 2>&1)
	rc=$?
	rm -rf "$d"
	if [ "$rc" -eq 0 ] && printf '%s' "$out" | grep -q 'classes=firmware'; then
		printf 'PASS  self-test/类别过滤只核所选（退出码 0）\n'
		return 0
	fi
	printf 'FAIL  self-test/类别过滤只核所选（期望 0 实得 %d）\n' "$rc"
	return 1
}

# 缺件（批186）：远端明确 not found 时结论是缺件（退出 1），不是"未定"。
rv_missing_case() {
	local d out rc
	d=$(mktemp -d)
	mkdir -p "$d/bin"
	rv_stub "$d"
	python3 "$rv_lib/release-verify.fixture.py" "$d/fix" none >/dev/null
	rm -f "$d/fix/distro.json"
	out=$(FIX="$d/fix" PATH="$d/bin:$PATH" bash "$rv_main" --tag v0.0.8 --classes distro 2>&1)
	rc=$?
	rm -rf "$d"
	if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -q '缺件'; then
		printf 'PASS  self-test/远端明确不存在报缺件（退出码 1）\n'
		return 0
	fi
	printf 'FAIL  self-test/远端明确不存在报缺件（期望 1/缺件 实得 %d）\n' "$rc"
	return 1
}

# 取数失败（批183）：401/网络错误必须报"未定"（退出 2），不能与"缺件"混成同一结论。
rv_fetch_fail_case() {
	local d out rc
	d=$(mktemp -d)
	mkdir -p "$d/bin"
	rv_stub "$d"
	python3 "$rv_lib/release-verify.fixture.py" "$d/fix" none >/dev/null
	out=$(RR3_ORAS_FAIL=1 FIX="$d/fix" PATH="$d/bin:$PATH" bash "$rv_main" --tag v0.0.8 2>&1)
	rc=$?
	rm -rf "$d"
	if [ "$rc" -eq 2 ] && printf '%s' "$out" | grep -q '取数失败'; then
		printf 'PASS  self-test/取数失败报未定（退出码 2）\n'
		return 0
	fi
	printf 'FAIL  self-test/取数失败报未定（期望 2/取数失败 实得 %d）\n' "$rc"
	return 1
}

# 用法面：非正式版 tag 必须在取数之前拒绝（退出 2）。
rv_usage_case() {
	local rc
	bash "$rv_main" --tag latest >/dev/null 2>&1
	rc=$?
	if [ "$rc" -eq 2 ]; then
		printf 'PASS  self-test/非semver拒绝（退出码 2）\n'
		return 0
	fi
	printf 'FAIL  self-test/非semver拒绝（期望 2 实得 %d）\n' "$rc"
	return 1
}

rr3_rv_self_test() {
	local fail=0
	rv_case 五类全齐过 0 none "" || fail=1
	rv_case distro缺层红 1 distro-sha "distro 缺层：distro-sha256.txt" || fail=1
	rv_case 镜像缺架构红 1 index-riscv "缺架构：riscv64" || fail=1
	rv_case 固件缺引导红 1 fw-boot "固件 缺层：rr3-bootloader-esp32-dev.bin" || fail=1
	rv_case 证明面不足红 1 attest "证明面缺：https://spdx.dev/Document" || fail=1
	rv_class_case || fail=1
	rv_missing_case || fail=1
	rv_fetch_fail_case || fail=1
	rv_usage_case || fail=1
	[ "$fail" -eq 0 ] || return 3
	return 0
}
