//! 双时钟（`scheduler::clock`，ADR-0010 / ADR-0017·D3）。
//!
//! 逻辑时钟归 `scheduler`（与时区正交，ADR-0010）。**逻辑模式**下 `RunLoop` 在「无立即任务」
//! 时**虚拟跳转**到最近 due 定时器（`advance`），**不 sleep**；**系统模式**下不跳转。

use std::fmt;
use std::time::{Duration, Instant};

/// 时钟模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockMode {
    /// 逻辑时钟（虚拟时间；可跳转，不 sleep）——测试与「离线生成」形态。
    Logical,
    /// 系统时钟（墙钟；`advance` 为 no-op）。
    System,
}

/// 双时钟接口。
pub trait Clock: fmt::Debug {
    /// 单调墙钟（deadline 判据在**任务边界**检查）。
    fn monotonic_now(&self) -> Instant;

    /// `performance.now` 口径的逻辑时间（毫秒）。
    fn logical_now_ms(&self) -> f64;

    /// 推进逻辑时间（由 `RunLoop` 在逻辑跳转时调用；系统模式为 no-op）。
    fn advance(&mut self, delta: Duration);

    /// 时钟模式。
    fn mode(&self) -> ClockMode;
}

/// 逻辑时钟：`monotonic_now` 亦由逻辑时间派生（使 deadline 判据在逻辑模式下可虚拟推进）。
#[derive(Debug)]
pub struct LogicalClock {
    base: Instant,
    logical_ms: f64,
}

impl LogicalClock {
    /// 从 0 起的逻辑时钟。
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            logical_ms: 0.0,
        }
    }

    /// 从给定逻辑毫秒起的逻辑时钟。
    #[must_use]
    pub fn from_ms(ms: f64) -> Self {
        Self {
            base: Instant::now(),
            logical_ms: if ms.is_finite() && ms > 0.0 { ms } else { 0.0 },
        }
    }
}

impl Default for LogicalClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for LogicalClock {
    fn monotonic_now(&self) -> Instant {
        self.base + Duration::from_secs_f64(self.logical_ms / 1000.0)
    }

    fn logical_now_ms(&self) -> f64 {
        self.logical_ms
    }

    fn advance(&mut self, delta: Duration) {
        self.logical_ms += delta.as_secs_f64() * 1000.0;
    }

    fn mode(&self) -> ClockMode {
        ClockMode::Logical
    }
}

/// 系统时钟：`monotonic_now` 用墙钟；`logical_now_ms` 自构造起的墙钟毫秒。
#[derive(Debug)]
pub struct SystemClock {
    base: Instant,
}

impl SystemClock {
    /// 建系统时钟（以构造时刻为逻辑零点）。
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }

    fn logical_now_ms(&self) -> f64 {
        self.base.elapsed().as_secs_f64() * 1000.0
    }

    fn advance(&mut self, _delta: Duration) {
        // 系统模式不能虚拟推进墙钟：no-op（`RunLoop` 在系统模式不跳转）。
    }

    fn mode(&self) -> ClockMode {
        ClockMode::System
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, ClockMode, LogicalClock, SystemClock};
    use std::time::{Duration, Instant};

    #[test]
    fn logical_clock_advance_no_sleep() {
        let mut clock = LogicalClock::new();
        assert_eq!(clock.mode(), ClockMode::Logical);
        assert!(clock.logical_now_ms().abs() < 1e-6);
        let wall_before = Instant::now();
        clock.advance(Duration::from_millis(1000));
        let wall_elapsed = wall_before.elapsed();
        assert!(
            (clock.logical_now_ms() - 1000.0).abs() < 1e-6,
            "逻辑应跳到 1000ms"
        );
        assert!(
            wall_elapsed < Duration::from_millis(200),
            "逻辑推进不得 sleep（实测 {wall_elapsed:?}）"
        );
        // monotonic_now 亦由逻辑派生。
        assert_eq!(clock.monotonic_now(), clock.monotonic_now());
    }

    #[test]
    fn system_clock_uses_wall() {
        let clock = SystemClock::new();
        assert_eq!(clock.mode(), ClockMode::System);
        let first = clock.logical_now_ms();
        let second = clock.logical_now_ms();
        assert!(second >= first, "系统逻辑时间单调不减");
        // 系统模式 advance 为 no-op。
        let mut clock = clock;
        let before = clock.logical_now_ms();
        clock.advance(Duration::from_millis(1000));
        let after = clock.logical_now_ms();
        assert!(after >= before);
        assert!(after - before < 500.0, "advance 在系统模式应无效");
    }
}
