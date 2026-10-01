#!/bin/bash
# post-build.sh —— 目标根文件系统装配（Buildroot post-build，宿主侧执行）。
# 运行环境由 Buildroot 提供：$TARGET_DIR（目标根）、$BINARIES_DIR（镜像产物目录）、
# $HOST_DIR（宿主工具）。档位：脚本就绪；实测=板上（串口出 shell + rrd 自启）。
#
# 四件事（对应 base/os/README.md 的交付物清单）：
#   1. rrd 开机自启（BusyBox init 的 /etc/init.d/S99rrd）；
#   2. /var/run tmpfs（rrd 默认 UDS 路径 /var/run/rock3-ipc.sock 落在易失区，
#      避免残留旧 socket 文件跨重启干扰 bind）；
#   3. data / rr_staging 挂载点（分区标签见 genimage.cfg，rr-ota 守护用例钉死）；
#   4. rrd 与模块产物安装位（交叉编译产物经 $BINARIES_DIR 注入，不在 Buildroot 里编译）。

set -eu
TARGET="${TARGET_DIR:?post-build.sh 需要 TARGET_DIR（Buildroot 提供）}"

# ---- 4. 产物安装位与目录骨架 -------------------------------------------------
install -d -m 0755 \
	"$TARGET/usr/bin" \
	"$TARGET/usr/lib/rr3/manifest" \
	"$TARGET/usr/lib/rr3/modules" \
	"$TARGET/var/lib/rr3/staging" \
	"$TARGET/data" \
	"$TARGET/extlinux"

# rrd 与模块二进制由 aarch64 交叉编译后放到 $BINARIES_DIR（交付方式见
# patches/README.md 的"产物注入"一节）；缺席只警告不阻断——镜像结构
# 仍应可构建，产物齐装是板上/容器侧收口项。
for bin in rrd rr3-ctl; do
	if [ -f "$BINARIES_DIR/$bin" ]; then
		install -m 0755 "$BINARIES_DIR/$bin" "$TARGET/usr/bin/$bin"
	else
		echo "post-build.sh: 警告：$BINARIES_DIR/$bin 缺席，跳过安装（产物注入未完成）" >&2
	fi
done
if [ -d "$BINARIES_DIR/rr3-modules" ]; then
	# 约定：$BINARIES_DIR/rr3-modules/<name>/{bin,manifest.toml}，逐模块就位
	for mod_dir in "$BINARIES_DIR"/rr3-modules/*/; do
		name="$(basename "$mod_dir")"
		install -d "$TARGET/usr/lib/rr3/modules/$name"
		[ -f "$mod_dir/manifest.toml" ] && \
			install -m 0644 "$mod_dir/manifest.toml" "$TARGET/usr/lib/rr3/manifest/$name.toml"
		[ -f "$mod_dir/bin" ] && \
			install -m 0755 "$mod_dir/bin" "$TARGET/usr/lib/rr3/modules/$name/bin"
	done
fi

# ---- 2. fstab：/var/run tmpfs + data / staging 挂载 --------------------------
cat > "$TARGET/etc/fstab" <<'EOF'
# <fs>                  <mountpoint>    <type>  <opts>                  <dump> <pass>
proc                    /proc           proc    defaults                0      0
devpts                  /dev/pts        devpts  defaults,gid=5,mode=620 0      0
tmpfs                   /dev/shm        tmpfs   mode=0777               0      0
tmpfs                   /tmp            tmpfs   mode=1777               0      0
tmpfs                   /var/run        tmpfs   mode=0755               0      0
tmpfs                   /var/log        tmpfs   mode=0755               0      0
# 用户数据：升级不动（base/ota/partition-layout.md 分区表）
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
		ARGS=""
		for m in /usr/lib/rr3/manifest/*.toml; do
			[ -e "$m" ] && ARGS="$ARGS --manifest $m"
		done
		start-stop-daemon -S -b -m -p /var/run/rrd.pid \
			-x /usr/bin/rrd -- --socket /var/run/rock3-ipc.sock $ARGS
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

echo "post-build.sh: 目标根装配完成（fstab + S99rrd + 产物安装位）"
