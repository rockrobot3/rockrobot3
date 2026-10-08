# espidf-std-hello——0.0.5 增补轮四 批W1 espidf std 构建链冒烟件

目的：证明 **rustc build-std（xtensa-esp32-espidf）+ esp-idf-sys 绑定 + ESP-IDF C 库编译 + xtensa 链接** 整链可通，为 0.0.7 层③④（ESP32 std 侧模块开发）预铺构建链。只走绑定面：`binstart` 入口 + 两枚 IDF C API 调用（`esp_get_idf_version` / `esp_get_free_heap_size`），不引 esp-idf-hal/svc。

## 版本钉死登记（实测可用组合，升级需重验整链）

| 组件 | 版本 | 落位 |
|---|---|---|
| ESP-IDF | v5.2.2 | `~/.espressif/frameworks/esp-idf-v5.2.2`（git clone --recursive，裁定 Q5：IDF 不进仓，经 `IDF_PATH` 挂接） |
| esp-idf-sys | =0.35.0 | crates.io（2024-06 发布，与 IDF v5.2.2 同代；0.36+ 弃 5.1/5.2.x） |
| embuild | =0.32.0 | crates.io（根 crate build-dep，对齐 esp-idf-sys 0.35.0 自身依赖线） |
| Rust 工具链 | esp-1.90.0.0（rustup alias `esp`） | `~/.rustup/toolchains/esp`，需 `rust-src` 组件（build-std 依赖） |
| xtensa esp-gcc | esp-13.2.0_20230928 | 由 embuild/idf_tools.py 自动装 `tools/espidf-std-hello/.embuild/espressif/tools/xtensa-esp-elf/` |
| ldproxy | 0.3.5 | `cargo install ldproxy` |
| esp-clang | esp-19.1.2_20250312（LLVM 19.1.2，乐鑫 LLVM fork） | `~/.espressif/tools/esp-clang/esp-19.1.2_20250312-x86_64-apple-darwin/esp-clang/` |
| cmake / ninja | 3.24.0 / 1.11.1 | embuild 自动装 `.embuild/espressif/tools/` |
| python | 3.9.6（macOS CLT 自带） | venv 落 `.embuild/espressif/python_env/idf5.2_py3.9_env` |

## 构建命令（本机）

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
cargo +esp build   # 镜像注入见 tools/ci/espidf-std-check.sh
```

产物：`target/xtensa-esp32-espidf/debug/espidf-std-hello`（ELF）。

## 已踩坑与机制留痕（复现必读）

1. **bindgen 的 libclang 必须是乐鑫 LLVM fork**——上游 libclang（含 CLT 自带）无 Xtensa
   后端，报 `unknown target triple 'xtensa'`。安装：从
   `github.com/espressif/llvm-project/releases`（或镜像
   `dl.espressif.com/github_assets/espressif/llvm-project/releases/download/...`）下
   `clang-esp-<ver>-x86_64-apple-darwin.tar.xz`（编译器）与
   `libs-clang-esp-<ver>-...tar.xz`（含 `lib/libclang.dylib`，**缺一不可**，espup 是两个都装）
   合并解包，再建 espup 惯例符号链接：
   `mkdir -p ~/.espup && ln -sfn <esp-clang>/lib ~/.espup/esp-clang`。
   esp-idf-sys 的 `setup_clang_env` 会读该 symlink 自动设 `LIBCLANG_PATH`（优先级高于手工 export）。
2. **链接参数接力**——`cargo:rustc-link-arg` 只作用于发射包自身的 bins 面，esp-idf-sys 是
   lib，其 `--ldproxy-linker`/sysroot 发射全被丢弃，根 crate 必须经 `links = "esp_idf"`
   元数据自行取回（本目录 build.rs 调 `embuild::build::LinkArgs::output_propagated("ESP_IDF")`，
   官方 esp-idf-template 同款）。缺位症状：ldproxy panic
   `Cannot locate argument '--ldproxy-linker <linker>'`。
3. **espidf_time64**——IDF 5.x C 头 time_t 已 64 位，`.cargo/config.toml` 的
   `rustflags = ["--cfg", "espidf_time64"]` 必设（esp-idf-sys BUILD-OPTIONS 要求）。
4. **venv dist-info**——pip≥24 规范化 dist-info 目录名（`ruamel.yaml` → `ruamel_yaml`），
   py3.9 importlib.metadata 按 dir stem 严格匹配点号名 → idf-component-manager 依赖检查
   误报 not found。门禁脚本已带改名兜底（见 espidf-std-check.sh ⑥）。
5. **镜像**——IDF 组件下载走 `IDF_GITHUB_ASSETS=dl.espressif.com/github_assets`，
   pip 走腾讯镜像（espidf-std-check.sh ⑤ 注入）；仓库源不硬编码镜像。

## 门禁

本工程被 `tools/ci/check.sh` ④ 显式排除（xtensa 工具链宿主必红），由
`tools/ci/espidf-std-check.sh` 单独覆盖，SKIP 纪律同 esp32-uart-echo 门。
