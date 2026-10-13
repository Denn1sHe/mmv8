# ADR-0009：`bindings-webidl` ↔ `engine-v8` 的依赖方向与 V8 类型边界（决策 G1-PRE-1）

- 状态：**已接受**（维护者 2026-10-12 授权 AI 内部决策；结论落档后即可执行，不阻塞等待）
- 日期：2026-10-12
- 关联：`AGENTS.md` **铁律 5**（`AGENTS.md:22`「V8 / rusty_v8 类型不得越出 `engine-v8` crate」）
  与**技术方案 §1 退出机制**（「把所有 V8 类型限制在 engine-v8 crate 内，业务层只看
  **RealmId、JsHandle、HostValue** 和调度接口」）；
  `docs/TASKS.md` 的 **G1-PRE-1**（TASKS.md:116，`todo`）、第二轮 review 的 **P1-7**（TASKS.md:102）；
  **ADR-0007**（T2.1 实测：cppgc ↔ V8 跨语言环无法回收）
- 决策者：AI 内部独立子 agent 讨论 + lead 汇总（依 2026-10-12 授权变更）
- 待决项来源：`docs/remaining-scope-2026-10-12.md` §1 的 Q1~Q5（许清楚）

---

## 问题

铁律 5 与「生成 Web IDL 绑定」在**字面上**互斥。

Web IDL 绑定的职责是「把 JS 值转成宿主类型、把宿主值转成 JS 值」。这个转换过程在语义上
**必然要构造、读取、调用 V8 值**（`Local<Value>`、`Function::new`、`Object::set`……）。
技术方案 §16 又把 `engine-v8 / bindings-webidl` 列为**同一组**，且 `bindings-webidl` 的
crate 描述是「由固定 Web IDL 快照生成的 Rust 绑定（生成物目录，禁止手改 `*-generated` 文件）」。

> 一份「不产出 V8 值的 Web IDL 绑定」在语义上不成立；
> 而一份「产出 V8 值的 Web IDL 绑定」在字面上就违反铁律 5。

这不是措辞问题，是**类型面（type surface）没有被定义**。

### 为什么必须先定 ADR 再写 G1 代码

G1 闸门（技术方案 §18）的必产出是「原生绑定、模块/WASM、微任务、GC 循环、控制入口隔离、
Python 回调」——**G1 正是 `bindings-webidl` 开始产出代码的那一刻**。若先写代码后定规则，
下表每一条都是可预见的失败：

| # | 若规则未先定会发生什么 | 违反哪条 |
|---|---|---|
| 1 | 生成器开始产出 `v8::Local<Value>` 出现在 `bindings-webidl` 的**公开签名**里 → 铁律 5 被**静默**违反，**没有任何门禁会拦住它**（已核实：`scripts/check-unsafe-whitelist.sh` 白名单只有 `crates/engine-v8/` 与 `poc/`；`crates/runner/tests/workspace_policy.rs` 现有三条断言**无一条检查类型越界**） | 铁律 5 |
| 2 | 为绕过限制把 `use v8::...` 塞进 `web-dom` / `navigation` / `scheduler` → 这些 crate 被迫加入 unsafe 白名单（它们现在**不在**白名单里） | 铁律 4 + 5 |
| 3 | `RealmId` / `JsHandle` / `HostValue` 被**复制**到多个 crate → `engine-v8/src/lib.rs:11` 声明的「唯一归属」失效，`NodeId ↔ JsHandle` 映射表出现两个真相源 | 技术方案 §1 的隐含前提 |
| 4 | 事后补救需改生成器 + **重生成全部 `*-generated` 文件** → 违反「golden 只能独立 PR 修改」（铁律 2）与「一事一 PR，diff ≤ 400 行」（铁律 3） | 铁律 2、3 |
| 5 | 没有机器门禁，只能靠人 review —— 正是第二轮 P0-2「守卫脚本无必跑路径」的同款形态 | P0-2 处置形态 |

**结论**：本 ADR 的产出不是一份好看的文档，而是 **G1 开工前必须存在的一组类型面规则
+ 一个机器可执行的反向验证**。

---

## 已核实的语言/工具事实（原文证据）

**事实 1 —— `v8` crate 在类型面上无法被「藏起来」。** `v8` 是外部 crate，
`crates/bindings-webidl` 若要调用它必须在自己的 `Cargo.toml` 写 `v8 = "152.2.0"`。
因此「`v8` 不得越出 `engine-v8`」**在依赖层面**就等于「只有 `engine-v8/Cargo.toml` 允许出现
`[dependencies] v8`」。这不是风格偏好，是 Cargo 的硬事实。

**事实 2 —— `*_generated.rs` 里的 `v8::` 是被 Cargo 解析的路径，不是「注释」。**
一旦生成文件里写 `use v8::Local;`，`bindings-webidl` 就必须能解析 `v8` 这个 crate，
即必须在 manifest 里有 `v8` 依赖。**想靠生成器「不 import v8」来绕过约束在物理上不可能。**

**事实 3 —— 工作区当前状态（已核实）。**

```
crates/bindings-webidl/Cargo.toml   → [dependencies] 为空（依赖方向尚未确立）
crates/engine-v8/src/lib.rs:11      → 「V8 / rusty_v8 类型**不得越出本 crate**」
scripts/check-unsafe-whitelist.sh:42→ WHITELIST=( "crates/engine-v8/"  "poc/" )
crates/runner/tests/workspace_policy.rs → 3 条断言，无一条查类型越界
```

---

## 备选方案（≥2，含关键接口签名原文 / 伪码）

### 方案 A：绑定代码**内联为 `engine-v8` 的子模块**（生成物落在白名单 crate 内）

**做法**：`bindings-webidl` 降级为**纯生成器 crate**（只含 `build.rs` / xtask，输入 Web IDL
快照，输出 Rust 文本），生成的 `*-generated.rs` 由 `engine-v8` 以
`include!` 或 `#[path]` 引入。`engine-v8` 因此是唯一持有 `v8` 依赖的 crate。

```rust
// crates/engine-v8/src/lib.rs
#[path = "../../bindings-webidl/generated/document-generated.rs"]
mod document_generated; // ← 生成物：v8 类型止步于此
```

**关键签名**（生成物内部，可见 `v8::`）：

```rust
// bindings-webidl/generated/document-generated.rs（内联进 engine-v8 后）
use v8::{Local, Context, Function, Object};
pub fn make_node<'s>(scope: &mut v8::PinScope<'s, '_>, ns: u32) -> Local<'s, Object> { … }
```

**优点**
- **铁律 5 零例外**：`v8` 只出现在 `engine-v8/Cargo.toml`，无例外可被援引。
- unsafe 白名单**完全不变**（生成物落在 `crates/engine-v8/` 内，逐块 `SAFETY:` 即可）。
- 依赖图**只有一条边**，无环风险。

**缺点**
- 违反技术方案 §16 的成员边界语义（`bindings-webidl` 不再是「生成物目录」，而是生成器）。
- 生成物进入 `crates/engine-v8/src/` 的路径下，**触发跨平台铁律第 1 条**（大小写）：
  mac 文件系统不区分大小写而 Linux 区分，`#[path]` 写错大小写会在远程静默失败。
- `engine-v8` 体积被生成物撑大，与 ADR-0002「白名单最小化」的取舍相反。

**长期影响**：G1 之后每次重新生成都会改动 `engine-v8` 的文件树，`engine-v8` 的 PR diff
不再等于「手写逻辑的 diff」，评审噪声上升。

---

### 方案 B：**新建类型面 crate `crates/v8-abi`**（`RealmId`/`JsHandle`/`HostValue` 的唯一归属）

**做法**：新建成员 crate `v8-abi`，**只依赖 `core`/std，不依赖 `v8`**。三个安全类型定义在此。
`engine-v8` 依赖 `v8-abi`（实现 `From<HostValue> for v8::Local<Value>` 等转换，方向
`engine-v8 → v8-abi`）。`bindings-webidl` 也依赖 `v8-abi`，并**同时**依赖 `v8`
（生成物必然要用），但**其公开签名只准出现 `v8-abi` 的类型**。

```rust
// crates/v8-abi/src/lib.rs  —— 零 v8 依赖，可独立编译与测试
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RealmId(u64);          // 不透明；高 32 位 = isolate 序号，低 32 位 = realm 序号
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct JsHandle(u64);         // 不透明槽位索引，**不是 GC 根**（见条款 6）
#[derive(Clone, Debug, PartialEq)]
pub enum HostValue {               // 有界枚举，不是 trait 对象
    Undefined, Null, Bool(bool), Number(f64), Str(String),
    Object { realm: RealmId, handle: JsHandle },
    Array { realm: RealmId, handle: JsHandle },
    Function { realm: RealmId, handle: JsHandle },   // 裸 function，非 JsHandle 的别名
}
```

```rust
// crates/engine-v8/src/v8_bridge.rs  —— 唯一允许写 v8:: 的转换层
impl HostValue {
    pub fn to_local<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Value>>;
}
```

**优点**
- 三个类型**物理上不可能**在 `engine-v8` 之外被误定义（它们在一个独立的、极小的、
  可被 `crates/runner/tests/workspace_policy.rs` 单独断言的 crate 里）。
- 「业务层只看 RealmId / JsHandle / HostValue」（技术方案 §1）得到**类型系统层面的保证**，
  而非纪律层面的保证。
- `v8-abi` 无 `v8` 依赖 ⇒ 门禁可以**极简且无假绿**：解析 Cargo.toml 判 `v8` 出现位置，
  不需要读任何 Rust 源码（ADR-0007/0008 的「不要手写解析器」教训得到遵守）。

**缺点**
- **必须新增 workspace 成员** → 触碰 `Cargo.toml` 的 19 成员名单，
  而该名单被 `workspace_policy.rs::workspace_members_equal_scaffold_roster` **精确锁定**
  （TASKS.md:48）。**这是本方案的主要成本**，需要同步改名单 + `runner::workspace_crates()`。
- 违反铁律 3（一事一 PR，diff ≤ 400 行）：建 crate + 改名单 + 改门禁至少 3 个 PR。

**长期影响**：多一条永久依赖边；换来的是可断言的类型面。**本 ADR 认为值得。**

---

### 方案 C：绑定侧**只做「HostValue ↔ JS」双向转换**，其余全在 `engine-v8`（最窄包装面）

**做法**：`bindings-webidl` 不生成任何绑定结构体，只生成**转换函数表**；
所有 V8 交互通过 `engine-v8` 暴露的一组**窄接口**。

```rust
// crates/engine-v8/src/api.rs  —— engine-v8 对外的**全部**面
impl Realm {
    pub fn eval(&mut self, src: &str) -> Result<HostValue, JsError>;
    pub fn call(&mut self, h: JsHandle, args: &[HostValue]) -> Result<HostValue, JsError>;
    pub fn set_prop(&mut self, h: JsHandle, key: &str, v: HostValue) -> Result<(), JsError>;
    pub fn get_prop(&mut self, h: JsHandle, key: &str) -> Result<HostValue, JsError>;
    pub fn new_function(&mut self, f: extern "C" fn(&mut Realm, &[HostValue]) -> HostValue) -> JsHandle;
}
```

**优点**：暴露面最收敛，完全对齐 ADR-0007 的「单向/弱引用」结论。
**缺点**：
- Web IDL 规范要求绑定能表达**每个接口的每个成员**。方案 C 的 5 个函数无法表达
  `Node.prototype.appendChild` 的存在性与 descriptor（`writable`/`enumerable`/`configurable`），
  而 ADR-0006 已实测「描述符差分」是验收层之一（技术方案 §6）。
- 「暴露面不退化成 V8 镜像」的收敛判据在本方案里会**逼出作弊**：
  为了让绑定能描述 descriptor，必然要加 `define_property`/`get_own_property_descriptor`
  等，包装层逐步长成 V8 的镜像——这正是许清楚 §1 风险表第一行指出的失败模式。

---

### 方案 D：**生成器用 trait 抹平 V8 类型**（`bindings-webidl` 只依赖一个 `JsEngine` trait）

**做法**：在 `v8-abi`（或 `bindings-webidl` 内）定义 trait，V8 实现放在 `engine-v8`。
`bindings-webidl` 只见 trait，不见 `v8`。

```rust
pub trait JsEngine {
    type Scope;
    fn eval(&mut self, src: &str) -> Result<HostValue, JsError>;
    fn define_method(&mut self, target: JsHandle, name: &str, f: HostFn);
}
```

**优点**：看起来最干净，`bindings-webidl` 的依赖表里确实没有 `v8`。
**缺点（否决理由）**：
1. `HostValue` 已经是**有界枚举**，trait 化后 `Object` 变体仍需 `JsHandle`，
   而 `JsHandle` 的**填充**必须在 `engine-v8`（要拿 `v8::Global::get_index`），
   于是每个「把 JS 值变成 `HostValue`」的点都必须过 trait 边界——**抽象收益为零**，
   因为 `HostValue` 本身已经是类型擦除后的安全形态。
2. **GAT（关联类型）穿透 trait** 会让生成代码难以生成：每个绑定函数都要多一个
   `Self::Scope` 生命周期参数，与 ADR-0007 记录的「v8 scope 类型多生命周期参数，
   抽成具名函数会 E0106」叠加后极易编译失败。
3. 违反铁律 3 的「禁止跨 crate 的顺手改动」——引入 trait 抽象会波及所有 19 个成员。

---

## 决策

**采用方案 B，并按方案 A 的方式把生成物落在 `engine-v8` 内。** 即 B + A 的组合，
但**生成器（`bindings-webidl`）与生成物（`engine-v8/src/generated/`）职责分离**：
`bindings-webidl` 产出文本，`engine-v8` 持有文本。

### 决策 1 —— 依赖方向（一句话，**A3**）

> **类型面（`RealmId` / `JsHandle` / `HostValue`）的 Rust 定义点落在 `crates/engine-v8/src/abi.rs`；
> 其跨语言契约（Python 侧可见的同名投影）落在 `contracts/iv8_contract/v8_abi.py`，
> 由 `contracts/tests/test_v8_abi.py` 断言两者**逐项对应**。
> `v8` crate 只允许出现在 `crates/engine-v8/Cargo.toml`。
> **不新增 workspace 成员，§16 的 19 成员名单保持冻结。**

依赖图（无环，且**不新增节点**）：

```
（既有）crates/engine-v8 ── 唯一 [dependencies] v8 = "152.2.0"
      │  src/abi.rs  ← RealmId / JsHandle / HostValue 的唯一 Rust 定义点
      ▼
（既有）crates/bindings-webidl ── use engine_v8::{RealmId, JsHandle, HostValue}
                                  （公开签名中不得出现 v8::）
（既有）crates/sdk-python ── use engine_v8::{RealmId, JsHandle, HostValue}
                                  （PyO3 转换层，见决策 2）

（既有）contracts/iv8_contract/v8_abi.py ← 同一三个类型的 Python 契约投影
                    （契约测试 test_v8_abi.py 断言它与 abi.rs 逐项对应）
```

**关键性质**：这两条门禁断言的是**真实的 Cargo workspace**，
**与 `abi.rs` 这个文件放在哪无关**，因此它们**立即可实施**（见决策 3）。

**反向依赖（`engine-v8 → bindings-webidl`）明确禁止**：生成器若被引擎依赖，
生成器的依赖变化会传导进 unsafe 白名单 crate，破坏 ADR-0002 的边界最小化。

### 决策 2 —— 三个类型的定义与表示（**A4**）

**两个定义点，职责不同，不是重复定义**：

| 面 | 位置 | 内容 | 保持同步的手段 |
|---|---|---|---|
| **Rust 定义（权威）** | `crates/engine-v8/src/abi.rs` | 三个类型的 `struct` / `enum` 定义与全部方法 | 唯一可被 Rust 代码 `use` 的定义点 |
| **Python 契约（投影）** | `contracts/iv8_contract/v8_abi.py` | 三个类型在 Python 侧的**值域与序列化契约**（不是重新实现语义） | `contracts/tests/test_v8_abi.py` 逐项断言与 Rust 定义对应 |

| 类型 | Rust 定义 | 底层表示 | `Send`/`Sync` | Python 投影 |
|---|---|---|---|---|
| `RealmId` | `crates/engine-v8/src/abi.rs` | `#[repr(transparent)] struct RealmId(u64)`，高 32 位 isolate 序号、低 32 位 realm 序号 | 自动（无 V8 类型） | frozen dataclass / NamedTuple，`int` |
| `JsHandle` | 同上 | `#[repr(transparent)] struct JsHandle(u64)`，为 `engine-v8` 内部 `slot table` 的下标（**见决策 5 C-1：不是 GC 根**） | 自动 | 同上 |
| `HostValue` | 同上 | 有界 `enum`（8 变体），**不是** `trait 对象`、**不是** `Vec<u8>` | 自动 | 枚举 + 显式转换函数 |

**Rust 侧「禁止 re-export」约束照留**：`engine-v8` 内部对这三个类型只用
`use crate::abi::{RealmId, JsHandle, HostValue};`，**不得** `pub use` 出去
（`pub use` 会制造第二定义点的歧义来源）。`abi.rs` 里的 `mod abi;` 本身
可为 `pub(crate)` 或 `pub`，但**类型的定义只有一处**。

> 术语（给不懂 Rust 的维护者）：`pub use` ≈ 「把内部的东西再 publicly 转发一次」。
> 本 ADR 禁止它，是为了保证「一个类型只有一个家」这件事在代码上是字面成立的，
> 而不是一个需要人记住的约定。

**`RealmId` vs `ContextId`**（许清楚 §1 待确认 2 的回答）：
`RealmId` **不等于** `ContextId`。技术方案 §5 说「每个异步请求、句柄、Worker 和日志条目携带
ContextId、RealmId、navigation_epoch」——三者是**并列**字段，说明 Realm 比 Context 细。
本 ADR 定义：`ContextId` = 「一个 JS 执行上下文」；`RealmId` = 「一个 V8 Context 及其宿主身份」。
**一个 Context 恰好一个 RealmId**（本仓库不做同 Context 多 Realm，故不区分这两个维度）。
`navigation_epoch` 属 `navigation` crate，**不进 `abi.rs`**。

**`HostValue` 跨 PyO3 的转换点**（许清楚 §1 待确认 1 的回答）：
转换**发生在 `sdk-python`**，**不**在 `engine-v8`、**不**在 `contracts/`。
理由：`engine-v8` 是 unsafe 白名单，把 Python 转换放进去会让每个转换点都在白名单 crate 里
（扩大 unsafe 面）；而 `contracts/` 是**契约层**，它只描述类型面、不实现转换。
因此 `sdk-python` 依赖 `engine-v8` 的 `abi.rs` 类型（`use engine_v8::{RealmId, JsHandle, HostValue}`），
**不**依赖 `bindings-webidl`。

### 决策 3 —— `v8` crate 的唯一允许位置与机器检查（**A5**）

**规则**：`v8 = "152.2.0"` 只允许出现在 **`crates/engine-v8/Cargo.toml`**。

**机器检查落点：两个都做，分工不同。**（铁律 7：`ci.yml` 不得由 AI 修改，故只挂在既有必跑路径上）

| 落点 | 断言名 | 检查什么 | 为什么放这里 |
|---|---|---|---|
| **`crates/runner/tests/workspace_policy.rs`**（新增第 4 条断言） | `v8_dependency_only_in_engine_v8` | 遍历 `runner::workspace_crates()` 的每个 `Cargo.toml`，用**已存在的 `toml` 官方解析器**读 `[dependencies]`/`[build-dependencies]`/`[dev-dependencies]`/`[target.*.dependencies]`，断言 `v8` 与 `rusty_v8` 两个 key **只在 `engine-v8` 出现** | 依赖 manifest 是 **TOML 结构**，用官方解析器 ⇒ **无假绿风险**。且该文件已有三条同类断言的先例 |
| **`scripts/check-unsafe-whitelist.sh`**（新增规则 5） | `VIOLATION[v8-type-escape]` | 扫描**非** `crates/engine-v8/` 与**非** `poc/` 的 `.rs`，命中 `use v8::` / `v8::Local` / `::v8::` / `extern crate v8` 即违规 | 抓「manifest 干净但源码里手写路径」的绕过形态 |

**挂哪条必跑路径**：ADR-0005 决策 4 已确定三条（`ci.yml` lint job、`AGENTS.md` 自检清单、
本地开发流程）。规则 5 挂在 **`AGENTS.md:104` 的自检命令行之后**——即工程师每次本地提交前
必然执行，无需新增 CI 配置。**注**：`AGENTS.md` 本身 AI 不得修改（铁律 7）；
若需把这条命令写进自检清单，须由**维护者**执行。本 ADR 只声明脚本侧的规则 5 存在即可生效
（脚本已被三条路径调用）。**Poc 例外**：`poc/**` 已在白名单内，探针允许直接用 `v8::`
（ADR-0008 已确立），但**新增探针不得让 `v8` 出现在 `crates/` 任何其他成员**。

**注入场景（会被抓住）**：

```rust
// crates/bindings-webidl/src/lib.rs
use v8::Local;              // ← 注入
pub fn leak<'s>(l: Local<'s, v8::Value>) {}
```

预期输出（两条断言同时变红）：

```
VIOLATION[v8-type-escape] crates/bindings-webidl/src/lib.rs:1: use v8::Local;
test v8_dependency_only_in_engine_v8 ... FAILED
  `v8` 依赖出现在非白名单 crate：["bindings-webidl"]
```

**反向验证（放松规则后断言必须变红 —— 证明断言不是恒真）**：
把 `workspace_policy.rs` 里的断言改成「**跳过** `engine-v8`」再跑，
则同一份注入代码**不再被报错**，`test result: ok` —— 说明这条断言的判别力确实来自
「白名单精确列举」而不是「碰巧没匹配到」。反向验证必须把两条注入的**期望**都记录：

```
### 反向验证 R1：把断言的 crate 过滤条件改为跳过 engine-v8
test v8_dependency_only_in_engine_v8 ... ok     ← 放松后变绿 ⇒ 断言非恒真（符合预期）
### 恢复后复核
test v8_dependency_only_in_engine_v8 ... FAILED（注入仍在）
```

**已知局限（显式写出，不给假绿门禁 —— 对齐 ADR-0008 决策 3 的纪律）**：

1. **经类型别名的间接引用检不出。** 若 `engine-v8` 写
   `pub type MyVal = v8::Local<'static, v8::Value>;`（**我们禁止 re-export，故此形态不应出现**），
   其他 crate 写 `engine_v8::MyVal` —— 守卫脚本与 manifest 断言**都不会报错**。
   缓解：`engine-v8` 侧禁止 `pub use v8_abi::*` 之外的 re-export，由**代码评审**兜住，
   **机器门禁在此处是「辅助」而非「完备」，本 ADR 不宣称它是完备的**。
2. **`extern crate v8 as something` 后再引用**检不出（守卫只匹配 `v8::` 路径首段）。
3. **依赖 / proc-macro 引入的 v8 类型**不在扫描范围（与 ADR-0002 遗留同款盲区，
   由 `cargo geiger` 趋势留档观测）。
4. **只覆盖主 workspace 的 19（或 20）个成员**，不覆盖 `poc/`（`poc/` 是有意例外）。

### 决策 4 —— 暴露面的收敛判据（防「包装层退化成 V8 镜像」）

许清楚 §1 风险表第一行指出：包装 API 面可能退化成 V8 镜像。本 ADR 定三条**可判定的**收敛判据：

1. **零 V8 环境可编译这条判据被方案 E 放弃，替换为等效的机器判据。**
   原判据「`cargo build -p v8-abi` 在没有 V8 静态库的机器上也应成功」依赖独立 crate，
   方案 E 下不成立。**替代判据**：`crates/engine-v8/src/abi.rs` 中
   **不得出现 `v8::` 路径**（由守卫脚本规则 5 从外部强制），
   且契约测试断言 `v8_abi.py` 的 import 列表不含 `v8`。
   **如实记录代价**：这条替代判据比原判据弱——它是「文本 + 门禁」级保证，
   而非「编译期不可能」。弱在哪：若有人用类型别名间接引入 V8 类型（如
   `use crate::v8_bridge::Val;` 而 `Val = v8::Local<...>`），门禁 5 抓不到。
   **该盲区与 ADR-0008 决策 3 的守卫脚本盲区同款，不新增风险等级。**
2. `bindings-webidl` 的**公开签名**（`pub fn` / `pub struct` / `pub enum` 的参数与返回值）
   中**不得**出现 `v8::` 开头的类型；生成物**只允许** `use engine_v8::{RealmId, JsHandle, HostValue}`。
   门禁：规则 5 抓源码路径；评审核对公开签名。
3. `HostValue` 的变体**上限 8 个**且**只增不改语义**。若某天需要 `BigInt`/`Symbol`/`ArrayBuffer`
   传输，必须走独立 ADR —— 这条限制**故意制造摩擦**，让「随手加个 V8 类型进枚举」变成
   需要论证的动作。**由契约测试强制**：`enum HostValue` 的变体名集合必须与
   `v8_abi.py` 的枚举成员集合**相等**（两侧任一改动而另一侧未改 ⇒ 立刻红）。

### 决策 5 —— 与 ADR-0007 的相容性条款（**A6**，硬约束）

ADR-0007 实测原文：

```
=== EXP3 真循环：DOM → Global(闭包) → 闭包捕获 DOM ===
  WeakRef 判定：exp3 存活 = true（**true = 被跨语言环保活，即泄漏**）
CYCLE_LEAK=true
```

根因原文：「cppgc 堆与 V8 堆是**两个独立堆**，V8 GC 只认识自己的根，而 `v8::Global`
正是这样一个根。」

**因此本 ADR 定四条强约束**：

| # | 约束 | 可判定 |
|---|---|---|
| **C-1** | **`JsHandle` 不是 GC 根。** 它的实现只能是 `engine-v8` 内部 `slot table` 的下标（一个 `Vec` 的下标），**不得**是 `v8::Global` 的包装 | 代码评审 + 决策 2 的表示；`engine-v8` 的 `JsHandle` impl 中禁止出现 `Global::new` |
| **C-2** | **`web-dom` / `web-events` 的类型面上不得出现 `JsHandle`。** 事件监听器槽只允许 `NodeId`（整数）或 `engine-v8` 提供的**弱**句柄类型（`WeakJsHandle`，另行 ADR） | 门禁规则 5 + 评审；ADR-0007 已明确推荐「闭包持有宿主侧 `NodeId`，回调时反查 DOM」 |
| **C-3** | `NodeId ↔ JsHandle` 映射表归 **`engine-v8`**（`engine-v8/src/node_map.rs`），**不是** `web-dom`、**不是** `bindings-webidl` | 合并回答许清楚 §1 待确认 3 与 §3 待确认 4（两个任务问的是同一问题）；依据：ADR-0007 说「回调时**反查** DOM」，反查入口必须与 `JsHandle` 的定义同 crate，否则每次反查都要跨 crate |
| **C-4** | **强引用环禁令**：任何 crate 都不得构造 `DOM → (JsHandle 强引用) → 闭包 → DOM` 的环。若确需双向强引用，必须实现跨堆追踪（ephemeron），而 ADR-0007 已实测 **`v8` crate 未暴露 `TracedReference`**（对 `src/` 全量 grep 无匹配），只能在 `engine-v8` 内自行 unsafe 桥接 —— 属铁律 6 高风险路径，须**另起 ADR + 另起 PoC**，本轮不做 | ADR-0007 已落档的结论，本 ADR 只是把它升格为**生成绑定层面的强制条款**（生成器不得产出持有 `JsHandle` 的 DOM 侧结构） |

**为什么必须在 ADR 里显式写**：G1 一旦用 `JsHandle` 把 DOM 和闭包连起来，
就把 T2.1 已证明会泄漏的结构又造了一遍，而泄漏要到 G2/G3 才暴露，返工成本极高。

### 决策 6 —— 生成代码（`*-generated`）的 unsafe 归属（**A7**）

| 问题 | 结论 |
|---|---|
| 生成代码允许含 `unsafe` 吗？ | **允许，但生成物必须落在 `crates/engine-v8/` 内**（方案 A 的落点），因此**已自动在白名单里** |
| unsafe 白名单是否要扩？ | **不扩。** 白名单保持 `("crates/engine-v8/", "poc/")` 不变（ADR-0008 决策 2） |
| 每个 `unsafe` 须带什么？ | 紧邻 `// SAFETY:` 标记（**必须在 unsafe 行前 2 行内** —— ADR-0008 决策 4 实测的守卫窗口） |
| 生成器能否产出无 `SAFETY:` 的 unsafe？ | **能生成，但守卫会拦下。** 生成器模板的**每一个** `unsafe` 块必须由模板硬编码一行 `SAFETY:`。ADR-0008 决策 4 的原文理由适用：「`SAFETY:` 应紧邻 unsafe 行；长篇论证可放在其上，但必须在紧邻处保留一行标记」 |
| 生成器自身的 crate 呢？ | `bindings-webidl` 作为生成器**零 unsafe**（它只做文本生成）。若将来它需要 `build.rs` 调 `webidl` crate 的解析器，而那个 crate 内部含 unsafe（FFI 到 C 版 webidl-bindgen），则**该依赖会被迫进白名单** —— 本 ADR 的对策是：**生成器的依赖必须全部是纯 Rust 或已被 ADR-0008 白名单覆盖**；若做不到，生成器改为「离线生成 + 提交生成物」模式（`xtask` 在有白名单的 crate 内运行），**不在 `bindings-webidl` 里放 `build.rs`** |

**结论一句话**：`*-generated` 的 unsafe **归属 `crates/engine-v8/`**，**白名单不扩**，
逐块 `SAFETY:`，生成器零 unsafe 且不引入新白名单依赖。

### 决策 7 —— 三个类型的定义点如何被机器断言

`crates/runner/tests/workspace_policy.rs` 新增断言
`three_safety_types_have_single_definition_point`：

- 全仓扫描 `crates/*/src/**/*.rs`，断言 `pub struct RealmId` / `pub struct JsHandle` /
  `pub enum HostValue` 各**恰好命中 1 次**，且命中文件位于 `crates/engine-v8/src/abi.rs`。
- 实现方式：**`grep` crate 不在 workspace 依赖里**，故用 `std::fs::read_dir` 递归 + 字符串匹配。
  **已知局限**：会误报字符串字面量或注释里的同名文本 —— 故断言消息必须打印命中**文件:行号:内容**，
  便于人工区分；这与 ADR-0008 守卫脚本「保守误报，可接受」的立场一致。

---

## 机器可验证性（**A8**）

| 保障 | 实现 | 自证方式 |
|---|---|---|
| `v8` 依赖只在 `engine-v8` | `workspace_policy.rs::v8_dependency_only_in_engine_v8` | **注入 + 反向验证 R1**（见决策 3） |
| V8 类型不越出白名单目录 | `check-unsafe-whitelist.sh` **规则 5** | **注入场景 INJ-1**：`crates/bindings-webidl/src/lib.rs` 写 `use v8::Local;` → 期望 `VIOLATION[v8-type-escape]`；**INJ-2 反向验证**：临时把 `WHITELIST` 加上 `crates/bindings-webidl/` → 同一注入**不再报错**，证明规则确实依赖白名单而非恒真 |
| 三类型单一定义点 | `workspace_policy.rs::three_safety_types_have_single_definition_point` | **INJ-3**：在 `crates/web-dom/src/lib.rs` 复制一份 `pub struct JsHandle(u64);` → 期望命中数 2 → FAILED |
| 类型面无 V8 泄漏 | `crates/engine-v8/src/abi.rs` 中不得出现 `v8::` 路径（**方案 E 下的替代判据**，见决策 4 判据 1）；契约测试断言 `v8_abi.py` 的 import 列表不含 `v8` | 守卫脚本规则 5 从外部强制 + 契约测试（**实施属 G1**） |
| 生成物 unsafe 受约束 | 现有 `check-unsafe-whitelist.sh` 规则 3（`SAFETY:` 窗口） | 复用 T0.5 已有的 10 个注入场景，无需新增 |

**四条断言的双向验证方法已按 ADR-0002 的「10 个注入场景」同一纪律设计完毕；
验证的实施属 G1 的 PR（见 A9）。** 本表描述的是「门禁长什么样」，不是「门禁已存在」。

### 门禁的落地时点与三要素（**A9**）

**本 ADR 不要求本轮就把门禁实现完毕** —— 门禁代码属 **G1 的 PR**。本轮只锁定
「门禁长什么样」，使 G1 开工时**不需要再做架构决策**。三要素如下（这是 A9 的全部要求，
「只写以后加门禁」不算通过）：

| 要素 | 内容 |
|---|---|
| **落在哪个文件** | ① `crates/runner/tests/workspace_policy.rs`（新增 2 条断言，与现有 3 条同类同风格）<br/>② `scripts/check-unsafe-whitelist.sh`（新增**规则 5**，与现有 4 条规则并列） |
| **断言叫什么** | ① `v8_dependency_only_in_engine_v8`<br/>② `three_safety_types_have_single_definition_point`<br/>③ 守卫脚本的违规标签：`VIOLATION[v8-type-escape]` |
| **反向验证怎么做** | **R1（放松白名单）**：把断言的 crate 遍历条件改成「跳过 `engine-v8`」，重跑 ⇒ 同一份注入**不再报错**、`test result: ok` ⇒ 证明判别力来自「白名单精确列举」而非恒真。<br/>**INJ-2（放松守卫脚本）**：把 `WHITELIST` 加上 `crates/bindings-webidl/` ⇒ 同一注入不再报 `VIOLATION[v8-type-escape]`。<br/>**恢复后复核**：条件复原，注入仍在 ⇒ 断言与规则**同时**变红。<br/>**必须记录**：放松期与恢复期**两次的输出原文**，不得只写结论。 |

**G1 的 PR 拆分**（铁律 3 一事一 PR、diff ≤ 400 行）：

| PR | 内容 | 依赖 |
|---|---|---|
| PR-1 | `crates/engine-v8/src/abi.rs`：三个类型的 Rust 定义 + `mod abi;` 接线（**不动 workspace 名单**） | 无 |
| PR-2 | `contracts/iv8_contract/v8_abi.py` + `contracts/tests/test_v8_abi.py`：Python 契约投影 + 逐项对应断言 | PR-1（须有 Rust 定义才能断言「对应」） |
| PR-3a | `check-unsafe-whitelist.sh` **规则 5** + INJ-1/INJ-2 双向验证 | **无依赖，可立即落地** |
| PR-3b | `v8_dependency_only_in_engine_v8` 断言 + R1 反向验证 | **无依赖，可立即落地** |

> ⚠️ **依赖关系更正（架构师自查发现，原表有误）**：初版写「三条 PR 的依赖是串行的」，
> 这是错的。经逐条核对两条断言的实现依据，真实关系是**部分并行**：
>
> - `check-unsafe-whitelist.sh` **规则 5**（扫 `use v8::` 源码路径）**完全不依赖类型定义放在哪**，
>   与 workspace 名单无关，当前即可独立落地；
> - `v8_dependency_only_in_engine_v8`（遍历 `runner::workspace_crates()` 读各 crate 的
>   `Cargo.toml`）断言的是「`v8` 只在 `engine-v8`」——该断言**当前即成立**
>   （`bindings-webidl/Cargo.toml` 的 `[dependencies]` 现在是空的，已核实），
>   因此**不需要任何新 crate 或新文件**；
> - 只有 PR-2（契约对应测试）需要 PR-1 先落地。
>
> ⇒ **PR-3a / PR-3b 不依赖任何前置**，可在维护者对 §16 名单尚未表态时**先行落地**。
> 这正是方案 E 相比原「新增 `crates/v8-abi`」方案的最大工程收益：
> **铁律 5 的机器执行面不必等任何审批。**

**PR-3a / 3b 先行落地的价值**：即使后续对任何边界问题的讨论反复，铁律 5 的执行面已建立。

### 原「新增 workspace 成员 `crates/v8-abi`」方案已被本 ADR 取代

初版曾把三类型放进一个新的 workspace 成员 `crates/v8-abi`。**该方案已废弃**，
理由见「后果」章节的代价对比；§16 的 19 成员名单**保持冻结**，
`workspace_policy.rs::workspace_members_equal_scaffold_roster`（T0.10 验收产物、
已做过反向注入验证）**完全不受影响**。

### 契约测试如何做到「逐项对应」而不流于形式（关键设计）

`contracts/tests/test_v8_abi.py` **不得**手写一份「期望的 Rust 源码」再与之比对——
那会变成一份需要人工同步的副本，必然腐化。**做法**（利用既有基建，已核实可用）：

| 断言 | 手段 | 为什么不是假绿 |
|---|---|---|
| 三个类型名在 Rust 侧各恰好定义一次 | 读 `crates/engine-v8/src/abi.rs` 文本，正则计数 `pub struct RealmId` / `pub struct JsHandle` / `pub enum HostValue` | 计数为 1 才有意义；定义点漂移会立刻变 0 或 2 |
| `HostValue` 变体数 ≤ 8 且只增不改语义 | 从 `abi.rs` 解析 `enum HostValue` 的变体名，与 `v8_abi.py` 的枚举成员**集合相等** | 两侧任一侧改动而另一侧未改 ⇒ 集合不等 ⇒ 红 |
| Python 侧类型面不含 `v8` | 断言 `v8_abi.py` 的 import 列表不含 `v8`；断言全仓 `.rs` 中 `v8::` 仅出现在 `crates/engine-v8/` 与 `poc/` | 与守卫脚本规则 5 同源，不新增盲区 |
| 序列化契约稳定 | `repr(transparent)` 的两个 `u64` 类型在 Python 侧必须是 `int` 且宽度固定 64 | 位宽改变会破坏与已有 Profile 的兼容 |

**已核实的基建前提**：`contracts/` 已是既有目录，`pyproject.toml` 已配好
`pythonpath = ["."]` 与 `testpaths = ["tests"]`，架构师实跑 `uv run pytest`
得 **`48 passed in 0.46s`**（2026-10-12）。新增一个测试文件即可接入既有门禁，
**不需要新依赖**（现有依赖仅 `jsonschema` 与 `pyyaml`，本方案两者都用不到）。

### §16 名单变更的决策归属（架构师立场）

初版把「新增第 20 个成员」列为「AI 不代填」，而同一份 ADR 又在方案 B 里论证它「值得」——
**两处口径不一致，这是初版的缺陷**。现明确如下。

**结论：③ 属于「AI 内部决策」的范围，但架构师同时给出零成员变更的等价方案，
使该决策不必阻塞任何工作。**

理由（为什么可内部决策）：

1. 维护者 2026-10-12 授权原文「拿不准的你们 ai 内部开多个子 agent 讨论根据讨论结果来决策就行」
   ——依 `TASKS.md:20-26` 的授权变更，决策上报协议的**评审者已由外部 AI 改为内部独立子 agent，
   且结论不再阻塞等待，落 ADR 后即可执行**。
2. 本变更**收紧**而非放松约束：新增一个零依赖薄 crate，唯一目的是让铁律 5 从「纪律」变成
   「机器可断言」。「不许加一层保护」不是需要维护者授权的方向。
3. `AGENTS.md` 对成员边界变更给出的**程序**是「须先落 ADR」，本 ADR 即该程序；
   授权变更后，落 ADR + 内部论证即满足该程序。

**但架构师同意主理人提���的「代价不对称」判断**，因此采取更稳的处置：

> **不把铁律 5 的落地押在名单变更上。** PR-3a / 3b 立即落地（零成员变更），
> PR-1 / PR-2 等维护者一句话。名单变更若被否决，**只需把三个类型移进 `engine-v8/src/lib.rs`**
> （见下方方案 E），两条已落地的门禁**一条都不用改**，因为它们断言的是
> 「`v8` 依赖只在 `engine-v8`」与「三类型只有一个定义点」，**都与定义点所在的 crate 无关**。

### 方案 E（**已采纳 · 现行方案**）—— Rust 定义留 `engine-v8`，契约校验落 `contracts/`

**采纳日期**：2026-10-12（主理人采纳，取代原「方案 B = 新增 `crates/v8-abi`」）

**做法**：三个类型的 **Rust 定义**放在 `crates/engine-v8/src/abi.rs`；
它们的**跨语言契约**（Python 侧可见的值域与序列化规则）放在
`contracts/iv8_contract/v8_abi.py`，由 `contracts/tests/test_v8_abi.py`
断言两侧**逐项对应**。**不新增任何 workspace 成员。**

> **与主理人转述的措辞一处修正**（重要，否则 G1 会踩）：
> 「让 `v8-abi` 成为对 `contracts/` 的 `path` 依赖」在**方向上不成立**——
> `contracts/` 是 Python 包，Rust 不能把它当 crate 依赖；反之 `contracts/`
> 也不该依赖 Rust。**正确的方向是**：
> - Rust 定义 **在** `crates/engine-v8/src/abi.rs`（Rust 侧唯一权威定义点）；
> - Python 契约 **在** `contracts/iv8_contract/v8_abi.py`（**描述**同一套值域，非重新实现）；
> - 两者的一致性由 **pytest 读取 `abi.rs` 文本**来断言（方向：Python → 读 Rust 文件，
>   而非 Rust → 依赖 Python）。
>
> 这样既满足「零成员变更」，又不产生任何新的 crate 依赖边。

**为什么这个落法满足铁律 5（关键论证）**：铁律 5 要保证的是
「V8 类型不越出 `engine-v8`」这一**属性**，而不是「类型必须住在一个独立的 crate 里」。
方案 E 下该属性由**三道互补的机制**保证，且**一道都不需要新增 workspace 成员**：

| # | 机制 | 保证什么 | 落点 |
|---|---|---|---|
| 1 | `v8` 依赖只允许在 `engine-v8` 的 manifest | 类型面**拿不到** `v8` crate | `workspace_policy.rs` 第 4 条断言（PR-3b） |
| 2 | 非白名单目录的 `v8::` 路径引用被拦 | 源码层**写不出**越界引用 | `check-unsafe-whitelist.sh` 规则 5（PR-3a） |
| 3 | 类型定义点唯一 + 契约两侧一致 | 不会出现两个「真相源」 | `abi.rs` 单一定义 + `test_v8_abi.py`（PR-1/PR-2） |

| 维度 | 原方案 B（新增 `crates/v8-abi`） | **方案 E（已采纳）** |
|---|---|---|
| 铁律 5 可机器执行 | ✅ | ✅ **同样成立**（三道机制如上表） |
| §16 的 19 成员名单 | ❌ 需改，且 `workspace_policy.rs` 名单测试受影响 | ✅ **完全不动** |
| 新增依赖 | 新增成员 + 其传递依赖 | ✅ **零** |
| 门禁 PR 能否立即落地 | ❌ 需先等 crate 落地 | ✅ **PR-3a/3b 无任何前置** |
| 类型「编译期不可能」泄漏 | ✅ 编译期保证 | ⚠️ **放弃**，降级为「门禁 5 + 契约测试」保证 |
| 跨语言契约的显式载体 | 需另建机制 | ✅ `v8_abi.py` 天然承载，且复用既有 pytest 基建 |
| `cargo deny` 许可面 | 需复核新依赖 | ✅ **不变** |
| 复用既有基建 | 少 | ✅ `contracts/` 已有 `pyproject.toml` + 48 测试全绿（架构师实跑核实） |

**采纳日期的额外收益**：ADR-0008 决策 1 已确立「探针/附属物不拖进主门禁」的形态纪律；
方案 E 复用同一条纪律——**契约层校验属于 `contracts/`，不进 Rust 门禁的
MSRV/unsafe/链接预算**（远程验证机仅 3.6 GB，`AGENTS.md:67`）。

## 后果

### 正面

- 铁律 5 从「纪律」升级为「**门禁 + 契约测试**」双重机器保证。
- **§16 的 19 成员名单保持冻结**；`workspace_policy.rs::workspace_members_equal_scaffold_roster`
  （T0.10 验收产物、已做过反向注入验证）**完全不受影响**。
- **零新增依赖** ⇒ `cargo deny check` 的许可面不变；**不需要**维护者对成员边界表态。
- 契约校验复用**既有** `contracts/` 基建（架构师实跑 `uv run pytest`：
  **`48 passed in 0.46s`**），符合 ADR-0008「附属物不拖进主门禁」的形态纪律。
- ADR-0007 的泄漏结论从「T2.1 的发现」升格为「**生成绑定层面的强制条款**」。

### 代价（**如实登记方案 E 放弃了什么**）

1. **放弃了「编译期类型隔离」，改为门禁 + 契约测试隔离。**
   原方案 B 下，`v8-abi` 不依赖 `v8`，因此「类型面持有 V8 类型」是**编译期不可能**；
   方案 E 下 `abi.rs` 与 `engine-v8` 同 crate，Rust 本身**允许**在 `abi.rs` 里写 `v8::`，
   只能靠门禁规则 5 从外部拦住。**这是本 ADR 明确接受的降级**，不是疏漏。
2. **弱在哪、弱到什么程度（必须如实说清）**：
   - 门禁 5 匹配的是 `v8::` **路径首段**。若有人写
     `use crate::v8_bridge::Val;` 而 `Val` 是 `type Val = v8::Local<...>`，
     **门禁 5 抓不到**，契约测试也抓不到（它只比对类型名与值域，不做全量语义分析）。
   - 该盲区与 ADR-0008 决策 3 登记的守卫脚本盲区（依赖/proc-macro 引入的 unsafe）
     **同款**，**不新增风险等级**；但**不得**因此宣称类型面已完全闭合。
3. **决策 4 判据 1 被替换**：原判据「`cargo build -p v8-abi` 在无 V8 静态库机器上可成功」
   在方案 E 下不成立，已替换为「`abi.rs` 中不得出现 `v8::` 路径 + `v8_abi.py` 不 import `v8`」。
   替换后的判据**强度低于**原判据，此处如实标注。
4. **`engine-v8` 的 PR diff 会被生成物噪声污染**（生成物落 `crates/engine-v8/src/generated/`）——
   缓解：生成物**单独 PR**，提交信息只含「重新生成 X」。
5. **`contracts/` 侧新增一个 Python 模块与一个测试文件**：它们**不受** Rust 门禁
   （fmt/clippy/deny）保护，只受 `uv run pytest` 保护。若有人只跑 Rust 门禁就漏掉了。
   **缓解**：`AGENTS.md` 已把 `cd contracts && uv run pytest` 列入自检清单（两侧门禁都要过）。

### 需要后续 ADR 的遗留（本 ADR **不做**）

- `WeakJsHandle` 的定义与 ADR-0007 的「DOM 侧只存弱引用」如何对接 —— 随 `web-events` 的
  事件监听器模型一并决定（ADR-0007 已登记同款遗留）。
- 跨堆追踪（ephemeron）的 `unsafe` 桥接 —— 另起 PoC。
- `HostValue` 若需新增 `BigInt` / `Symbol` / `ArrayBuffer` 变体 —— 按决策 4 判据 3，另起 ADR。

---

## 附：本文档对许清楚 §7 待确认问题的回答

| # | 问题 | 归属 | **本 ADR 的结论** |
|---|---|---|---|
| **Q-1** | `bindings-webidl` ↔ `engine-v8` 依赖方向 | 架构师 | **已定**：`bindings-webidl → engine-v8`（只用 `abi.rs` 的三个类型，公开签名不得出现 `v8::`）；`v8` 只在 `engine-v8/Cargo.toml`。**类型定义放 `engine-v8/src/abi.rs`，契约投影放 `contracts/iv8_contract/v8_abi.py`，不新增 workspace 成员**。见决策 1 |
| **Q-2** | `JsHandle` 是否重新引入 ADR-0007 的强引用环 | 架构师 | **已定：不会**，由 C-1~C-4 四条硬约束保证。`JsHandle` 是槽位下标不是 GC 根；`web-dom` 类型面上不得出现 `JsHandle`。**该结论独立于类型定义放在哪，方案 E 下照留** |
| **Q-6** | `NodeId ↔ JsHandle` 映射表归属 | 架构师 | **已定：归 `engine-v8/src/node_map.rs`**（决策 5 的 C-3）。合并回答 §1 待确认 3 与 §3 待确认 4 |
| **Q-12** | 门禁的已知局限是否可接受 | 架构师 | **已定：可接受，但必须显式登记**。类型别名间接引用检不出 ⇒ 门禁是「辅助」不是「完备」；四条盲区已在决策 3 与「后果」代价 2 逐条列出 |
| 待确认 1 | `HostValue` 跨 PyO3 的转换点 | 架构师 | **已定：归 `sdk-python`**，它依赖 `engine-v8` 的 `abi.rs` 类型，**不依赖 `bindings-webidl`**；转换逻辑不放 `contracts/`（那是契约描述层，不实现转换）。见决策 2 |
| 待确认 2 | `RealmId` 与 `ContextId` 的关系 | 架构师 | **已定：一 Context 一 RealmId，本仓库不区分这两个维度**（决策 2）。若将来要同 Context 多 Realm，需另起 ADR |

### 需维护者拍板事项的当前状态

| 事项 | 状态 |
|---|---|
| `blitz-dom` 是否用 `=` 精确锁定版本 | **仍待维护者**（属依赖基线决策，铁律 2 范畴） |
| 若 T2.4 裁决 `SWITCH_TO_STYLO` 是否立即启动直连路线 | **仍待维护者**（属路线级投入决策） |
| ~~新增第 20 个 workspace 成员 `crates/v8-abi`~~ | ✅ **已由方案 E 消解，不需维护者介入**。<br/>理由：方案 E 使 §16 的 19 成员名单**保持冻结**，`workspace_policy.rs` 的精确名单测试不受影响，零新增依赖，且门禁 PR 无前置可立即落地。<br/>**原判定的形式依据仍成立**：这是「收紧约束」的边界调整，属 `TASKS.md:20-26` 授权下 AI 内部决策的范围；而现在**连边界都不动**，连该判定的适用性都不必援引 |

---

## 修正注记（2026-10-12 · G1 落地期 · R1 措辞与红绿表述更正）

> **性质**：本注记**只更正措辞与红绿表述**，**不改动原正文语义**（原正文保留，供审计追溯）。
> **触发**：G1 门禁落地（PR-3b，`workspace_policy.rs::v8_dependency_only_in_engine_v8`）时，
> 工程师四态反向验证确认决策 3 的「反向验证 R1」小节（本文件 ~315–325 行）有**两处不自洽**。

### 两处不自洽（原文对照）

1. **标题与示例的红绿互斥**：R1 小节标题写「放松规则后断言**必须变红**」，
   但其下示例却写「`test v8_dependency_only_in_engine_v8 ... ok ← 放松后变绿`」。
   ——「必须变红」与「变绿」**在同一小节里互相矛盾**。
2. **放松方式写成 no-op、解释不了现象**：示例把「放松方式」表述为
   「把断言的 crate 过滤条件改为**跳过 `engine-v8`**」。但该断言**本就**跳过 `engine-v8`
   —— `engine-v8` 是白名单 crate，不参与 offending 判定
   （实现：`if *name == WHITELISTED_CRATE { continue; }`）。
   因此「跳过 engine-v8」是**空操作**，**无法解释**示例声称的「注入的 `v8` 依赖不再被报错」。

### 实际执行的放松方式（四态验证，取代原示例的表述）

**放松 = 把「注入的目标 crate」`crates/bindings-webidl/` 加入允许集合**
（即让断言把 `bindings-webidl` 视作白名单/豁免成员、不计入 `offenders`）。
如此，注入到 `bindings-webidl` 的 `v8 = "152.2.0"` 依赖**不再被计入 offenders** ⇒ 断言变绿。
**这是正确、可复现的放松方式**，原示例的「跳过 engine-v8」表述**作废**。

### 四态（原文目击位置）

- **目击位置**：`.workbuddy/memory/2026-10-12.md:426`
  （原文：「四态验证（注入红/放松绿/恢复红/清理绿）；bindings-webidl 已还原（sha256 一致）」）；
  轮次收尾由 lead 回写 `docs/TASKS.md`「第五轮证据台账」。
- **四态序列**：

  | 态 | 动作 | 期望 | 含义 |
  |---|---|---|---|
  | ① 注入 | `bindings-webidl/Cargo.toml` 加 `v8 = "152.2.0"` | **红**（FAILED） | 断言**确实会拦**（非恒绿） |
  | ② 放松 | 把 `bindings-webidl` 加入**允许集合** | **绿**（ok） | 放松判别力后不再拦 ⇒ **证明断言非恒真** |
  | ③ 恢复 | 撤回放松（移除允许集合豁免） | **红**（FAILED） | 判别力恢复，注入仍在 ⇒ 再次拦 |
  | ④ 清理 | 移除注入，`bindings-webidl` 还原（**sha256 一致**） | **绿**（ok） | 干净仓库复绿 |

### 更正后的红绿表述（取代 R1 小节原措辞）

> **放松后必须变绿**（这是「断言非恒真」的证据：放松判别力后，同一注入不再被拦）；
> **恢复放松后必须变红**（判别力恢复，注入仍在）。
> 即正确配对为：**注入 = 红；放松 = 绿；恢复 = 红；清理 = 绿**。

> 说明：原 R1 小节标题的「放松后**必须变红**」**系笔误**，应读作「放松后**必须变绿**」；
> 其示例中的「跳过 engine-v8」**系错误放松方式**，应读作「把 `bindings-webidl` 加入允许集合」。
> **本注记与此后落地的断言实现为准。**

### 追加（2026-10-12 · G1 落地期 · re-export 口径落定）

**两处不自洽**：决策 2 的「`engine-v8` 内部……**不得** `pub use` 出去」（在「三个类型」上**一律禁止** re-export）
与决策 3 局限节的「`engine-v8` 侧**禁止 `pub use v8_abi::*` 之外的 re-export**」（**允许** abi 类型的 `pub use`、
只禁其余）**互斥**。若不落定，S3 接线时**无法跨 crate 引用**三类型（`pub(crate) mod abi` 下不可见）。

**G1 落定解释（本节为准）**：类型**定义唯一性**由**机器断言**保证
（`contracts/tests/test_v8_abi.py` 逐项对应 + `workspace_policy.rs::three_safety_types_have_single_definition_point`
断言全仓定义点各恰好 1 次），因此**允许** crate 根包含**唯一**类型 re-export：

```rust
// crates/engine-v8/src/lib.rs
pub mod abi;
pub use abi::{HostValue, JsHandle, RealmId, WeakJsHandle};   // 唯一类型 re-export
```

- **abi 四类型**跨 crate 走**根路径**（`engine_v8::RealmId` 等形态）。
- **其余类型一律走模块路径**（`engine_v8::realm::Realm` / `engine_v8::handles::HandleLease` /
  `engine_v8::native_fn::NativeFn` / `engine_v8::control::ControlPlane` / `engine_v8::error::JsError`）。
- 决策 2「不得 `pub use`」的**保护意图**（防第二定义点）由上述机器断言承接，故对 abi 四类型**开此唯一例外**；
  仍**禁止** `pub use` 任何其它类型（含 `v8::` 类型）——决策 3 局限章原意不变。

**依据**：ADR-0013 决策 7（类型定义唯一性）与施工图 S3；`workspace_policy.rs` 第 5 条断言为承接机制。
