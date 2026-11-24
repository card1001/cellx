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
22. 동적 배열 증분 재계산: 넘치는 배열이 있어도 `evaluate_changed`가 부분 계산. 다시 계산하는 배열은 먼저(전체 계산이 찾은 순서로), 그 넘친 범위를 읽는 수식도 함께 계산. 범위 크기가 바뀌거나, 수정이 다른 배열의 범위 안이거나, #SPILL!·초기화된 배열이 있으면 전체 계산. 부분 계산에서 다시 계산하지 않는 배열의 넘친 셀은 저장된 값으로 읽음.
23. 병렬 전체 계산(`parallel.rs`): 의존 관계로 이어진 수식 묶음을 스레드마다 모델 사본에서 계산해 값을 합치고, 입력을 알 수 없는 수식·동적 수식이 있는 묶음은 마지막에 계산. 지난 전체 계산이 250 ms 이상일 때만(`set_parallel_threshold_ms`), 스레드 수는 `set_evaluation_threads`. 묶음 나누기는 의존 인덱스에 보관. `StaticResult`에 `Clone`.
24. Excel 호환성(1900 날짜 체계): 일련번호 0은 1900-01-00, 60은 실재하지 않는 1900-02-29(`convert_to_serial_number`, `from_excel_date`, `formatter::dates::excel_date_parts`). DAY·MONTH·YEAR·WEEKDAY·WEEKNUM·ISOWEEKNUM·EOMONTH·WORKDAY·NETWORKDAYS·YEARFRAC·DATEDIF·TEXT가 이 달력을 따름. 1904 날짜 체계(`WorkbookSettings::date1904`, xlsx `workbookPr date1904` 읽기·쓰기, 스레드별 오프셋 `set_date1904`), 음수 날짜 표시. 바뀐 IronCalc 단위 테스트는 Excel 값으로 갱신.
25. Excel 호환성(함수): TIME 인수 32767 제한, HOUR·MINUTE·SECOND 범위, TEXT 시간 반올림·`ss.0` 소수 초·`mm:ss`의 분, 생략 인수(#N/A·기본값), 주말 코드, DATE 일·월 한계, COUPPCD/COUPNCD(만기일에서 바로 계산)·COUPDAYS(basis 1), ACCRINT·AMORLINC 경계, RATE guess(0 포함), IRR 정밀도, BASE·DECIMAL, NUMBERVALUE, AND·OR 오류 전파, SUBTOTAL 6·110·111, BINOM.INV, SKEW.P, WRAPCOLS·WRAPROWS, TEXTAFTER·TEXTBEFORE 빈 구분자, ENCODEURL 길이, CONVERT 섭씨, BESSELJ·BESSELY(Numerical Recipes 근사, Excel과 같은 값), 숫자→텍스트 변환 15자리·지수 표기(`excel_number_to_text`).
26. 배열: 단일 인수 함수(CHAR, CODE, NOT, IS 계열, ENCODEURL)를 범위·배열에 원소별로 적용(`Model::lift_unary`), `@F(범위)`는 행·열이 만나면 그 셀에 적용. CONCAT·TEXTJOIN이 함수가 돌려준 배열을 받음. 동적 배열 표시가 없는 수식의 배열 결과는 왼쪽 위 값. `@` 아래 연산식에 괄호 유지.
27. 이름 정의: 상수·수식 이름(`ParsedDefinedName::Formula`), 시트 범위 이름 `Data!이름`, 삭제된 참조 `Sheet2!#REF!`.
28. 이름 정의 보강: 상수·수식 이름을 새로 만들 수 있음(시트 없는 맨 참조만 거부), 이름이 자기 자신을 참조하면 #CIRC!, `'Q1 Sales'!이름`처럼 따옴표 시트의 이름, 시트 이름을 바꾸면 `시트!이름`도 따라 바뀜. `Sheet2!#REF!`는 시트 이름을 유지(`WrongReferenceKind`, 행·열 `i32::MIN`).
29. 다른 통합 문서 참조 `[1]Sheet1!A1`는 존재하지 않는 시트 참조로 읽어 #REF!(수식 텍스트는 유지). FORMULATEXT에 범위를 주면 셀마다 수식 텍스트 배열.
30. 테스트: CellX 기본값(글꼴 11pt, 행 높이, 열 너비 변환)과 구조적 참조 표기에 맞춰 기존 테스트 기대값 갱신, `test_cellx_excel_compat` 추가. CI에서 이 사본의 단위 테스트를 실행.
31. 배열 전파: 스칼라 인수만 받는 함수(약 280개, IF·IFERROR·WORKDAY 등 제외)에 범위나 배열을 주면 원소마다 계산(`Model::broadcast_scalar_call`, Excel처럼 한 행·한 열은 반복, 모자란 칸은 #N/A). 원래 배열만 전파하고 범위는 수식 행과 교차하던 ROUND 등도 포함. 정적 분석에서 인수가 모두 스칼라면 이 경로를 타지 않음. 열 전체 범위는 사용 영역만 읽고 빈 칸 결과는 한 번만 계산(`evaluate_range_clipped`). AVERAGEA 배열, N 원소별, LARGE·SMALL 오류 전파, SHEETS(참조), 다른 시트의 CELL("address").
32. 이름 정의: 상대 참조가 있는 이름(`Sheet1!$A1`)은 쓰는 셀 기준(A1에서 센 거리)으로 계산. 의존 인덱스가 이름 뒤의 참조(셀·범위·수식 이름)를 따라가 증분 재계산 대상이 됨(`collect_with_names`).
33. 다른 통합 문서 시트 이름 `[1]Sheet1`은 따옴표 없이 씀(`quote_name`).
