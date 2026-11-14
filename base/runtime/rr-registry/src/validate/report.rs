//! 一次 manifest 校验的结果载体。

/// 一次 manifest 校验的结果
#[derive(Debug, Default)]
pub struct Rr3ValidationReport {
    pub errors: Vec<String>,
}

impl Rr3ValidationReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}
