//! L-5 空面规则：两数组皆空须明写 no_injection，与注入面互斥。

/// 两数组均在文本中出现且均为空（L-5 空面的前件）。
fn rr3_empty_surface(
    handles_present: bool,
    handles_empty: bool,
    ops_present: bool,
    ops_empty: bool,
) -> bool {
    [handles_present, handles_empty, ops_present, ops_empty]
        .iter()
        .all(|b| *b)
}

/// L-5 空面规则（0.0.5 批M MC-10）：两数组均在文本中出现且均为空 ⇒ 必须明写
/// no_injection = true；明写 no_injection 时两数组必须都空。件级与组合件级共用。
pub(crate) fn rr3_no_injection_err(
    handles_present: bool,
    handles_empty: bool,
    ops_present: bool,
    ops_empty: bool,
    no_injection: bool,
) -> Vec<String> {
    let mut errs: Vec<String> = Vec::new();
    let empty = rr3_empty_surface(handles_present, handles_empty, ops_present, ops_empty);
    rr3_noinj_empty(empty, no_injection, &mut errs);
    rr3_noinj_clash(empty, no_injection, &mut errs);
    errs
}

/// 两数组均空却未明写 no_injection = true（L-5）。
fn rr3_noinj_empty(empty: bool, no_injection: bool, errs: &mut Vec<String>) {
    if empty && !no_injection {
        errs.push(
            "注入面两数组均空却未明写 no_injection = true（L-5：或补齐 handles/ops，或明写该件不需注入面）"
                .into(),
        );
    }
}

/// 明写 no_injection 却与注入面并存（两者互斥）。
fn rr3_noinj_clash(empty: bool, no_injection: bool, errs: &mut Vec<String>) {
    if no_injection && !empty {
        errs.push("no_injection = true 只允许与空的 handles/ops 并存（该声明与注入面互斥）".into());
    }
}
