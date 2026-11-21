//! CellX core.
//!
//! * [`workbook::Workbook`] — the spreadsheet: IronCalc for cells and formulas,
//!   DuckDB for large data sheets.
//! * [`bridge`] — turns `SUMIFS(sales[amount], …)` and friends into SQL.
//! * [`data::DataStore`] — the DuckDB side: imports, paging, safe user SQL.
//! * [`project`] — the `.cellx` project file.

pub mod bridge;
pub mod criteria;
pub mod data;
pub mod error;
pub mod project;
pub mod refs;
pub mod value;
pub mod workbook;

pub use error::{CellxError, Result};
pub use value::Scalar;
pub use workbook::Workbook;
