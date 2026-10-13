//! `report` —— 探针判定的**单一真相源**。
//!
//! 汇总 S1 四项探针（P-1/P-2/P-8/P-11）的判定，避免「打印的结论」与「裁决用的结论」不一致
//! （对齐 `poc/t2_4_blitz_probe` 的 `Outcomes` 范式）。

use crate::pins::check_pins;
use crate::prefs::headless_init_probe;

/// `style::dom::TElement` 的**必需** `unsafe fn` 方法（无默认实现）。
///
/// 来源：`stylo-0.20.0/dom.rs`——
/// `set_handled_snapshot`(:648) / `set_dirty_descendants`(:675) / `unset_dirty_descendants`(:680)
/// / `ensure_data`(:739) / `clear_data`(:744)。
///
/// 参考实现 `blitz-dom-0.3.0-beta.2/src/stylo.rs:671/675/680/692/697` 亦逐条声明为 `unsafe fn`。
pub const TELEMENT_UNSAFE_FNS: &[(&str, u32)] = &[
    ("set_handled_snapshot", 648),
    ("set_dirty_descendants", 675),
    ("unset_dirty_descendants", 680),
    ("ensure_data", 739),
    ("clear_data", 744),
];

/// 单项探针判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 未通过（能力不成立）。
    Fail,
    /// **阻断**：发现必须 `unsafe` / 与硬约束冲突 ⇒ 停上报。
    Blocked,
    /// 尚未执行（依赖前置项）。
    Pending,
}

impl Verdict {
    /// 单字符标签（`PASS` / `FAIL` / `BLOCK` / `PEND`）。
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::Blocked => "BLOCK",
            Verdict::Pending => "PEND",
        }
    }
}

/// 全部探针的判定结果。
#[derive(Clone, Debug)]
pub struct S1Outcome {
    /// P-8：精确锁版整组编译通过 + 版本核对一致。
    pub p8: Verdict,
    /// P-11：prefs 无 GUI 可运行。
    pub p11: Verdict,
    /// P-1-机制：opaque 侧表（零 unsafe）反查命中。
    pub p1_mechanism: Verdict,
    /// P-1-总：投影编译 + 级联 + 反查 + **零 unsafe**。
    pub p1: Verdict,
    /// P-2：stylo→taffy 桥。
    pub p2: Verdict,
    /// 锁版整组是否逐条一致（供打印）。
    pub pins_rows: usize,
    /// 依赖树里发现的禁用包。
    pub forbidden: Vec<String>,
    /// P-11 样式线程数。
    pub style_threads: Option<usize>,
}

/// 计算 S1 判定（除 P-1/P-2 需实地编译-级联外，其余在本环境实测）。
#[must_use]
pub fn evaluate() -> S1Outcome {
    let pins = check_pins();
    let prefs = headless_init_probe();

    let p8 = if pins.all_match && pins.forbidden_found.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    let p11 = if prefs.global_style_data_init {
        Verdict::Pass
    } else {
        Verdict::Fail
    };

    // P-1-机制：侧表在 `#![forbid(unsafe_code)]` 下编译且 `opaque` 反查命中（由 `opaque` 单测证）。
    let p1_mechanism = Verdict::Pass;

    // P-1-总：**阻断** —— `TElement` 的必需 `unsafe fn` 与全仓 `unsafe_code = forbid` 冲突。
    let p1 = Verdict::Blocked;

    // P-2：需在 P-1 投影（`TElement`）成立后经 `to_taffy_style` 出布局；P-1 阻断 ⇒ 未执行。
    let p2 = Verdict::Pending;

    S1Outcome {
        p8,
        p11,
        p1_mechanism,
        p1,
        p2,
        pins_rows: pins.rows.len(),
        forbidden: pins.forbidden_found,
        style_threads: prefs.style_threads,
    }
}

impl S1Outcome {
    /// **综合判定**：任一 `Blocked` ⇒ `Blocked`；否则任一 `Fail` ⇒ `Fail`；全 `Pass` ⇒ `Pass`。
    #[must_use]
    pub fn overall(&self) -> Verdict {
        let all = [self.p1, self.p2, self.p8, self.p11];
        if all.contains(&Verdict::Blocked) {
            Verdict::Blocked
        } else if all.contains(&Verdict::Fail) {
            Verdict::Fail
        } else if all.contains(&Verdict::Pending) {
            Verdict::Pending
        } else {
            Verdict::Pass
        }
    }
}
