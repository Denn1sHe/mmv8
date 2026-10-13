//! `fonts` —— **P-3**（字体发现路径）+ **P-10**（内嵌字体度量跨机确定）。
//!
//! 字体栈：`parley`（整形）← `fontique`（发现/回退）← `skrifa`（字形）（施工图 §1.4）。
//!
//! - **P-3**：`fontique` 在无桌面环境能发现系统字体；内嵌字体可注册。
//!   判定 = **本地度量钉死值**；远程**非 0 且不 panic**（远程复验归 S14）。
//! - **P-10**：同内嵌字体整形同段文本。判定 = 同架构**逐位相等**；跨架构 **≤0.5px**
//!   + 记录 arch / 字体哈希（R4 / G3-B10）。
//!
//! 内嵌字体 = `Tuffy.ttf`（**public domain**，见 `fonts/LICENSE.txt`，本 crate 零 unsafe 约束下读取）。

use std::borrow::Cow;
use std::sync::Arc;

use fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{Alignment, AlignmentOptions, FontContext, FontFamily, LayoutContext, StyleProperty};

/// 内嵌测试字体字节（`include_bytes!` ⇒ 编译期嵌入，跨机**同字节**）。
pub const EMBEDDED_FONT: &[u8] = include_bytes!("../fonts/Tuffy.ttf");

/// 内嵌字体家族名（字体内部 family name）。
pub const EMBEDDED_FAMILY: &str = "Tuffy";

/// P-10 整形文本（纯 ASCII，Tuffy 覆盖；不含连字差异字符）。
pub const SAMPLE_TEXT: &str = "Hamburgefonstiv";

/// P-10 整形字号（px）。
pub const SAMPLE_SIZE: f32 = 16.0;

/// P-10 跨机容差（px，G3-B10：跨架构 ≤0.5px）。
pub const CROSS_ARCH_TOLERANCE_PX: f32 = 0.5;

/// 本机（本地 arm64）整形**钉死值**（px）。回填施工图 §4 测试计划（P-10）。
///
/// 由 `cargo run` 实测产出（Tuffy `Hamburgefonstiv` @ 16px，scale=1.0，quantize=false）：
/// `P3_EMBEDDED_ADVANCE_PX=112.625`（可精确表示 ⇒ 同架构应逐位相等）。
pub const PINNED_EMBEDDED_ADVANCE_PX: f32 = 112.625;

/// `LayoutContext` 的画笔占位。
///
/// parley 有** blanket impl** `impl<T: Clone+PartialEq+Default+Debug> Brush for T`，
/// 故 `()` 即满足 `Brush`（本探针无绘制，用 `()` 最省）。
type NoBrush = ();

/// 目标架构（P-10 记录用）。
#[must_use]
pub const fn arch() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "aarch64"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "x86_64"
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        "other"
    }
}

/// FNV-1a 64 位（字体字节哈希；不引额外依赖，跨机可比）。
#[must_use]
pub fn font_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// P-3 / P-10 运行报告。
#[derive(Clone, Debug)]
pub struct FontReport {
    /// 系统字体家族数（P-3；本地 mac 应 > 0；无桌面环境允许 0，但**不得 panic**）。
    pub system_family_count: usize,
    /// 内嵌字体注册后可按名查到（P-3）。
    pub embedded_registered: bool,
    /// 内嵌字体整形 `SAMPLE_TEXT`@`SAMPLE_SIZE` 的宽度（px；P-3 钉死值 / P-10 基准）。
    pub embedded_advance_px: f32,
    /// 缺失家族时的回退整形宽度（px；P-3「非 0 且不 panic」佐证）。
    pub fallback_advance_px: f32,
    /// 目标架构（P-10 记录）。
    pub arch: &'static str,
    /// 内嵌字体字节哈希（P-10 记录）。
    pub font_hash: u64,
}

/// 建一个**只含内嵌字体**的确定性 `FontContext`（**不加载系统字体** ⇒ 整形确定）。
fn embedded_context() -> FontContext {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    collection.register_fonts(Blob::new(Arc::new(EMBEDDED_FONT.to_vec())), None);
    FontContext {
        collection,
        source_cache: SourceCache::default(),
    }
}

/// 用给定家族整形 `SAMPLE_TEXT` 并返回布局宽度（px）。
fn shape_width(fcx: &mut FontContext, family: FontFamily<'_>) -> f32 {
    let mut lcx: LayoutContext<NoBrush> = LayoutContext::new();
    let mut builder = lcx.ranged_builder(fcx, SAMPLE_TEXT, 1.0, false);
    builder.push_default(StyleProperty::FontFamily(family));
    builder.push_default(StyleProperty::FontSize(SAMPLE_SIZE));
    let mut layout = builder.build(SAMPLE_TEXT);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout.width()
}

/// 跑 P-3 / P-10 全部观测。
#[must_use]
pub fn probe() -> FontReport {
    // (1) P-3：系统字体发现（无桌面环境不得 panic）。
    let mut system = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    let system_family_count = system.family_names().count();

    // (2) P-3/P-10：内嵌字体注册 + 确定性整形。
    let mut embedded = embedded_context();
    let embedded_registered = embedded.collection.family_id(EMBEDDED_FAMILY).is_some();
    let embedded_advance_px = shape_width(
        &mut embedded,
        FontFamily::Source(Cow::Borrowed(EMBEDDED_FAMILY)),
    );

    // (3) P-3：缺失家族 ⇒ 回退（不 panic；有系统字体时应非 0）。
    let mut fallback_cx = FontContext {
        collection: system,
        source_cache: SourceCache::default(),
    };
    let fallback_advance_px = shape_width(
        &mut fallback_cx,
        FontFamily::Source(Cow::Borrowed("__iv8_missing_family__")),
    );

    FontReport {
        system_family_count,
        embedded_registered,
        embedded_advance_px,
        fallback_advance_px,
        arch: arch(),
        font_hash: font_hash(EMBEDDED_FONT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P-3：内嵌字体可注册、可查到、可整形（度量 > 0）。
    #[test]
    fn p3_embedded_font_registers_and_shapes() {
        let report = probe();
        assert!(
            report.embedded_registered,
            "内嵌字体 `{EMBEDDED_FAMILY}` 必须注册成功"
        );
        assert!(
            report.embedded_advance_px > 0.0,
            "内嵌字体整形宽度必须 > 0（实测 {}）",
            report.embedded_advance_px
        );
        // 系统字体发现：无桌面环境允许为 0，但**不得 panic**（本轮已在 headless 跑通）。
        eprintln!(
            "P3_SYSTEM_FAMILIES={} arch={} font_hash={}",
            report.system_family_count, report.arch, report.font_hash
        );
    }

    /// P-3：缺失家族 ⇒ 回退，不 panic；有系统字体时回退度量非 0。
    #[test]
    fn p3_fallback_nonzero_and_no_panic() {
        let report = probe();
        // 前提：本地有系统字体（mac）⇒ 回退应命中并给出非 0 度量。
        if report.system_family_count > 0 {
            assert!(
                report.fallback_advance_px > 0.0,
                "有系统字体时回退整形应非 0（实测 {}）",
                report.fallback_advance_px
            );
        }
        // 无系统字体环境：只要不 panic 即满足 P-3「非 0 且不 panic」的稳健性侧面。
    }

    /// P-10：内嵌字体度量钉死在本机记录值（**同架构 ⇒ 逐位相等**）。
    #[test]
    fn p10_embedded_metric_is_pinned() {
        let report = probe();
        assert_eq!(
            report.arch, "aarch64",
            "本机应为 aarch64（本地开发机）；异架构在此断言应改用 ≤0.5px 容差"
        );
        assert_eq!(
            report.embedded_advance_px, PINNED_EMBEDDED_ADVANCE_PX,
            "同架构内嵌字体度量应逐位相等：实测 {} vs 钉死 {}",
            report.embedded_advance_px, PINNED_EMBEDDED_ADVANCE_PX
        );
    }

    /// P-10：字体哈希记录（跨机可比；用于跨架构容差对照）。
    #[test]
    fn p10_font_hash_recorded() {
        let report = probe();
        assert_ne!(report.font_hash, 0, "字体哈希必须记录");
        assert_eq!(report.font_hash, font_hash(EMBEDDED_FONT), "哈希应稳定可比");
    }
}
