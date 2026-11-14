//! 装配清单解析单测（自同名单测模块原样抽出，判据不变）。

#[cfg(test)]
mod tests {
    use super::*;

    const OK_SAMPLE: &str = r#"
schema_version = 2

[assemble]
handles = [
  { trait = "Rr3Power", desc_key = "i2c_bus" },
  { trait = "Rr3Power", desc_key = "ina219_addr" },
]
ops = [
  { slot = "low_voltage_mv", src_key = "rr3-power.battery.low_voltage_mv" },
]
"#;

    #[test]
    fn sample_and_empty_arrays_parse() {
        let f = rr3_assemble_parse(OK_SAMPLE).expect("正例必须过");
        assert_eq!(f.handles.len(), 2);
        assert_eq!(f.handles[0].trait_name, "Rr3Power");
        assert_eq!(f.handles[0].desc_key, "i2c_bus");
        assert_eq!(f.ops.len(), 1);
        assert_eq!(f.ops[0].slot, "low_voltage_mv");

        // 空数组合法，但须明写 no_injection（L-5）
        let f = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .expect("明写不需注入面的空数组必须合法");
        assert!(f.handles.is_empty() && f.ops.is_empty() && f.no_injection);
    }

    /// L-5 空面规则：两数组均空而漏写 no_injection ⇒ 红；no_injection 与非空面并存 ⇒ 红。
    #[test]
    fn empty_surface_requires_no_injection_marker() {
        // 漏写 → 点名"未明写 no_injection"
        let errs = rr3_assemble_parse("schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n")
            .expect_err("空面漏写声明必须红");
        assert!(
            errs.iter().any(|e| e.contains("未明写 no_injection")),
            "{errs:?}"
        );

        // 写了 no_injection 却又给了 handles → 互斥，红
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nno_injection = true\nhandles = [ { trait = \"Rr3Power\", desc_key = \"i2c_bus\" } ]\nops = []\n",
        )
        .expect_err("声明与注入面并存必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("只允许与空的 handles/ops 并存")),
            "{errs:?}"
        );

        // no_injection = false 不认（只认 true）
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = false\n",
        )
        .expect_err("no_injection=false 必须红");
        assert!(errs.iter().any(|e| e.contains("只认 true")), "{errs:?}");
    }

    #[test]
    fn bad_shapes_are_all_named() {
        let text = concat!(
            "schema_version = 1\n", // 版本不认
            "mystery = 1\n",        // 顶层未知键
            "\n[assemble]\n",
            "extra = 1\n",   // [assemble] 未知键
            "handles = [\n", // 条目两处不在册
            "  { trait = \"Rr3Nope\", desc_key = \"nope_key\" },\n",
            "  \"bare-string\",\n", // 非表条目
            "]\n",
            "ops = [\n",
            "  { slot = \"\", src_key = \"a..b\" },\n", // 空槽 + 坏点路径
            "  { slot = \"s\", src_key = 3 },\n",       // 非字符串
            "]\n",
        );
        let errs = rr3_assemble_parse(text).expect_err("坏形必须报错");
        assert!(
            errs.iter().any(|e| e.contains("schema_version 必须为 2")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("顶层未知键 mystery")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("[assemble] 未知键 extra")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("trait \"Rr3Nope\" 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("desc_key \"nope_key\" 未在册")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("handles[1] 应为内联表")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("ops[0].slot 不得为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("src_key \"a..b\" 不是合法点路径")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("ops[1].src_key 应为字符串")),
            "{errs:?}"
        );
    }

    #[test]
    fn missing_required_keys_are_named() {
        // 缺 schema_version / 缺 [assemble] / 缺两键
        let errs =
            rr3_assemble_parse("[assemble]\nhandles = []\nops = []\n").expect_err("缺版本必须点名");
        assert!(errs[0].contains("缺 schema_version"), "{errs:?}");

        let errs = rr3_assemble_parse("schema_version = 2\n").expect_err("缺 [assemble] 必须点名");
        assert!(
            errs.iter().any(|e| e.contains("缺 [assemble] 表")),
            "{errs:?}"
        );

        let errs = rr3_assemble_parse("schema_version = 2\n[assemble]\n")
            .expect_err("缺 handles/ops 必须点名");
        assert!(errs.iter().any(|e| e.contains("缺 handles 键")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("缺 ops 键")), "{errs:?}");
    }

    #[test]
    fn injection_gaps_walk_config_paths() {
        let facts = rr3_assemble_parse(OK_SAMPLE).expect("正例必须过");
        let cfg = "[rr3-power.battery]\nlow_voltage_mv = 3300\n";
        // i2c 句柄不参与注入缺；ops 的一条在、一条缺 → 恰一条缺口
        let gaps = rr3_injection_gaps(&facts, cfg);
        assert_eq!(gaps.len(), 0, "{gaps:?}");

        let facts2 = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = [\n  { slot = \"a\", src_key = \"rr3-power.battery.low_voltage_mv\" },\n  { slot = \"b\", src_key = \"rr3-power.battery.missing_key\" },\n  { slot = \"c\", src_key = \"no.such.path\" },\n]\n",
        )
        .expect("必须过形状级");
        let gaps = rr3_injection_gaps(&facts2, cfg);
        assert_eq!(gaps.len(), 2, "{gaps:?}");
        assert!(
            gaps[0].contains("missing_key") && gaps[0].contains("注入缺"),
            "{gaps:?}"
        );

        // config 解析失败 = 全部来源键不可证
        let gaps = rr3_injection_gaps(&facts, "garbage =\n");
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert!(gaps[0].contains("解析失败"), "{gaps:?}");

        // ops 为空 → 永无缺口（config 坏也不点名）；空面须明写 no_injection（L-5）
        let empty = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
        )
        .unwrap();
        assert!(rr3_injection_gaps(&empty, "garbage =\n").is_empty());
    }

    /// 电流需求申报面（基-36 首件）：正例解析 + 缺键 = 空申报合法 + 坏形逐条点名。
    /// 判据本体（Σ ≤ cap）不在本模块——读者在 rrd rr3_demand_budget。
    #[test]
    fn demands_parse_and_shape_errors_are_named() {
        // 正例：servo 真实申报形状（五键定形 = ADR-0021 裁定 2；handles 同 servo 真实
        // 句柄，demands 不豁免 L-5 空面规则——它不是注入面）
        let f = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = [\n  { trait = \"Rr3PwmServo\", desc_key = \"pwm_chip\" },\n]\nops = []\ndemands = [\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [] },\n]\n",
        )
        .expect("正例必须过");
        assert_eq!(f.demands.len(), 1);
        assert_eq!(f.demands[0].rail, "rail5v");
        assert_eq!(f.demands[0].peak_ma, 700);
        assert_eq!(f.demands[0].sustained_ma, 120);
        assert!(f.demands[0].critical);
        assert!(f.demands[0].concurrency_group.is_empty());

        // 缺 demands 键 = 空申报合法（执法面归门 c）
        let f = rr3_assemble_parse(OK_SAMPLE).expect("无 demands 必须过");
        assert!(f.demands.is_empty());

        // 坏形逐条点名（五键全显式，T-5）：空 rail / 峰值非正 / 缺 sustained_ma /
        // sustained>peak 物理矛盾 / critical 缺失 / concurrency_group 缺键 / 非数组 /
        // 空组名与非串元素 / 未知键 / 非表条目
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = [\n  { trait = \"Rr3PwmServo\", desc_key = \"pwm_chip\" },\n]\nops = []\ndemands = [\n  { rail = \"\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 0, sustained_ma = 120, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 800, critical = true, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, concurrency_group = [] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = \"axis\" },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [\"\", 1] },\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = [], wat = 1 },\n  \"bare\",\n]\n",
        )
        .expect_err("坏形必须报错");
        assert!(
            errs.iter().any(|e| e.contains("demands[0].rail 不得为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[1].peak_ma = 0 须为正整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[2].sustained_ma 应为整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains(
                "demands[3].sustained_ma = 800 > peak_ma = 700（连续分量超过峰值是物理矛盾"
            )),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[4].critical 应为布尔")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[5].concurrency_group 缺失")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[6].concurrency_group 应为字符串数组")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[7].concurrency_group[0] 不得为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("demands[7].concurrency_group[1] 应为字符串")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("demands[8] 未知键 wat")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("demands[9] 应为内联表")),
            "{errs:?}"
        );
    }

    /// 内存需求申报面（校验⑩ 分子，批N2 n2c-v10）：正例解析 + 缺键 = 未申报 0
    /// 合法 + 负数/非整数红。判据本体（Σ ≤ 预算）不在本模块——读者在
    /// rrd `rr3_memory_budget`（分母住 [supplies].budget）。
    #[test]
    fn memory_bytes_parse_and_shape_errors_are_named() {
        // 正例：两键显式给出（件级语义非每实例，直加不扩算——头注同源）
        let f = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = [\n  { trait = \"Rr3Display\", desc_key = \"spi_device\" },\n]\nops = []\ntask_stack_bytes = 8192\nheap_peak_bytes = 24576\n",
        )
        .expect("正例必须过");
        assert_eq!(f.task_stack_bytes, 8192);
        assert_eq!(f.heap_peak_bytes, 24576);

        // 缺两键 = 未申报 = 0 不进 Σ（申报义务未触发，同 demands 缺键合法）
        let f = rr3_assemble_parse(OK_SAMPLE).expect("无内存键必须过");
        assert_eq!(f.task_stack_bytes, 0);
        assert_eq!(f.heap_peak_bytes, 0);

        // 坏形逐条点名：负数（物理矛盾）/ 非整数 / 未知键拼错（词表封闭）
        let errs = rr3_assemble_parse(
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\ntask_stack_bytes = -1\nheap_peak_bytes = \"16k\"\nstack_bytes = 4096\n",
        )
        .expect_err("坏形必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("task_stack_bytes = -1 须为非负整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("heap_peak_bytes 应为整数")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("[assemble] 未知键 stack_bytes")),
            "{errs:?}"
        );
    }
}
