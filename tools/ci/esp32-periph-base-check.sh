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
# 管理令牌门（httpd.rs 编译期断言）：本门只验构建面、不产生产镜像，显式允许
# 公开缺省令牌；生产镜像必须带 RR3_BOARD_ADMIN_TOKEN（见 tools/deploy/flash-esp32.sh）。
export RR3_ALLOW_DEV_ADMIN_TOKEN=1
fail=0

# ---- 甲面：xtensa 外设底座（工具链缺位 = 整门 SKIP，同 espidf-sentinel-check.sh 口径）----
cd "$root/tools/esp32-periph-base" || { echo "ERROR: 无法进入 tools/esp32-periph-base" >&2; exit 1; }
# 产物定位共享单点：target-dir 由仓根 .cargo/config.toml 移出源码区，不得写死
# crate 内 target/（否则编译成功也会被判"ELF 不在树"）。
. "$root/tools/ci/cargo-artifact.sh"

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
if ! rr3_esp_clang_lib >/dev/null; then
  echo "SKIP  esp-clang 缺位（~/.espup/esp-clang 无 libclang.dylib/.so）——esp32-periph-base 门不执行"
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
venv_sp="/tmp/rr3-embuild/espressif/python_env/idf5.2_py3.9_env/lib/python3.9/site-packages"
for d in "$venv_sp"/ruamel_yaml-*.dist-info; do
  if [ -d "$d" ]; then
    mv "$d" "${d/ruamel_yaml-/ruamel.yaml-}"
    echo "PASS  venv dist-info 兼容改名: ${d##*/} → ruamel.yaml-*"
  fi
done

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

# ⑦ 纯逻辑单测编译面（批N13 件一：httpd 路由/漂移/JSON 形状单测；批N14 cred
#    零回显契约单测；批N23 件一起 atoms/<名>/mech.rs 机制核单测——xtensa 目标
#    本机不可跑，--no-run 只验测试码编译；xtensa 测试 harness 执行面归真板批）
esp_test_log=/tmp/rr3-esp32-periph-test.log
if cargo +esp test --no-run \
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

# ⑧ release 构建面（生产烧录档）：dev 档 opt-level=2 只代表开发构建，
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
  else
    echo "FAIL  classic release 编译退出 0 但 ELF 不在树: $rel_elf" >&2
    fail=1
  fi
else
  echo "FAIL  classic release 构建失败" >&2
  fail=1
fi

exit "$fail"
