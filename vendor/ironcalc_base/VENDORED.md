# ironcalc_base 0.9.1 (CellX 포크)

원본: https://github.com/ironcalc/IronCalc (MIT OR Apache-2.0), crates.io `ironcalc_base` 0.9.1.
`Cargo.toml`의 `[patch.crates-io]`로 이 사본을 씁니다.

CellX에서 바꾼 부분:

1. 한국어 Excel 기본값: 기본 글꼴 맑은 고딕 11pt, 기본 열 너비 72px(8.38자), 기본 행 높이 22px(16.5pt).
   열 너비 변환을 OOXML 규칙(픽셀 = 글자 수 × 8 + 5)으로, 행 높이 변환을 96dpi 기준(pt × 4/3)으로 변경.
2. `update_range_style`에 `font.name` 경로 추가 (글꼴 이름 변경).
3. `UserModel::get_model_mut` 추가 — 데이터 엔진 결과를 실행 취소 기록 없이 쓰기 위해.
4. `Clipboard`에 `csv()`, `sheet()`, `range()`, `data()` 접근자 추가.
5. `UserModel::undo_depth` 추가 — CellX의 실행 취소 기록(데이터 엔진 수식)을 IronCalc 기록과 맞추기 위해.
6. `supported_function_names()` 추가 — 수식 자동완성 목록.
