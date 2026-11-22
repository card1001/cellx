//! 자동 복구: while a workbook has unsaved changes it is saved every few minutes
//! to a recovery folder, as a .cellx project (data sheets included). A clean exit
//! or a save removes it. After a crash, the next start offers what was left.
//!
//! Each running CellX holds a lock on its own `<id>.lock`, so the files of
//! another window that is still open are never offered for recovery.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use cellx_core::project;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{AppState, Book, BookInfo, CmdResult};

pub struct Recovery {
    dir: PathBuf,
    id: String,
    /// Held for the life of the process.
    _lock: Option<File>,
    /// The edit count of the book when it was last saved here.
    saved_at_edit: Option<u64>,
}

pub struct RecoveryState(pub Mutex<Recovery>);

#[derive(Serialize, Deserialize, Clone)]
pub struct RecoveredFile {
    pub id: String,
    /// The file the work came from, if it had been saved before.
    pub original: Option<String>,
    pub name: String,
    /// Seconds since 1970.
    pub saved_at: u64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Recovery {
    pub fn new(dir: PathBuf) -> Recovery {
        let _ = std::fs::create_dir_all(&dir);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let id = format!("{}-{nanos}", std::process::id());
        let lock = File::create(dir.join(format!("{id}.lock"))).ok().filter(|f| f.try_lock().is_ok());
        Recovery { dir, id, _lock: lock, saved_at_edit: None }
    }

    fn file(&self, id: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{id}.{ext}"))
    }

    /// Forgets this session's recovery file (after a save, or a new or opened book).
    pub fn clear(&mut self) {
        let _ = std::fs::remove_file(self.file(&self.id, "cellx"));
        let _ = std::fs::remove_file(self.file(&self.id, "json"));
        self.saved_at_edit = None;
    }

    /// A clean exit: nothing of this session is left behind.
    pub fn finish(&mut self) {
        self.clear();
        self._lock = None;
        let _ = std::fs::remove_file(self.file(&self.id, "lock"));
    }

    fn save(&mut self, book: &mut Book) -> Result<bool, String> {
        if !book.dirty || self.saved_at_edit == Some(book.edits) {
            return Ok(false);
        }
        let target = self.file(&self.id, "cellx");
        project::save_project(&mut book.wb, &target).map_err(|e| e.to_string())?;
        let original = book.path.as_ref().map(|p| p.display().to_string());
        let name = book
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "통합 문서1".into());
        let meta = RecoveredFile { id: self.id.clone(), original, name, saved_at: now() };
        std::fs::write(self.file(&self.id, "json"), serde_json::to_vec(&meta).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        self.saved_at_edit = Some(book.edits);
        Ok(true)
    }

    /// Recovery files of sessions that are no longer running.
    fn left_behind(&self) -> Vec<RecoveredFile> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            if id == self.id || self.alive(&id) || !self.file(&id, "cellx").exists() {
                continue;
            }
            if let Some(meta) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<RecoveredFile>(&b).ok()) {
                out.push(meta);
            }
        }
        out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
        out
    }

    /// Whether the session `id` still holds its lock.
    fn alive(&self, id: &str) -> bool {
        match File::options().write(true).open(self.file(id, "lock")) {
            Ok(f) => f.try_lock().is_err(),
            Err(_) => false,
        }
    }

    fn discard(&self, id: &str) {
        for ext in ["cellx", "json", "lock"] {
            let _ = std::fs::remove_file(self.file(id, ext));
        }
    }
}

fn rec<'a>(state: &'a State<'_, RecoveryState>) -> CmdResult<std::sync::MutexGuard<'a, Recovery>> {
    state.0.lock().map_err(|_| "복구 상태를 잠글 수 없습니다".to_string())
}

/// Saves the recovery copy if there are changes since the last one. Returns
/// whether it saved.
#[tauri::command]
pub fn autosave(state: State<'_, AppState>, recovery: State<'_, RecoveryState>) -> CmdResult<bool> {
    let mut r = rec(&recovery)?;
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    r.save(&mut book)
}

#[tauri::command]
pub fn recovery_list(recovery: State<'_, RecoveryState>) -> CmdResult<Vec<RecoveredFile>> {
    Ok(rec(&recovery)?.left_behind())
}

/// Opens a recovered workbook in place of the current one. It keeps the file
/// it came from, but is unsaved until the user saves it.
#[tauri::command]
pub fn recovery_open(state: State<'_, AppState>, recovery: State<'_, RecoveryState>, id: String) -> CmdResult<BookInfo> {
    let r = rec(&recovery)?;
    let meta = r
        .left_behind()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| "복구할 파일이 없습니다".to_string())?;
    let mut opened = Book::open(&r.file(&id, "cellx")).map_err(|e| e.to_string())?;
    opened.path = meta.original.map(PathBuf::from);
    opened.dirty = true;
    opened.edits += 1;
    r.discard(&id);
    drop(r);
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    *book = opened;
    crate::commands::info(&mut book)
}

#[tauri::command]
pub fn recovery_discard(recovery: State<'_, RecoveryState>, id: String) -> CmdResult<()> {
    let r = rec(&recovery)?;
    if r.left_behind().iter().any(|m| m.id == id) {
        r.discard(&id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_behind_only_when_not_running() {
        let dir = std::env::temp_dir().join(format!("cellx-rec-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = Recovery::new(dir.clone());
        let mut book = Book::new().unwrap();
        book.wb.set_input(0, 1, 1, "복구").unwrap();
        book.dirty = true;
        book.edits = 1;
        assert!(a.save(&mut book).unwrap());
        assert!(!a.save(&mut book).unwrap(), "nothing new since the last save");
        // Another session sees nothing while `a` runs.
        let b = Recovery::new(dir.clone());
        assert!(b.left_behind().is_empty());
        // `a` crashes: its lock goes, its files stay.
        a._lock = None;
        let found = b.left_behind();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "통합 문서1");
        let back = Book::open(&b.file(&found[0].id, "cellx")).unwrap();
        assert_eq!(back.wb.input(0, 1, 1).unwrap(), "복구");
        b.discard(&found[0].id);
        assert!(b.left_behind().is_empty());
        // A clean exit leaves nothing.
        let mut c = Recovery::new(dir.clone());
        book.edits = 2;
        c.save(&mut book).unwrap();
        c.finish();
        assert!(Recovery::new(dir.clone()).left_behind().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
