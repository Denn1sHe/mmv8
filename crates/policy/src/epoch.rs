//! 导航代次（`policy::epoch`，ADR-0017·D1）。
//!
//! `NavigationEpoch` 的**定义点落 `policy`**：`navigation` 与 `network` 均依赖 `policy`，
//! 从而**避免 `navigation ↔ network` 依赖环**。新导航 +1；旧代次的异步完成消息据此作废。

/// 导航代次：新导航 +1；旧代次的消息据此作废。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NavigationEpoch(u64);

impl NavigationEpoch {
    /// 初始代次（0）。
    #[must_use]
    pub const fn initial() -> Self {
        Self(0)
    }

    /// 取原始 `u64`。
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// 下一代（`+1`，溢出回绕）。
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

impl From<u64> for NavigationEpoch {
    fn from(raw: u64) -> Self {
        Self(raw)
    }
}

impl From<NavigationEpoch> for u64 {
    fn from(epoch: NavigationEpoch) -> Self {
        epoch.0
    }
}

#[cfg(test)]
mod tests {
    use super::NavigationEpoch;

    #[test]
    fn epoch_monotonic() {
        let e0 = NavigationEpoch::initial();
        assert_eq!(e0.get(), 0);
        let e1 = e0.next();
        assert_eq!(e1.get(), 1);
        assert!(e0 < e1, "代次应严格递增");
        assert_eq!(u64::from(e1), 1);
        assert_eq!(
            NavigationEpoch::from(7u64),
            NavigationEpoch::initial()
                .next()
                .next()
                .next()
                .next()
                .next()
                .next()
                .next()
        );
    }
}
