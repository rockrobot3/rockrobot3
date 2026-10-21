//! 链接参数接力（照 tools/espidf-sentinel/build.rs，批W1 同款）。
//!
//! esp-idf-sys 的 build 脚本经 `cargo:rustc-link-arg` 发射的 `--ldproxy-linker`
//! 与 sysroot 参数只作用于它自己包内的 bins/examples（它没有 bin，全被丢弃）；
//! 依赖方 bins 必须经 `links = "esp_idf"` 元数据（DEP_ESP_IDF_EMBUILD_LINK_ARGS）
//! 自行取回并转发，否则 ldproxy 因收不到 `--ldproxy-linker <gcc>` 而 panic。
//!
//! 双档工程护栏：宿主构建不解析 espidf target 表（无 esp-idf-sys 依赖 → 无
//! DEP_ESP_IDF_* 元数据），接力只在 espidf 目标跑（TARGET 由 cargo 注入 build 脚本）。
fn main() {
    let espidf = std::env::var("TARGET")
        .map(|t| t.contains("espidf"))
        .unwrap_or(false);
    if !espidf {
        return;
    }
    embuild::build::LinkArgs::output_propagated("ESP_IDF").expect("ESP_IDF 链接参数接力失败");
}
