#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Buildroot 出厂镜像内容只读断言（0.0.8 buildroot 轨）。genimage 成功不等于分区
# 内容对，本脚本把它变成可判的四组事实：
#   ① e2fsck 抓坏文件系统；② dumpe2fs 抓槽位标签漂移；③ debugfs 抓 rootfs 内容
#      （/usr/bin/rrd、S99rrd、/etc/fstab 四键、六模块各自的 manifest 与二进制、
#      装载正门输入（装配单/契约册/平台事实表/接线档）与运维入口——装配单声明什么，
#      镜像就得有什么，缺任一模块 rrd 装载门判 assemble_broken 拒起）；④ fdtdump 抓 u-boot.itb 缺
#      BL31（binman 成功但 FIT 里没有 bl31 = "能烧不能启"）＋ idbloader 体量下限；
#   ⑤ inittab 抓串口登录面（M1"串口出 shell"的前件：缺 respawn getty 时镜像能启动
#      却没人能登录，板上只剩 blind 串口）。
#
# 用法：bash tools/ci/platform/buildroot-image-check.sh [镜像目录] [宿主工具目录]
#   缺省 /tmp/br-out/images 与 /tmp/br-out/host（buildroot job 的固定产出位）。
# 退出码：0 = 全过；1 = 任一组红（错误标题沿用 CI 注解面 br-fs/br-fsck/br-label/
#   br-tool/br-rootfs/br-mount/br-fstab/br-s99rrd/br-fit/br-tpl/br-getty，取数面不变）。
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
  local p
  if [ ! -x "${host}/sbin/debugfs" ]; then
    echo "::error title=br-tool::host debugfs 缺席（host-e2fsprogs 未装 debugfs）"
    exit 1
  fi
  for p in /usr/bin/rrd /etc/init.d/S99rrd; do
    rr3_br_need "$p" "rootfs 未装入"
  done
}

# 单件在场判据（debugfs stat 看 Inode）；error 文案由调用方给。
rr3_br_need() {
  if ! "${host}/sbin/debugfs" -R "stat $1" "${d}/rr_a.ext4" 2>&1 | grep -q 'Inode:'; then
    echo "::error title=br-rootfs::rr_a.ext4 缺 $1（$2）"
    exit 1
  fi
  echo "PASS  rr_a.ext4 含 $1"
}

# 装配覆盖（批350）：装配单 enabled_slots 声明的六模块，镜像必须逐件带 manifest 与
# 二进制。此前只查 brain 两件——任何一模块漏装，rrd 装载门判 assemble_broken 拒起，
# 而镜像断言仍绿（批320 的 brain 漏装正是这条病形的实例）。
rr3_br_modules() {
  local m p
  for m in act connect interact sense power brain; do
    for p in module.toml "rr3-$m"; do
      rr3_br_need "/usr/lib/rockrobot3/modules/rr3-$m/$p" "装配缺模块"
    done
    echo "PASS  模块 rr3-${m}（manifest + binary）在场"
  done
}

# 装载正门输入与运维入口（批350）：S99rrd 的 --profile/--contracts 各有落点，缺席
# 即开机拒起；rr3-ctl / rr3-ota-ctl 是运维面入口，镜像缺件只在现场才发现。
# 成员清单对齐 rrd 装载真值集（profile/契约册/平台事实表/接线档）：本轨是 RK3566
# 参考机（distro 档 chip=rk3566），换机型档时这四类路径随装配单改，不在此盲猜。
rr3_br_install() {
  local p
  for p in /usr/lib/rockrobot3/profiles/distro.toml \
    /usr/lib/rockrobot3/contracts/topics.toml \
    /usr/lib/rockrobot3/contracts/platform/rk3566.toml \
    /usr/lib/rockrobot3/hardware/distro/wiring.toml \
    /usr/bin/rr3-ctl /usr/bin/rr3-ota-ctl; do
    rr3_br_need "$p" "装配缺安装位"
  done
  echo "PASS  装载正门输入（装配单/契约/平台表/接线档）与运维入口齐套"
}

# 挂载面（批343/348）：fstab 三键 + /dev 不重复挂载 + tmpfs 挂载点目录在场。
rr3_br_fstab() {
  local k p fstab
  fstab=$("${host}/sbin/debugfs" -R "cat /etc/fstab" "${d}/rr_a.ext4" 2>&1 || true)
  for k in sysfs "LABEL=rr_data" "LABEL=rr_staging"; do
    if ! printf '%s\n' "$fstab" | grep -q "$k"; then
      echo "::error title=br-fstab::rr_a.ext4 的 /etc/fstab 缺 '$k'"
      printf '%s\n' "$fstab"
      exit 1
    fi
  done
  echo "PASS  rr_a.ext4 /etc/fstab 含 sysfs/rr_data/rr_staging"
  # /dev 不得再写 devtmpfs 行（批348）：内核已按 CONFIG_DEVTMPFS_MOUNT=y 自动挂载，
  # 重复挂载每次启动多一条 "already mounted" 错误行；该前件由 kernel.fragment 显式
  # 钉住、构建期 .config 断言复核（buildroot.yml）。
  if printf '%s\n' "$fstab" | grep -qE '^devtmpfs[[:space:]]+/dev[[:space:]]'; then
    echo "::error title=br-fstab::/etc/fstab 仍写 devtmpfs /dev（内核已自动挂载，重复挂载会报错）"
    exit 1
  fi
  echo "PASS  /etc/fstab 不重复挂载 /dev（内核 devtmpfs 自动挂载）"
  # tmpfs 挂载点必须在根里先存在（批343）：mount -a 不建目录，缺 /var/run 时
  # rrd 的 pid 文件与 UDS bind 全失败，镜像"能启动但守护起不来"。
  for p in /var/run /var/log; do
    if ! "${host}/sbin/debugfs" -R "stat ${p}" "${d}/rr_a.ext4" 2>&1 | grep -q 'Inode:'; then
      echo "::error title=br-mount::rr_a.ext4 缺 tmpfs 挂载点 ${p}（mount -a 会失败）"
      exit 1
    fi
    echo "PASS  tmpfs 挂载点 ${p} 在场"
  done
}

# S99rrd 两件（批340/341）：装载正门参数同形 + 启动失败点名。
rr3_br_s99() {
  local k s99
  # 装载正门参数（批340）：S99rrd 必须与 rrd.service 的 ExecStart 同形传 --contracts
  # 与 --profile。历史上这里只传 --socket/--manifest，装载门十位判据一位不跑而 rrd
  # 退出 0（status 敞口 20 的病形）——参数漂移必须由镜像断言拦下。
  s99=$("${host}/sbin/debugfs" -R "cat /etc/init.d/S99rrd" "${d}/rr_a.ext4" 2>&1 || true)
  for k in "--contracts" "--profile"; do
    if ! printf '%s\n' "$s99" | grep -q -- "$k"; then
      echo "::error title=br-s99rrd::/etc/init.d/S99rrd 缺 ${k}（装载门不跑却会绿）"
      exit 1
    fi
  done
  echo "PASS  S99rrd 传 --contracts 与 --profile（装载正门同形）"
  # 启动失败点名（批341）：-b 后台化后 rrd 的 stderr 不落串口，装载门拒起时若连
  # 点名都没有，现场只剩"没有模块、没有原因"。脚本里必须有这条诊断与手工复现命令。
  if ! printf '%s\n' "$s99" | grep -q 'S99rrd: rrd 未存活'; then
    echo "::error title=br-s99rrd::S99rrd 缺启动失败点名（装载门拒起时启动面静默）"
    exit 1
  fi
  echo "PASS  S99rrd 含启动失败点名与手工复现命令"
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
rr3_br_modules
rr3_br_install
rr3_br_fstab
rr3_br_s99
rr3_br_getty
rr3_br_fit
