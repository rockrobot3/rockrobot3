//! 命令行解析：device/out/secs/baud/doa 逐项收集、值域校验与档位表白名单门。

use std::path::PathBuf;

use rr_ipc::rr3_uart_baud_speed;

pub(crate) struct Args {
    pub(crate) device: String,
    pub(crate) out: PathBuf,
    pub(crate) secs: u64,
    pub(crate) baud: u32,
    /// 批D⑤ DoA 冒烟面：逐 1s 窗 ILD 出角打印（不影响 wav 落盘）。
    pub(crate) doa: bool,
}

/// 参数解析中间态（device 必填校验与缺省输出名在 [`parse_args`] 收尾段）。
struct ArgsDraft {
    device: Option<String>,
    out: Option<PathBuf>,
    secs: u64,
    baud: u32,
    doa: bool,
}

impl Default for ArgsDraft {
    fn default() -> Self {
        Self {
            device: None,
            out: None,
            secs: 30,
            baud: 1_500_000,
            doa: false,
        }
    }
}

/// flag 后的值参数（缺＝点名错误；原文案逐字保留）。
fn arg_val<'a>(argv: &'a [String], i: usize, miss: &str) -> Result<&'a String, String> {
    argv.get(i).ok_or_else(|| miss.to_string())
}

/// `--secs` 值域（≥1；解析失败点名原值）。
fn parse_secs(v: &str) -> Result<u64, String> {
    let secs: u64 = v.parse().map_err(|_| format!("--secs 非法: \"{v}\""))?;
    if secs == 0 {
        return Err("--secs 须 ≥1".into());
    }
    Ok(secs)
}

/// `--baud` 值域（解析失败点名原值；档位表校验在 [`parse_args`] 收尾段）。
fn parse_baud(v: &str) -> Result<u32, String> {
    v.parse().map_err(|_| format!("--baud 非法: \"{v}\""))
}

/// 裸参数按 device 收（重复即拒）。
fn set_device(device: &mut Option<String>, other: &str) -> Result<(), String> {
    if device.is_some() {
        return Err(format!("device 重复: {other}"));
    }
    *device = Some(other.to_string());
    Ok(())
}

/// 单参数步进：消费一个 flag（含其后值参数）或一个裸 device 参数。
fn arg_step(d: &mut ArgsDraft, argv: &[String], i: &mut usize) -> Result<(), String> {
    match argv[*i].as_str() {
        "--out" => arg_out(d, argv, i),
        "--secs" => arg_secs(d, argv, i),
        "--baud" => arg_baud(d, argv, i),
        "--doa" => arg_doa(d),
        other => arg_other(d, other),
    }
}

/// `--out` 族：取路径参数（缺值点名原文案）。
fn arg_out(d: &mut ArgsDraft, argv: &[String], i: &mut usize) -> Result<(), String> {
    *i += 1;
    d.out = Some(PathBuf::from(
        arg_val(argv, *i, "--out 缺路径参数")?.clone(),
    ));
    Ok(())
}

/// `--secs` 族：取秒数参数（缺值点名原文案）。
fn arg_secs(d: &mut ArgsDraft, argv: &[String], i: &mut usize) -> Result<(), String> {
    *i += 1;
    d.secs = parse_secs(arg_val(argv, *i, "--secs 缺秒数参数")?)?;
    Ok(())
}

/// `--baud` 族：取波特率参数（缺值点名原文案）。
fn arg_baud(d: &mut ArgsDraft, argv: &[String], i: &mut usize) -> Result<(), String> {
    *i += 1;
    d.baud = parse_baud(arg_val(argv, *i, "--baud 缺波特率参数")?)?;
    Ok(())
}

/// `--doa` 族：无值开关。
fn arg_doa(d: &mut ArgsDraft) -> Result<(), String> {
    d.doa = true;
    Ok(())
}

/// 非已知 flag：`--` 前缀是未知参数，其余按裸 device 收。
fn arg_other(d: &mut ArgsDraft, other: &str) -> Result<(), String> {
    if other.starts_with("--") {
        return Err(format!("未知参数: {other}"));
    }
    set_device(&mut d.device, other)
}

/// 参数解析（device 必给；波特率过 rr-ipc 档位表白名单——真值单点）。
pub(crate) fn parse_args(argv: &[String]) -> Result<Args, String> {
    rr3_args_finish(rr3_parse_draft(argv)?)
}

/// 收尾：device 必给 → 档位过表 → 缺省输出名补齐。
fn rr3_args_finish(d: ArgsDraft) -> Result<Args, String> {
    let device = d
        .device
        .ok_or("缺 <device> 参数（如 /dev/cu.usbserial-0001）")?;
    rr3_baud_gate(d.baud)?;
    Ok(Args {
        device,
        out: d.out.unwrap_or_else(default_out),
        secs: d.secs,
        baud: d.baud,
        doa: d.doa,
    })
}

/// 波特率必须落 rr-ipc 档位表白名单（真值单点）。
fn rr3_baud_gate(baud: u32) -> Result<(), String> {
    rr3_uart_baud_speed(baud).map_err(|e| format!("--baud {baud} 不在档位表: {e}"))?;
    Ok(())
}

/// 参数逐项收集（缺省值由 [`ArgsDraft::default`] 给）。
fn rr3_parse_draft(argv: &[String]) -> Result<ArgsDraft, String> {
    let mut d = ArgsDraft::default();
    let mut i = 0;
    while i < argv.len() {
        arg_step(&mut d, argv, &mut i)?;
        i += 1;
    }
    Ok(d)
}

/// 缺省输出名（unix 秒时戳防覆盖；chrono 不进依赖闭包）。
fn default_out() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    PathBuf::from(format!("rr3-audio-{ts}.wav"))
}
