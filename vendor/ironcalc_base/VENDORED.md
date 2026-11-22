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
10. 행 정보(`Worksheet::rows`)를 행 번호 순으로 유지하고 이진 탐색 — 행 숨기기·높이·스타일 조회가 행마다 전체를 훑어 필터·윤곽에서 O(n²)이던 문제.
11. `Styles::get_style` 공개 — 정렬·중복 제거에서 서식을 인덱스로 비교하고 필요할 때만 다시 씀.
12. 증분 재계산: `dependencies` 모듈(수식→참조 셀 역방향 인덱스), `Model::evaluate_changed`·`invalidate_dependencies`, `UserModel::content_changes_since`·`content_changes_on_top`.
13. 조회 캐시: `functions/lookup_cache` — 한 번의 계산 안에서 VLOOKUP·HLOOKUP·MATCH 범위를 한 번만 읽고 정확히 일치 해시 인덱스 사용.
14. 행 삽입·삭제: `row_insert` — 셀을 그대로 옮기고 참조는 해석된 수식에서 조정(#REF!가 되는 수식만 문자열 경로). 행 삭제 실행 취소 때 #REF!가 된 수식 복원(`Diff::DeleteRows::refs`).
15. 숫자 반올림(`to_excel_precision`, `to_precision`): 자릿수가 정밀도보다 적은 정수는 문자열 왕복 없이 그대로 반환(결과 동일).
16. 협업용 전송 대기열을 끌 수 있게 함(`UserModel::set_send_queue_enabled`). 실행 취소·다시 실행은 변경 목록을 복사하지 않고 옮김.
17. 행·열 숨김(`SetRowHidden`, `SetColumnHidden`)은 증분 재계산 대상, SUBTOTAL은 휘발성.
18. `Diff::SetCells`·`UserModel::set_cells`: 셀을 통째로(내용·서식·해석된 수식) 바꾸는 실행 취소 가능한 변경 — 정렬·중복 제거.
19. 열 삽입·삭제도 패치 14와 같은 방식(`row_insert`의 `columns`), 열 삭제 실행 취소 때 #REF! 수식 복원(`Diff::DeleteColumns::refs`).
20. 수식 분석: `audit` 모듈 — `Model::cell_precedents`(참조되는 셀), `cell_dependents`(참조하는 셀), `formula_steps`(수식 계산: IF·IFERROR·IFNA는 선택된 분기만 계산).
21. 구조적 참조 유지: `Node::ReferenceKind`·`RangeKind`의 `structured` 필드(`StructuredRef`)로 `표1[금액]`·`표1[@금액]`을 쓴 그대로 표시·저장(R1C1 내부 문자열 포함, xlsx에는 `[#This Row]` 표기). `[#This Row]`는 행 기준 상대 참조로 해석해 열 전체가 수식 하나를 공유. 렉서가 `@` 표기와 표 안의 이름 없는 `[@열]`을 읽음. 표 이름·열 이름은 대소문자 무시. `Model::restructure` — 표가 커지거나 이름이 바뀌거나 범위로 변환될 때 수식을 다시 해석.
