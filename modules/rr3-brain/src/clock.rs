// SPDX-License-Identifier: Apache-2.0
// 系统时钟就绪判定（批347）。brain 的云调用走 TLS，证书有效期校验依赖系统时钟；
// 无 RTC 的板子首启时钟为 1970，首个云调用只会得到"证书时间错"这类二级症状。
// 本模块把判据前移到云调用入口：epoch 秒低于 release 纪元下限即拒并点名当前值。
//
// 边界：这里只回答"时钟是否明显未就绪（1970/回退）"，不替代系统授时；精确授时
// 归 NTP/RTC（传输选型见 docs/process/dev/0.0.8/无硬件开发/
// 0.0.8版本出厂镜像网络与时间面裁定材料.md）。mock 档不触网，不走此门。

/// 可接受的最小 epoch 秒（2026-01-01T00:00:00Z）。发布时抬到"上一发布年"即可——
/// 目的仅是挡住 1970/无 RTC 首启窗口，不做精细授时判据。
pub(crate) const RR3_CLOCK_MIN_UNIX: u64 = 1_767_225_600;

/// 纯判定：`now_unix` 是否已达下限（边界由单测钉死，不依赖运行环境）。
pub(crate) fn rr3_clock_ready(now_unix: u64) -> bool {
    now_unix >= RR3_CLOCK_MIN_UNIX
}

/// 当前 epoch 秒；系统时间早于 UNIX_EPOCH（时钟回退）按 0 处理，即未就绪。
pub(crate) fn rr3_clock_now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 云调用前门：未就绪即 Err（点名当前 epoch 与下限，附处置提示）。
pub(crate) fn rr3_clock_gate() -> Result<(), String> {
    let now = rr3_clock_now_unix();
    if rr3_clock_ready(now) {
        return Ok(());
    }
    Err(format!(
        "时钟未就绪（epoch={now}s < 下限 {}s）：TLS 证书校验会失败——先完成系统授时（NTP/RTC）；无 RTC 的板子首启为 1970 属预期",
        RR3_CLOCK_MIN_UNIX
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 边界：下限前一秒未就绪、下限当秒就绪。
    #[test]
    fn boundary() {
        assert!(!rr3_clock_ready(RR3_CLOCK_MIN_UNIX - 1));
        assert!(rr3_clock_ready(RR3_CLOCK_MIN_UNIX));
    }

    /// 1970 与时钟回退（now 取 0）都判未就绪。
    #[test]
    fn epoch_zero_not_ready() {
        assert!(!rr3_clock_ready(0));
        assert!(!rr3_clock_ready(RR3_CLOCK_MIN_UNIX / 2));
    }
}
