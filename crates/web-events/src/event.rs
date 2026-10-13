//! 事件对象与初值（`web-events::event`）。
//!
//! 本 `Event` 是**纯 Rust struct**；JS 侧 `Event` 对象桥（构造 / 逐 step 更新 `currentTarget` /
//! `preventDefault` / `stopPropagation` 回读）由 `navigation::input` 经 `engine-v8` 实现
//! （ADR-0019·D1）。

use web_dom::node::NodeId;

/// 事件所处阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventPhase {
    /// 未分派。
    None,
    /// 捕获阶段。
    Capturing,
    /// 目标阶段。
    AtTarget,
    /// 冒泡阶段。
    Bubbling,
}

/// `Event` 构造初值。
#[derive(Clone, Copy, Debug, Default)]
pub struct EventInit {
    /// 是否冒泡。
    pub bubbles: bool,
    /// 是否可取消（`preventDefault` 生效前提）。
    pub cancelable: bool,
    /// 是否跨 shadow 边界（G2 保留，暂不参与分派）。
    pub composed: bool,
}

/// 事件对象（宿主侧权威状态；JS 侧读经对象桥）。
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct Event {
    /// 事件类型（如 `"click"`）。
    pub type_: String,
    /// 是否冒泡。
    pub bubbles: bool,
    /// 是否可取消。
    pub cancelable: bool,
    /// 是否跨 shadow 边界。
    pub composed: bool,
    /// 是否可信：宿主 `input` 合成 ⇒ `true`；`new Event(...)` ⇒ `false`。
    pub is_trusted: bool,
    /// 原始目标。
    pub target: Option<NodeId>,
    /// 当前目标（逐 `DispatchStep` 更新）。
    pub current_target: Option<NodeId>,
    /// 当前阶段。
    pub phase: EventPhase,
    /// 时间戳（由 `scheduler` 双时钟派生）。
    pub time_stamp: f64,
    /// 是否已 `preventDefault`。
    pub default_prevented: bool,
    /// 是否已 `stopPropagation`。
    pub propagation_stopped: bool,
}

impl Event {
    /// 建一个**不可信**事件（`isTrusted = false`）。
    #[must_use]
    pub fn new(type_: &str, init: EventInit) -> Self {
        Self {
            type_: type_.to_owned(),
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            is_trusted: false,
            target: None,
            current_target: None,
            phase: EventPhase::None,
            time_stamp: 0.0,
            default_prevented: false,
            propagation_stopped: false,
        }
    }

    /// 建一个**可信**事件（`isTrusted = true`；宿主 `input.dispatch` 合成）。
    #[must_use]
    pub fn new_trusted(type_: &str, init: EventInit) -> Self {
        let mut event = Self::new(type_, init);
        event.is_trusted = true;
        event
    }

    /// 标注默认动作被阻止（仅当事件可取消时生效）。
    pub fn prevent_default(&mut self) {
        if self.cancelable {
            self.default_prevented = true;
        }
    }

    /// 标注传播被停止。
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, EventInit, EventPhase};

    #[test]
    fn event_trust_and_defaults() {
        let mut event = Event::new(
            "click",
            EventInit {
                bubbles: true,
                cancelable: true,
                composed: false,
            },
        );
        assert!(!event.is_trusted, "new Event ⇒ 不可信");
        assert_eq!(event.phase, EventPhase::None);
        event.prevent_default();
        assert!(event.default_prevented);
        event.stop_propagation();
        assert!(event.propagation_stopped);

        let trusted = Event::new_trusted("click", EventInit::default());
        assert!(trusted.is_trusted);

        // 不可取消 ⇒ preventDefault 不置位。
        let mut non_cancelable = Event::new("x", EventInit::default());
        non_cancelable.prevent_default();
        assert!(!non_cancelable.default_prevented);
    }
}
