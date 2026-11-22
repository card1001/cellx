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
7. `ClipboardCell`의 `text()`, `style()` 접근자 추가와 공개 — 선택하여 붙여넣기(값만, 서식만, 행/열 바꿈).
8. 이름 정의의 첫 글자에 한글 등 유니코드 문자 허용(Excel과 같음) — `is_valid_a1_identifier`.
9. `Parser::set_tables`, `Model::refresh_parser_tables` — 불러온 뒤에 만든 표의 구조적 참조(`표1[금액]`) 해석.
