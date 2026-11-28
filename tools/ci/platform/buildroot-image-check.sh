#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Buildroot 出厂镜像内容只读断言（0.0.8 buildroot 轨）。genimage 成功不等于分区
# 内容对，本脚本把它变成可判的四组事实：
#   ① e2fsck 抓坏文件系统；② dumpe2fs 抓槽位标签漂移；③ debugfs 抓 rootfs 内容
#      （/usr/bin/rrd、S99rrd、/etc/fstab 四键、brain 的 manifest 与二进制——装配单
#      enabled_slots 含 rr3-brain，镜像缺它即开机拒启）；④ fdtdump 抓 u-boot.itb 缺
#      BL31（binman 成功但 FIT 里没有 bl31 = "能烧不能启"）＋ idbloader 体量下限；
#   ⑤ inittab 抓串口登录面（M1"串口出 shell"的前件：缺 respawn getty 时镜像能启动
#      却没人能登录，板上只剩 blind 串口）。
#
# 用法：bash tools/ci/platform/buildroot-image-check.sh [镜像目录] [宿主工具目录]
#   缺省 /tmp/br-out/images 与 /tmp/br-out/host（buildroot job 的固定产出位）。
# 退出码：0 = 全过；1 = 任一组红（错误标题沿用 CI 注解面 br-fs/br-fsck/br-label/
#   br-tool/br-rootfs/br-fstab/br-fit/br-tpl/br-getty，取数面不变）。
set -euo pipefail

d="${1:-/tmp/br-out/images}"
host="${2:-/tmp/br-out/host}"

# 五个 ext4 分区：文件名 → 期望卷标（与 genimage.cfg / 分区布局文档同源）。
rr3_br_fsck() {
  local spec f want img got
  for spec in rr_a:rr_a rr_b:rr_b rr_factory:rr_factory rr_staging:rr_staging data:rr_data; do
    f="${spec%%:*}"
    want="${spec##*:}"
    img="${d}/${f}.ext4"
    if [ ! -s "$img" ]; then
      echo "::error title=br-fs::${f}.ext4 缺失"
      exit 1
    fi
    if ! "${host}/sbin/e2fsck" -fn "$img" >"/tmp/fsck-${f}.log" 2>&1; then
      echo "::error title=br-fsck::${f}.ext4 未过 e2fsck -fn"
      tail -8 "/tmp/fsck-${f}.log"
      exit 1
    fi
    got=$("${host}/sbin/dumpe2fs" -h "$img" 2>/dev/null | sed -n 's/^Filesystem volume name:[[:space:]]*//p')
    if [ "$got" != "$want" ]; then
      echo "::error title=br-label::${f}.ext4 标签 '$got' != '$want'（槽位标签漂移）"
      exit 1
    fi
    echo "PASS  ${f}.ext4 e2fsck + label=${got}"
  done
}

# rootfs 内容：只查 fstab 不够——空文件系统也可能有标签；rrd 与 S99rrd 必须真在
# rr_a.ext4 里，才证明 srcpath 生效。
rr3_br_rootfs() {
  local p k fstab
  if [ ! -x "${host}/sbin/debugfs" ]; then
    echo "::error title=br-tool::host debugfs 缺席（host-e2fsprogs 未装 debugfs）"
    exit 1
  fi
  # brain 的两件同断言（批320）：distro 装配单声明了 cognition 四格，镜像必须带
  # manifest 与二进制；只装五模块时开机有"manifest 读取失败"拒绝启动，镜像却绿。
  for p in /usr/bin/rrd /etc/init.d/S99rrd \
    /usr/lib/rockrobot3/modules/rr3-brain/module.toml \
    /usr/lib/rockrobot3/modules/rr3-brain/rr3-brain; do
    if ! "${host}/sbin/debugfs" -R "stat ${p}" "${d}/rr_a.ext4" 2>&1 | grep -q 'Inode:'; then
      echo "::error title=br-rootfs::rr_a.ext4 缺 ${p}（rootfs 未装入）"
      exit 1
    fi
    echo "PASS  rr_a.ext4 含 ${p}"
  done
  fstab=$("${host}/sbin/debugfs" -R "cat /etc/fstab" "${d}/rr_a.ext4" 2>&1 || true)
  for k in sysfs devtmpfs "LABEL=rr_data" "LABEL=rr_staging"; do
    if ! printf '%s\n' "$fstab" | grep -q "$k"; then
      echo "::error title=br-fstab::rr_a.ext4 的 /etc/fstab 缺 '$k'"
      printf '%s\n' "$fstab"
      exit 1
    fi
  done
  echo "PASS  rr_a.ext4 /etc/fstab 含 sysfs/devtmpfs/rr_data/rr_staging"
}

# FIT 与 TPL：u-boot.itb 缺 atf-bl31 = 能烧不能启；idbloader 体量下限抓缺 TPL。
rr3_br_fit() {
  local sz
  if ! "${host}/bin/fdtdump" "${d}/u-boot.itb" 2>/dev/null | grep -q 'atf-bl31'; then
    echo "::error title=br-fit::u-boot.itb 缺 atf-bl31（BL31 未进 FIT）"
    exit 1
  fi
  echo "PASS  u-boot.itb 含 atf-bl31"
  sz=$(stat -c%s "${d}/idbloader.img")
  if [ "$sz" -lt 100000 ]; then
    echo "::error title=br-tpl::idbloader.img 仅 ${sz} B，疑似缺 TPL"
    exit 1
  fi
  echo "PASS  idbloader.img ${sz} B（含 TPL/SPL）"
}

# 串口登录面：Buildroot 缺省 getty 落 console（BR2_TARGET_GENERIC_GETTY 默认 y、
# port=console），console 即 extlinux cmdline 的 ttyS2。缺这条时镜像能启动、串口却
# 没有登录 prompt——M1 的"串口出 shell"就无从谈起（板上只剩 blind 串口）。
rr3_br_getty() {
  local initab line
  initab=$("${host}/sbin/debugfs" -R "cat /etc/inittab" "${d}/rr_a.ext4" 2>&1 || true)
  line=$(printf '%s\n' "$initab" | grep -E '^[^#].*:respawn:/sbin/getty' | head -1)
  if [ -z "$line" ]; then
    echo "::error title=br-getty::rr_a.ext4 的 /etc/inittab 无 respawn getty（串口无登录面）"
    printf '%s\n' "$initab"
    exit 1
  fi
  echo "PASS  inittab getty: ${line}"
}

rr3_br_fsck
rr3_br_rootfs
rr3_br_getty
rr3_br_fit
