#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# crates 下载抖动兜底（本地与 CI 同口径）：esp 门的构建走镜像源，共享出口偶发
# "transfer too slow / transferred 0 bytes"（2026-11-15 实测：一轮 20 分钟的 esp 门
# 白跑在 winnow 的下载上）。cargo 自带重试默认 3 次、低速阈值 10B/30s 太紧，这里放宽；
# 依赖源与锁文件都不动。
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-120}"
export CARGO_HTTP_LOW_SPEED_LIMIT="${CARGO_HTTP_LOW_SPEED_LIMIT:-1}"
# ESP32 外设底座构建门（0.0.5 增补轮五 批B）。check.sh ④/⑥ 显式排除
# tools/esp32-periph-base：xtensa-esp32-espidf 目标编译需 esp fork 工具链
# （cargo +esp）+ build-std + esp-idf-sys（ESP-IDF v5.2.2 本地克隆 + ldproxy +
# 乐鑫 LLVM fork esp-clang），宿主工具链 check 必红，本脚本单独覆盖。SKIP 纪律
# 同 espidf-sentinel-check.sh：工具链缺位 = 本地 SKIP / CI 即红（tools/ci/skip.sh）；工具链在位但
# 编译失败 = FAIL；编译退出 0 但产物不在树 = FAIL。
# 批B 判据之①（构建面）在本门执法：SSD1306 I2C 驱动 + LEDC 双轴舵机 + UART2
# 行指令面在 xtensa-esp32-espidf 可编译。判据之②（板上真跑：I2C 扫描 0x3C
# 应答、OLED 出字、五点扫描）走 README runbook 真板验收，不属 CI。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
# 管理令牌门（httpd.rs 编译期断言）：本门只验构建面、不产生产镜像，显式允许
# 公开缺省令牌；生产镜像必须带 RR3_BOARD_ADMIN_TOKEN（见 tools/deploy/flash-esp32.sh）。
export RR3_ALLOW_DEV_ADMIN_TOKEN=1
fail=0

# ---- 甲面：xtensa 外设底座（工具链缺位 = 本地整门 SKIP / CI 即红，同 espidf-sentinel-check.sh 口径）----
cd "$root/tools/esp32-periph-base" || { echo "ERROR: 无法进入 tools/esp32-periph-base" >&2; exit 1; }
# 产物定位共享单点：target-dir 由仓根 .cargo/config.toml 移出源码区，不得写死
# crate 内 target/（否则编译成功也会被判"ELF 不在树"）。
. "$root/tools/ci/cargo-artifact.sh"
. "$root/tools/ci/skip.sh"

# W7-b1a 分区表核验时间锚：先于本门任何构建创建。缓存恢复的旧 partition-table.bin
# （早于锚点）不参与判定，避免"拿上一轮的绿表冒充本轮"。
part_marker=/tmp/rr3-part-marker-esp32
: > "$part_marker"

# ① esp fork 工具链（rustup alias esp，0.0.7 批A'）
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  rr3_skip "esp 工具链未安装（rustup toolchain list 无 'esp'）——esp32-periph-base 门不执行"
fi
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
  rr3_skip "esp 工具链 rustc 不可用（rustc +esp 失败）——esp32-periph-base 门不执行"
}
if [ ! -d "$esp_sysroot/lib/rustlib/src/rust" ]; then
  rr3_skip "esp 工具链缺 rust-src 源码树（build-std 依赖）——esp32-periph-base 门不执行"
fi
# ② ldproxy
if ! command -v ldproxy >/dev/null 2>&1; then
  rr3_skip "ldproxy 未安装（cargo install ldproxy）——esp32-periph-base 门不执行"
fi
# ③ ESP-IDF 本地克隆（版本钉死登记见 tools/espidf-std-hello/README.md）
if [ ! -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ]; then
  rr3_skip "ESP-IDF v5.2.2 本地克隆缺位——esp32-periph-base 门不执行"
fi
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
# ④ esp-clang（bindgen 需 Xtensa 后端）
if ! rr3_esp_clang_lib >/dev/null; then
  rr3_skip "esp-clang 缺位（~/.espup/esp-clang 无 libclang.dylib/.so）——esp32-periph-base 门不执行"
fi
# ⑤ 镜像注入（仓库源不硬编码镜像，同 espidf-sentinel-check.sh）
export IDF_GITHUB_ASSETS=dl.espressif.com/github_assets
export PIP_INDEX_URL="${PIP_INDEX_URL:-https://mirrors.cloud.tencent.com/pypi/simple}"
export PIP_DEFAULT_TIMEOUT=60

# ⑥ venv dist-info 兼容（espidf-sentinel-check.sh ⑥ 同坑同兜底）：pip≥24
#    规范化 dist-info 目录名（ruamel.yaml → ruamel_yaml），py3.9
#    importlib.metadata 按 dir stem 严格匹配点号名 → idf-component-manager 依赖
#    检查误报 not found → CMake Python 检查退 255。venv 落本工程 .embuild 下，
#    构建前改名兜底。
venv_sp=".embuild/espressif/python_env/idf5.2_py3.9_env/lib/python3.9/site-packages"
for d in "$venv_sp"/ruamel_yaml-*.dist-info; do
  if [ -d "$d" ]; then
    mv "$d" "${d/ruamel_yaml-/ruamel.yaml-}"
    echo "PASS  venv dist-info 兼容改名: ${d##*/} → ruamel.yaml-*"
  fi
done

# ⑧pre clippy 门（2026-11-21 治理）：esp fork 工具链实测带 clippy（0.1.97），以
# 固件最全特性档 autonomy,tts + all-targets 跑 -D warnings；其余档由既有五档
# build/test 编译面覆盖。测试目标在 panic=abort 下必须带 -Zpanic_abort_tests
# （与 ⑧ 单测编译同口径，否则 clippy 直接拒编测试目标）。
esp_clippy_log=/tmp/rr3-esp32-periph-clippy.log
esp_probe_rf="--cfg espidf_time64 -Zunstable-options -Cpanic=immediate-abort -Zpanic_abort_tests"
if ! cargo +esp clippy --version >/tmp/rr3-esp-clippy-ver 2>&1; then
  echo "FAIL  esp clippy 不可用（esp 工具链缺 clippy 组件）: $(tail -1 /tmp/rr3-esp-clippy-ver)" >&2
  fail=1
elif RUSTFLAGS="$esp_probe_rf" cargo +esp clippy --all-targets --features autonomy,tts \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' \
     -- -D warnings >"$esp_clippy_log" 2>&1; then
  echo "PASS  esp32-periph-base clippy -D warnings（autonomy,tts all-targets）"
else
  errs=$(grep -A1 -E '^error' "$esp_clippy_log" | head -12 | tr '\n' '|' | cut -c1-900)
  echo "FAIL  esp32-periph-base clippy -D warnings: ${errs}" >&2
  tail -3 "$esp_clippy_log" >&2
  fail=1
fi

# target-dir 由仓根 .cargo/config.toml 统一到 /tmp/rr3-build（P0-5），
# 严禁在源码区留构建产物；CI 与本地同口径。
elf="$(rr3_cargo_artifact xtensa-esp32-espidf debug esp32-periph-base)" || {
  echo "FAIL  无法解析 cargo target_directory（metadata 不可用）" >&2
  exit 1
}
if cargo +esp build \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$elf" ]; then
    echo "PASS  甲面 esp32-periph-base xtensa-esp32-espidf 构建（批B 判据之①构建面）: $elf"
  else
    echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
    fail=1
  fi
else
  echo "FAIL  esp32-periph-base xtensa-esp32-espidf 构建失败" >&2
  fail=1
fi

# ⑦b W4-1 I2S TX 验收档编译面（spk-tone feature；默认固件零行为变化）
if cargo +esp build --features spk-tone \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  echo "PASS  classic spk-tone 档构建（W4-1 I2S TX 验收面）"
else
  echo "FAIL  classic spk-tone 档构建失败" >&2
  fail=1
fi

# ⑦c W3-2a 自治档编译面（autonomy feature：SNTP 对时；默认固件零行为变化）
if cargo +esp build --features autonomy \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  echo "PASS  classic autonomy 档构建（W3-2a SNTP）"
else
  echo "FAIL  classic autonomy 档构建失败" >&2
  fail=1
fi

# ⑦e ADR-0030 全链档 no-http 编译面（网络照常起，HTTP 配置面关闭；W7 探针）
nohttp_elf=""
if cargo +esp build --features autonomy,tts,no-http \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  nohttp_elf="$(rr3_cargo_artifact xtensa-esp32-espidf debug esp32-periph-base)" \
    || { echo "FAIL  no-http 档无法解析 target_directory" >&2; fail=1; }
  if [ -n "${nohttp_elf:-}" ] && [ -f "$nohttp_elf" ]; then
    cp "$nohttp_elf" /tmp/rr3-build/esp32-classic-nohttp
    echo "PASS  classic autonomy+tts+no-http 档构建（W7 全链探针）"
  else
    echo "FAIL  no-http 档 ELF 不在树" >&2
    fail=1
  fi
else
  echo "FAIL  classic autonomy+tts+no-http 档构建失败" >&2
  fail=1
fi

# ⑦d W4-2 播放面编译（tts feature；与 autonomy 组合＝W6 全链固件形态；
#     与 spk-tone 互斥，故单独一条腿）
if cargo +esp build --features autonomy,tts \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  echo "PASS  classic autonomy+tts 档构建（W4-2 播放面）"
else
  echo "FAIL  classic autonomy+tts 档构建失败" >&2
  fail=1
fi

# ⑦g W7-b1a 分区表落地核验（ADR-0031）：spool 分区必须出现在构建产物的
#     partition-table.bin 里。CONFIG_PARTITION_TABLE_CUSTOM 未生效时编译照绿，
#     但板上没有 spool——配置漂移必须点名，不能靠烧录后才发现。
if rr3_partition_has xtensa-esp32-espidf spool "$part_marker" \
     "$root/tools/esp32-periph-base/partitions.csv"; then
  echo "PASS  classic 分区表含 spool 条目（W7-b1a 存储承载）"
else
  echo "FAIL  classic 分区表未含 spool（查 sdkconfig.defaults CUSTOM/FILENAME 与 partitions.csv）" >&2
  fail=1
fi

# ⑧ 纯逻辑单测编译面（批N13 件一：httpd 路由/漂移/JSON 形状单测；批N14 cred
#    零回显契约单测；批N23 件一起 atoms/<名>/mech.rs 机制核单测——xtensa 目标
#    本机不可跑，--no-run 只验测试码编译；xtensa 测试 harness 执行面归真板批）
esp_test_log=/tmp/rr3-esp32-periph-test.log
# RUSTFLAGS 一旦设置会**整体覆盖** .cargo/config.toml 的 target rustflags，故整条
# 重写（与 [target.xtensa-esp32-espidf] 同值，两处必须同批改）。补
# `-Zpanic_abort_tests`：panic=abort 下测试 harness 的唯一合法形状，缺失时 rustc
# 直接拒编（"building tests with panic=abort is not supported"）。
esp_test_rf="--cfg espidf_time64 -Zunstable-options -Cpanic=immediate-abort -Zpanic_abort_tests"
if RUSTFLAGS="$esp_test_rf" cargo +esp test --no-run --features spk-tone,autonomy \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' \
     >"$esp_test_log" 2>&1; then
  echo "PASS  甲面单测编译（--no-run）：板侧纯逻辑层单测在编（httpd/cred + atoms 机制核）"
else
  # 归因：本门在 CI 里只把末 4 条含 error 的行写进 check-run 注解，输出被吞掉
  # 就等于"红而无因"。压成单行（GitHub 会把多行注解拆散并截断）。
  errs=$(grep -E "error(\[|:)|panicked at" "$esp_test_log" | head -4 | tr '\n' ' ')
  echo "FAIL  esp32-periph-base 单测编译失败（cargo +esp test --no-run）: ${errs:0:900}" >&2
  tail -3 "$esp_test_log" >&2
  fail=1
fi

# ⑧b W4-2b 云 TTS/播放面测试码编译（tts 与 spk-tone 互斥，故单列一条；
#     xtensa 目标只验编译，用例执行归宿主机制核与真板）
esp_tts_test_log=/tmp/rr3-esp32-periph-tts-test.log
if RUSTFLAGS="$esp_test_rf" cargo +esp test --no-run --features autonomy,tts \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' \
     >"$esp_tts_test_log" 2>&1; then
  echo "PASS  甲面 tts 档单测编译（--no-run）：cloud/playout 测试码在编"
else
  errs=$(grep -E "error(\[|:)|panicked at" "$esp_tts_test_log" | head -4 | tr '\n' ' ')
  echo "FAIL  esp32-periph-base tts 单测编译失败: ${errs:0:900}" >&2
  tail -3 "$esp_tts_test_log" >&2
  fail=1
fi

# ⑨ release 构建面（生产烧录档）：dev 档 opt-level=2 只代表开发构建，
# release 的 codegen 差异必须独立出产物证据（与 S3 门 ⑨ 同口径）。
rel_elf="$(rr3_cargo_artifact xtensa-esp32-espidf release esp32-periph-base)" || {
  echo "FAIL  无法解析 cargo target_directory（metadata 不可用）" >&2
  exit 1
}
if cargo +esp build --release \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$rel_elf" ]; then
    echo "PASS  classic release 构建（生产烧录档）: $rel_elf"
    # 发布固件集随带同版 bootloader（与 publish firmware job 同一导航函数）：
    # release 面也必须在产出树找到它，否则发布面缺件。
    if ! rel_boot="$(rr3_idf_bootloader "$rel_elf")"; then
      echo "FAIL  classic release bootloader 不在产出树（发布集缺件）" >&2
      fail=1
    elif [ "$(head -c 1 "$rel_boot" | od -An -tx1 | tr -d ' \n')" != "e9" ]; then
      echo "FAIL  classic release bootloader magic≠e9: $rel_boot" >&2
      fail=1
    else
      echo "PASS  classic release bootloader 同树: $rel_boot"
    fi
  else
    echo "FAIL  classic release 编译退出 0 但 ELF 不在树: $rel_elf" >&2
    fail=1
  fi
else
  echo "FAIL  classic release 构建失败" >&2
  fail=1
fi

exit "$fail"
