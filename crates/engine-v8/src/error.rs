//! 安全错误类型：**不泄漏任何 V8 类型**的失败原因（ADR-0009 / ADR-0013 / ADR-0014）。
//!
//! `JsErrorKind` 的变体集合是**跨语言契约的一部分**（`sdk-python` 与未来契约层需逐项
//! 对应）。本文件内的「变体可达性矩阵」单测保证**每个变体都能被构造并原样取回**，
//! 防止「枚举加了变体却没有实现分支」。

/// 失败类别。**每个变体都必须可被至少一条测试触发**（变体可达性矩阵）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JsErrorKind {
    /// 语法 / 编译错误。
    Compile,
    /// JS 抛出的可捕获异常（经 `TryCatch` 取回）。
    Exception,
    /// 弱引用已空——对应值已被 GC 回收（ADR-0013 决策 3）。
    Collected,
    /// 句柄 generation 不匹配——已释放或槽位被复用（复用防呆，ADR-0013 决策 3）。
    StaleHandle,
    /// 句柄来自另一个 Realm。
    WrongRealm,
    /// 执行被 `terminate_execution` 打断（watchdog）。
    Terminated,
    /// 微任务预算耗尽（防死循环）。
    MicrotaskBudget,
    /// Python 回调墙钟超限（**事后可判定，不挂死**；ADR-0014 决策 2）。
    CallbackDeadline,
    /// 违反强引用环禁令（编程错误；防环由 C-2 类型面禁令承担，ADR-0013 决策 6）。
    GcCycleForbidden,
    /// 页面脚本执行期访问控制入口（控制入口隔离）。
    ControlRevoked,
    /// 回调内再次求值同一 Context（ADR-0012 D5 / ADR-0014 决策 3，RAII 强制）。
    ReentrantEval,
    /// 其它（兜底）。
    Internal,
}

/// 携带失败类别与人类可读消息的错误值。
#[derive(Clone, Debug)]
pub struct JsError {
    kind: JsErrorKind,
    message: String,
}

impl JsError {
    /// 由失败类别与消息构造。
    #[must_use]
    pub fn new(kind: JsErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 取失败类别。
    #[must_use]
    pub fn kind(&self) -> JsErrorKind {
        self.kind
    }

    /// 取人类可读消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::{JsError, JsErrorKind};

    /// 全部变体（穷举）。新增变体时 [`kind_slot`] 的 `match` 会编译失败，强制此处同步。
    const ALL_KINDS: [JsErrorKind; 12] = [
        JsErrorKind::Compile,
        JsErrorKind::Exception,
        JsErrorKind::Collected,
        JsErrorKind::StaleHandle,
        JsErrorKind::WrongRealm,
        JsErrorKind::Terminated,
        JsErrorKind::MicrotaskBudget,
        JsErrorKind::CallbackDeadline,
        JsErrorKind::GcCycleForbidden,
        JsErrorKind::ControlRevoked,
        JsErrorKind::ReentrantEval,
        JsErrorKind::Internal,
    ];

    /// 编译期穷举守卫：为每个变体分配一个稳定的槽位号。**不设 `_` 通配分支**，
    /// 因此一旦新增变体而未同步 `ALL_KINDS`，本函数即编译失败。
    const fn kind_slot(kind: JsErrorKind) -> usize {
        match kind {
            JsErrorKind::Compile => 0,
            JsErrorKind::Exception => 1,
            JsErrorKind::Collected => 2,
            JsErrorKind::StaleHandle => 3,
            JsErrorKind::WrongRealm => 4,
            JsErrorKind::Terminated => 5,
            JsErrorKind::MicrotaskBudget => 6,
            JsErrorKind::CallbackDeadline => 7,
            JsErrorKind::GcCycleForbidden => 8,
            JsErrorKind::ControlRevoked => 9,
            JsErrorKind::ReentrantEval => 10,
            JsErrorKind::Internal => 11,
        }
    }

    #[test]
    fn every_kind_roundtrips() {
        // 变体可达性矩阵起点：每个变体都能构造 JsError 并原样取回 kind() / message()。
        for (expected_slot, kind) in ALL_KINDS.into_iter().enumerate() {
            assert_eq!(
                kind_slot(kind),
                expected_slot,
                "ALL_KINDS 顺序应与槽位号一致"
            );
            let err = JsError::new(kind, format!("probe {kind:?}"));
            assert_eq!(err.kind(), kind, "kind() 往返失败：{kind:?}");
            assert!(!err.message().is_empty(), "message() 不应为空");
        }
        // 所有槽位互不相同（无重复变体被登记两次）。
        let slots: std::collections::BTreeSet<usize> =
            ALL_KINDS.iter().copied().map(kind_slot).collect();
        assert_eq!(slots.len(), ALL_KINDS.len(), "ALL_KINDS 中不得有重复变体");
    }
}
