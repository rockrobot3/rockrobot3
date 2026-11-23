// SPDX-License-Identifier: Apache-2.0
//! OTA 清单版本三元组：严格 `major.minor.patch` 解析与比较（无预发布/构建元数据）。

use std::cmp::Ordering;

use super::rr3_u64_parse;

/// 严格三段式版本：`major.minor.patch`，无前导零、无预发布/构建元数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rr3Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// 解析版本；失败点名段位，不猜。
pub fn rr3_version_parse(s: &str) -> Result<Rr3Version, String> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("version 必须是 major.minor.patch（实得 {s:?}）"));
    }
    let mut nums = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = rr3_u64_parse(p, ["major", "minor", "patch"][i])?;
    }
    Ok(Rr3Version {
        major: nums[0],
        minor: nums[1],
        patch: nums[2],
    })
}

/// 版本比较（供调用方与测试共用）。
pub fn rr3_version_cmp(a: &str, b: &str) -> Result<Ordering, String> {
    Ok(rr3_version_parse(a)?.cmp(&rr3_version_parse(b)?))
}
