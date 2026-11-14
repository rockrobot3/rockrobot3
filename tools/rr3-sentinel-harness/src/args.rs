//! 命令行参数：设备必给、拍数缺省 8、证据路径可选。

/// 命令行参数：设备必给（缺即按用法退 2），拍数缺省 8，证据路径可选。
pub(super) fn rr3_args(args: &[String]) -> (String, usize, Option<String>) {
    let Some(device) = args.get(1) else {
        eprintln!(
            "用法: rr3-sentinel-harness <串口设备> [拍数，缺省 8] [证据文件路径（C12 三样）]"
        );
        std::process::exit(2);
    };
    let beats: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    (device.clone(), beats, args.get(3).cloned())
}
