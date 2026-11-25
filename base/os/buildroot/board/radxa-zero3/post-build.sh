#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# post-build.sh —— 目标根文件系统装配（Buildroot post-build，宿主侧执行）。
# 运行环境由 Buildroot 提供：${TARGET_DIR}（目标根）、${BINARIES_DIR}（镜像产物目录）、
# ${HOST_DIR}（宿主工具）。档位：脚本就绪；实测=板上（串口出 shell + rrd 自启）。
#
# 五件事（对应 base/os/README.md 的交付物清单）：
#   1. rrd 开机自启（BusyBox init 的 /etc/init.d/S99rrd）；
#   2. /var/run tmpfs（rrd 默认 UDS 路径 /var/run/rock3-ipc.sock 落在易失区，
#      避免残留旧 socket 文件跨重启干扰 bind）；
#   3. data / rr_staging 挂载点（分区标签见 genimage.cfg，rr-ota 守护用例钉死）；
#   4. rrd 与模块产物安装位（交叉编译产物经 $BINARIES_DIR 注入，不在 Buildroot 里编译；
#      manifest 归位 /usr/lib/rockrobot3/modules/<name>/module.toml，entry 改写为装机绝对路径，
#      config 从仓源随装——线上注册门禁拒绝相对 entry，模块开机即读同目录 config）；
#   5. 装载消费真值集装机（profiles/contracts/hardware 三目录随装，增补轮六 基-25——
#      装载必需件缺席的洞此前在 buildroot 装机侧完全不可见，块-15）；
#   6. S99rrd 传 --contracts/--profile 启用装载正门（基-23：出厂轨装载门此前不存在——
#      不传 --profile 时 rrd 走 --manifest 后门、十条校验一个判定位都不跑，L-31）。
#
# 安装位与 deb 快路轨同根 /usr/lib/rockrobot3（rrd.service 头注"两轨同一组安装位"
# 纪律，基-23 由假变真）：这不只是口径美观——rrd 装载链由装配单路径上两级推导
# modules-root（profile.rs），S99rrd 传 --profile /usr/lib/rockrobot3/profiles/distro.toml
# 后 manifests 必然落 /usr/lib/rockrobot3/modules/<名>/module.toml，模块装机位不同根
# 即 spawn 必败。数据面 /var/lib/rr3/staging 不随根动（rr_staging 分区挂载点，
# 分区布局与OTA槽位.md 钉死，属分区布局不属安装位）。

set -eu
TARGET="${TARGET_DIR:?post-build.sh 需要 TARGET_DIR（Buildroot 提供）}"

# ---- 4. 产物安装位与目录骨架 -------------------------------------------------
install -d -m 0755 \
	"$TARGET/usr/bin" \
	"$TARGET/usr/lib/rockrobot3/modules" \
	"$TARGET/var/lib/rr3/staging" \
	"$TARGET/data" \
	"$TARGET/extlinux" \
	"$BINARIES_DIR/extlinux"

# genimage 的 --inputpath 是 $BINARIES_DIR：boot.vfat 的 files 里
# "extlinux/extlinux.conf" 必须在 BINARIES_DIR 下真实存在，否则全量 make
# 在 post-image 才报"文件不存在"。extlinux.conf 是板级静态件，不从 TARGET
# 搬（TARGET 里的同路径只作装机面留档）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
install -m 0644 "$SCRIPT_DIR/extlinux.conf" "$BINARIES_DIR/extlinux/extlinux.conf"
install -m 0644 "$SCRIPT_DIR/extlinux.conf" "$TARGET/extlinux/extlinux.conf"

# ---- 4/5. 产物 / 模块 / 装载真值集：单一装配程序 rr3-pack -------------------
# 快路轨（deb）与保底轨（Buildroot）共用同一份装配裁定：三件入口、模块树
# （manifest entry 绝对化、config/atoms/composite 随装）、装载真值集
# （profiles/contracts/hardware 按 rrd rr3_truth_set 清单）全部由 rr3-pack
# 落位。本脚本不再保留第二份 entry 改写与文件清单（旧实现见 0.0.2 保底轨）。
# rr3-pack 由 BINARIES_DIR 注入（宿主原生构建，随 cross-artifacts 产物同级）。
# 缺产物/坏模块 = 整镜像构建失败：装机半成品比不出镜像更贵（rrd 会 spawn
# 必退出模块形成重启环）。
REPO_ROOT="${RR3_REPO_ROOT:-}"
if [ -z "$REPO_ROOT" ]; then
	REPO_ROOT="$(cd "$(dirname "$0")/../../../../.." 2>/dev/null && pwd || true)"
fi
PACK="$BINARIES_DIR/rr3-pack"
if [ -z "$REPO_ROOT" ] || [ ! -d "$REPO_ROOT" ]; then
	echo "post-build.sh: ERROR：仓根不可达（RR3_REPO_ROOT=${RR3_REPO_ROOT:-未设}）——装配真值集无法落位" >&2
	exit 1
fi
if [ ! -x "$PACK" ]; then
	echo "post-build.sh: ERROR：$PACK 缺席——宿主 rr3-pack 须随 BINARIES_DIR 注入（两轨同一装配程序）" >&2
	exit 1
fi
"$PACK" --payload "$BINARIES_DIR" --repo "$REPO_ROOT" \
	--install-root "$TARGET" --profile "${RR3_PROFILE:-distro}" || {
	echo "post-build.sh: ERROR：rr3-pack 目标根装配失败（见上一条点名）" >&2
	exit 1
}
echo "post-build.sh: 产物/模块/真值集已由 rr3-pack 装配（profile=${RR3_PROFILE:-distro}）"

# ---- 2. fstab：内核伪文件系统 + /var/run tmpfs + data / staging 挂载 ----------
# sysfs/devtmpfs 必须显式列出：本轨是 BusyBox init，唯一的伪文件系统挂载器是
# inittab 的 `mount -a`（读本文件）。缺 sysfs 时 /sys 为空（模块拿不到设备树与
# class 节点）；devtmpfs 若内核未开自动挂载，/dev 下连 null 都没有。
cat > "$TARGET/etc/fstab" <<'EOF'
# <fs>                  <mountpoint>    <type>  <opts>                  <dump> <pass>
proc                    /proc           proc    defaults                0      0
sysfs                   /sys            sysfs   defaults                0      0
devtmpfs                /dev            devtmpfs mode=0755,nosuid       0      0
devpts                  /dev/pts        devpts  defaults,gid=5,mode=620 0      0
tmpfs                   /dev/shm        tmpfs   mode=0777               0      0
tmpfs                   /tmp            tmpfs   mode=1777               0      0
tmpfs                   /var/run        tmpfs   mode=0755               0      0
tmpfs                   /var/log        tmpfs   mode=0755               0      0
# 用户数据：升级不动（docs/design/分区布局与OTA槽位.md 分区表）
LABEL=rr_data           /data           ext4    defaults                0      1
# 模块级更新暂存区（ADR-0007 决策二），rrd 拉包 → 校验 → 落位
LABEL=rr_staging        /var/lib/rr3/staging ext4 defaults              0      2
EOF

# ---- 1. rrd 开机自启（BusyBox init）-----------------------------------------
cat > "$TARGET/etc/init.d/S99rrd" <<'EOF'
#!/bin/sh
# rockrobot3 守护进程自启（post-build.sh 生成）。最后一位 S99：
# 等所有内核驱动与基础服务就位后再起 rrd。
case "$1" in
	start)
		[ -x /usr/bin/rrd ] || exit 0
		# 装载正门（基-23）：与发行版快路轨 rrd.service 的 ExecStart 三参数同形——
		# 两轨注入约定同源，装配链（装载/查册/缺格判定/manifest 解析）在 bind 前完成，
		# 缺册/缺格 fail-closed 点名。不再逐个 --manifest（与 --profile 同给 rrd 在
		# parse 处即拒）：manifests 由装配单按 modules-root（/usr/lib/rockrobot3，
		# 装配单路径上两级）推导，模块装机布局见本脚本第 4 件。
		# 机型档选型：distro.toml = RK3566 参考机 host 档（chip=rk3566，与 rrd.service
		# 缺省锚点同源）；机型专属装配单归真板档（M1）另立后此处随改。
		# 硬件音量下限预置（批N38 件一，与发行版 rrd.service 的 ExecStartPre 同语义同源）：
		# ALSA 混音器电平易失——重启回落 38%（-23dB，批N37 真板实测）；无 amixer 或卡名
		# 不在则点名跳过，**不阻断启动**（有界 fail-open，与装载门 fail-closed 不同面）。
		if command -v amixer >/dev/null 2>&1; then
			amixer -c Device sset PCM 100% >/dev/null 2>&1 || \
				echo "S99rrd: amixer 预置跳过（卡名 Device 不在）"
		else
			echo "S99rrd: amixer 缺席，跳过硬件电平预置"
		fi
		# 重启执行面解锁（与 distro 的 rrd.service 同一口径）：本轨是 BusyBox init，
		# rrd 的 ctl.reboot 走 /sbin/reboot 回退（systemctl 不在）；面板/运维 reboot
		# 是产品动作，缺省恒拒只保留给 e2e/调试路径。
		export RR3_REBOOT_OK=1
		start-stop-daemon -S -b -m -p /var/run/rrd.pid \
			-x /usr/bin/rrd -- --socket /var/run/rock3-ipc.sock \
			--contracts /usr/lib/rockrobot3/contracts \
			--profile /usr/lib/rockrobot3/profiles/distro.toml
		;;
	stop)
		start-stop-daemon -K -p /var/run/rrd.pid
		;;
	restart)
		"$0" stop
		"$0" start
		;;
	*)
		echo "Usage: $0 {start|stop|restart}"
		exit 1
		;;
esac
EOF
chmod 0755 "$TARGET/etc/init.d/S99rrd"

echo "post-build.sh: 目标根装配完成（fstab + S99rrd 装载正门 + 产物安装位）"
