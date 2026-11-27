//! The `.cellx` project file: one ZIP holding everything a workbook needs.
//!
//! ```text
//! manifest.json   format version and app version
//! workbook.xlsx   the sheets, readable by Excel if extracted
//! bridges.json    the original bridge formulas (the xlsx holds their results)
//! pivots.json     pivot table definitions (the xlsx holds their output; memos and charts are in the xlsx)
//! data.duckdb     the data sheets
//! ```

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::data::DataStore;
use crate::error::{CellxError, Result};
use crate::extras::PivotDef;
use crate::workbook::{BridgeCell, Workbook};
use crate::transform::TransformPipeline;

/// A named, project-persisted transformation recipe.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedPipeline {
    pub id: String,
    pub name: String,
    pub pipeline: TransformPipeline,
}

/// A reusable local file source. Paths are intentionally local to the machine
/// and are never uploaded or resolved through a remote service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedConnector {
    pub id: String,
    pub name: String,
    pub path: String,
    pub table_name: String,
    pub locale: String,
}

pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    format_version: u32,
    app_version: String,
}

#[derive(Serialize, Deserialize)]
struct StoredBridge {
    sheet: u32,
    row: i32,
    col: i32,
    formula: String,
}

/// A fresh working directory for a project's data engine file.
pub fn new_work_dir() -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir()
        .join("cellx")
        .join(format!("{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn save_project(wb: &mut Workbook, path: &Path) -> Result<()> {
    save_project_with_assets(wb, path, &[], &[])
}

pub fn save_project_with_pipelines(wb: &mut Workbook, path: &Path, pipelines: &[SavedPipeline]) -> Result<()> {
    save_project_with_assets(wb, path, pipelines, &[])
}

pub fn save_project_with_assets(
    wb: &mut Workbook,
    path: &Path,
    pipelines: &[SavedPipeline],
    connectors: &[SavedConnector],
) -> Result<()> {
    if wb.data_path().is_none() {
        let dir = new_work_dir()?;
        wb.persist_data_to(&dir.join("data.duckdb"))?;
    }
    wb.recalc()?;
    wb.data().checkpoint()?;
    let data_path = wb
        .data_path()
        .map(Path::to_path_buf)
        .ok_or_else(|| CellxError::Project("데이터 파일 위치가 없습니다".into()))?;

    let tmp = path.with_extension("cellx-tmp");
    {
        let file = File::create(&tmp)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        let manifest = Manifest {
            format: "cellx".into(),
            format_version: FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
        };
        zip_err(zip.start_file("manifest.json", opts))?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(json_err)?)?;

        let xlsx = wb.xlsx_bytes()?;
        zip_err(zip.start_file("workbook.xlsx", opts))?;
        zip.write_all(&xlsx)?;

        zip_err(zip.start_file("pivots.json", opts))?;
        zip.write_all(&serde_json::to_vec_pretty(&wb.pivots()).map_err(json_err)?)?;

        zip_err(zip.start_file("pipelines.json", opts))?;
        zip.write_all(&serde_json::to_vec_pretty(pipelines).map_err(json_err)?)?;

        zip_err(zip.start_file("connectors.json", opts))?;
        zip.write_all(&serde_json::to_vec_pretty(connectors).map_err(json_err)?)?;

        let bridges: Vec<StoredBridge> = wb
            .bridges()
            .iter()
            .map(|((sheet, row, col), b)| StoredBridge {
                sheet: *sheet,
                row: *row,
                col: *col,
                formula: b.formula.clone(),
            })
            .collect();
        zip_err(zip.start_file("bridges.json", opts))?;
        zip.write_all(&serde_json::to_vec_pretty(&bridges).map_err(json_err)?)?;

        zip_err(zip.start_file("data.duckdb", opts))?;
        let mut f = File::open(&data_path)?;
        std::io::copy(&mut f, &mut zip)?;
        zip_err(zip.finish())?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn open_project(path: &Path) -> Result<Workbook> {
    Ok(open_project_with_assets(path)?.0)
}

pub fn open_project_with_pipelines(path: &Path) -> Result<(Workbook, Vec<SavedPipeline>)> {
    let (wb, pipelines, _) = open_project_with_assets(path)?;
    Ok((wb, pipelines))
}

pub fn open_project_with_assets(path: &Path) -> Result<(Workbook, Vec<SavedPipeline>, Vec<SavedConnector>)> {
    let file = File::open(path)?;
    let mut zip = zip_err(zip::ZipArchive::new(file))?;

    let manifest: Manifest = serde_json::from_slice(&read_entry(&mut zip, "manifest.json")?).map_err(json_err)?;
    if manifest.format != "cellx" || manifest.format_version > FORMAT_VERSION {
        return Err(CellxError::Project(format!(
            "지원하지 않는 프로젝트 형식입니다 (버전 {})",
            manifest.format_version
        )));
    }

    let xlsx = read_entry(&mut zip, "workbook.xlsx")?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Book1".into());
    let model = crate::workbook::model_from_xlsx_bytes(&xlsx, &name)?;

    let dir = new_work_dir()?;
    let data_path = dir.join("data.duckdb");
    // The data file goes straight to disk, whatever its size: it is opened
    // from there anyway, and going through memory only cost RAM.
    extract_entry(&mut zip, "data.duckdb", &data_path)?;
    let store = DataStore::open(&data_path)?;

    let stored: Vec<StoredBridge> = serde_json::from_slice(&read_entry(&mut zip, "bridges.json")?).map_err(json_err)?;
    let mut bridges = BTreeMap::new();
    for b in stored {
        bridges.insert(
            (b.sheet, b.row, b.col),
            BridgeCell { formula: b.formula, rewritten: None, error: None },
        );
    }

    let mut wb = Workbook::from_model(model, store, Some(data_path))?;
    wb.set_bridges(bridges);
    wb.import_extras(&xlsx);
    // Older projects have no pivots.json.
    if let Ok(bytes) = read_entry(&mut zip, "pivots.json") {
        let pivots: Vec<PivotDef> = serde_json::from_slice(&bytes).map_err(json_err)?;
        wb.set_pivots(pivots);
    }
    wb.recalc()?;
    // Pipelines were added after the initial .cellx format; older projects remain valid.
    let pipelines = if zip.file_names().any(|name| name.is_ok_and(|entry| entry == "pipelines.json")) {
        serde_json::from_slice(&read_entry(&mut zip, "pipelines.json")?).map_err(json_err)?
    } else {
        Vec::new()
    };
    let connectors = if zip.file_names().any(|name| name.is_ok_and(|entry| entry == "connectors.json")) {
        serde_json::from_slice(&read_entry(&mut zip, "connectors.json")?).map_err(json_err)?
    } else {
        Vec::new()
    };
    Ok((wb, pipelines, connectors))
}

// Upper bounds for untrusted project archives. These limits are intentionally
// generous for normal workbooks while preventing oversized allocations and
// decompression bombs from exhausting the application process.
const MAX_PROJECT_ENTRY_SIZE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PROJECT_TOTAL_SIZE: u64 = 4 * 1024 * 1024 * 1024;

/// Unpacks an entry into a file, checking that it is as large as the archive
/// says. A partial file is removed on failure (a full disk, for one).
fn extract_entry(zip: &mut zip::ZipArchive<File>, name: &str, to: &Path) -> Result<()> {
    let mut entry = zip_err(zip.by_name(name))?;
    let declared = entry.size();
    let result = (|| -> Result<()> {
        let mut out = std::io::BufWriter::with_capacity(1 << 20, File::create(to)?);
        let written = std::io::copy(&mut (&mut entry).take(declared.saturating_add(1)), &mut out)?;
        out.flush()?;
        if written != declared {
            return Err(CellxError::Project(format!("프로젝트 항목 크기가 올바르지 않습니다: {name}")));
        }
        Ok(())
    })();
    if result.is_err() {
        std::fs::remove_file(to).ok();
    }
    result
}

/// Reads a small entry (manifest, workbook, JSON) into memory. The data file
/// is not one of them: it is unpacked to disk and not counted here.
fn read_entry(zip: &mut zip::ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut total_size = 0u64;
    for index in 0..zip.len() {
        let e = zip_err(zip.by_index(index))?;
        if e.name().is_ok_and(|n| n == "data.duckdb") {
            continue;
        }
        let size = e.size();
        total_size = total_size.checked_add(size)
            .ok_or_else(|| CellxError::Project("프로젝트 압축 해제 크기가 너무 큽니다".into()))?;
    }
    if total_size > MAX_PROJECT_TOTAL_SIZE {
        return Err(CellxError::Project("프로젝트 전체 압축 해제 크기가 허용 한도를 초과했습니다".into()));
    }

    let entry = zip_err(zip.by_name(name))?;
    let declared_size = entry.size();
    if declared_size > MAX_PROJECT_ENTRY_SIZE {
        return Err(CellxError::Project(format!(
            "프로젝트 항목 크기가 허용 한도를 초과했습니다: {name}"
        )));
    }
    let capacity = usize::try_from(declared_size)
        .map_err(|_| CellxError::Project("프로젝트 항목 크기를 처리할 수 없습니다".into()))?;
    let mut buf = Vec::with_capacity(capacity);
    entry.take(MAX_PROJECT_ENTRY_SIZE + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_PROJECT_ENTRY_SIZE || buf.len() as u64 != declared_size {
        return Err(CellxError::Project(format!(
            "프로젝트 항목 크기가 올바르지 않습니다: {name}"
        )));
    }
    Ok(buf)
}

fn zip_err<T>(r: std::result::Result<T, zip::result::ZipError>) -> Result<T> {
    r.map_err(|e| CellxError::Project(e.to_string()))
}

fn json_err(e: serde_json::Error) -> CellxError {
    CellxError::Project(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Scalar;

    #[test]
    fn project_round_trip() {
        let mut wb = Workbook::new().unwrap();
        wb.data()
            .create_table_from_sql("sales", "SELECT * FROM (VALUES ('a', 1.5::DOUBLE), ('b', 2.5)) AS t(k, v)")
            .unwrap();
        wb.set_input(0, 1, 1, "=SUM(sales[v])").unwrap();
        wb.set_input(0, 1, 2, "=A1*10").unwrap();
        let dir = new_work_dir().unwrap();
        let path = dir.join("book.cellx");
        let saved_pipeline = SavedPipeline {
            id: "pipeline-1".into(),
            name: "Clean sales".into(),
            pipeline: TransformPipeline {
                source_table: "sales".into(),
                steps: vec![],
            },
        };
        let saved_connector = SavedConnector {
            id: "connector-1".into(),
            name: "Sales source".into(),
            path: "/local/sales.csv".into(),
            table_name: "sales".into(),
            locale: "ko-KR".into(),
        };
        save_project_with_assets(&mut wb, &path, &[saved_pipeline.clone()], &[saved_connector.clone()]).unwrap();

        let (mut reopened, pipelines, connectors) = open_project_with_assets(&path).unwrap();
        assert_eq!(pipelines, vec![saved_pipeline]);
        assert_eq!(connectors, vec![saved_connector]);
        assert_eq!(reopened.value(0, 1, 2).unwrap(), Scalar::Number(40.0));
        assert_eq!(reopened.input(0, 1, 1).unwrap(), "=SUM(sales[v])");
        // The data sheet came back and the bridge still works against it.
        reopened.update_data_cell("sales", 1, "v", "10").unwrap();
        assert_eq!(reopened.value(0, 1, 1).unwrap(), Scalar::Number(12.5));
    }

    /// A large project opens without the data file passing through memory.
    /// `CELLX_BIG_PROJECT=path.cellx cargo test --release -- --ignored open_big_project`
    #[test]
    #[ignore]
    fn open_big_project() {
        let Ok(path) = std::env::var("CELLX_BIG_PROJECT") else { return };
        let t = std::time::Instant::now();
        let mut wb = open_project(Path::new(&path)).unwrap();
        eprintln!("열기 {:?}, 표 {:?}", t.elapsed(), wb.tables().unwrap().iter().map(|t| (t.name.clone(), t.rows)).collect::<Vec<_>>());
    }
}
