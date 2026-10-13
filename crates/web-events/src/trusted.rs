//! 可信事件合成与 user activation（`web-events::trusted`，ADR-0019·D7）。
//!
//! **可信事件 ≠ 真实硬件输入**：`input.dispatch` 合成的事件 `isTrusted = true`，但不得当作
//! 真实输入的证明。

/// 宿主可合成的可信事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrustedEventKind {
    /// `click`。
    Click,
    /// `keydown`。
    KeyDown,
    /// `keyup`。
    KeyUp,
    /// `focus`。
    Focus,
    /// `blur`。
    Blur,
    /// 其他（保留）。
    Other,
}

impl TrustedEventKind {
    /// 对应的事件类型字符串。
    #[must_use]
    pub const fn as_type(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::KeyDown => "keydown",
            Self::KeyUp => "keyup",
            Self::Focus => "focus",
            Self::Blur => "blur",
            Self::Other => "",
        }
    }

    /// 是否属「激活类」事件（触发 user activation）。
    #[must_use]
    pub const fn is_activation(self) -> bool {
        matches!(self, Self::Click | Self::KeyDown)
    }
}

/// user activation 状态。
///
/// **G2 只置位 `sticky`；`transient` G2 不置位**（G3 再用）——不假绿，`capabilities` 明示
/// `input_user_activation = "sticky_only"`。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UserActivation {
    /// 粘性激活（一旦置位保持为真）。
    pub sticky: bool,
    /// 瞬态激活（G2 **不置位**）。
    pub transient: bool,
}

impl UserActivation {
    /// 置位粘性激活（G2 唯一置位路径）。
    pub fn activate(&mut self) {
        self.sticky = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{TrustedEventKind, UserActivation};

    #[test]
    fn trusted_kind_and_activation() {
        assert_eq!(TrustedEventKind::Click.as_type(), "click");
        assert_eq!(TrustedEventKind::KeyDown.as_type(), "keydown");
        assert!(TrustedEventKind::Click.is_activation());
        assert!(TrustedEventKind::KeyDown.is_activation());
        assert!(!TrustedEventKind::Focus.is_activation());

        let mut activation = UserActivation::default();
        assert!(!activation.sticky);
        activation.activate();
        assert!(activation.sticky);
        assert!(!activation.transient, "G2 不置位 transient");
    }
}
