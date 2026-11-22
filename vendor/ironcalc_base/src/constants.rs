#![deny(missing_docs)]

/// Default column width in pixels
pub(crate) const DEFAULT_COLUMN_WIDTH: f64 = 72.0;

/// Default row height in pixels
pub(crate) const DEFAULT_ROW_HEIGHT: f64 = 22.0;

/// A column width of Excel value `w` will result in `w * COLUMN_WIDTH_FACTOR` pixels
pub const COLUMN_WIDTH_FACTOR: f64 = 8.0;

/// Padding Excel adds to a column, in pixels (OOXML: px = chars * max digit width + 5).
pub const COLUMN_WIDTH_PADDING: f64 = 5.0;

/// Excel column width (characters) to pixels.
pub fn column_width_to_px(width: f64) -> f64 {
    (width * COLUMN_WIDTH_FACTOR + COLUMN_WIDTH_PADDING).round()
}

/// Pixels to Excel column width (characters).
pub fn px_to_column_width(px: f64) -> f64 {
    ((px - COLUMN_WIDTH_PADDING) / COLUMN_WIDTH_FACTOR).max(0.0)
}

/// A row height of Excel value `h` will result in `h * ROW_HEIGHT_FACTOR` pixels
pub const ROW_HEIGHT_FACTOR: f64 = 4.0 / 3.0; // points to pixels at 96 dpi

/// Default window height in pixels
pub(crate) const DEFAULT_WINDOW_HEIGHT: i64 = 600;

/// Default window width in pixels
pub(crate) const DEFAULT_WINDOW_WIDTH: i64 = 800;

/// Maximum number of columns
pub(crate) const LAST_COLUMN: i32 = 16_384;

/// Maximum number of rows
pub(crate) const LAST_ROW: i32 = 1_048_576;

/// Excel uses 15 significant digits of precision for all numeric calculations.
pub(crate) const EXCEL_PRECISION: usize = 15;

/// 693_594 is computed as:
/// NaiveDate::from_ymd(1900, 1, 1).num_days_from_ce() - 2
/// The 2 days offset is because of Excel 1900 bug
pub(crate) const EXCEL_DATE_BASE: i32 = 693_594;

/// We do not support dates before 1899-12-31.
pub(crate) const MINIMUM_DATE_SERIAL_NUMBER: i32 = 0;

/// Excel can handle dates until the year 9999-12-31
/// 2958465 is the number of days from 1900-01-01 to 9999-12-31
pub(crate) const MAXIMUM_DATE_SERIAL_NUMBER: i32 = 2_958_465;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_constants() {
        assert_eq!(column_width_to_px(8.38), DEFAULT_COLUMN_WIDTH);
        assert_eq!(ROW_HEIGHT_FACTOR * 16.5, DEFAULT_ROW_HEIGHT);
    }
}
