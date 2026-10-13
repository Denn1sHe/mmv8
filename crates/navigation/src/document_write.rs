//! `document.write` 插入点账本（ADR-0017·D4 / ADR-0006）。
//!
//! parser 结束后（`readyState != Loading`）的 `write` ⇒ **具名错误**
//! [`PageErrorKind::DocumentWriteAfterLoad`]（可判定、可测）。多条写入**保序**（调用序）。
//! 真正的 `push_front` 注入由 [`crate::parser::ParserHost::resume`] 完成（上层不直接操纵
//! `BufferQueue`，ADR-0006）。

use crate::error::{PageError, PageErrorKind};

/// 文档写出模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteMode {
    /// 解析进行中（`write` 允许）。
    Parsing,
    /// parser 已结束（`write` 被拒）。
    AfterLoad,
}

/// `document.write` 插入点账本：累积待注入文本（调用序）+ 模式。
#[derive(Debug)]
pub struct DocumentWrite {
    mode: WriteMode,
    pending: Vec<String>,
}

impl DocumentWrite {
    /// 建处于 [`WriteMode::Parsing`] 的账本。
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: WriteMode::Parsing,
            pending: Vec::new(),
        }
    }
}

impl Default for DocumentWrite {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentWrite {
    /// 当前模式。
    #[must_use]
    pub fn mode(&self) -> WriteMode {
        self.mode
    }

    /// 标记 parser 已结束（此后 `write` 被拒）。
    pub fn mark_loaded(&mut self) {
        self.mode = WriteMode::AfterLoad;
    }

    /// 记一次 `write`（保序入队）。
    ///
    /// # Errors
    /// parser 已结束 ⇒ [`PageErrorKind::DocumentWriteAfterLoad`]。
    pub fn write(&mut self, text: &str) -> Result<(), PageError> {
        if self.mode != WriteMode::Parsing {
            return Err(PageError::new(
                PageErrorKind::DocumentWriteAfterLoad,
                "load 完成后 document.write 被拒（ADR-0017·D4）",
            ));
        }
        self.pending.push(text.to_owned());
        Ok(())
    }

    /// 待注入文本（调用序，只读）。
    #[must_use]
    pub fn pending(&self) -> &[String] {
        &self.pending
    }

    /// 取出全部待注入文本（调用序），清空账本。
    pub fn take_pending(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending)
    }

    /// 是否无待注入文本。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentWrite, WriteMode};
    use crate::error::PageErrorKind;

    #[test]
    fn document_write_multiple_in_order() {
        // §3.3：`write("a");write("b")` ⇒ 注入序与调用序一致。
        let mut ledger = DocumentWrite::new();
        assert_eq!(ledger.mode(), WriteMode::Parsing);
        ledger.write("a").expect("write a");
        ledger.write("b").expect("write b");
        assert_eq!(ledger.take_pending(), vec!["a".to_owned(), "b".to_owned()]);
        assert!(ledger.is_empty());
    }

    #[test]
    fn document_write_after_load_defined() {
        // §3.3 / ADR-0017·D4：post-load ⇒ 具名错误 `DocumentWriteAfterLoad`。
        let mut ledger = DocumentWrite::new();
        ledger.mark_loaded();
        assert_eq!(ledger.mode(), WriteMode::AfterLoad);
        let err = ledger.write("<p>").expect_err("post-load write 应被拒");
        assert_eq!(err.kind(), PageErrorKind::DocumentWriteAfterLoad);
    }
}
