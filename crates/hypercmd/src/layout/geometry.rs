use crate::Error;
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LogicalRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(super) width: u16,
    pub(super) height: u16,
}

impl LogicalRect {
    pub(super) fn translated(self, x: i32, y: i32) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
            ..self
        }
    }
    pub(super) fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + i32::from(self.width)).min(other.x + i32::from(other.width));
        let bottom = (self.y + i32::from(self.height)).min(other.y + i32::from(other.height));
        Self {
            x,
            y,
            width: (right - x).max(0) as u16,
            height: (bottom - y).max(0) as u16,
        }
    }
    pub(super) fn empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl From<LogicalRect> for Rect {
    fn from(
        LogicalRect {
            x,
            y,
            width,
            height,
        }: LogicalRect,
    ) -> Self {
        let right = (x + i32::from(width)).clamp(0, i32::from(u16::MAX));
        let bottom = (y + i32::from(height)).clamp(0, i32::from(u16::MAX));
        let x = x.clamp(0, i32::from(u16::MAX));
        let y = y.clamp(0, i32::from(u16::MAX));
        Rect::new(
            x as u16,
            y as u16,
            (right - x).max(0) as u16,
            (bottom - y).max(0) as u16,
        )
    }
}
impl From<Rect> for LogicalRect {
    fn from(
        Rect {
            x,
            y,
            width,
            height,
        }: Rect,
    ) -> Self {
        Self {
            x: i32::from(x),
            y: i32::from(y),
            width,
            height,
        }
    }
}

pub(super) struct Boxes {
    pub(super) border: LogicalRect,
    pub(super) content: LogicalRect,
    pub(super) extent: (u16, u16),
}

// `extent` is how far the content overflows the content box.
pub(super) fn boxes(layout: &taffy::Layout, origin: (i32, i32)) -> Result<Boxes, Error> {
    let x = origin.0 + layout.location.x.round() as i32;
    let y = origin.1 + layout.location.y.round() as i32;
    let (width, height) = (cells(layout.size.width)?, cells(layout.size.height)?);
    let left = cells(layout.padding.left + layout.border.left)?;
    let top = cells(layout.padding.top + layout.border.top)?;
    let content = LogicalRect {
        x: x + i32::from(left),
        y: y + i32::from(top),
        width: width
            .saturating_sub(left)
            .saturating_sub(cells(layout.padding.right + layout.border.right)?),
        height: height
            .saturating_sub(top)
            .saturating_sub(cells(layout.padding.bottom + layout.border.bottom)?),
    };
    let extent = (
        cells(layout.scroll_width())?,
        cells(layout.scroll_height())?,
    );
    Ok(Boxes {
        border: LogicalRect {
            x,
            y,
            width,
            height,
        },
        content,
        extent,
    })
}

fn cells(value: f32) -> Result<u16, Error> {
    (value.is_finite() && value >= 0.0 && value <= f32::from(u16::MAX))
        .then(|| value.round() as u16)
        .ok_or_else(|| Error::template("layout exceeds the supported terminal coordinate range"))
}
