# ADR-0015：K-11 修复 —— 动态 import 宿主回调的**空返回纪律**

- 状态：**已接受**（根因由源码 + V8 断言确证；修复路径唯一；主理人复核 + 全门禁已验证）
- 日期：2026-10-13
- 关联：
  - **G1 遗留 K-11**（`docs/HANDOFF-2026-10-12.md` §四；`docs/design-g1-engine-2026-10-12.md` §3.2 / §7 U-7）
  - **ADR-0009**（V8 类型边界）、**ADR-0006**（模块宿主 / 脚本暂停）、**ADR-0013**（句柄纪律）
  - 门禁报告 `docs/gate-report-g1-2026-10-12.md` §9 ①（本条为其修正）
  - 施工图 `docs/design-g2-page-2026-10-13.md`（G2 起适用的纪律条款：§8 Shared Knowledge）
- 落地：`crates/engine-v8/src/realm.rs`（`dynamic_import_callback` / `reject_import`；`Realm::new` 安装）
- 决策者：主理人（经工程师专项 + 源码/断言确证 + 三测复跑 + 抽查代码/SAFETY 后确认）

---

## 问题

G1 期间，「动态 `import()`」在 `engine-v8` 上触发 **V8 引擎级 FATAL**（非 Rust panic）：

```
V8_Fatal → Isolate::UnwindAndFindHandler()
         → Builtins_CEntry_Return1_ArgvInRegister_NoBuiltinExit
         → … → Execution::CallScript → v8::Script::Run → Realm::eval_impl → Realm::eval
# Check failed: has_exception().   （进程 SIGTRAP）
```

当时为保持仓库树干净，**整体回退**并把 `dynamic_import_specifier` 标记为「已知失败项 / 待专项」
（G1 遗留 K-11）。本 ADR 记录：**根因、修复、以及由此确立的一条跨 G2+ 的宿主回调纪律。**

---

## 根因（源码 + V8 断言确证）

**V8 契约**：宿主回调（`HostImportModuleDynamicallyCallback`）返回**空 `MaybeLocal`**（Rust 侧 `None`）
**当且仅当存在可传播的挂起异常**；否则 `Isolate::UnwindAndFindHandler` 以
`Check failed: has_exception()` **FATAL**（引擎级致命错误，非 Rust panic，进程 `SIGTRAP`）。

**旧实现的缺陷**：宿主侧的失败（无模块宿主 / 解析失败 / 加载失败 / 建串失败）经 `?` / `.ok()?`
**直接空返回** ⇒ 在**没有挂起异常**时返回 `None` ⇒ 撞该断言 ⇒ FATAL。

**最小复现（临时 `#[ignore]` 探针，验证后已删）**：在 `import('./m.js')` 上安装**无条件返回 `None`**
的 `set_host_import_module_dynamically_callback`，`cargo test -p engine-v8 <probe> -- --ignored --nocapture`
⇒ **精确复现**上述 FATAL 栈。**对照**：同一探针对成功路径（宿主命中 `mem://m.js`）**不崩**，promise
resolve 出 namespace（`ns.v == 7`）。

> **结论**：这不是「v8 crate 限制」，也不是「V8 需要 thread-local 桥 + 嵌套 scope」（此前的猜测）。
> 官方 `HostImportModuleDynamicallyCallback` adapter 已自带 `CallbackScope`（`v8-152.2.0/src/isolate.rs:299`）。
> **根因是宿主回调违反 V8 的空返回契约。**

---

## 决策

### 决策 1 —— 修复：宿主侧失败**一律 reject**，绝不空返回

`crates/engine-v8/src/realm.rs` 新增：

- `dynamic_import_callback`：**先创建 `PromiseResolver`**（此后每个失败路径都能「拒绝而非空返回」）；
  宿主侧失败（无宿主 / 解析失败 / 加载失败 / 建串失败）一律经 `reject_import` **拒绝**该 promise
  （`TypeError`）；**只有 V8 自身已挂起异常**（模块编译 / 实例化 / 求值失败，`has_exception() == true`）
  才 `?` / `return None` 令其**合法**传播。
- `reject_import`：以 `v8::Exception::type_error` **拒绝** promise 的统一出口，返回 `Some(promise)`；
  仅在**连消息字符串都建不出**（= 进程级 OOM，V8 必已挂起异常）时返回 `None`。
- `Realm::new`：`isolate.set_host_import_module_dynamically_callback(dynamic_import_callback)` 安装
  （经 `REALM_DATA_SLOT` 取回 Realm，与静态解析回调 `module_resolver` 同构）。

关键代码（`realm.rs` 原文摘要）：

```rust
fn dynamic_import_callback<'s>(scope: &mut v8::PinScope<'s, '_>, ..) -> Option<v8::Local<'s, v8::Promise>> {
    // 先建 resolver：此后每个失败路径都能「拒绝而非空返回」。
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        // 连 resolver 都建不出 ⇒ 只可能是进程级 OOM，V8 必已挂起异常 ⇒ 空返回是合法传播。
        return None;
    };
    let promise = resolver.get_promise(scope);
    // …… 宿主侧失败一律 return reject_import(scope, &resolver, promise, "…");
    // …… 编译/实例化/求值失败 ⇒ `?` / return None（此时 has_exception()==true，合法）。
    Some(promise)
}

fn reject_import<'s>(scope: &mut v8::PinScope<'s, '_>, resolver: &v8::PromiseResolver,
                     promise: v8::Local<'s, v8::Promise>, message: &str) -> Option<v8::Local<'s, v8::Promise>> {
    let Some(text) = v8::String::new(scope, message) else { return None; };  // OOM ⇒ 合法空返回
    let exception = v8::Exception::type_error(scope, text);
    let _ = resolver.reject(scope, exception);
    Some(promise)
}
```

### 决策 2 —— **纪律条款（跨 G2+ 强制）**

> **宿主回调返回空 `MaybeLocal` 仅限「存在挂起异常」时。**
> **任何宿主侧失败都必须以「拒绝 promise / 抛出异常 / 返回有效值」显式表达，禁止空返回。**

**适用范围（G2 起）**：所有宿主回调，**尤其**：
- 动态 `import()`（本条修复对象）；
- **`fetch` / 网络请求**的宿主回填（G2 §2.5：`network.complete` 路径）；
- **模块脚本**（`<script type=module>`）的加载/实例化回调；
- 任何未来经 V8 回调边界（`NativeFn` trampoline / module resolver / import callback）的宿主实现。

**理由**：空返回是 V8 的**致命契约**（撞断言 ⇒ 整进程 SIGTRAP），且**在 Rust 类型上不可见**
（`Option<Local>` 看起来与普通 `Option` 无异）。故把它写成**纪律条款**，而非仅一处修复。

### 决策 3 —— 为什么**未走**背对背辩论（如实记录理由）

本条**未**按 G1 的高风险路径惯例提交两位独立评审背对背辩论，理由（供审计）：

1. **根因由「源码 + V8 断言」确证，非设计分歧**：缺陷是「宿主实现违反 V8 明文契约」，
   不是「多个合理架构方案之间的取舍」；不存在需要辩论的**架构分叉**。
2. **修复路径唯一**：V8 契约只给出一条正确路径（失败即 reject、绝不空返回）——
   没有第二备选可与之一辩（空返回已被断言证伪）。
3. **主理人复核 + 全门禁已验证**：工程师专项完成后，主理人**三测复跑 3/3** + 抽查代码 / `SAFETY`；
   本地门禁（fmt / clippy `-D warnings` / unsafe guard / contracts 55 / deny）全绿；
   `cargo test --workspace` engine-v8 **46 passed / 0 failed / 0 ignored**。
4. **本条不改架构边界**：不新增 unsafe 面之外的类型、不改 C-1~C-4、不改白名单
   （`dynamic_import_callback` 的 `unsafe` 只读借用 `REALM_DATA_SLOT`，与既有 `module_resolver` 同构，
   逐块带 `SAFETY`）。

> **登记**：若后续出现与该纪律**冲突**的场景（例如某 V8 API **要求**空返回表达「异步未决」），
> 须**另起决策上报**——本 ADR 不覆盖该假设。

### 决策 4 —— 测试（回归护栏）

| 测试名（`realm.rs`） | 断言要点 |
|---|---|
| `dynamic_import_specifier` | `import('./m.js')` 经宿主解析器命中，promise resolve 为 namespace（`ns.v === 7`）——**K-11 验收转绿** |
| `dynamic_import_unresolvable_rejects_without_fatal` | **K-11 回归护栏**：宿主侧失败 ⇒ promise **被拒绝**（`String(e)` 含 specifier），**非同步抛出、更非 FATAL** |
| `dynamic_import_caches_namespace_identity` | 动态 import 与 `eval_module` **共用模块缓存**：同一 URL 同一 namespace（`===`） |

---

## 后果

### 正面

- **K-11 消解（非 crate 限制）**：动态 import 可用；`engine-v8` 43 → **46** 测试。
- 确立**跨 G2+ 的宿主回调纪律**（决策 2），把一处修复升级为**可复用约束**——
  G2 的 `fetch`/网络/模块脚本回调直接受用。
- 保留原始事故记录（现象帧、最小复现）作为**判定方法自证**：探针先复现 FATAL、再证成功路径不崩，
  证明「空返回 = 触发条件」这一因果非推断。

### 代价 / 注意

- 每个宿主回调必须**先建 resolver / 先备异常路径**，再进入可能失败的宿主逻辑（多一层结构）。
- 「空返回仅限挂起异常」是**运行期契约**，Rust 类型层面**不表达** ⇒ 靠本纪律 + 代码评审兜；
  新增宿主回调须逐个体检「有无空返回失败路径」。

### 遗留（本 ADR 不做）

- G2 的**模块脚本纳入与否**仍属 G2 **D2** 范围决策（本 ADR 只确立「技术上无已知阻塞」，
  不裁定 G2 是否纳入）。

---

## 机器可验证 / 证据

- **修复代码**：`crates/engine-v8/src/realm.rs`（`dynamic_import_callback` 约 `:761-868`；
  `reject_import` 约 `:870-887`；`Realm::new` 安装约 `:139`）。
- **测试**：`dynamic_import_specifier`（`:1489`）、`dynamic_import_unresolvable_rejects_without_fatal`（`:1506`）、
  `dynamic_import_caches_namespace_identity`（`:1526`）。
- **门禁**：`cargo test --workspace`（engine-v8 46 passed/0 failed/0 ignored）；fmt / clippy(`-D warnings`) /
  `check-unsafe-whitelist.sh` / contracts 55 / `cargo deny check` **全绿**（主理人复跑）。
- **文档同步**：G1 施工图 §3.2/§7 · HANDOFF §四 · gate-report-g1 §9① · G2 施工图 §8（纪律条款）。
