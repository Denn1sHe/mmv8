//! policy —— origin、安全上下文、页面策略与**导航代次**（G2 最小面）。
//!
//! 状态：**G2 落地**（施工图 §1.2 / §2.4，ADR-0017·D1 / ADR-0018·D6）。
//! - [`Origin`] 由 `url` crate 解析并规范化（default-port 折叠、大小写、query/fragment 剥离）。
//! - [`NavigationEpoch`] 的**定义点落本 crate**（`navigation`/`network` 均依赖 `policy` ⇒ 无环）。
//! - 本 crate **零 unsafe**。

pub mod context;
pub mod epoch;
pub mod error;
pub mod origin;

pub use context::{PagePolicy, SecurityContext};
pub use epoch::NavigationEpoch;
pub use error::{PolicyError, PolicyErrorKind};
pub use origin::Origin;
