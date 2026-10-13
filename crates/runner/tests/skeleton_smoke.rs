//! runner 的 T0 骨架冒烟测试。
//!
//! 本文件同时验证两件事：
//! 1. 测试基建已接通 —— `insta`（快照）与 `proptest`（属性测试）都能编译并运行；
//! 2. crate 边界名单与《技术方案》第 16 节一致（数量 + 关键成员 + 命名规范）。
//!
//! 冒烟测试随 G0 交付物的落地被替换，不作为长期回归集合（技术方案 §17）。

use proptest::prelude::*;

/// 名单必须覆盖技术方案 §16 的全部 19 个成员，且关键成员不缺。
#[test]
fn roster_matches_tech_plan_section_16() {
    let roster = runner::workspace_crates();
    assert_eq!(
        roster.len(),
        19,
        "技术方案 §16 定义了 19 个 crate 分组成员，实际为 {}",
        roster.len()
    );
    for required in [
        "engine-v8",
        "bindings-webidl",
        "web-dom",
        "scheduler",
        "crypto",
        "sdk-python",
        "runner",
    ] {
        assert!(
            roster.contains(&required),
            "名单缺少技术方案 §16 要求的成员：{required}"
        );
    }
}

/// 名单内不允许出现重复项（重复意味着边界定义出现分叉）。
#[test]
fn roster_has_no_duplicates() {
    let roster = runner::workspace_crates();
    let mut sorted = roster.to_vec();
    sorted.sort_unstable();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(before, sorted.len(), "名单存在重复成员：{sorted:?}");
}

proptest! {
    /// 任意下标取出的成员名都必须是非空的 kebab-case（跨平台文件名纪律的前提：
    /// crate 名与目录名一一对应，且大小写严格一致）。
    #[test]
    fn roster_entries_are_kebab_case(index in 0usize..1024usize) {
        let roster = runner::workspace_crates();
        let name = roster[index % roster.len()];
        prop_assert!(!name.is_empty(), "名单不应包含空名称");
        prop_assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "成员名 {name} 不是 kebab-case",
        );
    }
}

/// 脚手架状态行的 insta 快照：钉死格式，任何无意漂移在 diff 中可见。
///
/// ⚠️ 本快照**只在脚手架期有效**（它钉死的是一个占位常量，不含真实行为）。
/// G0 交付后随 `runner` 的脚手架接口一并删除；届时删除/变更本快照必须走
/// **独立 PR**（AGENTS.md 铁律 2），并按 TASKS 的 golden 变更流程写明理由。
#[test]
fn scaffold_status_snapshot() {
    insta::assert_snapshot!("t0_scaffold_status", runner::scaffold_status());
}
