# ADR-0016：G2 D1 —— DOM 内核形态：**扁平 arena + generation**，并把「不重入」升级为**结构性借用规则**

- 状态：**已接受**（2026-10-13 主理人汇总裁决；两位内部独立评审背对背辩论后落档）
- 日期：2026-10-13
- 关联：
  - **ADR-0009**（类型面 / C-1~C-4）、**ADR-0007**（跨语言环泄漏）、**ADR-0013**（弱槽 + 租借 / `node_map`）
  - 施工图 `docs/design-g2-page-2026-10-13.md` §1.2 / §2.2 / §3.7 / §8
  - **决策 brief**：`docs/decision-briefs/2026-10-13-G2-D1-dom-kernel.md`
  - **评审意见（只读证据）**：`docs/decision-briefs/2026-10-13-G2-辩论意见-A.md`（§1）、`-B.md`（§1/§3 断链 10/11）
  - **裁决书**：`docs/decision-briefs/2026-10-13-G2-裁决-主理人.md` §1.D1 / §2（#4/#5）
- 决策者：主理人（两评审 D1 方向与 brief **全部一致**，分歧全在机制层 ⇒「方向不变 + 机制修正」）
- 落地步骤：施工图 **S1 / S2 / S7 / S11**

---

## 问题

`web-dom` 要建 DOM 节点树。要拍板：① 数据结构与节点身份；② 「节点↔JS wrapper」绑定在**弱引用被回收后**
的语义；③ 删除节点与 arena 槽回收的关系；④ 「同一节点同一 wrapper」由谁保证；⑤ 借用冲突（`Rc<RefCell>`
运行期 panic）用**纪律**还是**结构**解决。两评审均**同意方案 A（扁平 arena + generation + 不透明 `NodeId(u64)`）**，
但指出 brief 赖以成立的 3 条细节与已验收代码/施工图**互斥**。

## 讨论要点（两评审的真实分歧与采纳）

| 维度 | 评审 A（证伪） | 评审 B（从零 + 走查） | 主理人裁决 |
|---|---|---|---|
| 主形态 | arena + generation（方案 A） | 同 | **采纳 A**（二人一致） |
| 「不重入＝纪律」（施工图 R4） | **攻破**：与 D2 brief 否定方案 C 的「靠纪律」标准**自相矛盾**；本仓已有结构解（`sdk-python` 的 `try_borrow_mut`） | **同判**：技术方案 §7 行 189「释放 DOM 可变借用后交由 V8 执行」是**明文要求**，非可选纪律 | **采纳：升级为结构性借用规则**（决策 2） |
| `node_map` 复用（A-B5 = B 断链 11） | 攻破「同一节点同一 wrapper 由 node_map 承担」在回收后**不成立**（`bind` 只看有无比对、不判存活） | 同：复用前须存活性校验 | **采纳**（决策 3） |
| 「删除≠回收」与 `nodeid_generation_reuse` | 攻破：二者**互斥**（无 `dispose` ⇒ generation 永不递增 ⇒ 测试不可满足） | 同 | **采纳：加 `NodeTree::dispose`**（决策 4） |
| `MutationObserver` 归属类型 | 攻破：施工图只有 `MutationRecord`/`MutationBuffer`，**缺** observer 注册表/选项/`takeRecords` ⇒ 未归属组件 | 同款缺口（断链 9 关联） | **采纳：补最小类型 + 归属**（决策 5） |
| `class_list` 双真相源 | 指出 `attrs` 与 `class_list` 两写点靠纪律同步 | — | **采纳：惰性派生**（决策 6） |
| `Rc<RefCell>` 全面扁平化 | — | B 残余 #5：倾向扁平化缩小 panic 面（属架构取舍） | **部分采纳：不做全面扁平化，取「单锁层级 + copy-out 清单」最小必要修形**（决策 7） |
| 被拒方案 B/C 理由 | 换更硬理由（B 的致命点=与 `NodeData` 不兼容 + `Rc` 环易写 leak；C=依赖 + key 适配 + 类型面约束） | 同（拒 B/C） | **采纳**（决策 8） |

## 决策

### 决策 1 —— 主形态：扁平 arena + 每槽 generation + 不透明 `NodeId(u64)`

```rust
#[repr(transparent)] pub struct NodeId(u64);   // 高 32 = generation，低 32 = arena 下标（与 JsHandle 逐位一致，U-7）
pub struct NodeTree { nodes: Vec<Option<NodeData>>, generations: Vec<u32>, free: Vec<u32>, root: NodeId, len: usize }
```
`NodeId::raw()` 直接作 `engine_v8::node_map::NodeMap` 的 `u64` key（U-7「确定」成立，A-F1 已核 `abi.rs:59-80`）。
DOM 侧**不持任何 JS 引用**（C-2 / ADR-0007）；`NodeId`（u64）与 `WeakJsHandle` 是仅有的跨面句柄形态。**零 unsafe**。

### 决策 2 —— **结构性借用规则**（把 R4「不重入」从纪律升级为保证；A-F4 + B-断链 10）

技术方案 §7 docx:189 是**明文要求**（「解析暂停，**释放 DOM 可变借用后交由 V8 执行**」）⇒ 不是可选纪律。

1. **别名单元清单**（凡被多方 `Rc<RefCell<…>>` 共享、且其可变借用可被 JS 回调触达者）：
   `web-dom::NodeTree`、`engine-v8::NodeMap`、`web-events::ListenerRegistry`、
   `web-dom::mutation::MutationBuffer`（+ `PageShared` 的**子单元** `tree`/`writes`/`fetch_table`）。**任一元件的可变借用不得跨 V8 回调边界。**

   > **批 2E 精化（2026-10-13，非语义修改）**：`PageShared` 定为**值持有**（由 `BrowsingContext` 直接持，非 `Rc<RefCell<PageShared>>`）
   > ⇒ 清单中的「`PageShared` 整体」**精化为其子单元**（`tree`/`writes`/`fetch_table` 等 `Rc<RefCell<…>>`）；与细粒度借用规则一致。
2. **跨 V8 边界一律 copy-out**：
   - `MutationObserver` 交付前 `mem::take(&mut shared.mutations)` 取出 **owned** 记录，**释放借用后**再回调；
     回调期间新产生的记录留待下一轮（B-断链 10 序列①）。
   - `plan_dispatch(...)` 返回 **owned `Vec<DispatchStep>`**；**类型上禁止**返回借用 registry 的迭代器；
     其借用必须在任何 `Realm::call` **之前**结束（B-断链 10 序列②）。
   - 监听器调用前释放全部别名单元借用。
3. **取借用一律 `try_borrow`/`try_borrow_mut`**，失败 ⇒ 显式 `DomError`/`PageError`（**可判定错误，非 panic**），
   沿用 G1 `sdk-python/src/lib.rs:122` 六处同款形态。**专测** `refcell_reentry_is_error_not_panic`
   （构造「native fn A 借树 → 回调 JS → native fn B 再借树」路径——注意 `native_fn.rs:96` 的 `in_callback`
   **不阻断**嵌套 native fn）。
4. **单锁层级定稿 + 专测固化**（详见 §8 Shared Knowledge）。

> **QA-R1 · D4（已回填，2026-10-13）**：**已落** = binding 全部原生原语（`tree`/`mutations`/`registry`/`writes`/`node_map`）
> + `wrapper_for_impl` + `input::dispatch_named`（`tree`/`registry`）+ `page::deliver_mutations`（`mutations`/`observers`/`tree`）；
> **残余 = 宿主驱动路径**（`load`/`run_until`/`pump_fetch`/`resolve_completed`/`close`）**具名登记**
> （主理人裁定**不扩大**：非 V8 回调边界、冲突面小）。

### 决策 3 —— `NodeMap` 复用**存活性校验**（A-B5 = B 断链 11）

`PageBinding::wrapper_for` 复用前必须 `resolve_weak` 校验：
`Some` ⇒ 复用；`None`（`Collected`）⇒ **`unbind` → 新建 wrapper → `bind`**。
为此 `engine-v8::node_map` 需新增「**覆盖死亡弱句柄**」语义（新增 `bind_force` 或让 `handle_for` 先判存活）——
**这是对已验收 `node_map.rs:28-38` 的跨 crate 契约修改**（落 ADR-0013 的修正注记 + S7）。
**专测** `node_wrapper_recreated_after_collection`（强制 GC 后同节点取回 ⇒ **新 wrapper** 且身份自洽）。

### 决策 4 —— `NodeTree::dispose`（最小实现；A-B4）

新增**最小组件回收**：`dispose(NodeId)` = 离树 + 释放 arena 槽（入 `free`）+ **generation bump**。
使施工图 §4 的 `nodeid_generation_reuse`（旧 `NodeId` 于新节点必须判失效）**可写**。
「删除 ≠ 回收」解耦显式化：`remove_child` 只离树；`dispose` 才回收槽。（`dispose` 可 `pub(crate)`。）

### 决策 5 —— `MutationObserver` 最小类型 + 归属（A-F5；**批 2A 落盘注记**）

补最小规范面（归属 `web-dom::mutation`）：观察器**注册表 `ObserverRegistry<C = ()>`**（**泛型回调槽**）、
`observe` 选项（childList/attributes/subtree/attributeOldValue）、`takeRecords`。
交付点与顺序见 **ADR-0017 决策（MutationObserver 边界）**（一处定死 + 登记偏差）。

> **批 2A 实现注记（2026-10-13，非语义修改）**：`ObserverRegistry` 落为**泛型 `ObserverRegistry<C = ()>`**，
> 使 **`web-dom` 不依赖 `engine-v8`**；**navigation 侧实例化为 `ObserverRegistry<engine_v8::WeakJsHandle>`**。
> 故原「回调槽用 `WeakJsHandle`」的归属**改述为**：回调槽类型由泛型参数 `C` 提供，**web-dom 自身不绑定 `WeakJsHandle`**。

### 决策 6 —— `class_list` **惰性派生**（A-F6）

`ElementData.class_list` 改为**由 `attrs` 按需分解**（不设第二写点），消除 `attrs`/`class_list` 双真相源。

### 决策 7 —— `Rc<RefCell>` **不做全面扁平化**（B 残余 #5 的裁断）

取**「单锁层级 + copy-out 清单」最小必要修形**（决策 2），不动施工图 §1.2 的其余签名。

### 决策 8 —— 被拒方案与残余风险（供登记）

| 被拒方案 | 硬理由（采纳 A/B 的更硬版本） |
|---|---|
| **B：`Rc<RefCell<Node>>` 父子强/弱引用** | ① 无 generation ⇒ **ABA**（违反技术方案 §5）；② 与施工图 §1.2 `NodeData`（下标树链、可序列化、零 unsafe）**不兼容**；③ `Rc` 环 + `Weak` 父指针在 `remove_from_parent`/`reparent_children` 下**极易写成泄漏** |
| **C：`slotmap`** | ① 新增依赖须过 `deny.toml` + 跨平台确认；② 与 ADR-0009「类型面只用本仓定义类型」冲突；③ `NodeKey` 与 `engine-v8::node_map` 的 `u64` key 需**再适配层**（多一个真相源，A 已指出 brief 的「不能稳定 u64 序列化」是**未证推测**，改以上三条为据） |

## 后果

### 正面
- 方向（零 unsafe / DOM 侧无强 JS 引用 / generation）**未变且更稳**；「不重入」由纪律升级为**结构保证 + 可判定错误**。
- `node_map` 回收后复用有**确定性语义**；`nodeid_generation_reuse` 由「写不出」变「可写」。
- 补齐 `MutationObserver` 归属 ⇒ §3.7/§3.9 主语完整。

### 代价（如实登记）
- `engine-v8::node_map` 需改（新增覆盖语义）⇒ 触铁律 6 评估（改动小、无新 unsafe 面）。
- 别名单元清单 + copy-out 是**必须遵守的编码约束**，须靠专测（`refcell_reentry_is_error_not_panic`）+ 评审守住。

### 待实测登记

| # | 待实测项 | 落点 |
|---|---|---|
| T1 | `dispose` 后旧 `NodeId` 判失效（generation bump） | S1（`nodeid_generation_reuse`） |
| T2 | GC 后 `wrapper_for` 重建 wrapper（`node_map` 覆盖语义） | S7/S11（`node_wrapper_recreated_after_collection`） |
| T3 | `try_borrow` 重入路径报错而非 panic | S1/S11（`refcell_reentry_is_error_not_panic`） |

### 遗留（本 ADR 不做）
- 跨堆追踪（ephemeron）unsafe 桥接（ADR-0007 遗留，本轮不做）。
- `Rc<RefCell>` 全面扁平化（B 残余 #5）：**本轮不采纳**，若 G3 出现 panic 面证据再评估。

## 对施工图的更正注记
1. §1.2 `ElementData.class_list` → **惰性派生**（删去独立可写字段）。
2. §1.2 `NodeTree` 增 `dispose(NodeId)`（`pub(crate)`）。
3. §1.2 `PageBinding::wrapper_for` 增「复用前 `resolve_weak` 校验；`Collected` ⇒ unbind+重建+bind」。
4. §1.1/§1.2 `web_dom::mutation` 增 `MutationObserver`/`observe` 选项/`takeRecords`（注册表为**泛型 `ObserverRegistry<C = ()>`**；navigation 侧实例化为 `WeakJsHandle`）。
5. §2.2/§8 增「结构性借用规则（别名单元清单 + copy-out 清单）」；R4 由「纪律」改为**结构保证 + 专测**。
6. §3.7 增 `node_wrapper_recreated_after_collection` / `refcell_reentry_is_error_not_panic`；§4 `web-dom::tree` 行的
   `nodeid_generation_reuse` 标注「依赖 `dispose`（本 ADR）」。

## 交叉引用
- 借用/错误映射 → **ADR-0017（错误映射表）**、**ADR-0018（PyO3 重入覆盖）**
- 弱句柄纪律 → **ADR-0013**（本 ADR 决策 3 为其修正注记）
