"""`python -m rr3_brain` 入口（0.0.5 批C⑤ 原子重铺，组合件分装吸收）。

入口只做装配与启动：机制主体在 [`rr3_brain.composite`]（dialog ⊗ asr ⊗ llm ⊗ tts，
见 composite.toml），原子 manifest 在 `atoms/`（atom.toml + assemble.toml 两件
Python 面可落件——机制本体 .py 原位，Python 单文件即机制+入口合一，Rust 五件套
拆分随 ADR-0016 迁移收敛）；bus 总线壳留组合件机制壳（0.0.5 方案 §12.2）。
module.toml / config.toml / pyproject.toml 功能守恒零改动；启动打印序列逐字
不变（见 composite.rr3_brain_run 开头）。
"""

from rr3_brain.composite import main

if __name__ == "__main__":
    raise SystemExit(main())
