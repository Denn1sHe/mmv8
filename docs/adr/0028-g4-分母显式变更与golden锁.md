# ADR-0028：G4 分母**显式**变更（`1669 → 1676 + Credentials`）、`golden` 哈希锁与机器强制

> **状态**：**已裁定**（主理人 2026-10-16 授权架构师自主决策后落档）｜**作者**：架构师（夜班）
> **触发**：架构师源级核可产出 `F3`（抽取器静默丢 7 条）+ `F10`（`owner` 派生不看来源族）+ 前瞻清单发现 `Credentials` 家族 0 条。
> **性质**：本 ADR 是 `ADR-0027 §3` 所要求的「**分母增删须 ADR 显式**」的**那一份 ADR** —— 据此变更**合规**，**不是静默**。
> **不臆造**：新增条目的**最终计数与 `sha256` 由执行时回填**（见 `§2.3`），本 ADR **不预先编造**哈希值。

---

## §1 变更一：补入 7 条「联合类型属性」（`F3` / `U-8`）

### 1.1 问题（**静默家族第三例**）

`contracts/tools/extract_webidl_members.py:66` 的 `_ATTRIBUTE` 正则类型段为 `[\w.<>?\[\]]+`，
**不含 `(` / `)` 与空格** ⇒ WebIDL **联合类型属性**（`attribute (A or B) name;`）**整条无法匹配** ⇒ 成员**消失**。

**为何「纯静默」**（判别要点）：`_METHOD`（L71）会匹配到 `attribute (…)` 并取 `name = "attribute"`，
但 L172 的 `if name not in {"constructor", "attribute", "const"}` 将其排除
⇒ **不报错、不产错条目、只丢**。观测量（测试数 / CI / 台账）**全不变**。

最小复现（已实证）：
```python
idl = "interface Foo { readonly attribute (DOMString or ArrayBuffer)? unionAttr; };"
# extract_members(idl, {"Foo"})  ⇒ 不含 Foo.unionAttr
```

### 1.2 缺失清单（实测 7 条，现网 `1669` 中不存在）

| # | 成员 | 源 IDL 原文 | 源 |
|---|---|---|---|
| 1 | `AudioContext.sinkId` | `readonly attribute (DOMString or AudioSinkInfo) sinkId;` | webref |
| 2 | `CanvasRenderingContext2D.fillStyle` | `attribute (DOMString or CanvasGradient or CanvasPattern) fillStyle;` | webref |
| 3 | `CanvasRenderingContext2D.strokeStyle` | `attribute (DOMString or CanvasGradient or CanvasPattern) strokeStyle;` | webref |
| 4 | `FileReader.result` | `readonly attribute (DOMString or ArrayBuffer)? result;` | webref |
| 5 | `IDBCursor.source` | `readonly attribute (IDBObjectStore or IDBIndex) source;` | webref |
| 6 | `IDBRequest.source` | `readonly attribute (IDBObjectStore or IDBIndex or IDBCursor)? source;` | webref |
| 7 | `WebGLRenderingContext.canvas` | `readonly attribute (HTMLCanvasElement or OffscreenCanvas) canvas;` | khronos |

### 1.3 裁定 = **补**（`D-1`）

- **理由**：属「**`S0` 时少种**」，**非**事后删 —— 评审员 B（`docs/decision-briefs/2026-10-15-G4-评审-评审员B.md`）早已预警：
  「`4.0` 只防**事后删**，**不防 `S0` 时少种**」。本例正是后者的实证。
- **硬约束**：**必须连抽取器正则一并修**（`_ATTRIBUTE` 类型段须容纳 `( ... or ... )` 与可空 `?`），
  **否则下次重跑又丢** ⇒ 只补数据不修工具 = 假修复。
- **影响面预警**：`IDBCursor.source` / `IDBRequest.source` 撞 **`S1-d`（IDB 面）**；
  `WebGLRenderingContext.canvas` / `CanvasRenderingContext2D.fillStyle` / `strokeStyle` 撞 **`S4`（图形媒体）**。

---

## §2 变更二：分母 `1669 → 1676 + Credentials`（`D-1` + `D-S2-1`）

### 2.1 `Credentials` 家族扩源（`D-S2-1` = 补）

- 设计 v3 明文「**WebCrypto + Credentials 两家族**」进分母；实测 `CredentialsContainer` / `Credential` /
  `PasswordCredential` / `FederatedCredential` / `PublicKeyCredential` 等在分母中 **0 条**
  ⇒ `S0` 接口自名单**未含**该族 ⇒ 同为「`S0` 时少种」。
- **裁定 = 补**：以 **webref IDL 为准**扩源并重跑抽取器。
- **`Algorithm` 接口 0 条 = 正确缺席**（`U-5` ③ 已判：dictionary，无动作）⇒ **不补**，且**禁止**顺手补。
- **执行约束**：与 `§1` 的 7 条**同批一次重跑**（**不得分两次** ⇒ 省一轮、且避免中间态分母）。

### 2.2 分母变更的最终口径

- 变更**后**分母 = `1669` + `7` + `Credentials` 族实测条数 ⇒ **以执行后实测为准，回填本 ADR**（见 `§2.3`）。
- 变更**须同步**：`contracts/community-v0.1.4-members.json` → `contracts/compat-manifest.json` → **`FROZEN_DENOMINATOR` 常量** → **`docs/refs/denominator-golden.txt`** → **本 ADR 的 ADR 号回填**。

### 2.3 待回填（**禁止预先编造**）

| 项 | 变更前 | 变更后 |
|---|---|---|
| 分母条目数 `n` | `1669` | **待执行后回填** |
| sorted-members `sha256` | `74f06d62b0ca4dcc…`（前 16 位） | **待执行后回填** |
| `Credentials` 族条数 | `0` | **待执行后回填** |

> 回填者须**同时**更新 `docs/refs/denominator-golden.txt` 与本 ADR 的「变更记录」小节（`§9`）。

---

## §3 `golden` 哈希锁（`D-3`）—— **含落点裁定**

### 3.1 落点裁定：**`docs/refs/denominator-golden.txt` 为唯一真源**（架构师定）

| 备选 | 结论 | 理由 |
|---|---|---|
| **A. `docs/refs/denominator-golden.txt`（唯一真源），manifest **不承载**哈希 | ✅ **选定** | 见 §3.2 |
| **B. manifest 内 `denominator_sha256` 字段** | ❌ 否决 | 见 §3.2 |

#### 3.2 为何否决 B（`F1` 教训的直接应用）

**`F1` 的根因模式 = 「把纪律位放进会被生成器重跑覆写的文件」**。
`compat-manifest.json` 正是 `seed_manifest.py` **每次重跑都整体重写**的产物
（`F1` 实证：`denominator_fixed` 被无条件覆写为 `false`）。
把 golden 哈希放进 manifest ⇒ **复刻同一模式**，且要再改一次 seed 去"保留"这个新字段
（每加一个纪律字段就多一处覆写风险）。

故选 **A**：
- **单一真源**，无「manifest ↔ golden」双向同步问题；
- 与既有 `docs/refs/{webref,cdp,webgl}-hash.txt` **同规**（`U-6` 已立「**锁的统一可 grep 落点**」）；
- `docs/` 为架构师可维护区，**PR diff 中对人显眼**（审阅友好）；
- 路径固定 ⇒ 无需在 manifest 里再存一个"引用字段"（存了又是一处可覆写点）。

> 对主理人倾向「前者 + manifest 引用」的回应：**赞同前者**；但**不赞成在 manifest 存引用** ——
> 固定路径已足够，反向存指针属冗余且新增一处覆写面。已在 §3.2 记录理由。

### 3.3 golden 文件内容规约（≤10 行，与 `U-6` 同规）

```
# G4 分母 golden 锁
denominator_n      = <条目数>
denominator_sha256 = <sorted(members) 以 \n 连接的 sha256>
manifest           = contracts/compat-manifest.json
members            = contracts/community-v0.1.4-members.json
adr                = ADR-0028          # 本次变更的授权编号
updated_at         = 2026-10-16
# 复核：sorted(members) 的 sha256 须与 manifest 的 entries id 集合一致；
#       增删 ⇒ 须同步更新本文件 + 新 ADR 编号，否则加载侧报错。
```

### 3.4 机器强制三层（`D-3`）

| 层 | 机制 | 载体 |
|---|---|---|
| **写入侧**（最有效） | `seed_manifest.py` 写 manifest **前**校验成员集 `sha256 == golden`；不一致 ⇒ `SystemExit`（**拒绝产出错误产物**）。确需更新 ⇒ 显式 `--update-golden --adr NNNN` | 待落 |
| **实物态** | 断言实物 `manifest` / `members` / `golden` **三方对撞**（n + sha256 + ADR 号存在） | 待落（并入 `test_shipped_manifest_denominator_is_fixed`） |
| **类型层** | `Manifest` 加载/构造时，若 `denominator_fixed` 为真 ⇒ 校验成员集 `sha256` 等于 golden，不一致即报错 | 待落（`D-3` 缩范围版） |

**变更纪律**：**增删须同时更新 golden 文件 + 回填新 ADR 编号**，否则三层任一即红。
⇒ 「改分母」从**文档声明**变成**一次必须显式、且机器可验的动作**。

---

## §4 变更三：Khronos 独立来源族 slug（`D-2` = 立）

### 4.1 问题（与 `F2` / `U-9` 同根）

`webref` 与 `khronos-webgl` 两源**共用 slug `spec-webidl`**（`members.json` 的 `sources[0]` 与 `sources[2]`
皆 `source: "spec-webidl"`，仅 `source_version` 不同）⇒

1. **唯一性门禁失效**：`merge_members.py:50` 的 `provenance[member] != slug` 恒假 ⇒
   同名 id 时**不报错、静默去重**（实证：4 条输入 → 3 条，退出码 0）；
2. **来源族不可辨**：807 条 Khronos 成员被标 `spec-webidl`，与 webref 的 419 条**无法区分**
   ⇒ `A-2` 裁定的「来源族随条目承载」在 Khronos 上**失真**。

### 4.2 裁定

- **立独立 slug `khronos-webgl`**（与 `source_version` 前缀一致）。
- **冲突判据改取 `source_version`**（或 `(slug, source_version)`）—— 判据键必须是**逐源唯一**且**不可归并**的
  （新纪律：`冲突判据须用不可归并的键`）。
- **连带**：`test_entries_carry_source_family_in_category` 的「恰两族」断言须改为**三族**；
  `category` 分布由 `spec-webidl 1226 / cdp-protocol 443` 变为 **三族分布**（**显式改 + 理由注**）。
- **不改 id、不改分母大小**（纯元数据）。

---

## §5 变更四：`implementation_owner` 派生修正（`F10` = 批准随修）

- **落点**：`contracts/tools/seed_manifest.py:50-56` `owner_for(member_id)` **只按 id 前缀**匹配 `FAMILY_OWNERS`，**不读 `category`**。
- **实测缺陷**：
  | 类型 | 条目 | 现 owner | 应 owner |
  |---|---|---|---|
  | 误判 | CDP `Storage.*` **34 条** | `web-dom` | **host**（宿主调试面） |
  | 漏判 | `HTMLMediaElement.*` **45 条** | `uncategorized` | `graphics` |
- **裁定 = 修**：`owner_for` 增 `category` 入参，**CDP 源优先判 host**；前缀表补 `HTML*` 形态。
  **不改 id、不改分母**。与 `§4` 同根（前缀不编码来源域），**一并处理**。
- **注意**：修后 owner 分布变化 ⇒ 若有测试钉了分布，须**显式改 + 理由注**，**不得改期望迁就**。

---

## §6 变更五：CDP 域子集冻结（`D-S5-1` = 子集 7 域）

- **取 7 域**：`DOM` / `CSS` / `Page` / `Runtime` / `Network` / `Storage` / `IndexedDB`
  （与 `§4.5a` 最小可用集对齐：`DOM.getDocument`/`querySelectorAll`/`getOuterHTML` +
  `CSS.getMatchedStylesForNode` + `Page.navigate`/`loadEventFired` + `Storage`/`IndexedDB` 列举）。
- **未选中的 `Debugger` / `Target` / `DOMStorage` 三域 ⇒ 逐域具名 `missing`**。
  **禁静默丢域**（丢域 = 静默家族：观测量是"已支持清单"，丢掉的域**不出现在任何报告里**）。
- **子集冻结须 ADR** —— 本 `§6` 即为其授权；后续增删域须新 ADR 号。
- **口径具名**：CDP 是**宿主调试面**，非 Web API ⇒ `status` 判定语义与 spec 面**不同**，**须具名**，不得沿用 spec 面判据。

---

## §7 变更六：字体运行期判定（`D-S4-1`）

- **禁硬编码字体族名清单**（`R-12` 字体跨机：mac/Linux 字体不同）。
- **口径沿用 G3X-S2 已验证形态**：**单调超集断言** + **裸环境双腿负对照**
  （无系统字体时断言「开启/关闭族数相等」）。
- 运行期判定，**不得**以静态清单充当证据。

---

## §8 变更纪律：分母增删的**唯一合法路径**（5 步）

1. 修枚举器（如 `§1.3` 的联合类型正则）/ 扩源自名单（如 `§2.1` 的 Credentials）；
2. **重跑三源**（一次跑完，不得分批留中间态）；
3. 重新生成 `contracts/community-v0.1.4-members.json`；
4. 重跑 `seed_manifest.py`（**冻结位由生成器保留**；缩水 ⇒ 拒绝写入）；
5. **同步改** `FROZEN_DENOMINATOR` 常量 **+** `docs/refs/denominator-golden.txt` **+** 新 **ADR 编号**。

**任一步缺失 ⇒ 三层强制之一即红**（`§3.4`）。**不得**只改常量或只跑 seed。

---

## §9 变更记录（执行后回填）

| 次序 | ADR | 变更 | 变更前 n | 变更后 n | 变更前 sha256(前16) | 变更后 sha256(前16) |
|---|---|---|---|---|---|---|
| 0 | — | `S0` 封口（`5a005a6`） | — | `1669` | — | `74f06d62b0ca4dcc` |
| 1 | **ADR-0028** | 补 7 条联合类型 + Credentials 扩源 | `1669` | **待回填** | `74f06d62b0ca4dcc` | **待回填** |

> **回填纪律**：执行者在落笔 3 后**立即**回填本表与 golden 文件；**未回填即视为变更未完成**。

---

## §10 关联与遗留

- **`ADR-0027 §3`**（分母冻结 / 冻结时点 / 双向核）—— 本 ADR 是其所要求的「**显式 ADR**」。
- **`ADR-0027 §9.2`** 第 3 点已按 `F5` 修订为「`category`（来源族）承载」；本 ADR `§4` 使其**从两族到三族**。
- **`U-8`/`U-9`/`U-10`/`U-12`/`U-13`/`U-14`** —— 见活台账 `docs/g4-台账-2026-10-16.md §2`。
- **遗留（不在本 ADR 范围）**：`F2` merge 层判据改 `source_version`（属工具修，非分母变更）；
  `F7`/`U-11` manifest 真实流转（属流程，非分母变更）。
