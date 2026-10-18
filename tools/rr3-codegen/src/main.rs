//! 板侧常量表生成器（全案 0.0.5 增补轮六 §4.5 生成物表，批N1 n1-8；批N3 件五扩行程表）。
//!
//! 真值点 → 副本方向固定：契约/档案文件是唯一事实源，本生成器把板侧
//! （`tools/esp32-periph-base`）需要的常量写成 `src/topics_gen.rs` 与
//! `src/axis_gen.rs`；板侧只 include 生成物，不再手写副本。判据 = 重跑生成器
//! `git diff` 为空（门 f 生成物一致性，拟 ⑨-d，`tools/ci/check.sh` 同批接线）。
//!
//! 生成内容（全案 §4.5 门 f 点名的三样里本批落两样：主题串 + 白名单；
//! task 位形常量已有"两端常量自证"锚定测试（board.rs `RR3_PERIPH_TASK`
//! 与 rr-hal esp32_uart 同值），其真值点是帧头约定不是契约文件，不冒进）：
//! - 主题串三枚：`RR3_SERVO_TOPIC` / `RR3_FACE_TOPIC` / `RR3_AUDIO_TOPIC`
//!   ← `contracts/topics.toml` 在册查得（fail-closed：缺名/形状坏即红退出）
//! - 板侧表情白名单：`RR3_FACE_EXPRS`（board_subset 七态）
//!   ← `contracts/face-expressions.toml`
//! - 轴行程常量表：`RR3_AXIS_<INSTANCE>`（lo, hi, center）＋ `rr3_axis_travel`
//!   查表函数 ← `hardware/esp32-mini/axis/*.toml` 行程档案（批N3 件五门 g 收敛：
//!   行程唯一真值点在档案，板侧手写行程字面量撤除改读本表；生成器独立工程不
//!   依赖 rrd，对档案只做最小校验面——平表 schema=1、三键非负整数、lo<center<hi、
//!   文件名与 pin 末段对账，坏了宁可红也不生成带病副本）
//!
//! 用法：`cargo run -p rr3-codegen`（仓根执行，写生成文件）；
//! `cargo run -p rr3-codegen -- --check`（只比对不写，漂移退出 1——门禁用）。

use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

/// 仓根（Cargo.toml 的 tools/rr3-codegen 上两级）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .canonicalize()
        .expect("仓根可解析")
}

/// fail-closed 收口：任何一条错误都不写生成文件（宁可不生成，不许带病落盘）。
fn die(errs: &[String]) -> ! {
    for e in errs {
        eprintln!("rr3-codegen: {e}");
    }
    std::process::exit(1);
}

/// topics.toml 按名查在册主题串（fail-closed：[[topic]] 缺段/缺 name 键/不在册
/// 都逐条点名——生成器是契约层消费方，不静默放行）。
fn topic_string_of(topics: &Value, want: &str, errs: &mut Vec<String>) -> String {
    let Some(arr) = topics.get("topic").and_then(Value::as_array) else {
        errs.push("contracts/topics.toml 缺 [[topic]] 段".into());
        return String::new();
    };
    for t in arr {
        match t.get("name").and_then(Value::as_str) {
            Some(n) if n == want => {
                return want.to_string();
            }
            _ => continue,
        }
    }
    errs.push(format!(
        "contracts/topics.toml 不含在册主题 \"{want}\"（板侧常量只认契约在册名）"
    ));
    String::new()
}

/// face-expressions.toml 取 expressions 全集与 board_subset（fail-closed 同上）。
fn face_exprs(face: &Value, errs: &mut Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut arr_of = |key: &str| -> Vec<String> {
        match face.get(key).and_then(Value::as_array) {
            Some(a) => {
                let v: Vec<_> = a
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                if v.len() != a.len() {
                    errs.push(format!(
                        "contracts/face-expressions.toml {key} 含非字符串项"
                    ));
                }
                v
            }
            None => {
                errs.push(format!("contracts/face-expressions.toml 缺 {key}"));
                Vec::new()
            }
        }
    };
    let all = arr_of("expressions");
    let subset = arr_of("board_subset");
    // 子集闭合：board_subset 每项必须在全集在册且不重复（契约文件自身的形状校验，
    // 坏了宁可红也不生成带病副本）
    for s in &subset {
        if !all.iter().any(|a| a == s) {
            errs.push(format!("board_subset 项 \"{s}\" 不在 expressions 全集"));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for s in &subset {
        if !seen.insert(s.as_str()) {
            errs.push(format!("board_subset 项 \"{s}\" 重复"));
        }
    }
    (all, subset)
}

/// 读行程档案（批N3 件五门 g）：`hardware/esp32-mini/axis/*.toml` 是板侧行程
/// 常量表的真值点（esp32-periph-base 与形态一一对应，形态名在此点名——换板侧
/// 工程对应形态时此处同批改）。生成器独立工程不依赖 rrd 装载器，只做**最小
/// 校验面**（fail-closed，坏档宁可红不生成带病副本）：
/// 平表解析＋`schema = 1`＋`pin` 文本＋`center/lo/hi` 非负整数＋
/// `lo < center < hi`＋文件名 stem 与 `pin` 末段对账（档案的消歧锚）。
/// 其余键（时限/标定态等）不属板侧行程常量的生成输入，这里不读——rrd 装载器
/// （件二）才是档案全键的裁决面。
fn axis_travel_rows(root: &Path, errs: &mut Vec<String>) -> Vec<(String, u32, u32, u32)> {
    let dir = root.join("hardware/esp32-mini/axis");
    let Some(entries) = fs::read_dir(&dir).ok().map(|rd| {
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
        return Vec::new();
    };
    if entries.is_empty() {
        errs.push(format!(
            "{} 无 *.toml 档案（行程常量表拒绝生成空表——查无行程的轴在板侧 fail-closed）",
            dir.display()
        ));
        return Vec::new();
    }
    let mut rows: Vec<(String, u32, u32, u32)> = Vec::new();
    for path in entries {
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => {
                errs.push(format!("{} 文件名不可读", path.display()));
                continue;
            }
        };
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                errs.push(format!("读 {} 失败: {e}", path.display()));
                continue;
            }
        };
        let v: Value = match toml::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                errs.push(format!("{} 解析失败: {e}", path.display()));
                continue;
            }
        };
        // schema 钉 1（档案 schema 升版 = 装载器与生成器同批，此处显式拒绝旧版）
        match v.get("schema").and_then(Value::as_integer) {
            Some(1) => {}
            other => {
                errs.push(format!(
                    "{} schema = {other:?} ≠ 1（升版须同批改生成器）",
                    path.display()
                ));
                continue;
            }
        }
        let pin = match v.get("pin").and_then(Value::as_str) {
            Some(p) if !p.is_empty() => p.to_string(),
            other => {
                errs.push(format!(
                    "{} pin = {other:?} 非非空文本（外键对账锚）",
                    path.display()
                ));
                continue;
            }
        };
        let tail = pin.rsplit('.').next().unwrap_or(&pin).to_string();
        if tail != stem {
            errs.push(format!(
                "{} 文件名 {:?} 与 pin 末段 {:?} 不对账——instance 是档案的消歧锚",
                path.display(),
                stem,
                tail
            ));
            continue;
        }
        let mut int_of = |key: &str| -> Option<u32> {
            match v.get(key).and_then(Value::as_integer) {
                Some(n) if n >= 0 && n <= u32::MAX as i64 => Some(n as u32),
                other => {
                    errs.push(format!("{} {key} = {other:?} 非非负整数", path.display()));
                    None
                }
            }
        };
        let (Some(center), Some(lo), Some(hi)) =
            (int_of("center_deg"), int_of("lo_deg"), int_of("hi_deg"))
        else {
            continue;
        };
        if !(lo < center && center < hi) {
            errs.push(format!(
                "{} 行程窗口不自洽：须 lo < center < hi（{lo} < {center} < {hi}）",
                path.display()
            ));
            continue;
        }
        rows.push((stem, lo, hi, center));
    }
    rows
}

/// 生成 topics_gen.rs 文本（板侧 include 的常量表；头注点名生成器与真值点，
/// 防手改——手改会在门 f 重跑 diff 时暴露）。
fn render(servo: &str, face: &str, audio: &str, exprs: &[String]) -> String {
    let expr_lines: Vec<String> = exprs.iter().map(|e| format!("    \"{e}\",")).collect();
    format!(
        "// @generated by tools/rr3-codegen —— DO NOT EDIT（手改必被门 f 拟 ⑨-d 重跑 diff 抓红）。\n\
         // 真值点：contracts/topics.toml（主题串在册名）+ contracts/face-expressions.toml\n\
         // （board_subset 七态）。全案 0.0.5 增补轮六 §4.5：板侧常量是生成副本，\n\
         // 扩态/扩主题走契约级评审并同批改真值点与宿主投影。\n\
         //（include! 进 board.rs 条目流，头注用普通注释——inner doc 在 item 后不合法。）\n\n\
         /// 舵机点名主题（在册 request L1；形状事实源\n\
         /// contracts/events/act-servo-command.schema.json，与 topics.toml 同串）。\n\
         pub(crate) const RR3_SERVO_TOPIC: &str = \"{servo}\";\n\
         /// 表情点名主题（在册 request L1；形状事实源\n\
         /// contracts/events/interact-face-expression.schema.json，与 topics.toml 同串）。\n\
         pub(crate) const RR3_FACE_TOPIC: &str = \"{face}\";\n\
         /// 表情白名单（板侧可渲染子集 board_subset；契约全集 43 态在\n\
         /// contracts/face-expressions.toml，宿主投影 face.rs RR3_FACE_EXPRESSIONS）。\n\
         pub(crate) const RR3_FACE_EXPRS: [&str; {}] = [\n{}\n];\n\
         /// 音频 PCM 上行主题（在册 stream L1：contracts/topics.toml `rr3-sense/audio/pcm`；\n\
         /// 载荷形状事实源 modules/rr3-sense composite::rr3_pcm_event_body）。\n\
         pub(crate) const RR3_AUDIO_TOPIC: &str = \"{audio}\";\n",
        exprs.len(),
        expr_lines.join("\n"),
    )
}

/// 生成 axis_gen.rs 文本（板侧行程常量表，批N3 件五门 g：`RR3_AXIS_<INSTANCE>`
/// 三元组（lo, hi, center）＋按名查表函数。板侧 board.rs 撤手写行程字面量改读
/// 本表——查无此轴回 None 由调用方 fail-closed，生成器这边保证表内行程窗口
/// 自洽（axis_travel_rows 已校验）。头注规则与 render 同）。
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
        "// @generated by tools/rr3-codegen —— DO NOT EDIT（手改必被门 f 拟 ⑨-d 重跑 diff 抓红）。\n\
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

fn main() {
    let check_only = std::env::args().any(|a| a == "--check");
    let root = repo_root();
    let mut errs: Vec<String> = Vec::new();

    let topics_raw = fs::read_to_string(root.join("contracts/topics.toml"))
        .unwrap_or_else(|e| die(&[format!("读 contracts/topics.toml 失败: {e}")]));
    let face_raw = fs::read_to_string(root.join("contracts/face-expressions.toml"))
        .unwrap_or_else(|e| die(&[format!("读 contracts/face-expressions.toml 失败: {e}")]));
    let topics: Value = toml::from_str(&topics_raw)
        .unwrap_or_else(|e| die(&[format!("topics.toml 解析失败: {e}")]));
    let face: Value = toml::from_str(&face_raw)
        .unwrap_or_else(|e| die(&[format!("face-expressions.toml 解析失败: {e}")]));

    let servo = topic_string_of(&topics, "rr3-act/servo/command", &mut errs);
    let face_topic = topic_string_of(&topics, "rr3-interact/face/expression", &mut errs);
    let audio = topic_string_of(&topics, "rr3-sense/audio/pcm", &mut errs);
    let (_all, board_subset) = face_exprs(&face, &mut errs);
    let axes = axis_travel_rows(&root, &mut errs);
    if !errs.is_empty() {
        die(&errs);
    }

    // 写盘/对账两份生成物：topics_gen.rs（主题串+白名单）与 axis_gen.rs（行程表）。
    // 任一份漂移都退出 1——门 f 拟 ⑨-d 对两表一视同仁。
    let out_path = root.join("tools/esp32-periph-base/src/topics_gen.rs");
    let rendered = render(&servo, &face_topic, &audio, &board_subset);
    let axis_path = root.join("tools/esp32-periph-base/src/axis_gen.rs");
    let axis_rendered = render_axes(&axes);

    let compare_or_write = |path: &Path, text: String| {
        if check_only {
            let existing = fs::read_to_string(path).unwrap_or_else(|e| {
                die(&[format!(
                    "生成文件缺席（先跑一次不带 --check 的生成器落盘）: {e}"
                )])
            });
            if existing == text {
                println!(
                    "rr3-codegen: --check 一致（{} 与真值点同步）",
                    path.display()
                );
            } else {
                eprintln!(
                    "rr3-codegen: 生成物与真值点漂移——{} 与重跑结果不一致；\
                     在仓根跑 `cargo run -p rr3-codegen` 重生成并审阅（门 f 拟 ⑨-d）",
                    path.display()
                );
                std::process::exit(1);
            }
        } else {
            fs::write(path, text)
                .unwrap_or_else(|e| die(&[format!("写 {} 失败: {e}", path.display())]));
            println!("rr3-codegen: 已写 {}", path.display());
        }
    };
    compare_or_write(&out_path, rendered);
    compare_or_write(&axis_path, axis_rendered);
}

#[cfg(test)]
mod tests {
    use super::{render, render_axes};

    /// 生成文本的形状回归：四常量齐全、数组项逐行带逗号（无 " 相邻" 双逗号
    /// ——曾实际产出 `"turn",,`）、@generated 头注在位（手改必被门 f 抓红）。
    #[test]
    fn render_output_shape_is_pinned() {
        let out = render(
            "rr3-act/servo/command",
            "rr3-interact/face/expression",
            "rr3-sense/audio/pcm",
            &["idle".into(), "blink".into()],
        );
        assert!(out.contains("pub(crate) const RR3_SERVO_TOPIC: &str = \"rr3-act/servo/command\";"));
        assert!(out
            .contains("pub(crate) const RR3_FACE_TOPIC: &str = \"rr3-interact/face/expression\";"));
        assert!(out.contains("pub(crate) const RR3_AUDIO_TOPIC: &str = \"rr3-sense/audio/pcm\";"));
        assert!(out.contains("pub(crate) const RR3_FACE_EXPRS: [&str; 2]"));
        assert!(out.contains("    \"idle\",\n    \"blink\",\n];"), "{out}");
        assert!(
            !out.contains(",,"),
            "数组不得出现双逗号（曾实际产出，回归钉死）"
        );
        assert!(out.starts_with("// @generated by tools/rr3-codegen"));
    }

    /// 行程常量表形状回归（批N3 件五）：常量名 = RR3_AXIS_<大写 instance>、
    /// 三元组 (lo, hi, center) 序、match 臂逐轴点名、查无回 None、头注在位。
    #[test]
    fn render_axes_shape_is_pinned() {
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
        assert!(out.starts_with("// @generated by tools/rr3-codegen"));
    }
}
