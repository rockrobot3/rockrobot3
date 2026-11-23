// SPDX-License-Identifier: Apache-2.0
//! U-Boot 环境变量写侧（M6 回滚链的板侧件；解析/渲染/状态迁移主机可测）。
//!
//! 载体格式是 U-Boot 标准 raw env：`[crc32: u32 LE][k=v\0]…[\0]`，CRC 覆盖其后的
//! 全部字节（含补齐的 0 尾）。算法与 `rr_ipc::protocol::header::rr3_crc32` 同一份
//! 实现（IEEE 802.3 反射多项式，初值/异或 0xFFFF_FFFF）——U-Boot 的 `crc32(0, …)`
//! 即此形态，单点真值不抄第二份。
//!
//! 三件面：
//! 1. [`Rr3BootEnv`]：env 体解析/渲染 + `get`/`set`/`unset`（坏 CRC、重复键、
//!    非 UTF-8 一律 fail-closed，不猜）；
//! 2. [`Rr3EnvDev`]：解析 `/etc/fw_env.config`（`device offset env_size
//!    [sector_size num_sectors]`，数支持 `0x` 十六进制；`#` 行注释）；
//! 3. 回滚链状态迁移：`rr3_rollback_arm` / `rr3_rollback_ok` 落实
//!    `base/ota/partition-layout.md` 标准序第 4/6 步（bootcount/bootlimit/
//!    altbootcmd/upgrade_available 四变量）。
//!
//! 区读写走 `std::fs`（定位 + 读写，无 ioctl），只在 `cfg(unix)` 下编译：板上
//! Linux 与主机 macOS 测试面是同一份代码（测试用普通临时文件顶替块设备，
//! 不碰真实设备）；Windows 无部署面、不编。
//!
//! **明确不做**（板上核对项，见 `base/ota/partition-layout.md`）：冗余双份 env 的
//! 同步写、坏块/写保护探测、fw_env.config 与实际 U-Boot 布局的核对、
//! `CONFIG_BOOTCOUNT_LIMIT` 计数载体的 defconfig 确认。altbootcmd 的具体命令文本
//! 由调用方给（distro boot 的备援命令随板/构建而变，契约不预造）。

use rr_ipc::protocol::header::rr3_crc32;

/// `/etc/fw_env.config` 的一行：env 存储位置与容量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3EnvDev {
    device: String,
    offset: u64,
    size: usize,
}

impl Rr3EnvDev {
    /// 设备路径（块设备或普通文件；测试面用普通文件）。
    pub fn rr3_dev_path(&self) -> &str {
        &self.device
    }

    /// env 区在设备内的字节偏移。
    pub fn rr3_dev_offset(&self) -> u64 {
        self.offset
    }

    /// env 区容量（含 4B CRC）。
    pub fn rr3_dev_size(&self) -> usize {
        self.size
    }
}

/// 解析 fw_env.config 文本（多行 → 多条）。
///
/// 只认前三个字段（新式行尾的 `sector_size num_sectors` 读后忽略——冗余/擦除扇区
/// 参数只影响 U-Boot 自身的双份写，本工具的写侧是单份整区覆盖，见模块头"不做"）。
/// 空行与 `#` 注释跳过；字段数 < 3、数不是十进制或 `0x` 十六进制、size < 5 都点名报错。
pub fn rr3_env_cfg_parse(text: &str) -> Result<Vec<Rr3EnvDev>, String> {
    let mut out: Vec<Rr3EnvDev> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(dev) = rr3_env_cfg_line(line, i)? {
            out.push(dev);
        }
    }
    if out.is_empty() {
        return Err("fw_env.config 没有任何有效行（空册 = 不知道 env 在哪，拒）".into());
    }
    Ok(out)
}

/// 单行解析：空行/注释回 `None`，坏行按行号点名回 `Err`。
fn rr3_env_cfg_line(line: &str, i: usize) -> Result<Option<Rr3EnvDev>, String> {
    let ln = i + 1;
    let body = line.split('#').next().unwrap_or("").trim();
    if body.is_empty() {
        return Ok(None);
    }
    let f: Vec<&str> = body.split_whitespace().collect();
    if f.len() < 3 {
        return Err(format!(
            "fw_env.config 第 {ln} 行字段不足（要 device offset env_size，收到 {body:?}）"
        ));
    }
    let offset = rr3_env_num(f[1]).map_err(|e| format!("fw_env.config 第 {ln} 行 offset：{e}"))?;
    let size = rr3_env_num(f[2]).map_err(|e| format!("fw_env.config 第 {ln} 行 env_size：{e}"))?;
    if size < 5 {
        return Err(format!(
            "fw_env.config 第 {ln} 行 env_size={size} 过小（至少 4B CRC + 1B 载荷）"
        ));
    }
    Ok(Some(Rr3EnvDev {
        device: f[0].to_string(),
        offset,
        size: size as usize,
    }))
}

/// 十进制或 `0x` 十六进制的无符号数（U-Boot 配置惯用十六进制偏移）。
fn rr3_env_num(s: &str) -> Result<u64, String> {
    let (digits, radix) = rr3_num_parts(s);
    if digits.is_empty() {
        return Err(format!("{s:?} 缺数字"));
    }
    u64::from_str_radix(digits, radix).map_err(|_| format!("{s:?} 不是合法无符号数"))
}

/// 进制前缀：`0x`/`0X` 为十六进制，其余十进制。
fn rr3_num_parts(s: &str) -> (&str, u32) {
    s.strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .map_or((s, 10), |rest| (rest, 16))
}

/// 键值合法性：键非空且不含 `=`/`\0`，值不含 `\0`。
fn rr3_env_kv_chk(key: &str, value: &str) -> Result<(), String> {
    if !rr3_env_key_ok(key) {
        return Err(format!("env 键 {key:?} 非法（非空、不得含 '=' 或 \\0）"));
    }
    rr3_env_val_ok(key, value)
}

/// 值形制：不含 `\0`（写不进 env 载体）。
fn rr3_env_val_ok(key: &str, value: &str) -> Result<(), String> {
    if value.contains('\0') {
        return Err(format!("env 键 {key:?} 的值含 \\0，写不进 env 载体"));
    }
    Ok(())
}

/// 键形制：非空且不含 `=`/`\0`。
fn rr3_env_key_ok(key: &str) -> bool {
    !key.is_empty() && !key.contains(&['=', '\0'][..])
}

/// env 体（键值有序表；顺序即渲染顺序，U-Boot 不要求排序）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rr3BootEnv {
    vars: Vec<(String, String)>,
}

impl Rr3BootEnv {
    /// 从 raw env 字节解析（校验 CRC、UTF-8、`k=v` 形状、键不重复）。
    ///
    /// 载荷语义：`k=v\0` 重复，末尾一个空段收尾；收尾后的 0 字节是补齐，不参与解析。
    pub fn rr3_env_parse(raw: &[u8]) -> Result<Self, String> {
        if raw.len() < 5 {
            return Err(format!(
                "env 区 {}B 过短（至少 4B CRC + 1B 载荷）",
                raw.len()
            ));
        }
        let want = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let got = rr3_crc32(&raw[4..]);
        if want != got {
            return Err(format!(
                "env CRC 不符（区里 {want:#010x}，实算 {got:#010x}）——载体损坏或偏移错，拒读"
            ));
        }
        let mut vars: Vec<(String, String)> = Vec::new();
        let mut rest = &raw[4..];
        loop {
            let Some(end) = rest.iter().position(|b| *b == 0) else {
                return Err("env 载荷缺收尾 \\0（截断的 env 区，拒读）".into());
            };
            let seg = &rest[..end];
            rest = &rest[end + 1..];
            if seg.is_empty() {
                break; // 收尾空段；其后全为补齐 0
            }
            rr3_env_seg(seg, &mut vars)?;
        }
        Ok(Self { vars })
    }

    /// 渲染成 `size` 字节的整区镜像（含 4B CRC，载荷尾部补 0 到满区）。
    ///
    /// `size` 必须容纳 `4 + Σ(k=v\0) + 1`，否则点名需要多少字节。
    pub fn rr3_env_render(&self, size: usize) -> Result<Vec<u8>, String> {
        let mut payload = Self::rr3_env_payload(&self.vars);
        let need = payload.len() + 4;
        if need > size {
            return Err(format!("env 载荷需 {need}B（含 CRC），区容量只有 {size}B"));
        }
        payload.resize(size - 4, 0);
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(&rr3_crc32(&payload).to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    /// 渲染载荷：每键 `k=v\0`，末尾一个空段收尾（不含 4B CRC，也不含补齐）。
    fn rr3_env_payload(vars: &[(String, String)]) -> Vec<u8> {
        let mut payload: Vec<u8> = Vec::new();
        for (k, v) in vars {
            payload.extend_from_slice(k.as_bytes());
            payload.push(b'=');
            payload.extend_from_slice(v.as_bytes());
            payload.push(0);
        }
        payload.push(0);
        payload
    }

    /// 读一个键（不存在 → None）。
    pub fn rr3_env_get(&self, key: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// 写一个键（存在即覆盖，不存在即追加）。键/值的非法字符当场拒。
    pub fn rr3_env_set(&mut self, key: &str, value: &str) -> Result<(), String> {
        rr3_env_kv_chk(key, value)?;
        if let Some(slot) = self.vars.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.to_string();
            return Ok(());
        }
        self.vars.push((key.to_string(), value.to_string()));
        Ok(())
    }

    /// 删一个键（不存在视为幂等成功）。
    pub fn rr3_env_unset(&mut self, key: &str) {
        self.vars.retain(|(k, _)| k != key);
    }

    /// 回滚链上膛（标准序第 4 步）：`upgrade_available=1 bootcount=0
    /// bootlimit=<limit> altbootcmd=<altcmd>`。
    ///
    /// `altcmd` 是 U-Boot 备援命令（回原槽的落点），随板/构建而变，由调用方给；
    /// 空串拒绝——空 altbootcmd 等于"回滚落点不存在"，宁可失败不许假绿。
    /// `limit=0` 拒绝（bootlimit=0 在 U-Boot 里不进回滚链）。
    pub fn rr3_rollback_arm(&mut self, limit: u32, altcmd: &str) -> Result<(), String> {
        rr3_arm_chk(limit, altcmd)?;
        self.rr3_env_set("upgrade_available", "1")?;
        self.rr3_env_set("bootcount", "0")?;
        self.rr3_env_set("bootlimit", &limit.to_string())?;
        self.rr3_env_set("altbootcmd", altcmd)
    }

    /// 回滚链解除（标准序第 6 步）：`bootcount=0 upgrade_available=0`。
    ///
    /// `altbootcmd`/`bootlimit` 留在 env 里不清：U-Boot 只认 `upgrade_available`，
    /// 保留两键让下次升级 arm 时少写两笔，也让人工排障看得到上次的落点。
    pub fn rr3_rollback_ok(&mut self) -> Result<(), String> {
        self.rr3_env_set("bootcount", "0")?;
        self.rr3_env_set("upgrade_available", "0")
    }
}

/// 单个 env 段：必须是 `k=v`、键非空且不与已有键重复（坏形状一律 fail-closed）。
fn rr3_env_seg(seg: &[u8], vars: &mut Vec<(String, String)>) -> Result<(), String> {
    let s =
        std::str::from_utf8(seg).map_err(|_| format!("env 段非 UTF-8（{}B），拒读", seg.len()))?;
    let Some((k, v)) = s.split_once('=') else {
        return Err(format!("env 段 {s:?} 缺 '='（形状坏，拒读）"));
    };
    if k.is_empty() {
        return Err("env 段键为空（形状坏，拒读）".into());
    }
    if vars.iter().any(|(ek, _)| ek == k) {
        return Err(format!("env 键 {k:?} 重复（载体损坏，拒读）"));
    }
    vars.push((k.to_string(), v.to_string()));
    Ok(())
}

/// 上膛前件：altcmd 非空、limit 非 0（否则回滚链落空，假绿不许）。
fn rr3_arm_chk(limit: u32, altcmd: &str) -> Result<(), String> {
    if altcmd.trim().is_empty() {
        return Err("altbootcmd 不得为空（回滚落点必须可执行，不许假绿）".into());
    }
    if limit == 0 {
        return Err("bootlimit 不得为 0（U-Boot 以 0 表示不进回滚链）".into());
    }
    Ok(())
}

/// 从 config 指明的设备区读出 env（unix：真块设备/普通文件同码）。
#[cfg(unix)]
pub fn rr3_env_read(dev: &Rr3EnvDev) -> Result<Rr3BootEnv, String> {
    use std::io::Read;
    let mut f = rr3_env_open_r(dev)?;
    let mut buf = vec![0u8; dev.rr3_dev_size()];
    f.read_exact(&mut buf).map_err(|e| {
        format!(
            "读 {} 的 {}B env 区失败：{e}",
            dev.rr3_dev_path(),
            dev.rr3_dev_size()
        )
    })?;
    Rr3BootEnv::rr3_env_parse(&buf)
}

/// 以读打开设备区并按偏移定位（失败点名路径与偏移）。
#[cfg(unix)]
fn rr3_env_open_r(dev: &Rr3EnvDev) -> Result<std::fs::File, String> {
    use std::io::{Seek, SeekFrom};
    std::fs::OpenOptions::new()
        .read(true)
        .open(dev.rr3_dev_path())
        .map_err(|e| format!("打开 {} 失败：{e}", dev.rr3_dev_path()))
        .and_then(|mut f| {
            f.seek(SeekFrom::Start(dev.rr3_dev_offset()))
                .map(|_| f)
                .map_err(|e| {
                    format!(
                        "定位 {} 到 {} 失败：{e}",
                        dev.rr3_dev_path(),
                        dev.rr3_dev_offset()
                    )
                })
        })
}

/// 把 env 整区写回 config 指明的设备位置（仅 linux）。
///
/// 单份整区覆盖（不碰冗余副本——见模块头"不做"）；先渲染成功再落盘，渲染失败
/// （容量不够）时设备一个字节都不动。
#[cfg(unix)]
pub fn rr3_env_write(dev: &Rr3EnvDev, env: &Rr3BootEnv) -> Result<(), String> {
    let image = env.rr3_env_render(dev.rr3_dev_size())?;
    rr3_env_put(dev, &image)
}

/// 整区写到设备偏移处：写全量 + flush（失败点名路径与字节数）。
#[cfg(unix)]
fn rr3_env_put(dev: &Rr3EnvDev, image: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut f = rr3_env_open_w(dev)?;
    f.write_all(image).map_err(|e| {
        format!(
            "写 {} 的 {}B env 区失败：{e}",
            dev.rr3_dev_path(),
            image.len()
        )
    })?;
    f.flush()
        .map_err(|e| format!("flush {} 失败：{e}", dev.rr3_dev_path()))
}

/// 以写打开设备区并按偏移定位（失败点名路径与偏移）。
#[cfg(unix)]
fn rr3_env_open_w(dev: &Rr3EnvDev) -> Result<std::fs::File, String> {
    use std::io::{Seek, SeekFrom};
    std::fs::OpenOptions::new()
        .write(true)
        .open(dev.rr3_dev_path())
        .map_err(|e| format!("以写打开 {} 失败：{e}", dev.rr3_dev_path()))
        .and_then(|mut f| {
            f.seek(SeekFrom::Start(dev.rr3_dev_offset()))
                .map(|_| f)
                .map_err(|e| {
                    format!(
                        "定位 {} 到 {} 失败：{e}",
                        dev.rr3_dev_path(),
                        dev.rr3_dev_offset()
                    )
                })
        })
}

#[cfg(test)]
mod tests;
