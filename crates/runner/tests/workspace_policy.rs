//! 工程底座一致性测试（脚手架期，T0）。
//!
//! 把三条原本靠人工约定的事变成机器门禁：
//!
//! 1. `runner::workspace_crates()` 必须与 workspace 的**真实成员集合完全相等**
//!    （只比数量是无效门禁：把 `graphics` 换成别的 kebab-case 名字，数量仍为 19）。
//! 2. `engine-v8` 的 lint 块必须与 workspace 的 lint 块**逐项相等**，`unsafe_code` 除外 ——
//!    防止白名单 crate 的 lint 副本在后续全仓 lint 升级时静默漂移（形成检查盲区）。
//! 3. 除 `engine-v8` 外，所有成员必须继承 workspace lints（`[lints] workspace = true`）。
//!
//! 脚手架期放在 `runner` 的测试目录下；`xtask` 就位后应迁出（见 `docs/TASKS.md`）。
//! 之所以用 `toml` 官方解析器而不是手写解析：手写解析器一旦与 TOML 语法脱节，
//! 会给出一条「假绿」的门禁，比没有门禁更危险。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 允许不继承 workspace lints 的 crate（AGENTS.md 铁律 4 白名单）。
///
/// 两支（ADR-0002 + **ADR-0024 修正注记**）：
/// - `engine-v8`：V8/rusty_v8 的 FFI 薄桥（铁律 5）。
/// - `layout-backend`：**stylo FFI 薄桥**（`style::dom::TElement` 的 5 个必需 `unsafe fn`）。
const WHITELISTED_CRATES: &[&str] = &["engine-v8", "layout-backend"];

/// 是否白名单 crate（允许不继承 workspace lints / 允许逐 `fn` `#[allow(unsafe_code)]`）。
fn is_whitelisted_crate(name: &str) -> bool {
    WHITELISTED_CRATES.contains(&name)
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/crates/runner
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 至少有两级父目录")
        .to_path_buf()
}

/// 读取并解析一个 Cargo.toml；解析失败即让测试红（配置不可解析本身就是要报的错）。
fn read_cargo_toml(path: &Path) -> toml::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("读取 {} 失败：{e}", path.display()));
    // 注意：必须用 `toml::from_str`（文档解析），不能用 `str::parse::<toml::Value>()`
    // —— 后者在 toml 0.9 里按「单个值」解析，遇到 `[package]` 会报 unexpected content。
    toml::from_str::<toml::Value>(&text)
        .unwrap_or_else(|e| panic!("解析 {} 失败：{e}", path.display()))
}

/// 取 `[lints]` 下的 lint 表；表不存在返回 `None`。
fn lints_table<'a>(manifest: &'a toml::Value, kind: &str) -> Option<&'a toml::Value> {
    manifest.get("lints")?.get(kind)
}

/// 把 `<rust|lints>` 两个表收敛成扁平的「lint 名 → 级别」映射，便于逐项比较。
fn flatten_lints(
    rust: Option<&toml::Value>,
    clippy: Option<&toml::Value>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (prefix, table) in [("rust", rust), ("clippy", clippy)] {
        let Some(table) = table else { continue };
        let table = table
            .as_table()
            .unwrap_or_else(|| panic!("[lints.{prefix}] 必须是表，实际为 {table:?}"));
        for (key, value) in table {
            out.insert(format!("{prefix}.{key}"), value.to_string());
        }
    }
    out
}

/// 收集一个 manifest 中出现的**全部依赖名**（去重），覆盖四个依赖位置：
/// `[dependencies]` / `[build-dependencies]` / `[dev-dependencies]` /
/// `[target.*.dependencies]`（并顺带覆盖 `target.*` 下的 build/dev 变体，以防漏网）。
///
/// 只取**键名**（依赖名），不关心版本约束 —— 断言的命题是「某依赖名是否出现」，
/// 与版本字面量无关。
fn dependency_names(manifest: &toml::Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();

    for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
        if let Some(table) = manifest.get(section).and_then(toml::Value::as_table) {
            names.extend(table.keys().cloned());
        }
    }

    // `[target.<cfg>.dependencies]`：`target` 是表，其每个值是平台配置表，
    // 平台表下才是 dependencies 表。
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for cfg in targets.values() {
            let Some(cfg_table) = cfg.as_table() else {
                continue;
            };
            for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
                if let Some(table) = cfg_table.get(section).and_then(toml::Value::as_table) {
                    names.extend(table.keys().cloned());
                }
            }
        }
    }

    names
}

/// 递归收集 `dir` 下所有 `.rs` 文件的路径（跳过名为 `target` / `.git` 的目录）。
///
/// 依据 ADR-0009 决策 7：`grep` crate 不在 workspace 依赖里，故用 `std::fs::read_dir`
/// 递归 + 字符串匹配（**禁用外部 grep crate**）。
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = path.file_name().map(std::ffi::OsStr::to_string_lossy);
            if matches!(name.as_deref(), Some("target" | ".git")) {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// 在全部 `crates/*/src/**/*.rs` 中检索包含 `needle` 的行。
///
/// 只扫每个成员的 `src/` 子树（对齐 ADR-0009 决策 7 的 `crates/*/src/**/*.rs`）——
/// 这一点是**必要**的：本断言所在的测试文件自身也含有这些字符串字面量，
/// 若不排除 `tests/` 会造成自命中（假红）。
///
/// 返回 `(仓库相对路径, 行号从 1 起, 该行 trim 后内容)` —— 消息里打印**文件:行号:内容**，
/// 便于人工区分「真·定义」与「字符串字面量 / 注释里的同名文本」（ADR-0009 决策 7 的已知局限）。
fn source_hits(root: &Path, needle: &str) -> Vec<(String, usize, String)> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            let src = entry.path().join("src");
            if src.is_dir() {
                collect_rs_files(&src, &mut files);
            }
        }
    }

    let mut hits = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (lineno, line) in text.lines().enumerate() {
            if line.contains(needle) {
                let rel = file
                    .strip_prefix(root)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .replace('\\', "/");
                hits.push((rel, lineno + 1, line.trim().to_owned()));
            }
        }
    }
    hits
}

#[test]
fn workspace_members_equal_scaffold_roster() {
    let root = workspace_root();
    let manifest = read_cargo_toml(&root.join("Cargo.toml"));
    let members: BTreeSet<String> = manifest
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .expect("[workspace] members 必须是数组")
        .iter()
        .map(|v| {
            let raw = v.as_str().expect("members 元素必须是字符串");
            raw.strip_prefix("crates/")
                .unwrap_or_else(|| panic!("成员路径 {raw} 必须以 crates/ 开头"))
                .to_owned()
        })
        .collect();

    let roster: BTreeSet<String> = runner::workspace_crates()
        .iter()
        .map(|name| (*name).to_owned())
        .collect();

    let only_in_manifest: Vec<_> = members.difference(&roster).collect();
    let only_in_roster: Vec<_> = roster.difference(&members).collect();
    assert!(
        only_in_manifest.is_empty() && only_in_roster.is_empty(),
        "crate 边界出现分叉：Cargo.toml 多出 {only_in_manifest:?}，脚手架名单多出 {only_in_roster:?}"
    );
}

#[test]
fn whitelisted_crate_lint_blocks_match_workspace_except_unsafe_code() {
    // 泛化自 `engine_v8_lint_block_matches_workspace_except_unsafe_code`（ADR-0024 决策 4）：
    // 对**每一支**白名单 crate（engine-v8 / layout-backend）断言其 lint 副本与 workspace **逐项全等**
    // （`unsafe_code` 除外；`:224-227` 的逐项全等不得放宽）。
    let root = workspace_root();
    let workspace = read_cargo_toml(&root.join("Cargo.toml"));
    let ws_lints = workspace
        .get("workspace")
        .and_then(|w| w.get("lints"))
        .expect("根 Cargo.toml 必须有 [workspace.lints]");

    for name in WHITELISTED_CRATES {
        let manifest = read_cargo_toml(&root.join("crates").join(name).join("Cargo.toml"));

        let mut workspace_lints = flatten_lints(ws_lints.get("rust"), ws_lints.get("clippy"));
        let mut crate_lints = flatten_lints(
            lints_table(&manifest, "rust"),
            lints_table(&manifest, "clippy"),
        );

        // 唯一允许的差异：unsafe_code（forbid → deny，见 ADR-0002 / ADR-0024 决策 1）
        let ws_unsafe = workspace_lints.remove("rust.unsafe_code");
        let crate_unsafe = crate_lints.remove("rust.unsafe_code");
        assert_eq!(
            ws_unsafe.as_deref(),
            Some("\"forbid\""),
            "workspace 级 unsafe_code 必须保持不可被覆盖的 forbid"
        );
        assert_eq!(
            crate_unsafe.as_deref(),
            Some("\"deny\""),
            "{name} 必须显式把 unsafe_code 放宽为 deny（白名单的唯一开口）"
        );

        assert_eq!(
            workspace_lints, crate_lints,
            "{name} 的 lint 副本与 workspace lints 发生漂移；全仓 lint 升级时必须同步"
        );
    }
}

#[test]
fn only_whitelisted_crate_opts_out_of_workspace_lints() {
    let root = workspace_root();
    for name in runner::workspace_crates() {
        let manifest = read_cargo_toml(&root.join("crates").join(name).join("Cargo.toml"));
        let inherits = manifest
            .get("lints")
            .and_then(|l| l.get("workspace"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        if is_whitelisted_crate(name) {
            assert!(
                !inherits,
                "{name} 不应继承 workspace lints（forbid 无法被覆盖，白名单会失效）"
            );
        } else {
            assert!(
                inherits,
                "{name} 必须写 [lints] workspace = true，否则会静默脱离全仓 lint 约束"
            );
        }
    }
}

#[test]
fn v8_dependency_only_in_engine_v8() {
    // 铁律 5（AGENTS.md:22「V8 / rusty_v8 类型不得越出 engine-v8 crate」）的
    // 依赖层机器执行；依据 docs/adr/0009-bindings-webidl与engine-v8的V8类型边界.md
    // 决策 3 / A8（PR-3b）。
    //
    // 为什么查 manifest 而不是查源码：`v8` 是外部 crate，任何 crate 要在源码里写
    // `use v8::Local;` 就**必须**在自己的 Cargo.toml 声明 `v8` 依赖（Cargo 硬事实）。
    // 因此「`v8` 不得越出 engine-v8」在依赖层等价于「只有 engine-v8 的 manifest 允许
    // 出现 `v8`」。manifest 是 TOML 结构，用官方解析器读取 ⇒ 无手写解析器的假绿风险。
    //
    // 说明：G1 已落地 `crates/engine-v8/Cargo.toml` 的 `v8 = "152.2.0"`（ADR-0009 决策 1），
    // 故本条断言**当前并非「对空集成立」**——engine-v8 是唯一被允许持有 `v8` / `rusty_v8` 的
    // 成员，其余任何成员一旦引入该依赖即红。其判别力另由 ADR-0009 要求的四态反向验证证明
    // （在 `bindings-webidl/Cargo.toml` 注入 `v8 = "152.2.0"` ⇒ 立刻红）。
    // 评审不得以「现在就是绿的」为由判定它恒真。
    let root = workspace_root();
    // 只允许出现在 engine-v8 的两个依赖名（crate 名 `v8`；上游仓库名 `rusty_v8`）。
    let guarded = ["v8", "rusty_v8"];

    // crate -> 命中的被守卫依赖名列表。
    let mut offenders: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for name in runner::workspace_crates() {
        // engine-v8 是唯一允许持有 v8 依赖的 crate，跳过（不要求它**必须**有）。
        // 注意：layout-backend 虽在 lints 白名单内，但**不允许**持有 v8 依赖 ⇒ 这里只跳过 engine-v8。
        if *name == "engine-v8" {
            continue;
        }
        let manifest = read_cargo_toml(&root.join("crates").join(name).join("Cargo.toml"));
        let deps = dependency_names(&manifest);
        let hit: Vec<String> = guarded
            .iter()
            .filter(|dep| deps.contains(**dep))
            .map(|dep| (*dep).to_owned())
            .collect();
        if !hit.is_empty() {
            offenders.insert((*name).to_owned(), hit);
        }
    }

    assert!(
        offenders.is_empty(),
        "铁律 5：`v8`/`rusty_v8` 依赖只允许出现在 `engine-v8`，\
         却出现在非白名单 crate（ADR-0009 决策 3）：{offenders:?}"
    );
}

#[test]
fn three_safety_types_have_single_definition_point() {
    // ADR-0009 决策 7 / 决策 2：类型面三类型的 Rust 定义**各恰好一处**，
    // 且定义点必须是 `crates/engine-v8/src/abi.rs`（不得漂移到别的 crate，
    // 也不得在同一处出现第二份 —— 那会造成「两个真相源」）。
    //
    // 实现按 ADR-0009 决策 7：不用 grep crate，改用 `std::fs::read_dir` 递归 + 字符串匹配。
    // **已知局限**（同 ADR）：会误报字符串字面量 / 注释里的同名文本，故断言消息打印
    // **文件:行号:内容** 供人工区分（保守误报可接受，与守卫脚本立场一致）。
    let root = workspace_root();
    let abi_rs = "crates/engine-v8/src/abi.rs";

    for needle in [
        "pub struct RealmId",
        "pub struct JsHandle",
        "pub enum HostValue",
    ] {
        let hits = source_hits(&root, needle);
        assert_eq!(
            hits.len(),
            1,
            "`{needle}` 应恰好命中 1 次，实际命中的 文件:行号:内容 = {hits:?}"
        );
        let (file, lineno, content) = &hits[0];
        assert_eq!(
            file.as_str(),
            abi_rs,
            "`{needle}` 的定义点应位于 {abi_rs}，实际在 {file}（{lineno}: {content}）"
        );
    }
}
