#!/usr/bin/env bash
# modules/rr3-act + modules/rr3-sense 的 espidf 档编译面门（0.0.5 增补轮六 批N8
# 件三，MC-7 半边：rr3-act/rr3-sense 非 linux 目标构建）。两 crate 是**双档工程**：
# 宿主 linux 档照常进门禁 ④/⑥ 与 lint 扫面（不写死 [build] target、espidf 专属
# 依赖挂 target 表），本门只补 espidf 档——xtensa-esp32-espidf 目标编译需 esp
# fork 工具链（cargo +esp）+ build-std + esp-idf-sys（ESP-IDF v5.2.2 本地克隆 +
# ldproxy + 乐鑫 LLVM fork esp-clang），宿主工具链 check 必红，本脚本显式
# --target 单独覆盖（与 tools/ 专用门"整目录排除"的形状不同：那类工程 config
# 写死 target，本门不写死、按双档工程显式传）。SKIP 纪律同 espidf-sentinel-check.sh：
# 工具链缺位 = SKIP（退出 0 并明说）；工具链在位但编译失败 = FAIL；编译退出 0
# 但产物不在树 = FAIL。板侧固件 crate 收编与 [hardware] 进位归真板批（批N9），
# 本件范围红线 = 只建编译面。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
fail=0

# ---- 工具链缺位五查（缺一 = 整门 SKIP，同 espidf-sentinel-check.sh 口径）----
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  echo "SKIP  esp 工具链未安装（rustup toolchain list 无 'esp'）——rr3-act/rr3-sense espidf 门不执行"
  exit 0
fi
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
  echo "SKIP  esp 工具链 rustc 不可用（rustc +esp 失败）——rr3-act/rr3-sense espidf 门不执行"
  exit 0
}
if [ ! -d "$esp_sysroot/lib/rustlib/src/rust" ]; then
  echo "SKIP  esp 工具链缺 rust-src 源码树（build-std 依赖）——rr3-act/rr3-sense espidf 门不执行"
  exit 0
fi
if ! command -v ldproxy >/dev/null 2>&1; then
  echo "SKIP  ldproxy 未安装（cargo install ldproxy）——rr3-act/rr3-sense espidf 门不执行"
  exit 0
fi
if [ ! -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ]; then
  echo "SKIP  ESP-IDF v5.2.2 本地克隆缺位——rr3-act/rr3-sense espidf 门不执行"
  exit 0
fi
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
if [ ! -e "$HOME/.espup/esp-clang/libclang.dylib" ]; then
  echo "SKIP  esp-clang 缺位（~/.espup/esp-clang）——rr3-act/rr3-sense espidf 门不执行"
  exit 0
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
  elf="target/xtensa-esp32-espidf/debug/$mod"
  if cargo +esp build --target xtensa-esp32-espidf \
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
done

exit "$fail"
