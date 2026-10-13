//! navigation —— 页面生命周期、导航协调器与**流式解析协调**（含 `document.write` 插入点）。
//!
//! 状态：**S8 落地中**（施工图 §2.1，ADR-0006 / ADR-0017）。
//! - [`parser::ParserHost`]：自建的可暂停 / 恢复解析驱动（**不用**一次性 `parse_document`）。
//! - [`sink::DomSink`]：`markup5ever::TreeSink` 落到 `web-dom`（`Handle = NodeId`）。
//! - 本 crate **零 unsafe**（html5ever 为安全 Rust API）。
//!
//! 后续步骤（S9/S10/S11/S12/S13）将补 `document_write` / `controller` / `binding` / `input` / `page`。

pub mod binding;
pub mod controller;
pub mod document_write;
pub mod error;
pub mod input;
pub mod page;
pub mod parser;
pub mod sink;
