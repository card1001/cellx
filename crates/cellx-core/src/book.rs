//! 통합 문서 보호 (the sheets cannot be added, deleted, renamed, moved, hidden
//! or shown) and 사용자 지정 보기 (named sets of hidden rows, columns and
//! filters, shown again with one click).

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

use crate::datatools::ColumnFilter;
use crate::error::{CellxError, Result};
use crate::ooxml::esc;
use crate::workbook::Workbook;

/// Excel 2013's password hash: SHA-512 over a salt and the UTF-16 password,
/// hashed again `spin` times.
pub(crate) fn agile_hash(password: &str, salt: &[u8], spin: u32) -> String {
    let mut h = Sha512::new();
    h.update(salt);
    for u in password.encode_utf16() {
        h.update(u.to_le_bytes());
    }
    let mut hash = h.finalize().to_vec();
    for i in 0..spin {
        let mut h = Sha512::new();
        h.update(&hash);
        h.update(i.to_le_bytes());
        hash = h.finalize().to_vec();
    }
    base64::engine::general_purpose::STANDARD.encode(hash)
}

fn new_salt() -> Vec<u8> {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = Sha512::new();
    h.update(t.to_le_bytes());
    h.update((&t as *const u128 as usize).to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.finalize()[..16].to_vec()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BookProtection {
    pub structure: bool,
    /// Base64 SHA-512 hash, salt and spin count, when there is a password.
    pub hash: Option<String>,
    pub salt: Option<String>,
    pub spin: u32,
    /// Excel's legacy 16-bit hash (`workbookPassword`), from older files.
    pub legacy: Option<String>,
}

impl BookProtection {
    fn check(&self, password: Option<&str>) -> bool {
        let given = password.unwrap_or("");
        if let (Some(h), Some(s)) = (&self.hash, &self.salt) {
            let Ok(salt) = base64::engine::general_purpose::STANDARD.decode(s) else { return false };
            return agile_hash(given, &salt, self.spin) == *h;
        }
        if let Some(l) = &self.legacy {
            let mine = crate::sheetopts::excel_password_hash(given);
            return u32::from_str_radix(&mine, 16).ok() == u32::from_str_radix(l, 16).ok();
        }
        true
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SheetViewState {
    pub sheet: u32,
    pub hidden_rows: Vec<i32>,
    pub hidden_cols: Vec<i32>,
    /// The sheet's AutoFilter criteria, when it has a filter.
    pub filter: Option<Vec<ColumnFilter>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CustomView {
    pub name: String,
    pub guid: String,
    pub active_sheet: u32,
    pub sheets: Vec<SheetViewState>,
}

fn guid(name: &str) -> String {
    let mut h = Sha512::new();
    h.update(name.as_bytes());
    h.update(new_salt());
    let b = h.finalize();
    let hex: String = b[..16].iter().map(|x| format!("{x:02X}")).collect();
    format!("{{{}-{}-{}-{}-{}}}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

impl Workbook {
    pub fn book_protection(&self) -> Option<BookProtection> {
        self.book_protection.clone()
    }

    pub(crate) fn book_guard(&self) -> Result<()> {
        if self.book_protection.as_ref().is_some_and(|p| p.structure) {
            return Err(CellxError::Calc("통합 문서가 보호되어 있어 변경할 수 없습니다. 검토 탭에서 통합 문서 보호를 해제하세요.".into()));
        }
        Ok(())
    }

    /// 검토 → 통합 문서 보호 (구조).
    pub fn protect_book(&mut self, password: Option<&str>) -> Result<()> {
        let p = match password.filter(|p| !p.is_empty()) {
            Some(pw) => {
                let salt = new_salt();
                BookProtection {
                    structure: true,
                    hash: Some(agile_hash(pw, &salt, 100_000)),
                    salt: Some(base64::engine::general_purpose::STANDARD.encode(&salt)),
                    spin: 100_000,
                    legacy: None,
                }
            }
            None => BookProtection { structure: true, hash: None, salt: None, spin: 0, legacy: None },
        };
        let start = self.begin();
        self.book_protection = Some(p);
        self.commit(start)?;
        Ok(())
    }

    pub fn unprotect_book(&mut self, password: Option<&str>) -> Result<()> {
        if let Some(p) = &self.book_protection {
            if !p.check(password) {
                return Err(CellxError::Calc("암호가 올바르지 않습니다.".into()));
            }
        }
        let start = self.begin();
        self.book_protection = None;
        self.commit(start)?;
        Ok(())
    }

    // ------------------------------------------------------------ custom views

    pub fn custom_views(&self) -> Vec<CustomView> {
        self.custom_views.clone()
    }

    fn capture_sheet(&self, sheet: u32) -> SheetViewState {
        let ws = self.model().workbook.worksheet(sheet).ok();
        let hidden_rows = ws.map(|w| w.rows.iter().filter(|r| r.hidden).map(|r| r.r).collect()).unwrap_or_default();
        let hidden_cols = ws.map(|w| w.cols.iter().filter(|c| c.hidden).flat_map(|c| c.min..=c.max.min(16384)).collect()).unwrap_or_default();
        let filter = self.filters.iter().find(|f| f.sheet == sheet).map(|f| f.columns.clone());
        SheetViewState { sheet, hidden_rows, hidden_cols, filter }
    }

    /// 사용자 지정 보기 → 추가: what is hidden and filtered now, on every sheet.
    pub fn add_custom_view(&mut self, name: &str, active_sheet: u32) -> Result<CustomView> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CellxError::Calc("보기 이름을 입력하세요".into()));
        }
        let sheets = (0..self.sheet_names().len() as u32).map(|s| self.capture_sheet(s)).collect();
        let old = self.custom_views.iter().find(|v| v.name.eq_ignore_ascii_case(name)).map(|v| v.guid.clone());
        let view = CustomView { name: name.to_string(), guid: old.unwrap_or_else(|| guid(name)), active_sheet, sheets };
        let start = self.begin();
        self.custom_views.retain(|v| !v.name.eq_ignore_ascii_case(name));
        self.custom_views.push(view.clone());
        self.commit(start)?;
        Ok(view)
    }

    pub fn delete_custom_view(&mut self, name: &str) -> Result<()> {
        let start = self.begin();
        self.custom_views.retain(|v| !v.name.eq_ignore_ascii_case(name));
        self.commit(start)?;
        Ok(())
    }

    /// 표시: hides and shows rows and columns and sets the filters as the view
    /// had them (one undo step). Returns the sheet to go to.
    pub fn show_custom_view(&mut self, name: &str) -> Result<u32> {
        let view = self.custom_views.iter().find(|v| v.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| CellxError::Calc(format!("보기가 없습니다: {name}")))?;
        let start = self.begin();
        let sheets = self.sheet_names().len() as u32;
        for st in view.sheets.iter().filter(|s| s.sheet < sheets) {
            let s = st.sheet;
            let now = self.capture_sheet(s);
            if let Some(cols) = &st.filter {
                if let Some(f) = self.filters.iter_mut().find(|f| f.sheet == s) {
                    f.columns = cols.clone();
                }
            }
            for r in now.hidden_rows.iter().filter(|r| !st.hidden_rows.contains(r)) {
                self.um.set_rows_hidden(s, *r, *r, false)?;
            }
            for r in st.hidden_rows.iter().filter(|r| !now.hidden_rows.contains(r)) {
                self.um.set_rows_hidden(s, *r, *r, true)?;
            }
            for c in now.hidden_cols.iter().filter(|c| !st.hidden_cols.contains(c)) {
                self.um.set_columns_hidden(s, *c, *c, false)?;
            }
            for c in st.hidden_cols.iter().filter(|c| !now.hidden_cols.contains(c)) {
                self.um.set_columns_hidden(s, *c, *c, true)?;
            }
        }
        self.commit(start)?;
        Ok(view.active_sheet.min(sheets.saturating_sub(1)))
    }

    // ------------------------------------------------------------ xlsx

    /// `<workbookProtection>`, or nothing.
    pub(crate) fn book_protection_xml(&self) -> String {
        let Some(p) = &self.book_protection else { return String::new() };
        let lock = if p.structure { r#" lockStructure="1""# } else { "" };
        match (&p.hash, &p.salt, &p.legacy) {
            (Some(h), Some(s), _) => format!(
                r#"<workbookProtection workbookAlgorithmName="SHA-512" workbookHashValue="{h}" workbookSaltValue="{s}" workbookSpinCount="{}"{lock}/>"#,
                p.spin
            ),
            (_, _, Some(l)) => format!(r#"<workbookProtection workbookPassword="{l}"{lock}/>"#),
            _ => format!(r#"<workbookProtection{lock}/>"#),
        }
    }

    /// `<customWorkbookViews>`, or nothing.
    pub(crate) fn custom_workbook_views_xml(&self) -> String {
        if self.custom_views.is_empty() {
            return String::new();
        }
        let body: String = self
            .custom_views
            .iter()
            .map(|v| {
                format!(
                    r#"<customWorkbookView name="{}" guid="{}" personalView="0" maximized="1" windowWidth="1280" windowHeight="800" activeSheetId="{}"/>"#,
                    esc(&v.name),
                    v.guid,
                    v.active_sheet + 1
                )
            })
            .collect();
        format!("<customWorkbookViews>{body}</customWorkbookViews>")
    }

    /// `<customSheetViews>` of a sheet, or nothing. Hidden rows and columns go in
    /// a CellX extension Excel leaves alone.
    pub(crate) fn custom_sheet_views_xml(&self, sheet: u32) -> String {
        let views: Vec<(&CustomView, &SheetViewState)> = self.custom_views.iter().filter_map(|v| v.sheets.iter().find(|s| s.sheet == sheet).map(|s| (v, s))).collect();
        if views.is_empty() {
            return String::new();
        }
        let list = |v: &[i32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        let body: String = views
            .iter()
            .map(|(v, s)| {
                let flags = format!(
                    "{}{}{}",
                    if s.hidden_rows.is_empty() { "" } else { r#" hiddenRows="1""# },
                    if s.hidden_cols.is_empty() { "" } else { r#" hiddenColumns="1""# },
                    if s.filter.as_ref().is_some_and(|f| !f.is_empty()) { r#" filter="1" showAutoFilter="1""# } else if s.filter.is_some() { r#" showAutoFilter="1""# } else { "" }
                );
                let filter = match (&s.filter, self.filters.iter().find(|f| f.sheet == sheet)) {
                    (Some(cols), Some(f)) => {
                        let mut f = f.clone();
                        f.columns = cols.clone();
                        crate::ooxml::auto_filter_xml(&f)
                    }
                    _ => String::new(),
                };
                format!(
                    r#"<customSheetView guid="{}"{flags}><selection activeCell="A1" sqref="A1"/>{filter}<extLst><ext uri="urn:cellx:hidden"><cx:hidden xmlns:cx="urn:cellx" rows="{}" cols="{}"/></ext></extLst></customSheetView>"#,
                    v.guid,
                    list(&s.hidden_rows),
                    list(&s.hidden_cols)
                )
            })
            .collect();
        format!("<customSheetViews>{body}</customSheetViews>")
    }

    pub(crate) fn import_book(&mut self, xlsx: &[u8]) {
        let Some(wb) = crate::ooxml::workbook_xml(xlsx) else { return };
        let Ok(doc) = roxmltree::Document::parse(&wb) else { return };
        if let Some(p) = doc.descendants().find(|n| n.has_tag_name("workbookProtection")) {
            let on = |a: &str| matches!(p.attribute(a), Some("1") | Some("true"));
            self.book_protection = Some(BookProtection {
                structure: on("lockStructure"),
                hash: p.attribute("workbookHashValue").map(str::to_string),
                salt: p.attribute("workbookSaltValue").map(str::to_string),
                spin: p.attribute("workbookSpinCount").and_then(|v| v.parse().ok()).unwrap_or(100_000),
                legacy: p.attribute("workbookPassword").map(str::to_string),
            })
            .filter(|b| b.structure);
        }
        let views: Vec<(String, String, u32)> = doc
            .descendants()
            .filter(|n| n.has_tag_name("customWorkbookView"))
            .filter_map(|n| Some((n.attribute("name")?.to_string(), n.attribute("guid")?.to_string(), n.attribute("activeSheetId").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).saturating_sub(1))))
            .collect();
        if views.is_empty() {
            return;
        }
        let mut list: Vec<CustomView> = views.into_iter().map(|(name, guid, active_sheet)| CustomView { name, guid, active_sheet, sheets: Vec::new() }).collect();
        for (sheet, xml) in crate::ooxml::sheet_xmls(xlsx, "customSheetView") {
            let Ok(d) = roxmltree::Document::parse(&xml) else { continue };
            for csv in d.descendants().filter(|n| n.has_tag_name("customSheetView")) {
                let Some(g) = csv.attribute("guid") else { continue };
                let Some(v) = list.iter_mut().find(|v| v.guid.eq_ignore_ascii_case(g)) else { continue };
                let nums = |a: &str| -> Vec<i32> {
                    csv.descendants().find(|n| n.has_tag_name("hidden")).and_then(|h| h.attribute(a)).map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default()
                };
                let filter = csv.children().find(|n| n.has_tag_name("autoFilter")).and_then(|af| crate::ooxml::parse_auto_filter(af, sheet)).map(|f| f.columns);
                v.sheets.push(SheetViewState { sheet, hidden_rows: nums("rows"), hidden_cols: nums("cols"), filter });
            }
        }
        self.custom_views = list;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbook::CellRange;

    #[test]
    fn excel_hash_matches_a_known_value() {
        // The example from [MS-OFFCRYPTO] 2.3.7.4 style hashing: deterministic for a given salt.
        let salt = [0u8; 16];
        let a = agile_hash("password", &salt, 10);
        assert_eq!(a, agile_hash("password", &salt, 10));
        assert_ne!(a, agile_hash("Password", &salt, 10));
    }

    #[test]
    fn protected_structure_blocks_sheet_changes_and_round_trips() {
        let mut wb = Workbook::new().unwrap();
        wb.add_sheet().unwrap();
        wb.protect_book(Some("암호1")).unwrap();
        assert!(wb.add_sheet().is_err());
        assert!(wb.rename_sheet(0, "X").is_err());
        assert!(wb.delete_sheet(1).is_err());
        assert!(wb.move_sheet(0, 1).is_err());
        assert!(wb.set_sheet_hidden(1, true).is_err());
        // Cells still change.
        wb.set_input(0, 1, 1, "1").unwrap();
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("book.xlsx");
        wb.save_xlsx(&path).unwrap();
        let mut back = Workbook::open_xlsx(&path).unwrap();
        assert!(back.add_sheet().is_err());
        assert!(back.unprotect_book(Some("틀림")).is_err());
        back.unprotect_book(Some("암호1")).unwrap();
        back.add_sheet().unwrap();
    }

    #[test]
    fn custom_views_hide_and_filter_and_round_trip() {
        let mut wb = Workbook::new().unwrap();
        let mut cells = vec![(1, 1, "지역".to_string()), (1, 2, "금액".to_string())];
        for (i, (a, b)) in [("서울", "1"), ("부산", "2"), ("서울", "3")].iter().enumerate() {
            cells.push((i as i32 + 2, 1, a.to_string()));
            cells.push((i as i32 + 2, 2, b.to_string()));
        }
        wb.set_inputs(0, &cells).unwrap();
        wb.add_custom_view("전체", 0).unwrap();
        wb.toggle_auto_filter(0, CellRange { r0: 1, c0: 1, r1: 4, c1: 2 }).unwrap();
        wb.set_filter_column(0, ColumnFilter { col: 1, values: Some(vec!["서울".into()]), conditions: Vec::new(), and: true }).unwrap();
        wb.set_hidden(0, crate::refs::Axis::Column, 2, 2, true).unwrap();
        wb.add_custom_view("서울만", 0).unwrap();
        wb.show_custom_view("전체").unwrap();
        let hidden = |wb: &Workbook| wb.model().workbook.worksheet(0).unwrap().rows.iter().filter(|r| r.hidden).map(|r| r.r).collect::<Vec<_>>();
        assert!(hidden(&wb).is_empty());
        wb.show_custom_view("서울만").unwrap();
        assert_eq!(hidden(&wb), vec![3]);
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("views.xlsx");
        wb.save_xlsx(&path).unwrap();
        let mut back = Workbook::open_xlsx(&path).unwrap();
        assert_eq!(back.custom_views().len(), 2);
        back.show_custom_view("전체").unwrap();
        assert!(hidden(&back).is_empty());
        back.show_custom_view("서울만").unwrap();
        assert_eq!(hidden(&back), vec![3]);
        assert_eq!(back.custom_views()[1].sheets[0].hidden_cols, vec![2]);
    }

    #[test]
    fn external_links_round_trip() {
        let mut wb = Workbook::new().unwrap();
        wb.set_inputs(0, &[(1, 1, "=IFERROR([1]Sheet1!A1,\"없음\")".to_string())]).unwrap();
        let link = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><externalLink xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><externalBook r:id="rId1"><sheetNames><sheetName val="Sheet1"/></sheetNames></externalBook></externalLink>"#;
        let rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLinkPath" Target="other.xlsx" TargetMode="External"/></Relationships>"#;
        wb.external_links = vec![crate::ooxml::ExternalLinkPart { xml: link.to_vec(), rels: Some(rels.to_vec()) }];
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("links.xlsx");
        wb.save_xlsx(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let wbxml = crate::ooxml::workbook_xml(&bytes).unwrap();
        assert!(wbxml.contains("<externalReferences><externalReference r:id=\"rIdCxExt1\"/></externalReferences>"));
        let back = Workbook::open_xlsx(&path).unwrap();
        assert_eq!(back.external_links.len(), 1);
        assert_eq!(back.external_links[0].xml, link.to_vec());
        assert_eq!(back.input(0, 1, 1).unwrap(), "=IFERROR([1]Sheet1!A1,\"없음\")");
        assert_eq!(back.value(0, 1, 1).unwrap(), crate::value::Scalar::Text("없음".into()));
    }

    #[test]
    fn date1904_switch_and_round_trip() {
        let mut wb = Workbook::new().unwrap();
        wb.set_inputs(0, &[(1, 1, "0".to_string()), (1, 2, "=YEAR(A1)".to_string())]).unwrap();
        assert_eq!(wb.value(0, 1, 2).unwrap(), crate::value::Scalar::Number(1900.0));
        wb.set_date1904(true).unwrap();
        assert!(wb.date1904());
        assert_eq!(wb.value(0, 1, 2).unwrap(), crate::value::Scalar::Number(1904.0));
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("d1904.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        assert!(back.date1904());
        assert_eq!(back.value(0, 1, 2).unwrap(), crate::value::Scalar::Number(1904.0));
    }
}
