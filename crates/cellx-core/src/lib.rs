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
pub mod datatools;
pub mod error;
pub mod extras;
pub mod objects;
pub mod outline;
pub mod audit;
pub mod book;
pub mod pictures;
pub mod pivotxml;
pub mod ooxml;
pub mod project;
pub mod refs;
pub mod sheetopts;
pub mod shapes;
pub mod sheets;
pub mod slicers;
pub mod sparklines;
pub mod value;
pub mod whatif;
pub mod workbook;

pub use error::{CellxError, Result};
pub use value::Scalar;
pub use workbook::Workbook;
