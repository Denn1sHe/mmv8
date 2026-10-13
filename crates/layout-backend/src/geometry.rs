//! 几何查询类型与坐标口径（`layout-backend::geometry`，施工图 §1.2 / §1.3）。
//!
//! G3 语义（D5）：坐标一律 **CSS px**；
//! - `border_box` = **视口坐标**的 border box（`getBoundingClientRect` 口径，G3 视口原点 `(0,0)`、无滚动）；
//! - `offset` = border box 宽高 **四舍五入取整**（`offsetWidth`/`offsetHeight`）；
//! - `client` = **padding box**（不含 border；G3 无滚动条 ⇒ 不扣滚动条宽）。

/// 盒尺寸（CSS px）。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Size {
    /// 宽。
    pub width: f32,
    /// 高。
    pub height: f32,
}

impl Size {
    /// 构造。
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// 矩形（CSS px）。`getBoundingClientRect` 语义：**视口坐标**、border box。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    /// 左边（视口坐标）。
    pub x: f32,
    /// 上边（视口坐标）。
    pub y: f32,
    /// 宽。
    pub width: f32,
    /// 高。
    pub height: f32,
}

impl Rect {
    /// 构造。
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 右边界（`x + width`）。
    #[must_use]
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// 下边界（`y + height`）。
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// 一个节点的可读几何（G3 最小面）。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct NodeGeometry {
    /// `getBoundingClientRect`：**视口坐标** border box（非渲染 ⇒ 全 0）。
    pub border_box: Rect,
    /// `offsetWidth`/`offsetHeight`：border box **取整**。
    pub offset: Size,
    /// `clientWidth`/`clientHeight`：padding box（G3 无滚动条 ⇒ 不扣）。
    pub client: Size,
}

#[cfg(test)]
mod tests {
    use super::{NodeGeometry, Rect, Size};

    #[test]
    fn geometry_defaults_and_accessors() {
        assert_eq!(Size::default(), Size::new(0.0, 0.0));
        assert_eq!(NodeGeometry::default().offset, Size::default());
        let rect = Rect::new(1.0, 2.0, 10.0, 20.0);
        assert!((rect.right() - 11.0).abs() < f32::EPSILON);
        assert!((rect.bottom() - 22.0).abs() < f32::EPSILON);
        assert_eq!(Rect::default(), Rect::new(0.0, 0.0, 0.0, 0.0));
    }
}
