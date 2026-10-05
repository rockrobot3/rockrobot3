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
#   4. rrd 与模块产物安装位（交叉编译产物经 $BINARIES_DIR 注入，不在 Buildroot 里编译；
#      manifest 归位 /usr/lib/rr3/modules/<name>/module.toml，entry 改写为装机绝对路径，
#      config 从仓源随装——线上注册门禁拒绝相对 entry，模块开机即读同目录 config）。

set -eu
TARGET="${TARGET_DIR:?post-build.sh 需要 TARGET_DIR（Buildroot 提供）}"

# ---- 4. 产物安装位与目录骨架 -------------------------------------------------
install -d -m 0755 \
	"$TARGET/usr/bin" \
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

# 模块注入：$BINARIES_DIR/rr3-modules/<name>/{bin,manifest.toml}（两轨同源注入格式，
# 快路轨 rr3-pack 读同一格式，两轨不互抄装配）。装机布局对齐装配单语义：
#   /usr/lib/rr3/modules/<name>/{module.toml,<entry>,config.toml}
# 三处与旧版不同（0.0.2 保底轨修复，与快路轨 rr3-pack 同一套裁定）：
#   a. manifest 不再平铺 /usr/lib/rr3/manifest/<name>.toml——模块 rr3_config_load 读
#      manifest 同目录 config.toml，平铺下各模块 config 全部撞同一文件名，且 bin 的
#      相对 entry 解析到 manifest 所在目录必错位；
#   b. entry 改写为装机绝对路径（rr3-pack rr3_manifest_install_rewrite 同语义）：
#      模块开机读装机 manifest 原文上报，线上注册门禁拒绝相对值；
#   c. config 源真值从仓 modules/<name>/config.toml 随装（不是构建产物，不进注入格式；
#      仓根定位可用环境变量 RR3_REPO_ROOT 覆盖，缺省按脚本位置上溯五级）。
# 缺件 / entry 不可装机 = 警告 + 整模块跳过，与上面 rrd/rr3-ctl 缺席同口径：
# 镜像结构仍应可构建，产物齐装是板上收口项；但缺件还装 manifest 的话，rrd 会
# spawn 一个必退出的模块（重启环），故缺件时 manifest 一并不装。
REPO_ROOT="${RR3_REPO_ROOT:-}"
if [ -z "$REPO_ROOT" ]; then
	REPO_ROOT="$(cd "$(dirname "$0")/../../../../.." 2>/dev/null && pwd || true)"
fi
if [ -d "$BINARIES_DIR/rr3-modules" ]; then
	for mod_dir in "$BINARIES_DIR"/rr3-modules/*/; do
		name="$(basename "$mod_dir")"
		mod_dst="$TARGET/usr/lib/rr3/modules/$name"
		if [ ! -f "$mod_dir/manifest.toml" ]; then
			echo "post-build.sh: 警告：模块 $name 缺 manifest.toml，整模块跳过" >&2
			continue
		fi
		if [ ! -f "$mod_dir/bin" ]; then
			echo "post-build.sh: 警告：模块 $name 缺 bin，整模块跳过（缺 bin 还装 manifest 必重启环）" >&2
			continue
		fi
		# entry 行必须恰一条（rr3-pack 同门）：多行语义不明，零行不可装配
		entry_n="$(grep -c '^entry[[:space:]]*=' "$mod_dir/manifest.toml" || true)"
		if [ "$entry_n" != "1" ]; then
			echo "post-build.sh: 警告：模块 $name 的 manifest 有 $entry_n 条 entry 行（须恰一条），整模块跳过" >&2
			continue
		fi
		entry_rel="$(sed -n 's/^entry[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$mod_dir/manifest.toml" | head -n 1)"
		if [ -z "$entry_rel" ]; then
			echo "post-build.sh: 警告：模块 $name 的 entry 行不可解析（须双引号值），整模块跳过" >&2
			continue
		fi
		# entry 必须可装机：仓相对、不含 . / .. 段；且不含 sed 分隔符 |（改写安全）
		case "$entry_rel" in
			/*|.|..|./*|../*|*/.|*/..|*/./*|*/../*|*\|*)
				echo "post-build.sh: 警告：模块 $name entry 不可装机（$entry_rel：绝对路径 / . 与 .. 段 / 含 |），整模块跳过" >&2
				continue
				;;
		esac
		entry_dir="$(dirname "$entry_rel")"
		if [ "$entry_dir" = "." ]; then
			install -d "$mod_dst"
		else
			install -d "$mod_dst/$entry_dir"
		fi
		# bin 按入口落位：注入格式的固定名 bin 装到 entry 指向的装机相对路径
		install -m 0755 "$mod_dir/bin" "$mod_dst/$entry_rel"
		# entry 绝对化：仅引号内的值换成装机绝对路径，其余字节原样
		sed "s|entry\([[:space:]]*=[[:space:]]*\)\"[^\"]*\"|entry\1\"/usr/lib/rr3/modules/$name/$entry_rel\"|" \
			"$mod_dir/manifest.toml" > "$mod_dst/module.toml.tmp"
		chmod 0644 "$mod_dst/module.toml.tmp"
		mv "$mod_dst/module.toml.tmp" "$mod_dst/module.toml"
		# 改写复核：装机 manifest 的 entry 必须恰为新绝对路径（防同型错位回归）
		check="$(sed -n 's/^entry[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$mod_dst/module.toml" | head -n 1)"
		if [ "$check" != "/usr/lib/rr3/modules/$name/$entry_rel" ]; then
			echo "post-build.sh: 警告：模块 $name entry 改写复核不一致（得 $check），整模块跳过" >&2
			rm -rf "$mod_dst"
			continue
		fi
		if [ -n "$REPO_ROOT" ] && [ -f "$REPO_ROOT/modules/$name/config.toml" ]; then
			install -m 0644 "$REPO_ROOT/modules/$name/config.toml" "$mod_dst/config.toml"
		else
			echo "post-build.sh: 警告：模块 $name 的 config.toml 未随装（RR3_REPO_ROOT=$REPO_ROOT 下 modules/$name/config.toml 缺席）——模块开机即读 manifest 同目录 config，缺件必重启环" >&2
		fi
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
		for m in /usr/lib/rr3/modules/*/module.toml; do
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
