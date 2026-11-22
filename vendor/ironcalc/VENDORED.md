# ironcalc (xlsx 읽기·쓰기) — CellX 패치

crates.io `ironcalc` 0.9.1의 라이브러리 부분만 가져왔습니다(실행 파일, 예제 실행, 테스트 파일 제외. 문서에 포함되는 예제 3개만 남김). 계산 엔진 패치는 `vendor/ironcalc_base/VENDORED.md`에 있습니다.

1. xlsx 열기: 셀마다 따로 적힌 수식(`E5*F5`, `E6*F6`, …)을 상대 위치로 본 모양이 같으면 A1→R1C1 변환을 한 번만 합니다(`import/worksheets.rs`의 `from_a1_to_rc_cached`). 「이 행」 구조적 참조처럼 위치에 따라 결과가 달라지는 수식은 재사용하지 않습니다. 환경 변수 `CELLX_CHECK_RC_CACHE`를 켜면 모든 수식을 실제로 변환해 재사용 결과와 비교합니다.
