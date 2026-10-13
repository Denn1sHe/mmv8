//! 任务源队列（`scheduler::queue`）。
//!
//! `BTreeMap<TaskSource, VecDeque<TaskToken>>`：**源内 FIFO**（§6），**源间**按 [`TaskSource`] 的
//! 枚举顺序选择（ADR-0017·D3）。全部为 owned 语义，不吐借用迭代器出函数。

use std::collections::{BTreeMap, VecDeque};

use crate::task::{TaskSource, TaskToken};

/// 各任务源的令牌队列。
#[derive(Debug, Default)]
pub struct TaskQueues {
    inner: BTreeMap<TaskSource, VecDeque<TaskToken>>,
}

impl TaskQueues {
    /// 建空队列集。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 源内 FIFO 入队。
    pub fn push(&mut self, source: TaskSource, token: TaskToken) {
        self.inner.entry(source).or_default().push_back(token);
    }

    /// 取源队首（不移除）。
    #[must_use]
    pub fn front(&self, source: TaskSource) -> Option<TaskToken> {
        self.inner.get(&source).and_then(VecDeque::front).copied()
    }

    /// 源队首出队。
    pub fn pop_front(&mut self, source: TaskSource) -> Option<TaskToken> {
        self.inner.get_mut(&source).and_then(VecDeque::pop_front)
    }

    /// 某源的长度。
    #[must_use]
    pub fn len_of(&self, source: TaskSource) -> usize {
        self.inner.get(&source).map_or(0, VecDeque::len)
    }

    /// 某源的令牌快照（owned，供定时器有序插入）。
    #[must_use]
    pub fn tokens_of(&self, source: TaskSource) -> Vec<TaskToken> {
        self.inner
            .get(&source)
            .map(|deque| deque.iter().copied().collect())
            .unwrap_or_default()
    }

    /// 在某源的第 `index` 位插入（越界则追加）。
    pub fn insert_at(&mut self, source: TaskSource, index: usize, token: TaskToken) {
        let deque = self.inner.entry(source).or_default();
        let index = index.min(deque.len());
        deque.insert(index, token);
    }

    /// 是否所有源皆空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.values().all(VecDeque::is_empty)
    }

    /// 任务总数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.values().map(VecDeque::len).sum()
    }

    /// 清空全部源。
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// 非空且**按跨源选择顺序**排列的源（供可观测断言）。
    #[must_use]
    pub fn non_empty_sources(&self) -> Vec<TaskSource> {
        self.inner
            .iter()
            .filter(|(_, deque)| !deque.is_empty())
            .map(|(source, _)| *source)
            .collect()
    }
}
