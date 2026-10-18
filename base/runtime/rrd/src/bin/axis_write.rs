//! axis_write —— 执行件档案写入面的产线薄壳（批N3 件四）。
//!
//! 全部策略在 [`rrd::axis_write`]（模块头注释说清了职责边界与拒绝面）；
//! 本文件只有参数解析 → 编排 → 退出码三行事，不出现第二条写路径。

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match rrd::axis_write::rr3_axis_write_parse(&args) {
        Ok(c) => c,
        Err(e) => {
            // --help 走 Err 通道回 USAGE：按 0 退出，其余解析错按 2 退出
            //（rrd daemon 解析错按 1——本工具与 daemon 生命周期不同，退出码
            // 自守一套：2 = 用法错，1 = 写入失败，产线脚本可分辨）。
            if args.iter().any(|a| a == "-h" || a == "--help") {
                println!("{e}");
                return std::process::ExitCode::SUCCESS;
            }
            eprintln!("axis_write: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    match rrd::axis_write::rr3_axis_write_run(&cfg) {
        Ok(out) => {
            println!("{out}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("axis_write: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
