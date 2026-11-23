// SPDX-License-Identifier: Apache-2.0
//! 轴行程常量表生成：真值点 `hardware/esp32-mini/axis/*.toml`，生成物
//! `tools/esp32-periph-base/src/axis_gen.rs`。
//!
//! 生成器独立工程不依赖 rrd 装载器，只做**最小校验面**（fail-closed，坏档宁可红
//! 不生成带病副本）：平表解析＋`schema = 1`＋`pin` 文本＋`center/lo/hi` 非负整数＋
//! `lo < center < hi`＋文件名 stem 与 `pin` 末段对账（档案的消歧锚）。其余键
//! （时限/标定态等）不属板侧行程常量的生成输入，rrd 装载器才是档案全键的裁决面。

use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

/// 生成物路径（仓根相对）。
pub(crate) const OUT: &str = "tools/esp32-periph-base/src/axis_gen.rs";

/// 行程档案 → axis_gen.rs 文本（坏档折进 errs，不生成带病副本）。
pub(crate) fn rr3_axis_gen(root: &Path, errs: &mut Vec<String>) -> String {
    render_axes(&axis_travel_rows(root, errs))
}

/// 读行程档案（批N3 件五门 g）：`hardware/esp32-mini/axis/*.toml` 是板侧行程
/// 常量表的真值点（esp32-periph-base 与形态一一对应，形态名在此点名——换板侧
/// 工程对应形态时此处同批改）。
fn axis_travel_rows(root: &Path, errs: &mut Vec<String>) -> Vec<(String, u32, u32, u32)> {
    let dir = root.join("hardware/esp32-mini/axis");
    let Some(entries) = axis_entries(&dir, errs) else {
        return Vec::new();
    };
    rr3_axis_rows(entries, errs)
}

/// 逐档案取行（坏档折进 errs，不短路）。
fn rr3_axis_rows(entries: Vec<PathBuf>, errs: &mut Vec<String>) -> Vec<(String, u32, u32, u32)> {
    let mut rows: Vec<(String, u32, u32, u32)> = Vec::new();
    for path in entries {
        rr3_axis_row_into(&path, errs, &mut rows);
    }
    rows
}

/// 单档案落一行；坏档的逐条拒因折进 errs（不短路后续档案）。
fn rr3_axis_row_into(path: &Path, errs: &mut Vec<String>, rows: &mut Vec<(String, u32, u32, u32)>) {
    match axis_row_of(path) {
        Ok(row) => rows.push(row),
        Err(es) => errs.extend(es),
    }
}

/// 档案目录读面：缺席与空表各是一条独立拒因（都属"行程真值点没有档案"的
/// 两种形状），缺一不可。
fn axis_entries(dir: &Path, errs: &mut Vec<String>) -> Option<Vec<PathBuf>> {
    let Some(entries) = fs::read_dir(dir).ok().map(|rd| {
        let mut es: Vec<_> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .collect();
        es.sort();
        es
    }) else {
        errs.push(format!(
            "{} 缺席或不可读（板侧行程真值点必须有档案）",
            dir.display()
        ));
        return None;
    };
    if entries.is_empty() {
        errs.push(format!(
            "{} 无 *.toml 档案（行程常量表拒绝生成空表——查无行程的轴在板侧 fail-closed）",
            dir.display()
        ));
        return None;
    }
    Some(entries)
}

/// 单档案 → 一行 (stem, lo, hi, center)；不合规处逐条折 Err（可多条：三键各自
/// 独立校验，坏几个报几个），由调用方统一入 errs——判断面与收集面分离。
fn axis_row_of(path: &Path) -> Result<(String, u32, u32, u32), Vec<String>> {
    let (stem, center, lo, hi) = axis_core(path)?;
    axis_window_ok(path, center, lo, hi)?;
    Ok((stem, lo, hi, center))
}

/// 行程窗口自洽门：须 `lo < center < hi`。
fn axis_window_ok(path: &Path, center: u32, lo: u32, hi: u32) -> Result<(), Vec<String>> {
    if !(lo < center && center < hi) {
        return Err(vec![format!(
            "{} 行程窗口不自洽：须 lo < center < hi（{lo} < {center} < {hi}）",
            path.display()
        )]);
    }
    Ok(())
}

/// 身份、三键取数：`(stem, center, lo, hi)`；坏几个报几个。
fn axis_core(path: &Path) -> Result<(String, u32, u32, u32), Vec<String>> {
    let v = axis_doc_of(path).map_err(|e| vec![e])?;
    let stem = axis_ident(path, &v).map_err(|e| vec![e])?;
    let (center, lo, hi) = axis_triple(&v, path)?;
    Ok((stem, center, lo, hi))
}

/// 三键并检：`center_deg`/`lo_deg`/`hi_deg` 各自独立，逐键留痕不短路（一次
/// 运行把同一档案的坏键全报出来，省掉"改一个键再跑一次"的往返）。
fn axis_triple(v: &Value, path: &Path) -> Result<(u32, u32, u32), Vec<String>> {
    let (vals, errs) = axis_collect(v, path);
    if errs.is_empty() {
        Ok((vals[0], vals[1], vals[2]))
    } else {
        Err(errs)
    }
}

/// 三键逐把取数：各自独立留痕，不短路。
fn axis_collect(v: &Value, path: &Path) -> ([u32; 3], Vec<String>) {
    let keys = ["center_deg", "lo_deg", "hi_deg"];
    let mut vals = [0u32; 3];
    let mut errs: Vec<String> = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        match axis_u32(v, key, path) {
            Ok(n) => vals[i] = n,
            Err(e) => errs.push(e),
        }
    }
    (vals, errs)
}

/// 档案装载面：读文本 → 解析 → schema 钉 1（档案 schema 升版 = 装载器与
/// 生成器同批，此处显式拒绝旧版）。
fn axis_doc_of(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
    let v: Value =
        toml::from_str(&text).map_err(|e| format!("{} 解析失败: {e}", path.display()))?;
    rr3_axis_schema(v, path)
}

/// schema 钉 1（旧版显式拒收）。
fn rr3_axis_schema(v: Value, path: &Path) -> Result<Value, String> {
    let schema = v.get("schema").and_then(Value::as_integer);
    if schema == Some(1) {
        return Ok(v);
    }
    Err(format!(
        "{} schema = {schema:?} ≠ 1（升版须同批改生成器）",
        path.display()
    ))
}

/// 档案身份面：`pin` 非空且其末段必须等于文件名 stem——instance 是档案的
/// 消歧锚，两者不一致说明档案被改名却没改 pin。
fn axis_ident(path: &Path, v: &Value) -> Result<String, String> {
    let stem = rr3_file_stem(path)?;
    let pin = rr3_pin_of(path, v)?;
    let tail = pin.rsplit('.').next().unwrap_or(pin);
    if tail != stem {
        return Err(format!(
            "{} 文件名 {:?} 与 pin 末段 {:?} 不对账——instance 是档案的消歧锚",
            path.display(),
            stem,
            tail
        ));
    }
    Ok(stem)
}

/// 档案文件名 stem（不可读即点名）。
fn rr3_file_stem(path: &Path) -> Result<String, String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{} 文件名不可读", path.display()))
}

/// `pin` 须非空文本（外键对账锚）。
fn rr3_pin_of<'a>(path: &Path, v: &'a Value) -> Result<&'a str, String> {
    match v.get("pin").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => Ok(p),
        other => Err(format!(
            "{} pin = {other:?} 非非空文本（外键对账锚）",
            path.display()
        )),
    }
}

/// 三键取数：非负整数且在 u32 值域内，越界与类型不对同一句点名。
fn axis_u32(v: &Value, key: &str, path: &Path) -> Result<u32, String> {
    let n = v.get(key).and_then(Value::as_integer);
    if let Some(n) = n.filter(|n| (0..=u32::MAX as i64).contains(n)) {
        return Ok(n as u32);
    }
    Err(format!("{} {key} = {n:?} 非非负整数", path.display()))
}

/// 生成 axis_gen.rs 文本（板侧行程常量表，批N3 件五门 g：`RR3_AXIS_<INSTANCE>`
/// 三元组（lo, hi, center）＋按名查表函数。板侧 board.rs 撤手写行程字面量改读
/// 本表——查无此轴回 None 由调用方 fail-closed，生成器这边保证表内行程窗口
/// 自洽（axis_travel_rows 已校验）。头注规则与 board::render 同）。
fn render_axes(axes: &[(String, u32, u32, u32)]) -> String {
    let consts: Vec<String> = axes
        .iter()
        .map(|(name, lo, hi, center)| {
            format!(
                "/// 轴 {name} 行程真值三元组（lo, hi, center）——档案\n\
                 /// hardware/esp32-mini/axis/{name}.toml 的三键投影。\n\
                 pub(crate) const RR3_AXIS_{0}: (u32, u32, u32) = ({1}, {2}, {3});\n",
                name.to_uppercase(),
                lo,
                hi,
                center
            )
        })
        .collect();
    let mut arms: Vec<String> = axes
        .iter()
        .map(|(name, _, _, _)| {
            format!(
                "        \"{name}\" => Some(RR3_AXIS_{}),\n",
                name.to_uppercase()
            )
        })
        .collect();
    // 兜底臂并入同一拼接（模板行首空白被 `\` 续行吃掉，8 空格缩进在生成侧给出）
    arms.push("        _ => None,\n".to_string());
    format!(
        "// SPDX-License-Identifier: Apache-2.0\n\
         // @generated by tools/rr3-codegen —— DO NOT EDIT（手改必被门 f 拟 ⑨-d 重跑 diff 抓红）。\n\
         // 真值点：hardware/esp32-mini/axis/*.toml（行程档案的 center/lo/hi 三键；文件名\n\
         // instance 与 pin 末段对账、行程窗口 lo<center<hi 由生成器 fail-closed 校验）。\n\
         // 批N3 件五门 g 收敛：行程唯一真值点在档案——板侧 board.rs 撤手写\n\
         // 行程字面量改读本表；改行程只改档案并重跑本生成器（rrd 装配注入\n\
         // 与板侧本表同源同值）。\n\
         //（include! 进 board.rs 条目流，头注用普通注释——inner doc 在 item 后不合法。）\n\n\
         {}\n\
         /// 按轴名查行程（板侧 fail-closed：查无此轴回 None，调用方显式红，\n\
         /// 不猜缺省——行程缺真相就拒，不静默收窄或放行）。\n\
         pub(crate) fn rr3_axis_travel(name: &str) -> Option<(u32, u32, u32)> {{\n\
         \x20   match name {{\n{}\
         \x20   }}\n\
         }}\n",
        consts.join(""),
        arms.join("")
    )
}

#[cfg(test)]
mod tests {
    use super::render_axes;

    /// 行程常量表形状回归（批N3 件五）：常量名 = RR3_AXIS_<大写 instance>、
    /// 三元组 (lo, hi, center) 序、match 臂逐轴点名、查无回 None、头注在位。
    #[test]
    fn render_axes_pinned() {
        let out = render_axes(&[("pan".into(), 60, 120, 90), ("tilt".into(), 80, 100, 90)]);
        assert!(
            out.contains("pub(crate) const RR3_AXIS_PAN: (u32, u32, u32) = (60, 120, 90);"),
            "{out}"
        );
        assert!(
            out.contains("pub(crate) const RR3_AXIS_TILT: (u32, u32, u32) = (80, 100, 90);"),
            "{out}"
        );
        assert!(out.contains("\"pan\" => Some(RR3_AXIS_PAN),"), "{out}");
        assert!(out.contains("\"tilt\" => Some(RR3_AXIS_TILT),"), "{out}");
        assert!(out.contains("_ => None,"), "{out}");
        assert!(
            out.contains("pub(crate) fn rr3_axis_travel(name: &str) -> Option<(u32, u32, u32)>"),
            "{out}"
        );
        assert!(!out.contains(",,"), "不得出现双逗号");
        assert!(out.starts_with(
            "// SPDX-License-Identifier: Apache-2.0\n// @generated by tools/rr3-codegen"
        ));
    }
}
