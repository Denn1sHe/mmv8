//! 实验组合矩阵（F1 × F2 × F3）与结论分类。
//!
//! ## 三个因子（许清楚 §4 Q2）
//!
//! - **F1** 暂停期间宿主是否尝试获取 GIL：`no` / `yes`
//! - **F2** 宿主回调是否重入同一 Context 的 eval：`no` / `yes`
//! - **F3** 是否有第二线程参与：`none` / `msg_only` / `needs_locker`
//!
//! ## 不可测的组合与理由（D1 要求「或明确说明为何某些组合不可测」）
//!
//! 2×2×3 = 12 组中，**4 组不可测**：
//!
//! | # | F1 | F2 | F3 | 不可测理由 |
//! |---|---|---|---|---|
//! | 9  | no  | yes | needs_locker | 同一线程上重入 eval 会与 V8 的锁序互相等待，且此时 F2 与 F3 同时为真，**归因不可分离** |
//! | 10 | yes | yes | none        | F1 与 F2 同时为真，无法归因是 GIL 还是重入导致 |
//! | 11 | yes | yes | msg_only    | 同上 |
//! | 12 | yes | yes | needs_locker | 同上（三因子同时为真） |
//!
//! ⇒ `COMBOS_TRIED=8`、`COMBOS_UNMEASURABLE=4`。
//!
//! **偏离设计文档之处（显式说明）**：设计文档 §3.5 的表把 #9~#12 四行标为
//! ❌，但同节的汇总文字写 `COMBOS_TRIED=9`（= 12 − 3）。二者矛盾：
//! 按表格数是 4 组不可测（12 − 4 = 8）。本探针按**表格**执行（4 组不可测），
//! 因为逐行的 ❌ 标记才是可核对的依据，而 `9` 疑为算术笔误。
//! 该偏离已上报，见回传报告。

use std::fmt;

/// 第二线程的参与方式（F3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondThread {
    /// 无第二线程。
    None,
    /// 第二线程只向宿主的控制消息队列投递消息，**不触碰 Isolate**。
    ///
    /// 这对应真实架构里的「网络/UI 线程 → 宿主消息队列」通路：
    /// 技术方案 §13 要求暂停期间页面线程仍能收到宿主显式标记的控制消息，
    /// 而这些消息的生产者本来就不需要 V8 访问权。
    MsgOnly,
    /// 第二线程需要 `v8::Locker` 才能触碰同一 Isolate（D6 的锁序路径）。
    NeedsLocker,
}

impl SecondThread {
    /// 稳定的字符串标签（证据行用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::MsgOnly => "msg_only",
            Self::NeedsLocker => "needs_locker",
        }
    }
}

impl fmt::Display for SecondThread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一个实验组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    /// 组合编号（1~8，对应设计文档的表格行号）。
    pub id: u8,
    /// F1：暂停期间是否尝试获取 GIL。
    pub acquire_gil: bool,
    /// F2：宿主回调是否重入同一 Context 的 eval。
    pub reenter_eval: bool,
    /// F3：第二线程的参与方式。
    pub second_thread: SecondThread,
    /// 泵是否「除 watchdog 外无退出路径」。
    ///
    /// **仅 D0 自证场景使用**。为 `true` 时，泵循环只认 watchdog 的
    /// `abort_pump` 信号，不认宿主的 quit —— 即「若无 watchdog 则永远不返回」。
    /// 这正是 D0 要求的「必然挂起」场景。
    pub hang_forever: bool,
}

impl Combo {
    /// 取出第 `id` 号组合（1~8）。越界返回 `None`。
    pub fn by_id(id: u8) -> Option<Self> {
        let base = |id, acquire_gil, reenter_eval, second_thread| Self {
            id,
            acquire_gil,
            reenter_eval,
            second_thread,
            hang_forever: false,
        };
        Some(match id {
            1 => base(1, false, false, SecondThread::None),
            2 => base(2, false, false, SecondThread::MsgOnly),
            3 => base(3, false, false, SecondThread::NeedsLocker),
            4 => base(4, true, false, SecondThread::None),
            5 => base(5, true, false, SecondThread::MsgOnly),
            6 => base(6, true, false, SecondThread::NeedsLocker),
            7 => base(7, false, true, SecondThread::None),
            8 => base(8, false, true, SecondThread::MsgOnly),
            _ => return None,
        })
    }

    /// D0 的「必然挂起」自证组合（编号 0，不属于 1~8 的矩阵）。
    pub fn watchdog_selftest() -> Self {
        Self {
            id: 0,
            acquire_gil: false,
            reenter_eval: false,
            second_thread: SecondThread::None,
            hang_forever: true,
        }
    }

    /// 全部 8 个可测组合。
    pub fn measurable_all() -> Vec<Self> {
        (1..=8).filter_map(Self::by_id).collect()
    }

    /// F1 是否为真。
    pub fn acquires_gil(&self) -> bool {
        self.acquire_gil
    }

    /// F2 是否为真。
    pub fn reenters_eval(&self) -> bool {
        self.reenter_eval
    }

    /// F3 是否有第二线程。
    pub fn has_second_thread(&self) -> bool {
        self.second_thread != SecondThread::None
    }

    /// `COMBO[n] f1=.. f2=.. f3=..` 形式的标签。
    pub fn label(&self) -> String {
        format!(
            "COMBO[{}] f1={} f2={} f3={}",
            self.id,
            if self.acquire_gil { "yes" } else { "no" },
            if self.reenter_eval { "yes" } else { "no" },
            self.second_thread
        )
    }
}

/// 单个组合的实测结论分类（许清楚 §4 Q3 要求的五种之一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 正常解除暂停并继续执行。
    CleanResume,
    /// watchdog 到点后被报告为超时（本探针的可判定结论，**不是**进程挂住）。
    Timeout,
    /// 有异常传播出来（宿主可观测）。
    ErrorPropagated,
    /// 子进程被判定为死锁（超时仍未退出，被父进程 kill）。
    Deadlock,
    /// 子进程被 V8 的 FATAL 直接打死（信号退出）。
    Fatal,
    /// 组合本身不可测（**只出现在条件表的说明列**，不作为实测结论）。
    ///
    /// 保留该变体是为了让 `Verdict` 的分类集合与许清楚 §4 Q3
    /// 要求的「每种组合输出下列之一」保持同构，便于将来真的去测某组
    /// 不可测组合时不必改枚举。
    #[allow(dead_code)]
    Unmeasurable,
}

impl Verdict {
    /// 稳定字符串标签。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CleanResume => "CLEAN_RESUME",
            Self::Timeout => "TIMEOUT",
            Self::ErrorPropagated => "ERROR_PROPAGATED",
            Self::Deadlock => "DEADLOCK",
            Self::Fatal => "FATAL",
            Self::Unmeasurable => "UNMEASURABLE",
        }
    }

    /// 是否属于「会死锁/会失败」的一类（D2 的负向对照）。
    pub fn is_failure(self) -> bool {
        !matches!(self, Self::CleanResume)
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 4 组不可测组合的机器可读清单（D1 / D10 的条件表行）。
pub fn unmeasurable_combos() -> Vec<(u8, bool, bool, SecondThread, &'static str)> {
    vec![
        (
            9,
            false,
            true,
            SecondThread::NeedsLocker,
            "F2&F3 同时为真：重入 eval 与 Locker 锁序互相等待，归因不可分离",
        ),
        (
            10,
            true,
            true,
            SecondThread::None,
            "F1&F2 同时为真：无法归因是 GIL 互等还是同 Context 重入",
        ),
        (
            11,
            true,
            true,
            SecondThread::MsgOnly,
            "F1&F2 同时为真：无法归因是 GIL 互等还是同 Context 重入",
        ),
        (
            12,
            true,
            true,
            SecondThread::NeedsLocker,
            "F1&F2&F3 同时为真：三因子混杂，归因不可分离",
        ),
    ]
}

/// 不可测理由的汇总标签。
pub const UNMEASURABLE_REASON: &str = "two-factor-confound(F1&F2 或 F2&F3 同时为真时无法归因)";
