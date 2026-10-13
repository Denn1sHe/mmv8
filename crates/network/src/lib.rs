//! network —— 网络语义层最小面（G2：`OfflineBundle` 先行的 fetch 路径）。
//!
//! 状态：**G2 落地**（施工图 §1.2 / §2.5 / §3.5，ADR-0018·D3）。
//! - `RequestSpec.epoch: policy::NavigationEpoch`（类型归 `policy` ⇒ 无 `navigation ↔ network` 环）。
//! - 命中 [`OfflineBundle`](offline::OfflineBundle) ⇒ `Ready`；未命中 ⇒ `Pending`（等待宿主注入）。
//! - `complete`：幂等 / `abort_epoch` 过期语义（§2.5）。
//! - **本 crate 零 `engine-v8` 依赖**（`HandleLease` 表在 `navigation`）。
//! - 本 crate **零 unsafe**。

pub mod error;
pub mod log;
pub mod offline;
pub mod request;
pub mod response;
pub mod service;
pub mod transport;

pub use error::{NetworkError, NetworkErrorKind};
pub use log::{NetLogEntry, NetLogPhase};
pub use offline::OfflineBundle;
pub use request::{Credentials, RequestId, RequestSpec};
pub use response::{Response, ResponseKind};
pub use service::{FetchOutcome, NetworkService};
pub use transport::Transport;
