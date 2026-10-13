# 决策 brief —— G3 ①：Stylo/Taffy/Parley 栈的**锁版与补丁策略**（2026-10-13）

> **用途**：按 `AGENTS.md`「决策上报协议」（维护者授权 AI 内部决策，形式要求不变）填写。
> **读者没有本仓库任何上下文，也不预设懂 Rust。** 所有代码/命令/输出**原文粘贴**。
> **状态**：本 brief 是**待落 ADR 的输入**；本代理推荐**仅供参考**，等待评审辩论后由主理人裁决。

---

> ## ⚠️ 更正标注（2026-10-13 裁决后 · **不重写历史**）
>
> **结论 D2 采纳（方案 A + C 通道默认休眠）**；以下为**事实性更正**（裁决书 §二·D2/§三·G3-B4；ADR-0021）：
> 1. **删除不可复核数字**：正文 §3 方案 B 中「stylo 最新 `0.22`」「parley 最新 `0.12.0` MSRV 1.88」
>    **本地 registry 无该版本**（`stylo-*` 仅 0.20.0；`parley-*` 仅 0.11.1/0.2.0），且 AGENTS.md 禁联网 ⇒
>    **不可复核，予以删除**。仍成立的机制：`stylo_taffy 0.3.0-beta.2` 内部 `style.version = "0.20.0"`
>    （`^0.20.0`）⇒ 写 `stylo="0.22"` 会双版本并存。
> 2. **「同源锁定树」措辞更正**：由「同一锁定树实测通过」→「**经 blitz 传递**编译通过的组；
>    **直连后须以 P-1/P-8 复验**」（`stylo` 是 blitz 的**传递**依赖，非探针直接依赖）。
> 3. **补丁通道（方案 C）**：确认**默认休眠**，启用须独立 PR + 理由 + 裁决。
> **落地**：ADR-0021（已接受）。

---

## 0. 一句话问题

> **G3 要把样式与布局引擎（Stylo + Taffy + Parley 一套）接进 `layout-backend`——这套依赖该按
> 「**精确锁死到某个 beta 版本**」引入，还是「**跟随最新稳定版**」引入？锁死会让未来升级必须手动改，
> 跟随最新则可能与上游的版本互斥（Stylo 被上游钉在 0.20，而最新是 0.22）。**

## 1. 背景与上下文

- **这个仓库是什么**：`iv8` 是一个**自研浏览器运行时**（Rust 宿主 + V8 引擎 + Python 控制与契约层）。
  它要模拟浏览器 API，需要一套「把它自己的 DOM 树算成样式、再算成布局盒子」的引擎。
- **涉及的模块 / 文件路径**：
  - `crates/layout-backend/`（本轮**首次落地**；T0 起一直是空骨架）
  - `crates/web-style/`（当前只有内联声明值）
  - `poc/t2_4_blitz_probe/`（已有探针，实测过 blitz 的能力并锁定过一组版本）
- **当前状态**：
  - 上一轮（T2.4）已用探针实测 `blitz-dom 0.3.0-beta.2`：**动态 DOM 与样式失效可用，但 Shadow DOM 未实现**
    ⇒ 裁决切换为「**直接 Stylo + Taffy + Parley**」（见 `docs/adr/0011`）。
  - 该探针的 `Cargo.lock` **锁定的整组版本已在同一台机器上编译通过**（见 §4 证据 A）。
  - `crates/layout-backend/Cargo.toml` 目前**依赖为空**：
    ```toml
    [dependencies]
    ```
- **为什么现在必须决策**：G3 的 S2 步要把这套依赖写进 `layout-backend/Cargo.toml`。
  **锁法一旦定错，会导致编译冲突（版本互斥）或未来升级被卡死**；且 `Cargo.lock` 入库口径
  （技术方案 §19 要求「保存 Cargo.lock、Stylo/Blitz revision」）需要一并定。

## 2. 需要拍板的问题

- **主问题**：G3 引入 Stylo/Taffy/Parley 时，版本策略取「**精确锁定**」还是「**跟随最新**」？
- **子问题**：
  1. 是否采用 T2.4 探针**已实测通过**的整组版本作为「固定 revision」？
  2. `Cargo.lock` 是否入库？升级流程如何定义（谁能改、改动是否须独立 PR + 重跑探针）？
  3. 上游版本互斥（`stylo_taffy` 钉 `stylo 0.20`，而 `stylo` 最新 `0.22`）如何处理？
  4. 是否保留「补丁策略」——即若某上游 crate 有阻塞缺陷，是否允许 vendor + 打补丁？

## 3. 备选方案（至少两个）

### 方案 A：**精确锁定到已实测通过的一组版本**（`Cargo.lock` 入库）

- **做法**：`layout-backend/Cargo.toml` 里把整组依赖写成 `=` 精确版本，取值 = T2.4 探针
  `Cargo.lock` 中**实测编译通过**的那一组；`Cargo.lock` 提交入库；升级须**独立 PR + 重跑探针**。
- **关键代码**（原文草案）：
  ```toml
  stylo        = { version = "=0.20.0", package = "stylo" }
  stylo_dom    = "=0.20.0"
  stylo_traits = "=0.20.0"
  stylo_taffy  = "=0.3.0-beta.2"
  taffy        = "=0.14.0"
  parley       = "=0.11.1"
  selectors    = "=0.40.0"
  skrifa       = "=0.44.0"
  fontique     = "=0.11.1"
  ```
- **优点**：① 可重复构建（技术方案 §19）；② 规避 semver 漂移（`0.3.0-beta.2` 是 beta，`=` 是
  上游自己的锁法）；③ 与 T2.4 探针**同源**，证据链连续。
- **缺点与风险**：① **放弃自动升级**，上游修 bug 要手动改；② 与「跟随最新」的其它 crate 可能互斥；
  ③ 每次升级需重跑探针（成本）。
- **长期影响**：与「固定 revision 验证」的技术方案 §19 一致；维护成本可控（升级 = 一次探针回归）。
- **预估工作量**：小（写依赖 + 跑 S1 探针）。

### 方案 B：**跟随最新稳定版**（放宽到 `^` 语义）

- **做法**：`stylo = "0.22"`、`taffy = "0.14"`、`parley = "0.12"` 等，取各自最新稳定；不锁 `stylo_taffy`。
- **关键代码**（原文草案）：
  ```toml
  stylo     = "0.22"
  taffy     = "0.14"
  parley    = "0.12"
  ```
- **优点**：拿到最新修复；升级自动。
- **缺点与风险**：**极高**——① `stylo_taffy 0.3.0-beta.2` 内部钉 `stylo ^0.20.0`，若我们写
  `stylo = "0.22"`，则 `stylo` 会出现**两个不兼容版本并存**（编译错误或行为不可预期）；
  ② `parley` 最新 `0.12.0` 的 MSRV 是 **1.88**（本仓 CI 矩阵含 `1.92.0` + `stable`，暂无冲突，
  但与本仓 `rust-version = "1.85"` 声明冲突更甚）；③ semver 在 `0.x`/beta 上**不保证兼容**。
- **长期影响**：编译期/运行期不确定性最大。
- **预估工作量**：小（写依赖）但**风险溢出到后续所有步骤**。

### 方案 C：**锁版 + vendor 补丁策略**（A 的超集）

- **做法**：默认同方案 A；另设一条「**若上游 crate 有阻塞缺陷，允许 `[patch.crates-io]` vendor
  + 打补丁**」的通道，补丁须独立 PR 并写明理由。
- **关键代码**（原文草案）：
  ```toml
  [patch.crates-io]
  # stylo = { path = "vendor/stylo" }   # 仅在确需补丁时启用
  ```
- **优点**：兼顾稳定与可救济；与铁律 2（golden/基线独立 PR 修改）同纪律。
- **缺点与风险**：增加维护面；补丁需自证（diff ≤ 400 行、理由充分）。
- **长期影响**：最稳但最重。
- **预估工作量**：中。

## 4. 证据（原文粘贴）

**证据 A —— T2.4 探针已实测编译通过的整组版本（`poc/t2_4_blitz_probe/Cargo.lock` 原文）**

```
name = "blitz-dom"
version = "0.3.0-beta.2"
...
name = "stylo"
version = "0.20.0"
...
name = "taffy"
version = "0.14.0"
...
name = "parley"
version = "0.11.1"
```

（阅读指引：这就是「**已经在同一台机器上编译并跑通探针**」的一组版本；方案 A 即取此组。）

**证据 B —— 探针的依赖声明原文（`poc/t2_4_blitz_probe/Cargo.toml`）**

```toml
[dependencies]
blitz-dom = { version = "=0.3.0-beta.2", default-features = false }
blitz-traits = "=0.3.0-beta.2"
markup5ever = "0.39"
selectors = "=0.40.0"
style = { version = "=0.20.0", package = "stylo" }
style_traits = { version = "=0.20.0", package = "stylo_traits" }
```

（阅读指引：探针**自己**也用了 `=` 精确锁——这是上游 beta 的常规做法。）

**证据 C —— 本机 cargo registry 现存版本（命令输出原文）**

命令：`ls -d ~/.cargo/registry/src/rsproxy.cn-*/{stylo,stylo_taffy,stylo_dom,stylo_traits,taffy,parley,skrifa,fontique,blitz-dom}-*`

```
.../stylo-0.20.0
.../stylo_dom-0.20.0
.../stylo_traits-0.20.0
.../stylo_taffy-0.3.0-beta.2
.../taffy-0.14.0        （另有 taffy-0.7.7）
.../parley-0.11.1       （另有 parley-0.2.0）
.../skrifa-0.44.0       （另有 skrifa-0.22.3）
.../fontique-0.11.1     （另有 fontique-0.2.0）
.../blitz-dom-0.3.0-beta.2
```

（阅读指引：本机**已有**方案 A 所需整组版本的源码缓存，离线可编译 ⇒ 方案 A 无「拉不到包」风险。）

**证据 D —— ADR-0011 的关键条款原文（`docs/adr/0011-T2.4-Blitz裁决与Stylo直连路线.md`）**

> **切「直连 Stylo 路线」省掉的是 blitz 的 DOM 抽象层，不是样式与布局依赖。**
> 已核实的依赖树含 `stylo 0.20` / `stylo_taffy` / `taffy 0.14` / `parley 0.11`
> （另有 `skrifa 0.44` / `fontique 0.11` 等字体栈）——**这些在直连路线下仍然要引入**。

## 5. 本仓库的既定约束（外部建议必须在此边界内）

- **铁律 3**：「一事一 PR，diff ≤ 400 行」——版本升级/锁版改动应可独立成 PR。
- **铁律 7**：AI **不得修改** `deny.toml`。**新传递依赖若引入白名单外许可 ⇒ 上报，不自行扩白名单。**
  （已核实：`stylo` 系列 MPL-2.0 在 `deny.toml` 白名单内；`taffy` MIT、`parley` Apache-2.0 OR MIT 亦在内。）
- **技术方案 §19** 原文要求「**保存 Cargo.lock、Stylo/Blitz revision**」。
- **ADR-0011**：裁决 `SWITCH_TO_STYLO`；**`LayoutBackend` 对外接口保持不变**。
- **跨平台纪律**：新增依赖须支持 **linux x86_64**（`AGENTS.md:72`）。
- **不联网查 API**：依赖源码都在本地 `~/.cargo/registry/src/rsproxy.cn-*/`，直接读原文。

## 6. 本代理的推荐（**仅供参考**，等待外脑/评审讨论）

- **推荐：方案 A（精确锁定到已实测通过的一组版本），并保留方案 C 的补丁通道（默认不启用）。**
- **理由**：
  1. 与「固定 revision 验证」的技术方案 §19 一致，证据链与 T2.4 探针**连续**；
  2. `stylo_taffy 0.3.0-beta.2` 内部钉 `stylo ^0.20.0` —— **写 `stylo ="0.22"` 会造成版本并存**，
     方案 B 在本仓**大概率直接编译失败**；
  3. 本机 registry 已缓存整组版本 ⇒ 方案 A **离线可编译**（远程可 `rsync` 带过去）。
- **我不确定的点**：
  1. **`stylo` 是否只是「被 blitz 项目发布的一个镜像 crate」**——它的上游/维护方是谁？
     若长期无人维护，锁死是否可持续？
  2. `stylo_taffy` 是否是**唯一**的 stylo→taffy 桥（若它后来也要求 `blitz-dom` 类型，方案需调整）。
     （→ 这正是施工图 **P-2 探针**要证的。）
  3. 是否应把「锁版」写进一个新 ADR（而非仅写进 `Cargo.toml` 注释）。

## 7. 请重点回答的问题

1. 在「上游 beta + 内部钉版本」的现实下，**精确锁定 vs 跟随最新**，哪条对**可重复构建**更负责？
2. 若采用精确锁定，**升级流程**应如何约束（谁批准、是否强制重跑探针、是否须独立 PR）？
3. 是否需要现在就把「vendor 补丁通道」（方案 C）制度化，还是等到真出现阻塞缺陷再说？

---

> **回传后**：结论整理成 ADR（记录备选、讨论要点、选择理由），落档后方可动代码。
> **没有 ADR 落档的结论不执行。**
