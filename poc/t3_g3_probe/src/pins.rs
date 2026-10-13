//! `pins` —— **P-8**：ADR-0021 精确锁版整组编译通过 + 版本核对。
//!
//! 本模块读探针自己的 `Cargo.lock`，核对**整组精确锁版**（ADR-0021 决策 1）逐条一致，
//! 并确认**依赖树不含 `blitz-dom`**（直连路线，ADR-0020 决策 1）与 GUI 栈黑名单。

use std::collections::BTreeMap;

/// ADR-0021 决策 1 的精确锁版整组（`包名 -> 版本`）。
///
/// 取值 = `poc/t2_4_blitz_probe/Cargo.lock` 中经 blitz 传递编译通过的那一组。
pub const PINNED: &[(&str, &str)] = &[
    ("stylo", "0.20.0"),
    ("stylo_dom", "0.20.0"),
    ("stylo_traits", "0.20.0"),
    ("stylo_taffy", "0.3.0-beta.2"),
    ("taffy", "0.14.0"),
    ("parley", "0.11.1"),
    ("selectors", "0.40.0"),
    ("skrifa", "0.44.0"),
    ("fontique", "0.11.1"),
];

/// 直连路线**不得**引入的包（ADR-0020 决策 1：不依赖 `blitz-dom`）。
pub const FORBIDDEN: &[&str] = &[
    "blitz-dom",
    "blitz-traits",
    "blitz",
    "blitz-shell",
    "blitz-paint",
];

/// 锁文件版本解析结果。
#[derive(Clone, Debug, Default)]
pub struct PinsReport {
    /// 逐条 (包名, 期望版本, 实际版本, 是否一致)。
    pub rows: Vec<(&'static str, &'static str, Option<String>, bool)>,
    /// 是否全部一致。
    pub all_match: bool,
    /// 依赖树里出现的禁用包。
    pub forbidden_found: Vec<String>,
}

/// 解析 `Cargo.lock` 得 `包名 -> 版本`。
///
/// `Cargo.lock` 是 cargo 生成的固定形状 TOML，行扫描足够；解析失败由调用方的
/// `entries_parsed` 断言兜住，不会静默假绿。
#[must_use]
pub fn parse_lock_versions(lock: &str) -> BTreeMap<String, String> {
    fn unquote(s: &str) -> String {
        s.trim().trim_matches('"').to_string()
    }
    let mut out = BTreeMap::new();
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut in_package = false;
    for raw in lock.lines() {
        let line = raw.trim();
        if line == "[[package]]" {
            if let (Some(n), Some(v)) = (name.take(), version.take()) {
                out.insert(n, v);
            }
            in_package = true;
            continue;
        }
        if line.starts_with('[') {
            if let (Some(n), Some(v)) = (name.take(), version.take()) {
                out.insert(n, v);
            }
            in_package = false;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name = ") {
            name = Some(unquote(rest));
        } else if let Some(rest) = line.strip_prefix("version = ") {
            version = Some(unquote(rest));
        }
    }
    if let (Some(n), Some(v)) = (name, version) {
        out.insert(n, v);
    }
    out
}

/// 读探针目录下的 `Cargo.lock` 并核对锁版整组。
#[must_use]
pub fn check_pins() -> PinsReport {
    let lock_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    let lock = std::fs::read_to_string(&lock_path).unwrap_or_default();
    check_pins_from_lock(&lock)
}

/// 从给定锁文本核对（便于测试注入）。
#[must_use]
pub fn check_pins_from_lock(lock: &str) -> PinsReport {
    let versions = parse_lock_versions(lock);
    let mut rows = Vec::with_capacity(PINNED.len());
    let mut all_match = !versions.is_empty();
    for (pkg, want) in PINNED {
        let got = versions.get(*pkg).cloned();
        let ok = got.as_deref() == Some(*want);
        if !ok {
            all_match = false;
        }
        rows.push((*pkg, *want, got, ok));
    }
    let forbidden_found = FORBIDDEN
        .iter()
        .filter(|p| versions.contains_key(**p))
        .map(|p| (*p).to_string())
        .collect();
    PinsReport {
        rows,
        all_match,
        forbidden_found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P-8：整组精确锁版逐条一致，且不含 `blitz-dom`。
    #[test]
    fn pinned_group_matches_adr_0021() {
        let rep = check_pins();
        for (pkg, want, got, ok) in &rep.rows {
            assert!(
                *ok,
                "锁版不符：{pkg} 期望 ={want}，实际 {got:?}（ADR-0021 决策 1）"
            );
        }
        assert!(
            rep.forbidden_found.is_empty(),
            "直连路线不得引入：{:?}（ADR-0020 决策 1）",
            rep.forbidden_found
        );
    }
}
