use thiserror::Error;

#[derive(Debug, Error)]
pub enum CellxError {
    #[error("데이터 엔진 오류: {0}")]
    Duck(#[from] duckdb::Error),
    #[error("계산 엔진 오류: {0}")]
    Calc(String),
    #[error("xlsx 오류: {0}")]
    Xlsx(String),
    #[error("파일 오류: {0}")]
    Io(#[from] std::io::Error),
    #[error("프로젝트 파일 오류: {0}")]
    Project(String),
    #[error("데이터 시트가 없습니다: {0}")]
    UnknownTable(String),
    #[error("열이 없습니다: {0}")]
    UnknownColumn(String),
    #[error("이름을 쓸 수 없습니다: {0} (영문·한글·숫자·_만 가능, 첫 글자는 문자)")]
    InvalidName(String),
    #[error("허용되지 않는 SQL입니다: {0}")]
    UnsafeSql(String),
    #[error("지원하지 않는 CSV 인코딩입니다: {0} (UTF-8 또는 UTF-16 파일을 사용하세요)")]
    CsvEncoding(String),
    #[error("{0}")]
    Bridge(String),
}

impl CellxError {
    /// Stable machine-readable identifier for logs, tests and future structured APIs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Duck(_) => "DATA_ENGINE",
            Self::Calc(_) => "CALCULATION",
            Self::Xlsx(_) => "XLSX",
            Self::Io(_) => "FILE_IO",
            Self::Project(_) => "PROJECT_FILE",
            Self::UnknownTable(_) => "UNKNOWN_TABLE",
            Self::UnknownColumn(_) => "UNKNOWN_COLUMN",
            Self::InvalidName(_) => "INVALID_NAME",
            Self::UnsafeSql(_) => "UNSAFE_SQL",
            Self::CsvEncoding(_) => "CSV_ENCODING",
            Self::Bridge(_) => "FORMULA_BRIDGE",
        }
    }

    /// Localizes the user-facing summary while retaining the underlying diagnostic.
    /// Workbook data and formula evaluation are deliberately independent of this locale.
    pub fn localized_message(&self, locale: &str) -> String {
        if locale.to_ascii_lowercase().starts_with("ko") {
            return self.to_string();
        }

        match self {
            Self::Duck(e) => format!("{}: {e}", message(locale, "데이터 엔진 오류", "Data engine error", "データ エンジン エラー", "数据引擎错误", "資料引擎錯誤")),
            Self::Calc(e) => format!("{}: {e}", message(locale, "계산 엔진 오류", "Calculation engine error", "計算エンジン エラー", "计算引擎错误", "計算引擎錯誤")),
            Self::Xlsx(e) => format!("{}: {e}", message(locale, "XLSX 오류", "XLSX error", "XLSX エラー", "XLSX 错误", "XLSX 錯誤")),
            Self::Io(e) => format!("{}: {e}", message(locale, "파일 오류", "File error", "ファイル エラー", "文件错误", "檔案錯誤")),
            Self::Project(e) => format!("{}: {e}", message(locale, "프로젝트 파일 오류", "Project file error", "プロジェクト ファイル エラー", "项目文件错误", "專案檔案錯誤")),
            Self::UnknownTable(name) => format!("{}: {name}", message(locale, "데이터 시트가 없습니다", "Data sheet not found", "データ シートが見つかりません", "未找到数据工作表", "找不到資料工作表")),
            Self::UnknownColumn(name) => format!("{}: {name}", message(locale, "열이 없습니다", "Column not found", "列が見つかりません", "未找到列", "找不到欄")),
            Self::InvalidName(name) => format!("{}: {name} ({})",
                message(locale, "이름을 쓸 수 없습니다", "Invalid name", "使用できない名前です", "名称无效", "名稱無效"),
                message(locale, "영문·한글·숫자·밑줄을 사용할 수 있으며 문자 또는 밑줄로 시작해야 합니다", "Use letters, digits, and underscores; start with a letter or underscore", "英字・数字・アンダースコアを使用し、英字またはアンダースコアで開始してください", "可使用字母、数字和下划线；必须以字母或下划线开头", "可使用字母、數字和底線；必須以字母或底線開頭")),
            Self::UnsafeSql(detail) => format!("{}: {detail}", message(locale, "허용되지 않는 SQL입니다", "SQL is not allowed", "許可されていない SQL です", "不允许执行此 SQL", "不允許執行此 SQL")),
            Self::CsvEncoding(encoding) => format!("{}: {encoding}. {}",
                message(locale, "지원하지 않는 CSV 인코딩입니다", "Unsupported CSV encoding", "サポートされていない CSV エンコーディングです", "不支持的 CSV 编码", "不支援的 CSV 編碼"),
                message(locale, "UTF-8 또는 UTF-16 파일을 사용하세요", "Use a UTF-8 or UTF-16 encoded file", "UTF-8 または UTF-16 で保存されたファイルを使用してください", "请使用 UTF-8 或 UTF-16 编码的文件", "請使用 UTF-8 或 UTF-16 編碼的檔案")),
            Self::Bridge(detail) => format!("{}: {detail}", message(locale, "수식 연결 오류", "Formula bridge error", "数式ブリッジ エラー", "公式桥接错误", "公式橋接錯誤")),
        }
    }
}

fn message<'a>(locale: &str, ko: &'a str, en: &'a str, ja: &'a str, zh_cn: &'a str, zh_tw: &'a str) -> &'a str {
    let tag = locale.to_ascii_lowercase();
    if tag.starts_with("ja") {
        ja
    } else if tag == "zh-tw" || tag.starts_with("zh-hant") || tag == "zh-hk" {
        zh_tw
    } else if tag.starts_with("zh") {
        zh_cn
    } else if tag.starts_with("ko") {
        ko
    } else {
        en
    }
}

impl From<String> for CellxError {
    fn from(value: String) -> Self {
        CellxError::Calc(value)
    }
}

impl serde::Serialize for CellxError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CellxError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_summaries_follow_the_ui_locale() {
        let error = CellxError::UnknownTable("sales".into());
        assert_eq!(error.code(), "UNKNOWN_TABLE");
        assert_eq!(error.localized_message("ko-KR"), "데이터 시트가 없습니다: sales");
        assert_eq!(error.localized_message("en-US"), "Data sheet not found: sales");
        assert_eq!(error.localized_message("ja-JP"), "データ シートが見つかりません: sales");
        assert_eq!(error.localized_message("zh-CN"), "未找到数据工作表: sales");
        assert_eq!(error.localized_message("zh-TW"), "找不到資料工作表: sales");
    }

    #[test]
    fn csv_encoding_error_is_localized() {
        let error = CellxError::CsvEncoding("UTF-16LE".into());
        assert!(error.localized_message("en-US").contains("Use a UTF-8 or UTF-16 encoded file"));
        assert!(error.localized_message("ja-JP").contains("UTF-8"));
        assert!(error.localized_message("ko-KR").contains("UTF-8"));
    }
}
