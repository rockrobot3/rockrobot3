#!/usr/bin/env bash
# RK3566 参考机到货前静态点验（A3）：只查"数据面是否齐、是否自相矛盾"，
# 不冒充真板证据。缺件/字段不符退出 1；轴档案未标定只警告（归真板标定）。
#
# 用法：rk3566-onboard-check.sh [--strict]
#   --strict：轴档案缺失也判红（发布前使用）；缺省只警告。
set -euo pipefail

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
cd "$root"

strict=0
[ "${1:-}" = "--strict" ] && strict=1
fail=0
warn=0

ok() { echo "  PASS  $1"; }
bad() { echo "  FAIL  $1"; fail=1; }
warned() { echo "  WARN  $1"; warn=1; }

check_file() {
  [ -s "$1" ] && ok "$1" || bad "$1 缺席或为空"
}

echo "== RK3566 数据面 =="
for f in \
  profiles/default.toml profiles/distro.toml \
  hardware/default/wiring.toml hardware/distro/wiring.toml \
  contracts/platform/rk3566.toml contracts/slots.toml contracts/roles.toml \
  contracts/devices.toml contracts/topics.toml \
  base/os/buildroot/board/radxa-zero3/genimage.cfg \
  base/os/buildroot/board/radxa-zero3/extlinux.conf \
  base/os/distro/units/rrd.service; do
  check_file "$f"
done

for p in profiles/default.toml profiles/distro.toml; do
  grep -Eq '^chip = "rk3566"' "$p" && ok "$p chip=rk3566" || bad "$p chip 不是 rk3566"
  grep -Eq '^tier = "host"' "$p" && ok "$p tier=host" || bad "$p tier 不是 host"
  grep -Eq '^hal_backend = "linux"' "$p" && ok "$p hal_backend=linux" || bad "$p hal_backend 不是 linux"
done

echo "== 装配单引用面 =="
mods="$(awk -F'"' '/^"[a-z0-9._-]+" *= *"/ {print $4}' profiles/default.toml | sort -u)"
[ -n "$mods" ] || bad "default.toml bindings 为空"
for m in $mods; do
  [ -f "modules/$m/module.toml" ] && ok "bindings → modules/$m" || bad "bindings 引用未在册模块 $m"
done
for m in $mods; do
  [ -f "modules/$m/config.toml" ] || warned "modules/$m/config.toml 缺席（模块可能用内置缺省）"
done

echo "== 接线与轴档案 =="
grep -q '^\[\[wire\]\]' hardware/default/wiring.toml \
  && ok "hardware/default/wiring.toml 有 [[wire]]" \
  || bad "hardware/default/wiring.toml 无 [[wire]]"
grep -q '^\[\[wire\]\]' hardware/distro/wiring.toml \
  && ok "hardware/distro/wiring.toml 有 [[wire]]" \
  || bad "hardware/distro/wiring.toml 无 [[wire]]"
for d in default distro; do
  if compgen -G "hardware/$d/axis/*.toml" >/dev/null; then
    ok "hardware/$d/axis/*.toml 在位"
  else
    if [ "$strict" -eq 1 ]; then
      bad "hardware/$d/axis/*.toml 缺席（发布前必须标定）"
    else
      warned "hardware/$d/axis/*.toml 缺席（真板标定前允许，归 A-PLAT/A-VOL）"
    fi
  fi
done

echo "== 平台表来源注记 =="
grep -q '待器件\|演练实例\|来源' contracts/platform/rk3566.toml \
  && ok "rk3566.toml 保留来源/待器件注记（不冒充已核对）" \
  || warned "rk3566.toml 无来源/待器件注记（真板对账前应有）"

echo "== 结果 =="
if [ "$fail" -ne 0 ]; then
  echo "FAIL  RK3566 到货前静态点验未过（$warn 类警告）" >&2
  exit 1
fi
echo "PASS  RK3566 到货前静态点验通过（警告 $warn 类；真板证据不在本门）"
