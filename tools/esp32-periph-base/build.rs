//! 链接参数接力（同 tools/espidf-sentinel/build.rs，批W1 同款）。
//!
//! esp-idf-sys 的 build 脚本经 `cargo:rustc-link-arg` 发射的 `--ldproxy-linker`
//! 与 sysroot 参数只作用于它自己包内的 bins/examples（它没有 bin，全被丢弃）；
//! 依赖方 bins 必须经 `links = "esp_idf"` 元数据取回转发，否则 ldproxy 因收不到
//! `--ldproxy-linker <gcc>` 而 panic（官方 esp-idf-template 同款 build.rs）。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    embuild::build::LinkArgs::output_propagated("ESP_IDF")?;
    Ok(())
}
