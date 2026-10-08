#!/usr/bin/env bash
# ESP-IDF std 构建链冒烟门（0.0.5 增补轮四 批W1）。check.sh ④ 显式排除 tools/espidf-std-hello：
# xtensa-esp32-espidf 目标编译需 esp fork 工具链（cargo +esp）+ build-std + esp-idf-sys
# （ESP-IDF v5.2.2 本地克隆 + ldproxy + 乐鑫 LLVM fork esp-clang——上游 libclang 无 Xtensa
# 后端，bindgen 会报 unknown target triple 'xtensa'），宿主工具链 check 必红，本脚本单独覆盖。
# 纪律：工具链缺位 = SKIP（退出 0 并明说，同 esp-uart-echo-check.sh 口径）；
#       工具链在位但编译失败 = FAIL；编译退出 0 但 ELF 不在树 = FAIL。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
cd "$root/tools/espidf-std-hello" || { echo "ERROR: 无法进入 tools/espidf-std-hello" >&2; exit 1; }

export PATH="$HOME/.cargo/bin:$PATH"

# ① esp fork 工具链（rustup alias esp → esp-1.90.0.0，0.0.7 批A'）
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  echo "SKIP  esp 工具链未安装（rustup toolchain list 无 'esp'）——espidf std 冒烟门不执行，安装方式见 0.0.7 方案批A'"
  exit 0
fi
# esp fork 工具链的 rust-src 不走 rustup component 注册（espup 直装源码树，
# `rustup component list` 查不到），判据用 sysroot 内的 std 源码目录
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
  echo "SKIP  esp 工具链 rustc 不可用（rustc +esp 失败）——espidf std 冒烟门不执行"
  exit 0
}
if [ ! -d "$esp_sysroot/lib/rustlib/src/rust" ]; then
  echo "SKIP  esp 工具链缺 rust-src 源码树（$esp_sysroot/lib/rustlib/src/rust 不在，build-std 依赖）——espidf std 冒烟门不执行"
  exit 0
fi

# ② ldproxy（链接代理：--ldproxy-linker 注入 xtensa gcc，见本目录 build.rs 注释）
if ! command -v ldproxy >/dev/null 2>&1; then
  echo "SKIP  ldproxy 未安装（cargo install ldproxy）——espidf std 冒烟门不执行"
  exit 0
fi

# ③ ESP-IDF 本地克隆（裁定 Q5：IDF 不进仓，版本钉死登记见本目录 README.md）
if [ ! -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ]; then
  echo "SKIP  ESP-IDF v5.2.2 本地克隆缺位（~/.espressif/frameworks/esp-idf-v5.2.2）——espidf std 冒烟门不执行，克隆方式见本目录 README.md"
  exit 0
fi
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"

# ④ esp-clang（乐鑫 LLVM fork：bindgen 需 Xtensa 后端；esp-idf-sys 经 espup 惯例
#    symlink ~/.espup/esp-clang 自动发现并覆盖 LIBCLANG_PATH，见 common.rs setup_clang_env）
if [ ! -e "$HOME/.espup/esp-clang/libclang.dylib" ]; then
  echo "SKIP  esp-clang 缺位（~/.espup/esp-clang 无 libclang.dylib）——espidf std 冒烟门不执行，安装方式见本目录 README.md"
  exit 0
fi

# ⑤ 镜像注入（仓库源不硬编码镜像，他人环境各异，随本机；缺失时 embuild 走官方源）
export IDF_GITHUB_ASSETS=dl.espressif.com/github_assets
export PIP_INDEX_URL="${PIP_INDEX_URL:-https://mirrors.cloud.tencent.com/pypi/simple}"
export PIP_DEFAULT_TIMEOUT=60

# ⑥ venv dist-info 兼容：pip≥24 规范化 dist-info 目录名（ruamel.yaml → ruamel_yaml），
#    py3.9 importlib.metadata 按 dir stem 严格匹配点号名 → idf-component-manager 依赖检查
#    误报 not found。venv 已存在且带下划线名时改名兜底（python 3.9.6 + pip 26 实测踩坑）。
venv_sp=".embuild/espressif/python_env/idf5.2_py3.9_env/lib/python3.9/site-packages"
for d in "$venv_sp"/ruamel_yaml-*.dist-info; do
  if [ -d "$d" ]; then
    mv "$d" "${d/ruamel_yaml-/ruamel.yaml-}"
    echo "PASS  venv dist-info 兼容改名: ${d##*/} → ruamel.yaml-*"
  fi
done

# elf 判据：debug 构建（冒烟只证构建链通，无时序敏感性，同 release 判据不适用）
elf="target/xtensa-esp32-espidf/debug/espidf-std-hello"
if cargo +esp build \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$elf" ]; then
    echo "PASS  espidf-std-hello xtensa-esp32-espidf 构建: $elf"
    exit 0
  else
    echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
    exit 1
  fi
else
  echo "FAIL  espidf-std-hello xtensa-esp32-espidf 构建失败" >&2
  exit 1
fi
