use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("用法: rr3-mkmod <模块名> [输出根目录，默认 ./modules]");
    eprintln!("示例: rr3-mkmod rr3-arm");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 2 || args[0] == "-h" || args[0] == "--help" {
        usage();
    }
    let out_root = PathBuf::from(args.get(1).map(String::as_str).unwrap_or("modules"));
    if let Err(e) = std::fs::create_dir_all(&out_root) {
        eprintln!("无法创建输出根目录 {}: {e}", out_root.display());
        std::process::exit(1);
    }

    match rr3_mkmod::rr3_mkmod_generate(&args[0], &out_root) {
        Ok(files) => {
            println!("已生成模块骨架 ({} 个文件):", files.len());
            for f in files {
                // 仅规范化展示用的路径分隔符为 `/`，落盘与返回值不受影响
                let display = f.to_string_lossy().replace('\\', "/");
                println!("  {display}");
            }
            println!("\n下一步：在 contracts/topics.toml 登记主题（见模块 README）");
        }
        Err(e) => {
            eprintln!("生成失败: {e}");
            std::process::exit(1);
        }
    }
}
