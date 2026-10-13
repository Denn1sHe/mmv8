# T2.4 探针 —— `blitz-dom` 动态 DOM / 样式失效 / NodeId 映射 / Shadow DOM

**一句话**：本探针实测 `blitz-dom 0.3.0-beta.2` 能否承担技术方案 §8 的「LayoutBackend 首选封装
Blitz 样式/布局子集」，并据此给出**路线裁决**：走 Blitz，还是切直接 Stylo + Taffy + Parley 适配。

## 它证伪什么 / 证实什么

- **证实**：`blitz-dom` 的动态 DOM、layout 级失效、style 级失效、NodeId 映射、后端树可重建
  五项**都成立**（读数随变更而变，且不是恒真门禁 —— 每条都有正负成对的自证）。
- **证伪**：**Shadow DOM 不成立**。不是「没测出来」，而是实测到上游
  `todo!("Shadow roots not implemented")`（`blitz-dom-0.3.0-beta.2/src/stylo.rs:231`）。

⇒ 裁决 `T2_4_VERDICT=SWITCH_TO_STYLO`，与设计文档 §2.7 的预期一致。

## 为什么本探针**不**与 html5ever 联调

架构师决策 Q-3：`markup5ever 0.12`（html5ever 0.27 线）与 `0.39`（blitz 线）**源码级不兼容**
（`Token` 枚举变体、`TreeSink` 签名、`TokenizerResult` 位置都变了）。
把「blitz-dom 能不能用」与「html5ever 0.39 好不好用」解耦，T2.4 只回答前者。
初始 DOM 一律用 `DocumentMutator::create_element` / `append_children` 直接构造，
`DocumentConfig::html_parser_provider` 保持 `None`。

## 运行

```bash
cd poc/t2_4_blitz_probe
cargo run --offline          # 产出全部二值结论 + 裁决
cargo test --offline         # C9 / C2 的可执行回归契约（3 个测试）
```

`cargo run` 末尾单行汇总标记：`T2_4_PROBE_OK` / `T2_4_PROBE_FAIL`。

> `T2_4_PROBE_OK` 指「探针全部断言跑完且判定方法有效」，**不代表被测能力通过**。
> 与 `T2_4_VERDICT` 是两件事 —— `PROBE_OK` + `VERDICT=SWITCH_TO_STYLO` 是合法且预期的组合
> （对齐 ADR-0007 的 `BASELINE_VALID` 范式）。

### `cargo test` 的三个测试

| 测试 | 作用 |
|---|---|
| `shadow_root_host_is_not_implemented` | `#[should_panic(expected = "Shadow roots not implemented")]`。把「C9 必为 false」钉成**可进 CI 的回归契约** —— 上游一旦实现，这个测试会**失败**，提醒维护者回来更新 C9 结论与 ADR-0011，而不是让过期结论悄悄留在文档里 |
| `catch_unwind_returns_ok_when_no_panic` | **正面对照**：证明 `catch_unwind` 对不 panic 的闭包返回 `Ok`。没有它，「捕获到 panic」就无法区分「上游真炸了」与「`catch_unwind` 恒返回 `Err`」—— 那是另一种恒假门禁 |
| `no_gui_crates_in_lockfile` | C2 的机器复核（与 `main()` 的 EXP0 同源同数据源） |

`catch_unwind` 单独在 `main()` 里用是不够的：它对 panic / 不 panic 都可能返回 `Ok` 之外的路径，
若上游把 `todo!()` 换成真实现，那条线索埋在 `cargo run` 的输出里而不在 `cargo test` 的信号里。
两者合起来才既「拿到 panic 原文」又「保证它必panic」。

## 依赖纪律

只依赖 `blitz-dom`（`default-features = false`）+ `blitz-traits` + `markup5ever`，
另加 `stylo` / `stylo_traits` / `selectors` 三个**只读**用途的直接依赖
（读计算样式、调用 `TShadowRoot` / `selectors::Element` 的 shadow 相关方法）。
后三者**已在 `blitz-dom` 依赖树里**，实测**新增传递包数 = 0**
（探针的 `Cargo.lock` 与设计文档 §2.2 的最小依赖集锁包数、包名集合完全一致）。

**不引入** `blitz` 顶层 crate / `blitz-shell` / `blitz-paint` / `blitz-html` / `anyrender`
（技术方案 §3「不整包依赖 GUI」）。EXP0 会读 `Cargo.lock` 逐条核对 ⇒ `NO_GUI_DEP`。

## unsafe

**本探针零 `unsafe`**（代码行；文档注释里提到 `unsafe` 一词不构成代码行）。
EXP4 用 `std::panic::catch_unwind` 捕获上游 `todo!()`，
不需要 crate 级 / mod 级 `allow(unsafe_code)` 开口。

`bash scripts/check-unsafe-whitelist.sh` 对本目录**零命中**。

## 给 ADR-0011 的两条硬事实

1. **C9 的 panic 消息原文是 `not yet implemented: Shadow roots not implemented`**
   （`not yet implemented: ` 前缀由 `todo!()` 宏生成，不是 `not implemented:`）。
   抄进 ADR 时请用原文，别手抄简化。
2. **切「直连 Stylo + Taffy + Parley」省掉的是 blitz 的 DOM 抽象层，不是样式与布局依赖。**
   已实测依赖树含 `stylo 0.20.0` / `stylo_taffy 0.3.0-beta.2` / `taffy 0.14.0` / `parley 0.11.1`
   ——切路线后**仍需引入**。不写清楚会让人以为切换能省掉整棵依赖树。

## 边界（不做）

- 不与 html5ever 联调（决策 Q-3）；`HTML5EVER_VERSION_FORK_IMPACT` 由架构师定案，探针不重跑。
- 不做像素级 Chromium 差分（技术方案 §8：样式解析成功 / 计算值正确 / 实际布局正确是三层，
  本探针只验「变更 → 失效 → 重算」链路成立）。
- 不做 font fallback / 字体栈验证（属 G3）。
- 不做真实渲染 / GPU / 窗口：只到「布局数值可读」为止。
- 远程 Linux 复跑不在本探针内（由主理人统一安排）。`TARGET_LINUX_OK` **由平台判定**
  （`OS == "linux" && ARCH == "x86_64"`，**不是手写常量**）：在非 x86_64 Linux 上为 `false`
  且状态 `TARGET_LINUX_STATUS=PENDING_REMOTE`（照实，不是「实测为否」）；在 x86_64 Linux
  上就地验证，状态 `VERIFIED_ON_LINUX`。裁决串中的 `C1` 取值同样读该判定，不含硬编码。