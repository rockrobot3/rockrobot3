#!/usr/bin/env bash
# ESP32-S3 外设底座构建门（W1/T2，批N35 件八）。与 esp32-periph-base-check.sh
# 同款门禁形态：宿主工具链 check 必红（xtensa + build-std + esp-idf-sys），本脚本
# 单独覆盖。SKIP 纪律同款：工具链缺位 = SKIP（退出 0 并明说）；在位但编译失败 = FAIL；
# 退出 0 但产物不在树 = FAIL。
#
# 与 classic 门的差异只有三处：MCU=esp32s3、--target xtensa-esp32s3-espidf、ELF 路径；
# 外加芯片档 feature（`--no-default-features --features esp32s3`——esp-idf-sys 的
# rustc-cfg 不跨 crate 传播，本 crate 以 Cargo feature 表达芯片档，见 Cargo.toml）。
# 芯片专属 sdkconfig 由 esp-idf-sys 按 MCU 自动叠加 sdkconfig.defaults.esp32s3
# （BUILD-OPTIONS.md：<path>.<mcu> 更具体者覆盖），本脚本不显式传 SDKCONFIG。
#
# 本门只验**构建面**；上板验收（Flash/PSRAM 识别、I2C 0x3C、OLED、双轴、ADC 标定、
# 双麦 DoA、与 classic 的字节/行为对拍）走 W1/T2 runbook，不属 CI。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
fail=0

cd "$root/tools/esp32-periph-base" || { echo "ERROR: 无法进入 tools/esp32-periph-base" >&2; exit 1; }

# ① esp fork 工具链（rustup alias esp，0.0.7 批A'）
if ! rustup toolchain list 2>/dev/null | grep -q '^esp'; then
  echo "SKIP  esp 工具链未安装（rustup toolchain list 无 'esp'）——esp32s3-periph-base 门不执行"
  exit 0
fi
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || {
  echo "SKIP  esp 工具链 rustc 不可用（rustc +esp 失败）——esp32s3-periph-base 门不执行"
  exit 0
}
if [ ! -d "$esp_sysroot/lib/rustlib/src/rust" ]; then
  echo "SKIP  esp 工具链缺 rust-src 源码树（build-std 依赖）——esp32s3-periph-base 门不执行"
  exit 0
fi
# S3 目标由 esp fork rustc 内置 target spec 提供（build-std 从 rust-src 编译，
# 自定义工具链不会出现 lib/rustlib/<target> 目录，故查 target-list）。
if ! rustc +esp --print target-list 2>/dev/null | grep -qx 'xtensa-esp32s3-espidf'; then
  echo "SKIP  esp 工具链不认 xtensa-esp32s3-espidf 目标——esp32s3-periph-base 门不执行"
  exit 0
fi
# ② ldproxy
if ! command -v ldproxy >/dev/null 2>&1; then
  echo "SKIP  ldproxy 未安装（cargo install ldproxy）——esp32s3-periph-base 门不执行"
  exit 0
fi
# ③ ESP-IDF 本地克隆（版本钉死登记见 tools/espidf-std-hello/README.md）
if [ ! -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ]; then
  echo "SKIP  ESP-IDF v5.2.2 本地克隆缺位——esp32s3-periph-base 门不执行"
  exit 0
fi
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
# ④ esp-clang（bindgen 需 Xtensa 后端）
if [ ! -e "$HOME/.espup/esp-clang/libclang.dylib" ]; then
  echo "SKIP  esp-clang 缺位（~/.espup/esp-clang）——esp32s3-periph-base 门不执行"
  exit 0
fi
# ⑤ 镜像注入（仓库源不硬编码镜像，同 espidf-sentinel-check.sh）
export IDF_GITHUB_ASSETS=dl.espressif.com/github_assets
export PIP_INDEX_URL="${PIP_INDEX_URL:-https://mirrors.cloud.tencent.com/pypi/simple}"
export PIP_DEFAULT_TIMEOUT=60
# ⑥ 芯片档：覆盖 .cargo/config.toml [env] 的 MCU=esp32（未设 force，外部优先）
export MCU=esp32s3

# ⑦ venv dist-info 兼容（同 classic 门 ⑥）
venv_sp=".embuild/espressif/python_env/idf5.2_py3.9_env/lib/python3.9/site-packages"
for d in "$venv_sp"/ruamel_yaml-*.dist-info; do
  if [ -d "$d" ]; then
    mv "$d" "${d/ruamel_yaml-/ruamel.yaml-}"
    echo "PASS  venv dist-info 兼容改名: ${d##*/} → ruamel.yaml-*"
  fi
done

elf="target/xtensa-esp32s3-espidf/debug/esp32-periph-base"
if cargo +esp build \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3 \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$elf" ]; then
    echo "PASS  esp32s3-periph-base xtensa-esp32s3-espidf 构建（W1/T2 构建面）: $elf"
  else
    echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
    fail=1
  fi
else
  echo "FAIL  esp32-periph-base xtensa-esp32s3-espidf 构建失败" >&2
  fail=1
fi

# ⑧ 纯逻辑单测编译面（同 classic 门 ⑦；xtensa 目标本机不可跑，--no-run 只验编译）
if cargo +esp test --no-run \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3 \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' \
     >/dev/null 2>&1; then
  echo "PASS  甲面单测编译（--no-run）：S3 目标纯逻辑层单测在编"
else
  echo "FAIL  esp32-periph-base S3 单测编译失败（cargo +esp test --no-run）" >&2
  fail=1
fi

exit "$fail"
