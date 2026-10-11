#!/usr/bin/env bash
# ESP32 外设底座构建门（0.0.5 增补轮五 批B）。check.sh ④/⑥ 显式排除
# tools/esp32-periph-base：xtensa-esp32-espidf 目标编译需 esp fork 工具链
# （cargo +esp）+ build-std + esp-idf-sys（ESP-IDF v5.2.2 本地克隆 + ldproxy +
# 乐鑫 LLVM fork esp-clang），宿主工具链 check 必红，本脚本单独覆盖。SKIP 纪律
# 同 espidf-sentinel-check.sh：工具链缺位 = SKIP（退出 0 并明说）；工具链在位但
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
fail=0

# ---- 甲面：xtensa 外设底座（工具链缺位 = 整门 SKIP，同 espidf-sentinel-check.sh 口径）----
cd "$root/tools/esp32-periph-base" || { echo "ERROR: 无法进入 tools/esp32-periph-base" >&2; exit 1; }

# ① esp fork 工具链（rustup alias esp，0.0.7 批A'）
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  echo "SKIP  esp 工具链未安装（rustup toolchain list 无 'esp'）——esp32-periph-base 门不执行"
  exit 0
fi
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
  echo "SKIP  esp 工具链 rustc 不可用（rustc +esp 失败）——esp32-periph-base 门不执行"
  exit 0
}
if [ ! -d "$esp_sysroot/lib/rustlib/src/rust" ]; then
  echo "SKIP  esp 工具链缺 rust-src 源码树（build-std 依赖）——esp32-periph-base 门不执行"
  exit 0
fi
# ② ldproxy
if ! command -v ldproxy >/dev/null 2>&1; then
  echo "SKIP  ldproxy 未安装（cargo install ldproxy）——esp32-periph-base 门不执行"
  exit 0
fi
# ③ ESP-IDF 本地克隆（版本钉死登记见 tools/espidf-std-hello/README.md）
if [ ! -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ]; then
  echo "SKIP  ESP-IDF v5.2.2 本地克隆缺位——esp32-periph-base 门不执行"
  exit 0
fi
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
# ④ esp-clang（bindgen 需 Xtensa 后端）
if [ ! -e "$HOME/.espup/esp-clang/libclang.dylib" ]; then
  echo "SKIP  esp-clang 缺位（~/.espup/esp-clang）——esp32-periph-base 门不执行"
  exit 0
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

elf="target/xtensa-esp32-espidf/debug/esp32-periph-base"
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

exit "$fail"
