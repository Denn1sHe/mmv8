//! storage —— 浏览器存储：local/`sessionStorage`、`IndexedDB`、`CacheStorage` 与分区
//!
//! # `G4-S1-a`（M1 存储 · 基线）
//!
//! 本笔落 **`StorageService`（origin 分区）+ `Storage` 基线接口**（`getItem`/`setItem`/
//! `removeItem`/`clear`/`key`/`length`）与**具名配额**。`S1-b`~`S1-e` 另笔见
//! `docs/design-g4-真身-2026-10-15.md` `§3`。
//!
//! ## 口径（技术方案 `§11`）
//!
//! - **统一 origin 分区**：`StorageService` 按 origin 分桶；跨 origin **不可见**（硬隔离）。
//! - **离线默认内存存储**：本 crate 默认 [`StorageService::new`] 为内存后端（**可显式导入/导出
//!   快照** —— [`StorageSnapshot`]，`G4-S1-b`）。
//! - **持久化 = 快照传导**（`4.1b`）：**换实例后值仍在**由 [`StorageSnapshot`] **显式**传导达成，
//!   **不使用**任何同进程静态 map —— 因此「**未导入的新实例必为空**」（`S1-b` 负对照判据）。
//! - **key 顺序 = 插入序**（更新**保持原位**）；`key(i)` 按该序取。
//! - **配额**：见 [`STORAGE_QUOTA_BYTES`]（**具名策略值**）；`§4.0` 口径：**外部基线 = 申报值，
//!   待 A 路社区实物对账** —— **禁止**从实现里"读回来"当期望（那构成自证）。
//! - **字符串转换**：值经 `String(...)` 归一（JS `ToString`）——**归 shim 侧**（本 crate 收 `&str`）。

#![deny(missing_docs)]

use std::collections::BTreeMap;

/// **存储配额（字节）** —— `G4-S1` 的**具名策略值**（自定基线，`测试友好`规模）。
///
/// **口径**（`§4.0`）：外部 golden 基线 = **本申报值**（**待 A 路社区实物对账**）。
pub const STORAGE_QUOTA_BYTES: usize = 5 * 1024 * 1024;

/// 存储种类（`localStorage` / `sessionStorage`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StorageKind {
    /// `localStorage`（跨会话存活）。
    Local,
    /// `sessionStorage`（随会话）。
    Session,
}

/// 存储操作错误（**具名**；shim 侧映射为对应 DOM 异常）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    /// 超出配额（shim 侧应抛 **`QuotaExceededError`**）。
    QuotaExceeded {
        /// 本配额（字节）。
        quota: usize,
        /// 本次写入后**预计占用**（字节）。
        attempted: usize,
    },
}

/// 有序键值表（**插入序**；更新保持原位；`used_bytes` = `Σ(key + value)` 字节）。
#[derive(Clone, Debug, Default)]
struct OrderedMap {
    /// `(key, value)` 依**插入序**；`key` 唯一。
    items: Vec<(String, String)>,
    /// 当前占用（`key.len() + value.len()` 之和）。
    used_bytes: usize,
}

impl OrderedMap {
    /// 按下标查键。
    fn index_of(&self, key: &str) -> Option<usize> {
        self.items.iter().position(|(k, _)| k == key)
    }

    /// 读值。
    fn get(&self, key: &str) -> Option<&str> {
        self.index_of(key).map(|i| self.items[i].1.as_str())
    }

    /// 读键（按下标）。
    fn key_at(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(|(k, _)| k.as_str())
    }

    /// 写值：**新键追加、既有键原位替换**。
    fn set(&mut self, key: &str, value: &str) {
        if let Some(i) = self.index_of(key) {
            let old = self.items[i].1.len();
            // 原位复用分配（避免 `to_owned` 新分配；clippy `assigning_clones` 口径）。
            self.items[i].1.clear();
            self.items[i].1.push_str(value);
            self.used_bytes = self.used_bytes - old + value.len();
        } else {
            self.items.push((key.to_owned(), value.to_owned()));
            self.used_bytes += key.len() + value.len();
        }
    }

    /// 删键（**不改变其余键的相对顺序**）。
    fn remove(&mut self, key: &str) {
        if let Some(i) = self.index_of(key) {
            let (k, v) = self.items.remove(i);
            self.used_bytes -= k.len() + v.len();
        }
    }

    /// 清空本表。
    fn clear(&mut self) {
        self.items.clear();
        self.used_bytes = 0;
    }
}

/// 快照条目（`(origin, kind, key, value)`）—— **纯数据**，可跨实例 / 跨进程传导。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotEntry {
    /// 所属 origin（分区键）。
    pub origin: String,
    /// 存储种类。
    pub kind: StorageKind,
    /// 键。
    pub key: String,
    /// 值。
    pub value: String,
}

/// 快照**解码**错误（**不静默丢条目**：任何不可解析处都报错）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// 首行魔数 / 版本不符（不是本格式的快照）。
    BadHeader(String),
    /// 字段数不足（每行应为 4 段：种类 / origin / 键 / 值）。
    BadFieldCount {
        /// 出错行号（1 基，含首行魔数行）。
        line: usize,
        /// 实得段数。
        found: usize,
    },
    /// 种类字段非法（应为 `local` / `session`）。
    BadKind(String),
}

/// **存储快照**（`G4-S1-b`：`4.1b` 持久化的载体）。
///
/// 这是**纯数据值对象** —— **不持任何进程内共享状态**；跨实例（乃至跨进程）传导只能**显式**
/// 经 [`StorageService::export_snapshot`] / [`StorageService::import_snapshot`] 完成，
/// 因此「换实例后值仍在」**不依赖**同进程静态 map（`4.1b` 判据）。
///
/// ## 稳定文本格式（零新增依赖）
///
/// ```text
/// iv8-storage/1
/// local<TAB>https://a.example<TAB>key<TAB>value
/// ```
///
/// 字段内 `\` ⇒ `\\`、换行 ⇒ `\n`、制表 ⇒ `\t` 转义 —— **保证任意字符的值往返不丢**
/// （往返测钉住；防「静默丢字符」）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageSnapshot {
    /// 条目序列（**导出按 origin / 桶 / 插入序**；键在 `(origin, kind)` 内唯一）。
    entries: Vec<SnapshotEntry>,
}

/// 快照文本格式的**魔数行**（含版本号）。
const SNAPSHOT_MAGIC: &str = "iv8-storage/1";

impl StorageSnapshot {
    /// 空快照。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 全部条目（导出序）。
    #[must_use]
    pub fn entries(&self) -> &[SnapshotEntry] {
        &self.entries
    }

    /// 条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 追加一条条目（**导出侧**使用；重复 `(origin, kind, key)` 以**最后一条**为准）。
    pub fn push(&mut self, entry: SnapshotEntry) {
        self.entries.push(entry);
    }

    /// **稳定文本编码**（可跨进程；见类型文档）。
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = String::from(SNAPSHOT_MAGIC);
        for entry in &self.entries {
            out.push('\n');
            out.push_str(match entry.kind {
                StorageKind::Local => "local",
                StorageKind::Session => "session",
            });
            out.push('\t');
            push_escaped(&mut out, &entry.origin);
            out.push('\t');
            push_escaped(&mut out, &entry.key);
            out.push('\t');
            push_escaped(&mut out, &entry.value);
        }
        out.push('\n');
        out
    }

    /// **解码**稳定文本；**任一处不可解析 ⇒ 报错**（不静默丢条目）。
    ///
    /// # Errors
    /// 魔数 / 字段数 / 种类非法 ⇒ [`SnapshotError`]。
    pub fn decode(text: &str) -> Result<Self, SnapshotError> {
        let mut lines = text.split('\n');
        let header = lines.next().unwrap_or_default();
        if header != SNAPSHOT_MAGIC {
            return Err(SnapshotError::BadHeader(header.to_owned()));
        }
        let mut snapshot = Self::new();
        for (index, line) in lines.enumerate() {
            if line.is_empty() {
                continue; // 末尾换行
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() != 4 {
                return Err(SnapshotError::BadFieldCount {
                    line: index + 2,
                    found: fields.len(),
                });
            }
            let kind = match fields[0] {
                "local" => StorageKind::Local,
                "session" => StorageKind::Session,
                other => return Err(SnapshotError::BadKind(other.to_owned())),
            };
            snapshot.push(SnapshotEntry {
                origin: unescape(fields[1]),
                kind,
                key: unescape(fields[2]),
                value: unescape(fields[3]),
            });
        }
        Ok(snapshot)
    }
}

/// 写入一个**已转义**的字段（转义 `\` / 换行 / 制表）。
fn push_escaped(out: &mut String, field: &str) {
    for ch in field.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
}

/// 还原一个被 [`push_escaped`] 转义的字段。
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            // 转义符后为其它字符 / 行尾 ⇒ **原样保留**（不静默吞字符）。
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// **origin 分区**（一个 origin 下的 `local` / `session` 两桶）。
#[derive(Clone, Debug, Default)]
struct Partition {
    /// `localStorage` 桶。
    local: OrderedMap,
    /// `sessionStorage` 桶。
    session: OrderedMap,
}

/// **存储服务**（`§11`：`StorageService` 管 origin、session 与持久化策略）。
#[derive(Clone, Debug, Default)]
pub struct StorageService {
    /// origin ⇒ 分区（**跨 origin 硬隔离**）。
    partitions: BTreeMap<String, Partition>,
}

impl StorageService {
    /// 新建（**离线默认：内存后端**）。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 取某 origin 某种类的**存储句柄**。
    #[must_use]
    pub fn storage(&mut self, origin: &str, kind: StorageKind) -> Storage<'_> {
        Storage {
            origin: origin.to_owned(),
            kind,
            service: self,
        }
    }

    /// 某 origin 分区的**已用字节**（两桶合计；只读）。
    #[must_use]
    pub fn used_bytes(&self, origin: &str) -> usize {
        self.partitions
            .get(origin)
            .map_or(0, |p| p.local.used_bytes + p.session.used_bytes)
    }

    /// 某 origin 指定**桶**的已用字节（`G4-⑥` F9：逐桶取数，勿用 origin 合计当代理）。
    ///
    /// `used_bytes` 是**两桶合计** —— 把它当"某桶已填满"的代理会在另一桶有数据时失真。
    #[must_use]
    pub fn bucket_used_bytes(&self, origin: &str, kind: StorageKind) -> usize {
        self.partitions.get(origin).map_or(0, |p| match kind {
            StorageKind::Local => p.local.used_bytes,
            StorageKind::Session => p.session.used_bytes,
        })
    }

    /// **导出快照**（`G4-S1-b`）：纯数据副本，**不含**任何进程内共享状态。
    ///
    /// 序：`origin`（字典序）⇒ `local` 先于 `session` ⇒ 桶内**插入序**。
    #[must_use]
    pub fn export_snapshot(&self) -> StorageSnapshot {
        let mut snapshot = StorageSnapshot::new();
        for (origin, partition) in &self.partitions {
            for (kind, bucket) in [
                (StorageKind::Local, &partition.local),
                (StorageKind::Session, &partition.session),
            ] {
                for (key, value) in &bucket.items {
                    snapshot.push(SnapshotEntry {
                        origin: origin.clone(),
                        kind,
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }
        }
        snapshot
    }

    /// **导入快照**（`G4-S1-b`）：按 `(origin, kind, key)` **覆盖**，其余键保留。
    ///
    /// **先算后写**（与 [`Storage::set_item`] 同律）：若任一桶导入后**超配额** ⇒
    /// **整批拒绝**（返回 [`StorageError::QuotaExceeded`]），**不留半态**。
    ///
    /// # Errors
    /// 导入后任一桶超配额 ⇒ [`StorageError::QuotaExceeded`]。
    pub fn import_snapshot(&mut self, snapshot: &StorageSnapshot) -> Result<(), StorageError> {
        // 同一 `(origin, kind, key)` **以最后一条为准**；但**顺序取首现位置** ⇒ 导入后
        // **插入序与快照一致**（序若被排序改写 = 静默变形，故此处显式保序）。
        let mut order: Vec<(String, StorageKind, String)> = Vec::new();
        let mut planned: BTreeMap<(String, StorageKind, String), String> = BTreeMap::new();
        for entry in snapshot.entries() {
            let triple = (entry.origin.clone(), entry.kind, entry.key.clone());
            if !planned.contains_key(&triple) {
                order.push(triple.clone());
            }
            planned.insert(triple, entry.value.clone());
        }
        // ① 先算：按桶聚合「导入后占用」（既有键被覆盖 ⇒ 扣其旧成本）。
        let mut projected: BTreeMap<(String, StorageKind), usize> = BTreeMap::new();
        for (origin, partition) in &self.partitions {
            projected.insert(
                (origin.clone(), StorageKind::Local),
                partition.local.used_bytes,
            );
            projected.insert(
                (origin.clone(), StorageKind::Session),
                partition.session.used_bytes,
            );
        }
        for (origin, kind, key) in &order {
            let value = &planned[&(origin.clone(), *kind, key.clone())];
            let old = self
                .bucket(origin, *kind)
                .and_then(|b| b.get(key))
                .map_or(0, |v| key.len() + v.len());
            let slot = projected.entry((origin.clone(), *kind)).or_insert(0);
            *slot = *slot - old + key.len() + value.len();
            if *slot > STORAGE_QUOTA_BYTES {
                return Err(StorageError::QuotaExceeded {
                    quota: STORAGE_QUOTA_BYTES,
                    attempted: *slot,
                });
            }
        }
        // ② 后写：**按快照序**逐条落桶（`set` 保证既有键**原位替换**、新键追加）。
        for (origin, kind, key) in &order {
            let value = planned[&(origin.clone(), *kind, key.clone())].clone();
            self.bucket_mut(origin, *kind).set(key, &value);
        }
        Ok(())
    }

    /// 由快照**新建**一个服务实例（`4.1b`：**换实例后值仍在**的入口）。
    ///
    /// # Errors
    /// 同 [`StorageService::import_snapshot`]（超配额 ⇒ 整批拒绝）。
    pub fn from_snapshot(snapshot: &StorageSnapshot) -> Result<Self, StorageError> {
        let mut service = Self::new();
        service.import_snapshot(snapshot)?;
        Ok(service)
    }

    /// 桶可变引用（不存在则建）。
    fn bucket_mut(&mut self, origin: &str, kind: StorageKind) -> &mut OrderedMap {
        let partition = self.partitions.entry(origin.to_owned()).or_default();
        match kind {
            StorageKind::Local => &mut partition.local,
            StorageKind::Session => &mut partition.session,
        }
    }

    /// 桶只读引用。
    fn bucket(&self, origin: &str, kind: StorageKind) -> Option<&OrderedMap> {
        self.partitions.get(origin).map(|p| match kind {
            StorageKind::Local => &p.local,
            StorageKind::Session => &p.session,
        })
    }
}

/// **`Storage` 句柄**（绑定 `(origin, kind)`；`§11`：`localStorage`/`sessionStorage` 语义）。
///
/// 写操作**先过配额**（[`STORAGE_QUOTA_BYTES`]）；超出 ⇒ [`StorageError::QuotaExceeded`]（**不写入**）。
#[derive(Debug)]
pub struct Storage<'a> {
    /// 所属 origin（分区键）。
    origin: String,
    /// 种类。
    kind: StorageKind,
    /// 所属服务。
    service: &'a mut StorageService,
}

impl Storage<'_> {
    /// 所属 origin。
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// 种类。
    #[must_use]
    pub fn kind(&self) -> StorageKind {
        self.kind
    }

    /// `length`。
    #[must_use]
    pub fn length(&self) -> usize {
        self.service
            .bucket(&self.origin, self.kind)
            .map_or(0, |b| b.items.len())
    }

    /// `getItem(key)`（缺省 ⇒ `None`，对应 JS `null`）。
    #[must_use]
    pub fn get_item(&mut self, key: &str) -> Option<String> {
        self.service
            .bucket(&self.origin, self.kind)
            .and_then(|b| b.get(key))
            .map(str::to_owned)
    }

    /// `key(index)`（**按插入序**；越界 ⇒ `None`，对应 JS `null`）。
    #[must_use]
    pub fn key(&self, index: usize) -> Option<String> {
        self.service
            .bucket(&self.origin, self.kind)
            .and_then(|b| b.key_at(index))
            .map(str::to_owned)
    }

    /// `setItem(key, value)` —— **先算后写**（超配额 ⇒ 具名错误，**无半态**）。
    ///
    /// # Errors
    /// 超出配额 ⇒ [`StorageError::QuotaExceeded`]。
    pub fn set_item(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        let (current, old_value) = {
            let bucket = self.service.bucket(&self.origin, self.kind);
            (
                bucket.map_or(0, |b| b.used_bytes),
                bucket.and_then(|b| b.get(key)).map(str::to_owned),
            )
        };
        let old_cost = old_value.map_or(0, |v| key.len() + v.len());
        let attempted = current - old_cost + key.len() + value.len();
        if attempted > STORAGE_QUOTA_BYTES {
            return Err(StorageError::QuotaExceeded {
                quota: STORAGE_QUOTA_BYTES,
                attempted,
            });
        }
        self.service
            .bucket_mut(&self.origin, self.kind)
            .set(key, value);
        Ok(())
    }

    /// `removeItem(key)`（缺省为**空操作**，与 JS 一致）。
    pub fn remove_item(&mut self, key: &str) {
        self.service.bucket_mut(&self.origin, self.kind).remove(key);
    }

    /// `clear()`（**仅清本 `(origin, kind)` 桶**）。
    pub fn clear(&mut self) {
        self.service.bucket_mut(&self.origin, self.kind).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        STORAGE_QUOTA_BYTES, SnapshotEntry, SnapshotError, StorageError, StorageKind,
        StorageService, StorageSnapshot,
    };

    #[test]
    fn storage_roundtrip_and_key_order() {
        // `4.1`（前半）：**读写回环** + **key 顺序 = 插入序**（更新**保持原位**）。
        let mut service = StorageService::new();
        let mut local = service.storage("https://a.example", StorageKind::Local);
        local.set_item("b", "1").expect("set");
        local.set_item("a", "2").expect("set");
        local.set_item("c", "3").expect("set");
        local.set_item("b", "9").expect("update");

        assert_eq!(local.length(), 3);
        assert_eq!(local.get_item("b").as_deref(), Some("9"));
        assert_eq!(local.key(0).as_deref(), Some("b"), "更新应保持原位");
        assert_eq!(local.key(1).as_deref(), Some("a"));
        assert_eq!(local.key(2).as_deref(), Some("c"));
        assert_eq!(local.key(3), None, "越界 ⇒ None（JS null）");
        assert_eq!(local.get_item("missing"), None);

        local.remove_item("a");
        assert_eq!(local.length(), 2);
        assert_eq!(local.key(0).as_deref(), Some("b"));
        assert_eq!(local.key(1).as_deref(), Some("c"));
    }

    #[test]
    fn origin_partition_is_hard_isolated() {
        // `4.1` **负对照**：**不同 origin 不可见**（若共用默认分区 ⇒ 本测红）。
        let mut service = StorageService::new();
        service
            .storage("https://a.example", StorageKind::Local)
            .set_item("k", "v-a")
            .expect("set");
        let mut other = service.storage("https://b.example", StorageKind::Local);
        assert_eq!(other.get_item("k"), None, "跨 origin 必须不可见");
        assert_eq!(other.length(), 0);
        let mut back = service.storage("https://a.example", StorageKind::Local);
        assert_eq!(back.get_item("k").as_deref(), Some("v-a"));
    }

    #[test]
    fn local_and_session_buckets_are_separate() {
        // 同 origin 的 `local` 与 `session` **互不相扰**；`clear` 亦然。
        let mut service = StorageService::new();
        service
            .storage("https://a.example", StorageKind::Local)
            .set_item("k", "local-v")
            .expect("set");
        service
            .storage("https://a.example", StorageKind::Session)
            .set_item("k", "session-v")
            .expect("set");
        service
            .storage("https://a.example", StorageKind::Session)
            .clear();
        let mut local = service.storage("https://a.example", StorageKind::Local);
        assert_eq!(
            local.get_item("k").as_deref(),
            Some("local-v"),
            "clear 只清本桶"
        );
        assert_eq!(local.length(), 1);
        let session = service.storage("https://a.example", StorageKind::Session);
        assert_eq!(session.length(), 0);
    }

    #[test]
    fn quota_exceeded_on_n_plus_one_write() {
        // `4.1`（后半）：**第 N+1 次写抛具名错误**。
        // **负对照**：撤配额检查（去掉 `attempted > QUOTA` 分支）⇒ 本测红。
        let mut service = StorageService::new();
        let mut local = service.storage("https://a.example", StorageKind::Local);

        // **每项恰占 1024 B**（键 8 = `"kkkk"+4 位序号` + 值 1016）⇒ `fits = QUOTA / 1024` 精确填满。
        let key = "kkkk";
        let chunk = "x".repeat(1016);
        let fits = STORAGE_QUOTA_BYTES / 1024;
        for i in 0..fits {
            let single = format!("{key}{i:0>4}");
            local
                .set_item(&single, &chunk)
                .unwrap_or_else(|e| panic!("第 {i} 次写不应超配额（{e:?}）"));
        }
        let err = local
            .set_item("overflow", "y")
            .expect_err("第 N+1 次写必须抛具名错误");
        match err {
            StorageError::QuotaExceeded { quota, attempted } => {
                assert_eq!(quota, STORAGE_QUOTA_BYTES);
                assert!(attempted > quota, "attempted 应超配额");
            }
        }
        assert_eq!(local.get_item("overflow"), None, "超配额不得留下半态");
    }

    #[test]
    fn quota_is_enforced_per_bucket() {
        // `⑥`：**申报「per-bucket」的行为证据** —— `local` 桶填满后，同 origin 的 `session` 桶仍可写。
        // **负对照**：若配额改成 **按 origin 合计**（非 per-bucket）⇒ 本测红（`session` 写被拒）。
        let mut service = StorageService::new();
        let origin = "https://a.example";
        let key = "kkkk";
        let chunk = "x".repeat(1016);
        let fits = STORAGE_QUOTA_BYTES / 1024;
        {
            let mut local = service.storage(origin, StorageKind::Local);
            for i in 0..fits {
                let single = format!("{key}{i:0>4}");
                local
                    .set_item(&single, &chunk)
                    .unwrap_or_else(|e| panic!("第 {i} 次写不应超配额（{e:?}）"));
            }
        }
        assert_eq!(
            service.bucket_used_bytes(origin, StorageKind::Local),
            STORAGE_QUOTA_BYTES,
            "local 桶应恰被填满（**逐桶**取数：origin 合计在 session 有数据时会失真）"
        );
        assert_eq!(
            service.bucket_used_bytes(origin, StorageKind::Session),
            0,
            "此时 session 桶应为空（证明上面的断言确实是 local 桶，而非合计巧合）"
        );
        // 同 origin 的 `session` 桶：**独立配额** ⇒ 仍可写（per-bucket 判据）。
        let mut session = service.storage(origin, StorageKind::Session);
        session
            .set_item("s", "v")
            .expect("session 桶应仍可写（per-bucket）");
        assert_eq!(session.get_item("s").as_deref(), Some("v"));
        assert_eq!(
            service.bucket_used_bytes(origin, StorageKind::Local),
            STORAGE_QUOTA_BYTES,
            "写 session 不得改动 local 桶已用字节（两桶独立）"
        );
    }

    #[test]
    fn snapshot_survives_instance_swap() {
        // `4.1b`：**换存储实例后值仍在** —— 经**显式**快照传导（非同进程静态 map）。
        let mut first = StorageService::new();
        {
            let mut local = first.storage("https://a.example", StorageKind::Local);
            local.set_item("b", "1").expect("set b");
            local.set_item("a", "2").expect("set a");
        }
        {
            let mut session = first.storage("https://a.example", StorageKind::Session);
            session.set_item("s", "3").expect("set s");
        }
        let snapshot = first.export_snapshot();
        assert_eq!(snapshot.len(), 3, "导出应含 local 2 + session 1");

        // **另一个实例**（新 `StorageService`）导入后值仍在。
        let mut second = StorageService::from_snapshot(&snapshot).expect("import");
        let mut local = second.storage("https://a.example", StorageKind::Local);
        assert_eq!(local.get_item("b").as_deref(), Some("1"));
        assert_eq!(local.get_item("a").as_deref(), Some("2"));
        assert_eq!(local.key(0).as_deref(), Some("b"), "插入序应随之传导");
        assert_eq!(local.key(1).as_deref(), Some("a"));
        let mut session = second.storage("https://a.example", StorageKind::Session);
        assert_eq!(session.get_item("s").as_deref(), Some("3"));
    }

    #[test]
    fn fresh_instance_without_import_is_empty() {
        // `4.1b` **负对照**：**未导入**的新实例**必须为空**。
        // 若实现偷偷用「同进程静态 map」共享状态 ⇒ 本测红（这正是判据要排除的形态）。
        let mut first = StorageService::new();
        first
            .storage("https://a.example", StorageKind::Local)
            .set_item("k", "v")
            .expect("set");
        let mut other = StorageService::new();
        assert_eq!(
            other
                .storage("https://a.example", StorageKind::Local)
                .get_item("k"),
            None,
            "新实例未经导入不得看到别处的值（禁同进程静态 map）"
        );
        assert_eq!(other.export_snapshot().len(), 0);
    }

    #[test]
    fn snapshot_text_roundtrip_keeps_odd_characters() {
        // 稳定文本往返：**含换行 / 制表 / 反斜杠 / 空值** 的值**不丢字符**（防静默丢字符）。
        let mut snapshot = StorageSnapshot::new();
        snapshot.push(SnapshotEntry {
            origin: "https://a.example".to_owned(),
            kind: StorageKind::Local,
            key: "k\twith\ttabs".to_owned(),
            value: "line1\nline2\t\\end\\".to_owned(),
        });
        snapshot.push(SnapshotEntry {
            origin: "https://a.example".to_owned(),
            kind: StorageKind::Session,
            key: "empty".to_owned(),
            value: String::new(),
        });
        let text = snapshot.encode();
        let decoded = StorageSnapshot::decode(&text).expect("解码应成功");
        assert_eq!(decoded, snapshot, "往返应逐字段相等（含转义字符）");
        // 落回实例后取值仍原样（端到端）。
        let mut service = StorageService::from_snapshot(&decoded).expect("import");
        let mut local = service.storage("https://a.example", StorageKind::Local);
        assert_eq!(
            local.get_item("k\twith\ttabs").as_deref(),
            Some("line1\nline2\t\\end\\")
        );
        assert_eq!(
            service
                .storage("https://a.example", StorageKind::Session)
                .get_item("empty")
                .as_deref(),
            Some("")
        );
    }

    #[test]
    fn snapshot_decode_rejects_corrupted_text() {
        // 负对照：损坏输入 ⇒ **报错**（不静默丢条目、不静默当空快照）。
        assert!(matches!(
            StorageSnapshot::decode("garbage"),
            Err(SnapshotError::BadHeader(_))
        ));
        assert!(matches!(
            StorageSnapshot::decode("iv8-storage/1\nlocal\thttps://a.example\tonly-two"),
            Err(SnapshotError::BadFieldCount { line: 2, found: 3 })
        ));
        assert!(matches!(
            StorageSnapshot::decode("iv8-storage/1\nweird\thttps://a.example\tk\tv"),
            Err(SnapshotError::BadKind(kind)) if kind == "weird"
        ));
    }

    #[test]
    fn snapshot_import_overwrites_same_key_in_place() {
        // 同 `(origin, kind, key)` **覆盖**且**保持原位**；其余键不动。
        let mut service = StorageService::new();
        {
            let mut local = service.storage("https://a.example", StorageKind::Local);
            local.set_item("a", "old-a").expect("set a");
            local.set_item("b", "keep-b").expect("set b");
        }
        let mut patch = StorageSnapshot::new();
        patch.push(SnapshotEntry {
            origin: "https://a.example".to_owned(),
            kind: StorageKind::Local,
            key: "a".to_owned(),
            value: "new-a".to_owned(),
        });
        service.import_snapshot(&patch).expect("import");
        let mut local = service.storage("https://a.example", StorageKind::Local);
        assert_eq!(local.get_item("a").as_deref(), Some("new-a"));
        assert_eq!(local.key(0).as_deref(), Some("a"), "覆盖应保持原位");
        assert_eq!(local.get_item("b").as_deref(), Some("keep-b"));
    }

    #[test]
    fn snapshot_import_rejects_over_quota_without_half_state() {
        // 负对照：导入**超配额**快照 ⇒ **整批拒绝 + 无半态**
        // （若实现「边写边判」⇒ 会留下部分键 ⇒ 本测红）。
        let mut snapshot = StorageSnapshot::new();
        snapshot.push(SnapshotEntry {
            origin: "https://a.example".to_owned(),
            kind: StorageKind::Local,
            key: "ok".to_owned(),
            value: "v".repeat(1024),
        });
        snapshot.push(SnapshotEntry {
            origin: "https://a.example".to_owned(),
            kind: StorageKind::Local,
            key: "huge".to_owned(),
            value: "v".repeat(STORAGE_QUOTA_BYTES),
        });
        let mut service = StorageService::new();
        let err = service
            .import_snapshot(&snapshot)
            .expect_err("超配额导入应被拒");
        assert!(matches!(err, StorageError::QuotaExceeded { .. }));
        assert_eq!(
            service.used_bytes("https://a.example"),
            0,
            "拒绝后不得留半态"
        );
        assert_eq!(
            service
                .storage("https://a.example", StorageKind::Local)
                .get_item("ok"),
            None
        );
    }

    #[test]
    fn quota_allows_shrinking_update() {
        // 边界正例：**缩短值的原位更新** 不因重算而误判超配额。
        let mut service = StorageService::new();
        let mut local = service.storage("https://a.example", StorageKind::Local);
        let big = "y".repeat(4096);
        local.set_item("k", &big).expect("首次写");
        local.set_item("k", "small").expect("缩短值应允许");
        assert_eq!(local.get_item("k").as_deref(), Some("small"));
    }
}
