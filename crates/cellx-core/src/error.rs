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
    #[error("{0}")]
    Bridge(String),
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
