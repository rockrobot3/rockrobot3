#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# crates 下载抖动兜底（本地与 CI 同口径）：esp 门的构建走镜像源，共享出口偶发
# "transfer too slow / transferred 0 bytes"（2026-11-15 实测：一轮 20 分钟的 esp 门
# 白跑在 winnow 的下载上）。cargo 自带重试默认 3 次、低速阈值 10B/30s 太紧，这里放宽；
# 依赖源与锁文件都不动。
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-120}"
export CARGO_HTTP_LOW_SPEED_LIMIT="${CARGO_HTTP_LOW_SPEED_LIMIT:-1}"
# modules/rr3-act + modules/rr3-sense 的 espidf 档编译面门（0.0.5 增补轮六 批N8
# 件三，MC-7 半边：rr3-act/rr3-sense 非 linux 目标构建）。两 crate 是**双档工程**：
# 宿主 linux 档照常进门禁 ④/⑥ 与 lint 扫面（不写死 [build] target、espidf 专属
# 依赖挂 target 表），本门只补 espidf 档——xtensa-esp32-espidf 目标编译需 esp
# fork 工具链（cargo +esp）+ build-std + esp-idf-sys（ESP-IDF v5.2.2 本地克隆 +
# ldproxy + 乐鑫 LLVM fork esp-clang），宿主工具链 check 必红，本脚本显式
# --target 单独覆盖（与 tools/ 专用门"整目录排除"的形状不同：那类工程 config
# 写死 target，本门不写死、按双档工程显式传）。SKIP 纪律同 espidf-sentinel-check.sh：
# 工具链缺位 = 本地 SKIP / CI 即红（tools/ci/lib/skip.sh）；工具链在位但编译失败 = FAIL；编译退出 0
# 但产物不在树 = FAIL。板侧固件 crate 收编与 [hardware] 进位归真板批（批N9），
# 本件范围红线 = 只建编译面。
#
# 芯片参数（2026-11-16 流水线按芯片拆腿）：`$1` ∈ {esp32, esp32s3, all}，缺省 all。
# 流水线的 classic 腿传 esp32（只跑 classic 构建面）、S3 腿传 esp32s3（只跑 S3 check
# 面），失败归因不跨芯片；本地与其余调用方不传参，整门口径与此前完全一致。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
# 产物定位共享单点：target-dir 由仓根 .cargo/config.toml 移出源码区，不得写死
# crate 内 target/（否则编译成功也会被判"ELF 不在树"）。
. "$root/tools/ci/lib/cargo-artifact.sh"
. "$root/tools/ci/lib/skip.sh"
fail=0

# 芯片档解析放在工具链五查之前：参数笔误要立刻报错，不能先花时间查工具链。
chip="${1:-all}"
case "${chip}" in
  esp32|esp32s3|all) ;;
  *)
    echo "FAIL  未知芯片档「${chip}」——只认 esp32 / esp32s3 / all" >&2
    exit 2
    ;;
esac

# ---- 工具链缺位五查（缺一 = 整门 SKIP，同 espidf-sentinel-check.sh 口径）----
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  rr3_skip "esp 工具链未安装（rustup toolchain list 无 'esp'）——rr3-act/rr3-sense espidf 门不执行"
fi
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
  rr3_skip "esp 工具链 rustc 不可用（rustc +esp 失败）——rr3-act/rr3-sense espidf 门不执行"
}
if [ ! -d "$esp_sysroot/lib/rustlib/src/rust" ]; then
  rr3_skip "esp 工具链缺 rust-src 源码树（build-std 依赖）——rr3-act/rr3-sense espidf 门不执行"
fi
if ! command -v ldproxy >/dev/null 2>&1; then
  rr3_skip "ldproxy 未安装（cargo install ldproxy）——rr3-act/rr3-sense espidf 门不执行"
fi
if [ ! -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ]; then
  rr3_skip "ESP-IDF v5.2.2 本地克隆缺位——rr3-act/rr3-sense espidf 门不执行"
fi
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
if ! rr3_esp_clang_lib >/dev/null; then
  rr3_skip "esp-clang 缺位（~/.espup/esp-clang 无 libclang.dylib/.so）——rr3-act/rr3-sense espidf 门不执行"
fi
# 镜像注入（仓库源不硬编码镜像，同 espidf-sentinel-check.sh）
export IDF_GITHUB_ASSETS=dl.espressif.com/github_assets
export PIP_INDEX_URL="${PIP_INDEX_URL:-https://mirrors.cloud.tencent.com/pypi/simple}"
export PIP_DEFAULT_TIMEOUT=60

# 两模块逐一 venv dist-info 兜底 + 显式 --target 构建 + ELF 出树断言。
# venv dist-info 兼容（espidf-sentinel-check.sh ⑥ 同坑同兜底）：pip≥24 规范化
# dist-info 目录名（ruamel.yaml → ruamel_yaml），py3.9 importlib.metadata 按 dir
# stem 严格匹配点号名 → idf-component-manager 依赖检查误报 not found → CMake
# Python 检查退 255。venv 落各自工程 .embuild 下，构建前改名兜底。
for mod in rr3-act rr3-sense; do
  cd "$root/modules/$mod" || { echo "ERROR: 无法进入 modules/$mod" >&2; exit 1; }
  venv_sp=".embuild/espressif/python_env/idf5.2_py3.9_env/lib/python3.9/site-packages"
  for d in "$venv_sp"/ruamel_yaml-*.dist-info; do
    if [ -d "$d" ]; then
      mv "$d" "${d/ruamel_yaml-/ruamel.yaml-}"
      echo "PASS  $mod venv dist-info 兼容改名: ${d##*/} → ruamel.yaml-*"
    fi
  done
  # classic 面（esp32 档）：S3 腿跳过，避免同一条腿为另一芯片付构建时间。
  if [ "${chip}" != esp32s3 ]; then
    elf="$(rr3_cargo_artifact xtensa-esp32-espidf debug "$mod")" || {
      echo "FAIL  $mod 无法解析 cargo target_directory（metadata 不可用）" >&2
      fail=1
      continue
    }
    if cargo +esp build --locked --target xtensa-esp32-espidf \
         --config 'source.crates-io.replace-with="rsproxy-sparse"' \
         --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
      if [ -f "$elf" ]; then
        echo "PASS  $mod xtensa-esp32-espidf 构建（MC-7 半边编译面）: $elf"
      else
        echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
        fail=1
      fi
    else
      echo "FAIL  $mod xtensa-esp32-espidf 构建失败" >&2
      fail=1
    fi
  fi
  # S3 编译面（W1/T2 另一芯片档）：check 不链接，只验 target 无关代码在
  # xtensa-esp32s3-espidf 同样可编；esp-idf-sys 按 MCU=esp32s3 选 sdkconfig。
  # 工具链不认该目标 = 本地 SKIP 该腿 / CI 即红；classic 腿跳过。
  if [ "${chip}" != esp32 ]; then
    if rustc +esp --print target-list 2>/dev/null | grep -qx 'xtensa-esp32s3-espidf'; then
      if MCU=esp32s3 cargo +esp check --locked --target xtensa-esp32s3-espidf \
           --config 'source.crates-io.replace-with="rsproxy-sparse"' \
           --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
        echo "PASS  $mod xtensa-esp32s3-espidf check（S3 芯片档编译面）"
      else
        echo "FAIL  $mod xtensa-esp32s3-espidf check 失败" >&2
        fail=1
      fi
    else
      if [ "${CI:-}" = "true" ] || [ "${RR3_CI_STRICT:-}" = "1" ]; then
        echo "FAIL  esp 工具链不认 xtensa-esp32s3-espidf——$mod 的 S3 腿未执行（CI 内 SKIP 即红）" >&2
        fail=1
      else
        echo "SKIP  esp 工具链不认 xtensa-esp32s3-espidf——$mod 的 S3 腿不执行"
      fi
    fi
  fi
done

exit "$fail"
