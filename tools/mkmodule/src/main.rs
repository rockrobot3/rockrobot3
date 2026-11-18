//! `rr3-mkmod` CLI：生成模块骨架五件与原子件五件套。
//!
//! 用法与参数裁决在 `usage`/`parse`；生成机制在库侧 `rr3_mkmod_generate`
//! （本文件只做参数→调用→退出码）。生成器模板是模块契约的起手形状，改模板
//! 须同步 `tools/ci` 的模块形状门。

use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("用法:");
    eprintln!("  rr3-mkmod <模块名> [输出根目录，默认 ./modules]        —— 生成模块骨架五件");
    eprintln!(
        "  rr3-mkmod --atom <原子名> <格位> [原子根目录]          —— 生成原子件五件套（ADR-0019）"
    );
    eprintln!("示例:");
    eprintln!("  rr3-mkmod rr3-arm");
    eprintln!("  rr3-mkmod --atom battery power.battery modules/rr3-power/atoms");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("-h") | Some("--help") => usage(),
        Some("--atom") => run_atom(&args),
        Some(name) => run_module(name, args.get(1).map(String::as_str), args.len()),
    }
}

/// 原子件五件套：`--atom <名> <格位> [原子根目录]`。
fn run_atom(args: &[String]) {
    if !(3..=4).contains(&args.len()) {
        usage();
    }
    let out_root = rr3_out_root(args.get(3).map(String::as_str).unwrap_or("modules"), "原子");
    let files = rr3_gen(rr3_mkmod::rr3_mkmod_gen_atom(&args[1], &args[2], &out_root));
    print_files("原子件五件套", &files);
}

/// 模块骨架（老口径）：`rr3-mkmod <模块名> [输出根目录]`。
fn run_module(name: &str, out: Option<&str>, argc: usize) {
    if argc > 2 {
        usage();
    }
    let out_root = rr3_out_root(out.unwrap_or("modules"), "输出");
    let files = rr3_gen(rr3_mkmod::rr3_mkmod_generate(name, &out_root));
    print_files("模块骨架", &files);
    println!("\n下一步：在 contracts/topics.toml 登记主题（见模块 README）");
}

/// 输出根目录：建目录失败即点名退 1（`kind` 区分"输出/原子"两口径文案）。
fn rr3_out_root(raw: &str, kind: &str) -> PathBuf {
    let root = PathBuf::from(raw);
    std::fs::create_dir_all(&root).unwrap_or_else(|e| {
        eprintln!("无法创建{kind}根目录 {}: {e}", root.display());
        std::process::exit(1);
    });
    root
}

/// 生成调用：失败即点名退 1。
fn rr3_gen(r: Result<Vec<PathBuf>, rr3_mkmod::Rr3MkmodError>) -> Vec<PathBuf> {
    match r {
        Ok(files) => files,
        Err(e) => {
            eprintln!("生成失败: {e}");
            std::process::exit(1);
        }
    }
}

/// 生成物清单打印（`/` 规范化只作用于展示，落盘路径不受影响）。
fn print_files(title: &str, files: &[PathBuf]) {
    println!("已生成{title} ({} 个文件):", files.len());
    for f in files {
        let display = f.to_string_lossy().replace('\\', "/");
        println!("  {display}");
    }
}
