import { useEffect, useState } from "react";

export const LOCALES = ["ko-KR", "en-US", "ja-JP", "zh-CN", "zh-TW"] as const;
export type Locale = (typeof LOCALES)[number];

const messages = {
  "ko-KR": {
    language: "언어", autosaveFailed: "자동 복구 파일을 저장하지 못했습니다",
    cancel: "취소", enter: "입력", insertFunction: "함수 삽입", settings: "환경 설정",
  },
  "en-US": {
    language: "Language", autosaveFailed: "Could not save the recovery file",
    cancel: "Cancel", enter: "Enter", insertFunction: "Insert function", settings: "Settings",
  },
  "ja-JP": {
    language: "言語", autosaveFailed: "自動回復ファイルを保存できませんでした",
    cancel: "キャンセル", enter: "入力", insertFunction: "関数の挿入", settings: "設定",
  },
  "zh-CN": {
    language: "语言", autosaveFailed: "无法保存自动恢复文件",
    cancel: "取消", enter: "输入", insertFunction: "插入函数", settings: "设置",
  },
  "zh-TW": {
    language: "語言", autosaveFailed: "無法儲存自動復原檔案",
    cancel: "取消", enter: "輸入", insertFunction: "插入函數", settings: "設定",
  },
} as const;

export type MessageKey = keyof (typeof messages)["en-US"];

const STORAGE_KEY = "cellx.locale";

export function normalizeLocale(value?: string | null): Locale {
  if (!value) return "en-US";
  const normalized = value.toLowerCase();
  if (normalized.startsWith("ko")) return "ko-KR";
  if (normalized.startsWith("ja")) return "ja-JP";
  if (normalized === "zh-tw" || normalized.startsWith("zh-hant") || normalized === "zh-hk") return "zh-TW";
  if (normalized.startsWith("zh")) return "zh-CN";
  return "en-US";
}

const english: Record<string, string> = {
  "파일":"File","홈":"Home","삽입":"Insert","페이지 레이아웃":"Page Layout","수식":"Formulas","데이터":"Data","검토":"Review","보기":"View","표 디자인":"Table Design",
  "재무":"Financial","논리":"Logical","텍스트":"Text","날짜 및 시간":"Date & Time","찾기/참조":"Lookup & Reference","수학/삼각":"Math & Trig","통계":"Statistical","데이터 시트":"Data Sheet",
  "새로 만들기":"New","열기":"Open","저장":"Save","다른 이름으로 저장":"Save As","인쇄":"Print","옵션":"Options","정보":"Info","클립보드":"Clipboard","붙여넣기":"Paste","잘라내기":"Cut","복사":"Copy",
  "글꼴":"Font","글꼴 크기":"Font Size","굵게":"Bold","기울임꼴":"Italic","밑줄":"Underline","취소선":"Strikethrough","테두리":"Borders","채우기 색":"Fill Color","글꼴 색":"Font Color","자동":"Automatic",
  "맞춤":"Alignment","위쪽 맞춤":"Top Align","가운데 맞춤":"Middle Align","아래쪽 맞춤":"Bottom Align","왼쪽 맞춤":"Align Left","오른쪽 맞춤":"Align Right","자동 줄 바꿈":"Wrap Text","병합하고 가운데 맞춤":"Merge & Center",
  "표시 형식":"Number Format","사용자 지정":"Custom","통화 (₩)":"Currency (₩)","백분율 스타일 (Ctrl+Shift+%)":"Percent Style","정렬":"Sort","정렬 기준":"Sort by","다음 기준":"Then by","오름차순":"Ascending","내림차순":"Descending","필터":"Filter","필터 해제":"Clear Filter","검색":"Search","찾기":"Find","바꾸기":"Replace","닫기":"Close",
  "확인":"OK","취소":"Cancel","입력":"Enter","모두":"All","값":"Values","서식":"Formats","열 너비":"Column Widths","행 높이":"Row Height","행 삽입":"Insert Rows","열 삽입":"Insert Columns","행 삭제":"Delete Rows","열 삭제":"Delete Columns","삭제":"Delete","숨기기":"Hide","숨기기 취소":"Unhide","시트 삽입":"Insert Sheet","시트 삭제":"Delete Sheet","시트 이름 바꾸기":"Rename Sheet","시트 이동":"Move Sheet","시트 복사":"Copy Sheet",
  "셀 서식":"Format Cells","표":"Table","표 만들기":"Create Table","표 스타일":"Table Styles","차트":"Chart","차트 삽입":"Insert Chart","차트 편집":"Edit Chart","세로 막대형":"Column","가로 막대형":"Bar","꺾은선형":"Line","원형":"Pie","도넛형":"Doughnut","영역형":"Area","분산형":"Scatter",
  "피벗 테이블":"PivotTable","피벗 테이블 만들기":"Create PivotTable","슬라이서":"Slicer","슬라이서 삽입":"Insert Slicer","스파크라인":"Sparklines","스파크라인 만들기":"Create Sparklines","조건부 서식":"Conditional Formatting","조건부 서식 규칙 관리자":"Conditional Formatting Rules Manager",
  "데이터 유효성":"Data Validation","중복 값 제거":"Remove Duplicates","텍스트 마법사":"Text to Columns Wizard","텍스트 나누기":"Text to Columns","목록":"List","날짜":"Date","시간":"Time","정수":"Whole number","소수점":"Decimal","설정":"Settings","설명 메시지":"Input Message","오류 메시지":"Error Alert","원본":"Source","최소값":"Minimum","최대값":"Maximum",
  "수식 계산":"Evaluate Formula","실행 취소":"Undo","다시 실행":"Redo","저장 (Ctrl+S)":"Save (Ctrl+S)","열기 (Ctrl+O)":"Open (Ctrl+O)","새 통합 문서 (Ctrl+N)":"New Workbook (Ctrl+N)","인쇄 / PDF로 저장 (Ctrl+P)":"Print / Save as PDF (Ctrl+P)","다른 이름으로 저장 (F12)":"Save As (F12)","붙여넣기 (Ctrl+V)":"Paste (Ctrl+V)","잘라내기 (Ctrl+X)":"Cut (Ctrl+X)","복사 (Ctrl+C)":"Copy (Ctrl+C)","실행 취소 (Ctrl+Z)":"Undo (Ctrl+Z)","다시 실행 (Ctrl+Y)":"Redo (Ctrl+Y)",
  "통합 문서":"Workbook","시트":"Sheet","열":"Column","행":"Row","페이지 번호":"Page Number","페이지 수":"Number of Pages","현재 날짜":"Current Date","현재 시간":"Current Time","파일 이름":"File Name","시트 이름":"Sheet Name","머리글":"Header","바닥글":"Footer","왼쪽 구역":"Left Section","가운데 구역":"Center Section","오른쪽 구역":"Right Section","이동/복사":"Move or Copy","문서 복구":"Document Recovery","새 문서":"New Workbook","준비":"Ready",
  "같음":"Equals","같지 않음":"Does not equal","시작 문자":"Begins with","끝 문자":"Ends with","포함":"Contains","포함하지 않음":"Does not contain","보다 큼":"Greater than","크거나 같음":"Greater than or equal to","보다 작음":"Less than","작거나 같음":"Less than or equal to","숫자 오름차순 정렬":"Sort Smallest to Largest","숫자 내림차순 정렬":"Sort Largest to Smallest","텍스트 오름차순 정렬":"Sort A to Z","텍스트 내림차순 정렬":"Sort Z to A",
  "전체":"All","모든 값":"Any value","정렬 기준을 여러 개 지정합니다":"Specify multiple sort levels","셀 값":"Cell Values","열 방향 계열":"Series in columns","행 방향 계열":"Series in rows","범례":"Legend","추세선(선형)":"Trendline (Linear)","차트 종류":"Chart Type","차트 제목 (비우면 자동)":"Chart Title (blank for automatic)","데이터 범위":"Data Range",
  "예: $B$1":"e.g. $B$1","예: $B$2":"e.g. $B$2","예: 4, 6":"e.g. 4, 6","위치를 A1 형식으로 입력하세요":"Enter the location in A1 notation","결과 셀이 올바르지 않습니다":"The result cell is invalid","시나리오 이름을 입력하세요":"Enter a scenario name","시나리오 관리자":"Scenario Manager","시나리오 추가":"Add Scenario","시나리오 편집":"Edit Scenario","시나리오 요약":"Scenario Summary","시나리오 값":"Scenario Values",
  "데이터 가져오기":"Import Data","데이터 내보내기":"Export Data","가져오는 중":"Importing…","내보내는 중":"Exporting…","저장 중":"Saving…","계산 중":"Calculating…","여는 중":"Opening…","복구 중":"Recovering…","붙여넣는 중":"Pasting…","지우는 중":"Clearing…","바꾸는 중":"Replacing…","찾는 내용이 없습니다.":"No matches found.","복구한 파일입니다. 확인한 뒤 저장하세요.":"This is a recovered file. Review it and save it.",
  "셀 잠금은 시트를 보호해야 적용됩니다(검토 탭 → 시트 보호).":"Cell locking takes effect only when the sheet is protected (Review → Protect Sheet).","내 데이터에 머리글 표시":"My data has headers","모두 선택":"Select All","모두 선택 취소":"Clear Selection","필터 지우기":"Clear Filter","공백 무시":"Ignore blank","드롭다운 표시":"Show dropdown",
  "새 메모":"New Note","메모":"Note","메모 편집":"Edit Note","메모 삭제":"Delete Note","하이퍼링크 삽입":"Insert Hyperlink","하이퍼링크 편집":"Edit Hyperlink","이름 관리자":"Name Manager","정의된 이름":"Defined Names","함수 삽입":"Insert Function","함수 라이브러리":"Function Library","목표값 찾기":"Goal Seek","빠른 채우기":"Flash Fill","부분합":"Subtotal","중복 제거":"Remove Duplicates","쿼리 및 연결":"Queries & Connections",
  "보호":"Protection","보호 해제":"Unprotect","시트 보호":"Protect Sheet","시트 보호 해제":"Unprotect Sheet","통합 문서 보호":"Protect Workbook","통합 문서 보호 해제":"Unprotect Workbook","암호 확인":"Confirm Password","암호 확인이 일치하지 않습니다.":"The confirmation password does not match.",
  "색 없음":"No Color","채우기 없음":"No Fill","테두리 없음":"No Border","모든 테두리":"All Borders","바깥쪽 테두리":"Outside Borders","안쪽 테두리":"Inside Borders","굵은 바깥쪽 테두리":"Thick Outside Borders","아래쪽 테두리":"Bottom Border","위쪽 테두리":"Top Border","왼쪽 테두리":"Left Border","오른쪽 테두리":"Right Border",
  "페이지 설정":"Page Setup","인쇄 미리 보기와 인쇄 (Ctrl+P)":"Print Preview and Print (Ctrl+P)","인쇄 영역 설정":"Set Print Area","인쇄 영역 해제":"Clear Print Area","틀 고정":"Freeze Panes","틀 고정 취소":"Unfreeze Panes","확대/축소":"Zoom","확대/축소 100%":"Zoom 100%",
};
const japanese: Record<string, string> = {
  "파일":"ファイル","홈":"ホーム","삽입":"挿入","페이지 레이아웃":"ページ レイアウト","수식":"数式","데이터":"データ","검토":"校閲","보기":"表示","표 디자인":"テーブル デザイン",
  "새로 만들기":"新規作成","열기":"開く","저장":"保存","다른 이름으로 저장":"名前を付けて保存","인쇄":"印刷","옵션":"オプション","정보":"情報","클립보드":"クリップボード","붙여넣기":"貼り付け","잘라내기":"切り取り","복사":"コピー",
  "글꼴":"フォント","글꼴 크기":"フォント サイズ","굵게":"太字","기울임꼴":"斜体","밑줄":"下線","취소선":"取り消し線","테두리":"罫線","채우기 색":"塗りつぶしの色","글꼴 색":"フォントの色","자동":"自動",
  "맞춤":"配置","위쪽 맞춤":"上揃え","가운데 맞춤":"中央揃え","아래쪽 맞춤":"下揃え","왼쪽 맞춤":"左揃え","오른쪽 맞춤":"右揃え","자동 줄 바꿈":"折り返して全体を表示","병합하고 가운데 맞춤":"セルを結合して中央揃え",
  "표시 형식":"表示形式","사용자 지정":"ユーザー設定","정렬":"並べ替え","정렬 기준":"並べ替えのキー","다음 기준":"次のキー","오름차순":"昇順","내림차순":"降順","필터":"フィルター","필터 해제":"フィルターのクリア","검색":"検索","찾기":"検索","바꾸기":"置換","닫기":"閉じる","확인":"OK","취소":"キャンセル",
  "셀 서식":"セルの書式設定","표":"テーブル","표 만들기":"テーブルの作成","차트":"グラフ","차트 삽입":"グラフの挿入","차트 편집":"グラフの編集","세로 막대형":"縦棒","가로 막대형":"横棒","꺾은선형":"折れ線","원형":"円","도넛형":"ドーナツ","영역형":"面","분산형":"散布図",
  "피벗 테이블":"ピボットテーブル","피벗 테이블 만들기":"ピボットテーブルの作成","슬라이서":"スライサー","슬라이서 삽입":"スライサーの挿入","스파크라인":"スパークライン","조건부 서식":"条件付き書式","데이터 유효성":"データの入力規則","중복 값 제거":"重複の削除","텍스트 나누기":"区切り位置","문서 복구":"ドキュメントの回復","새 문서":"新しいブック","실행 취소":"元に戻す","다시 실행":"やり直し","시트":"シート","행":"行","열":"列","함수 삽입":"関数の挿入",
};
const simplifiedChinese: Record<string, string> = {
  "파일":"文件","홈":"开始","삽입":"插入","페이지 레이아웃":"页面布局","수식":"公式","데이터":"数据","검토":"审阅","보기":"视图","표 디자인":"表格设计",
  "새로 만들기":"新建","열기":"打开","저장":"保存","다른 이름으로 저장":"另存为","인쇄":"打印","옵션":"选项","정보":"信息","클립보드":"剪贴板","붙여넣기":"粘贴","잘라내기":"剪切","복사":"复制",
  "글꼴":"字体","글꼴 크기":"字号","굵게":"加粗","기울임꼴":"斜体","밑줄":"下划线","취소선":"删除线","테두리":"边框","채우기 색":"填充颜色","글꼴 색":"字体颜色","자동":"自动",
  "맞춤":"对齐","위쪽 맞춤":"顶端对齐","가운데 맞춤":"居中","아래쪽 맞춤":"底端对齐","왼쪽 맞춤":"左对齐","오른쪽 맞춤":"右对齐","자동 줄 바꿈":"自动换行","병합하고 가운데 맞춤":"合并后居中",
  "표시 형식":"数字格式","사용자 지정":"自定义","정렬":"排序","정렬 기준":"排序依据","다음 기준":"次要关键字","오름차순":"升序","내림차순":"降序","필터":"筛选","필터 해제":"清除筛选","검색":"搜索","찾기":"查找","바꾸기":"替换","닫기":"关闭","확인":"确定","취소":"取消",
  "셀 서식":"设置单元格格式","표":"表格","표 만들기":"创建表格","차트":"图表","차트 삽입":"插入图表","차트 편집":"编辑图表","세로 막대형":"柱形图","가로 막대형":"条形图","꺾은선형":"折线图","원형":"饼图","도넛형":"圆环图","영역형":"面积图","분산형":"散点图",
  "피벗 테이블":"数据透视表","피벗 테이블 만들기":"创建数据透视表","슬라이서":"切片器","슬라이서 삽입":"插入切片器","스파크라인":"迷你图","조건부 서식":"条件格式","데이터 유효성":"数据验证","중복 값 제거":"删除重复项","텍스트 나누기":"分列","문서 복구":"文档恢复","새 문서":"新建工作簿","실행 취소":"撤消","다시 실행":"重做","시트":"工作表","행":"行","열":"列","함수 삽입":"插入函数",
};
const traditionalChinese: Record<string, string> = {
  "파일":"檔案","홈":"常用","삽입":"插入","페이지 레이아웃":"版面配置","수식":"公式","데이터":"資料","검토":"校閱","보기":"檢視","표 디자인":"表格設計",
  "새로 만들기":"新增","열기":"開啟","저장":"儲存","다른 이름으로 저장":"另存新檔","인쇄":"列印","옵션":"選項","정보":"資訊","클립보드":"剪貼簿","붙여넣기":"貼上","잘라내기":"剪下","복사":"複製",
  "글꼴":"字型","글꼴 크기":"字型大小","굵게":"粗體","기울임꼴":"斜體","밑줄":"底線","취소선":"刪除線","테두리":"框線","채우기 색":"填滿色彩","글꼴 색":"字型色彩","자동":"自動",
  "맞춤":"對齊","위쪽 맞춤":"靠上對齊","가운데 맞춤":"置中","아래쪽 맞춤":"靠下對齊","왼쪽 맞춤":"靠左對齊","오른쪽 맞춤":"靠右對齊","자동 줄 바꿈":"自動換列","병합하고 가운데 맞춤":"合併後置中",
  "표시 형식":"數值格式","사용자 지정":"自訂","정렬":"排序","정렬 기준":"排序依據","다음 기준":"次要排序依據","오름차순":"遞增","내림차순":"遞減","필터":"篩選","필터 해제":"清除篩選","검색":"搜尋","찾기":"尋找","바꾸기":"取代","닫기":"關閉","확인":"確定","취소":"取消",
  "셀 서식":"儲存格格式","표":"表格","표 만들기":"建立表格","차트":"圖表","차트 삽입":"插入圖表","차트 편집":"編輯圖表","세로 막대형":"直條圖","가로 막대형":"橫條圖","꺾은선형":"折線圖","원형":"圓形圖","도넛형":"環圈圖","영역형":"區域圖","분산형":"散佈圖",
  "피벗 테이블":"樞紐分析表","피벗 테이블 만들기":"建立樞紐分析表","슬라이서":"交叉分析篩選器","슬라이서 삽입":"插入交叉分析篩選器","스파크라인":"走勢圖","조건부 서식":"設定格式化的條件","데이터 유효성":"資料驗證","중복 값 제거":"移除重複","텍스트 나누기":"分列","문서 복구":"文件復原","새 문서":"新增活頁簿","실행 취소":"復原","다시 실행":"取消復原","시트":"工作表","행":"列","열":"欄","함수 삽입":"插入函數",
};


Object.assign(english, {
  "기준 추가":"Add Level","기준 삭제":"Delete Level","셀 값":"Cell Values","필터 해제":"Clear Filter","필터 지우기":"Clear Filter",
  "모두 선택":"Select All","모두 선택 취소":"Clear Selection","구분 기호로 분리됨":"Delimited","너비가 일정함":"Fixed Width",
  "구분 기호":"Delimiters","탭":"Tab","세미콜론":"Semicolon","쉼표":"Comma","공백":"Space","기타":"Other",
  "연속된 구분 기호를 하나로 처리":"Treat consecutive delimiters as one","텍스트 한정자":"Text qualifier","나눌 위치 (글자 수, 쉼표로 구분)":"Break positions (character counts, comma-separated)",
  "대상":"Destination","마침":"Finish","다시 시도":"Retry","아니요":"No","예":"Yes","계속하시겠습니까?":"Do you want to continue?",
  "설정":"Settings","설명 메시지":"Input Message","오류 메시지":"Error Alert","제한 대상":"Allow","제한 방법":"Data","최대값":"Maximum",
  "제목":"Title","스타일":"Style","중지":"Stop","경고":"Warning","제한":"Restriction","값 또는 =A1":"Value or =A1",
  "적용할 서식:":"Format to apply:","규칙 (우선 순위 순)":"Rule (in priority order)","적용 대상":"Applies to","규칙 삭제":"Delete Rule",
  "이 시트에는 규칙이 없습니다.":"There are no rules on this sheet.","새 워크시트":"New Worksheet","기존 워크시트":"Existing Worksheet",
  "보고서에 추가할 필드 선택:":"Choose fields to add to the report:","표 또는 범위":"Table or Range","시트 범위":"Sheet Range",
  "행":"Rows","열":"Columns","값":"Values","범례":"Legend","추세선(선형)":"Trendline (Linear)","차트 종류":"Chart Type",
  "차트 제목 (비우면 자동)":"Chart Title (blank for automatic)","데이터 범위":"Data Range","열 방향 계열":"Series in Columns","행 방향 계열":"Series in Rows",
  "참조:":"Reference:","계산(V):":"Evaluate:","원본 이름:":"Source Name:","머리글":"Header","열 개수":"Number of Columns",
  "원하는 데이터 선택":"Select the data","데이터 범위(D):":"Data Range:","스파크라인을 배치할 위치 선택":"Choose where to place the sparklines","위치 범위(L):":"Location Range:",
  "종류":"Type","스파크라인 색":"Sparkline Color","음수·강조 색":"Negative/Highlight Color","데이터 없음":"No data",
  "닫기":"Close","찾을 내용":"Find what","바꿀 내용":"Replace with","대/소문자 구분":"Match case","다음 찾기":"Find Next","모두 바꾸기":"Replace All",
  "이름":"Name","범위":"Scope","참조 대상":"Refers to","편집...":"Edit...","삭제":"Delete","정의된 이름이 없습니다. 이름 상자에 이름을 입력해도 만들 수 있습니다.":"No defined names. You can also create one by entering a name in the Name Box.",
  "표시할 텍스트":"Text to display","주소":"Address","전자 메일 주소":"Email address","링크 제거":"Remove Link","셀 참조":"Cell reference",
  "필터 지우기 (Alt+C)":"Clear Filter (Alt+C)","(비어 있음)":"(Blanks)","(없음)":"(None)",
  "데이터 시트":"Data Sheet","데이터 가져오기":"Import Data","데이터 내보내기":"Export Data","통합 문서 보호":"Protect Workbook","통합 문서 보호 해제":"Unprotect Workbook",
  "선택한 셀의 규칙 지우기":"Clear Rules from Selected Cells","시트 전체에서 규칙 지우기":"Clear Rules from Entire Sheet",
  "숫자 필터":"Number Filters","텍스트 필터":"Text Filters","숫자 오름차순 정렬":"Sort Smallest to Largest","숫자 내림차순 정렬":"Sort Largest to Smallest",
  "텍스트 오름차순 정렬":"Sort A to Z","텍스트 내림차순 정렬":"Sort Z to A","그리고":"And","또는":"Or","검색":"Search","필드 값 없음":"(No field value)",
});
Object.assign(japanese, {
  "기준 추가":"レベルの追加","기준 삭제":"レベルの削除","셀 값":"セルの値","모두 선택":"すべて選択","모두 선택 취소":"選択を解除",
  "구분 기호로 분리됨":"区切り文字","너비가 일정함":"固定幅","구분 기호":"区切り文字","탭":"タブ","세미콜론":"セミコロン","쉼표":"カンマ","공백":"スペース","기타":"その他",
  "연속된 구분 기호를 하나로 처리":"連続した区切り文字を1つとして扱う","텍스트 한정자":"文字列の引用符","대상":"表示先","마침":"完了",
  "다시 시도":"再試行","아니요":"いいえ","예":"はい","계속하시겠습니까?":"続行しますか？","설정":"設定","설명 메시지":"入力メッセージ","오류 메시지":"エラーメッセージ",
  "제한 대상":"入力値の種類","제한 방법":"データ","최대값":"最大値","제목":"タイトル","스타일":"スタイル","중지":"停止","경고":"警告",
  "규칙 삭제":"ルールの削除","적용 대상":"適用先","새 워크시트":"新しいワークシート","기존 워크시트":"既存のワークシート",
  "표 또는 범위":"テーブルまたは範囲","행":"行","열":"列","값":"値","데이터 없음":"データなし","참조:":"参照:","계산(V):":"計算:",
  "머리글":"見出し","열 개수":"列数","원하는 데이터 선택":"データを選択","데이터 범위(D):":"データ範囲:","위치 범위(L):":"配置先の範囲:",
  "종류":"種類","스파크라인 색":"スパークラインの色","닫기":"閉じる","찾을 내용":"検索する文字列","바꿀 내용":"置換後の文字列",
  "대/소문자 구분":"大文字と小文字を区別する","다음 찾기":"次を検索","모두 바꾸기":"すべて置換","이름":"名前","범위":"範囲","참조 대상":"参照先",
  "편집...":"編集...","삭제":"削除","표시할 텍스트":"表示文字列","주소":"アドレス","전자 메일 주소":"電子メール アドレス","링크 제거":"リンクの削除",
  "필터 지우기":"フィルターのクリア","(비어 있음)":"(空白)","(없음)":"(なし)","숫자 필터":"数値フィルター","텍스트 필터":"テキストフィルター",
  "그리고":"かつ","또는":"または","필드 값 없음":"(フィールド値なし)",
});
Object.assign(simplifiedChinese, {
  "기준 추가":"添加级别","기준 삭제":"删除级别","셀 값":"单元格值","모두 선택":"全选","모두 선택 취소":"取消全选",
  "구분 기호로 분리됨":"分隔符号","너비가 일정함":"固定宽度","구분 기호":"分隔符","탭":"制表符","세미콜론":"分号","쉼표":"逗号","공백":"空格","기타":"其他",
  "연속된 구분 기호를 하나로 처리":"将连续分隔符视为一个","텍스트 한정자":"文本限定符","대상":"目标区域","마침":"完成",
  "다시 시도":"重试","아니요":"否","예":"是","계속하시겠습니까?":"是否继续？","설정":"设置","설명 메시지":"输入信息","오류 메시지":"错误警告",
  "제한 대상":"允许","제한 방법":"数据","최대값":"最大值","제목":"标题","스타일":"样式","중지":"停止","경고":"警告",
  "규칙 삭제":"删除规则","적용 대상":"应用于","새 워크시트":"新工作表","기존 워크시트":"现有工作表",
  "표 또는 범위":"表或区域","행":"行","열":"列","값":"值","데이터 없음":"无数据","참조:":"引用:","계산(V):":"计算:",
  "머리글":"标题行","열 개수":"列数","원하는 데이터 선택":"选择数据","데이터 범위(D):":"数据范围:","위치 범위(L):":"位置范围:",
  "종류":"类型","스파크라인 색":"迷你图颜色","닫기":"关闭","찾을 내용":"查找内容","바꿀 내용":"替换为",
  "대/소문자 구분":"区分大小写","다음 찾기":"查找下一个","모두 바꾸기":"全部替换","이름":"名称","범위":"范围","참조 대상":"引用位置",
  "편집...":"编辑...","삭제":"删除","표시할 텍스트":"要显示的文字","주소":"地址","전자 메일 주소":"电子邮件地址","링크 제거":"删除链接",
  "필터 지우기":"清除筛选","(비어 있음)":"(空白)","(없음)":"(无)","숫자 필터":"数字筛选","텍스트 필터":"文本筛选",
  "그리고":"与","또는":"或","필드 값 없음":"(无字段值)",
});
Object.assign(traditionalChinese, {
  "기준 추가":"新增層級","기준 삭제":"刪除層級","셀 값":"儲存格值","모두 선택":"全選","모두 선택 취소":"取消全選",
  "구분 기호로 분리됨":"分隔符號","너비가 일정함":"固定寬度","구분 기호":"分隔符號","탭":"定位點","세미콜론":"分號","쉼표":"逗號","공백":"空格","기타":"其他",
  "연속된 구분 기호를 하나로 처리":"將連續分隔符號視為一個","텍스트 한정자":"文字限定詞","대상":"目的地","마침":"完成",
  "다시 시도":"重試","아니요":"否","예":"是","계속하시겠습니까?":"要繼續嗎？","설정":"設定","설명 메시지":"輸入訊息","오류 메시지":"錯誤警示",
  "제한 대상":"允許","제한 방법":"資料","최대값":"最大值","제목":"標題","스타일":"樣式","중지":"停止","경고":"警告",
  "규칙 삭제":"刪除規則","적용 대상":"套用到","새 워크시트":"新工作表","기존 워크시트":"現有工作表",
  "표 또는 범위":"表格或範圍","행":"列","열":"欄","값":"值","데이터 없음":"沒有資料","참조:":"參照:","계산(V):":"計算:",
  "머리글":"標題列","열 개수":"欄數","원하는 데이터 선택":"選取資料","데이터 범위(D):":"資料範圍:","위치 범위(L):":"位置範圍:",
  "종류":"類型","스파크라인 색":"走勢圖色彩","닫기":"關閉","찾을 내용":"尋找目標","바꿀 내용":"取代為",
  "대/소문자 구분":"區分大小寫","다음 찾기":"尋找下一個","모두 바꾸기":"全部取代","이름":"名稱","범위":"範圍","참조 대상":"參照到",
  "편집...":"編輯...","삭제":"刪除","표시할 텍스트":"要顯示的文字","주소":"位址","전자 메일 주소":"電子郵件地址","링크 제거":"移除連結",
  "필터 지우기":"清除篩選","(비어 있음)":"(空白)","(없음)":"(無)","숫자 필터":"數值篩選","텍스트 필터":"文字篩選",
  "그리고":"且","또는":"或","필드 값 없음":"(沒有欄位值)",
});


Object.assign(english, {
  "계산 중":"Calculating…","저장 중":"Saving…","붙여넣는 중":"Pasting…","지우는 중":"Clearing…","가져오는 중":"Importing…","내보내는 중":"Exporting…","여는 중":"Opening…","복구 중":"Recovering…","바꾸는 중":"Replacing…",
  "서식":"Format","행 삽입":"Insert Rows","열 삽입":"Insert Columns","행 삭제":"Delete Rows","열 삭제":"Delete Columns","행 높이":"Row Height","열 너비":"Column Width","행 높이 (픽셀)":"Row Height (pixels)","열 너비 (픽셀)":"Column Width (pixels)",
  "새 문서":"New Workbook","통합 문서1":"Workbook1","통합 문서1.xlsx":"Workbook1.xlsx","모든 파일":"All Files","Excel 통합 문서":"Excel Workbook","Excel 통합 문서 / CellX 프로젝트":"Excel Workbook / CellX Project","CellX 프로젝트 (데이터 시트 포함)":"CellX Project (includes data sheets)","데이터 파일":"Data File","그림":"Picture",
  "데이터 시트에는 서식을 적용할 수 없습니다":"Formatting is not available on data sheets","데이터 시트에는 붙여넣을 수 없습니다":"Pasting is not available on data sheets",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"The active cell has no formula or precedents","현재 셀을 참조하는 수식이 없습니다":"No formulas refer to the active cell",
  "찾는 내용이 없습니다.":"No matches found.","복구한 파일입니다. 확인한 뒤 저장하세요.":"This file was recovered. Review it and save it.",
  "일반 시트만 인쇄할 수 있습니다":"Only regular worksheets can be printed","스파크라인은 일반 시트에 넣습니다":"Sparklines can only be inserted on regular worksheets","도형은 일반 시트에 넣습니다":"Shapes can only be inserted on regular worksheets","그림은 일반 시트에 넣을 수 있습니다":"Pictures can only be inserted on regular worksheets","차트는 일반 시트의 범위로 만듭니다":"Charts are created from ranges on regular worksheets",
  "스파크라인 만들기":"Create Sparklines","슬라이서를 만들 표 안의 셀을 선택하세요":"Select a cell inside the table to create a slicer","위치 범위가 올바르지 않습니다 (예: F2:F10)":"The location range is invalid (e.g. F2:F10)",
  "이 기능은 일반 시트에서 씁니다. 데이터 시트는 아래 필터 줄과 열 머리글로 정렬·필터합니다.":"This feature is available on regular worksheets. Sort and filter data sheets using the filter row and column headers.",
  "데이터 시트 가져오기":"Import Data Sheet","데이터 시트 이름 (수식에서 이름[열]로 참조합니다)":"Data sheet name (reference it in formulas as Name[Column])",
  "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"Saved as xlsx. Data sheets are only saved in .cellx projects",
  "데이터 엔진 계산 시간":"Data engine calculation time","새 시트 (Shift+F11)":"New Sheet (Shift+F11)","시트 이름 바꾸기":"Rename Sheet","새 이름":"New Name",
  "이름 바꾸기":"Rename","이름 관리자":"Name Manager","정렬":"Sort","필터":"Filter","표":"Table","슬라이서":"Slicer","도형 삽입":"Insert Shape","그림 삽입":"Insert Picture","그림 붙여넣기":"Paste Picture",
  "차트로 만들 데이터 범위를 선택하세요":"Select the data range for the chart","클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"There is no content to paste. Try Ctrl+V.",
  "변경하려는 셀이나 차트가 보호된 시트에 있습니다. 변경하려면 검토 탭에서 시트 보호를 해제하세요. 암호를 입력해야 할 수도 있습니다.":"The cell or chart you are trying to change is on a protected sheet. Unprotect the sheet on the Review tab. You may need to enter a password.",
  "1904 날짜 체계":"1904 Date System","엑셀 호환 고성능 스프레드시트":"High-performance spreadsheet compatible with Excel",
  "행":"Rows","열":"Columns","이름":"Name","범위":"Scope","참조 대상":"Refers to","찾을 내용":"Find what","바꿀 내용":"Replace with",
  "찾기":"Find","바꾸기":"Replace","표시 형식":"Number","가로":"Horizontal","세로":"Vertical","위쪽":"Top","왼쪽":"Left","가운데":"Center","오른쪽":"Right","아래쪽":"Bottom","일반":"General",
  "파일":"File","새 통합 문서":"New Workbook","새 통합 문서 (Ctrl+N)":"New Workbook (Ctrl+N)","저장했습니다: apps/desktop/src/i18n.ts":"Saved: apps/desktop/src/i18n.ts",
});
Object.assign(japanese, {
  "계산 중":"計算中…","저장 중":"保存中…","붙여넣는 중":"貼り付け中…","지우는 중":"クリア中…","가져오는 중":"読み込み中…","내보내는 중":"エクスポート中…","여는 중":"開いています…","복구 중":"復元中…","바꾸는 중":"置換中…",
  "서식":"書式","행 삽입":"行の挿入","열 삽입":"列の挿入","행 삭제":"行の削除","열 삭제":"列の削除","행 높이":"行の高さ","열 너비":"列の幅","행 높이 (픽셀)":"行の高さ (ピクセル)","열 너비 (픽셀)":"列の幅 (ピクセル)",
  "모든 파일":"すべてのファイル","Excel 통합 문서":"Excel ブック","데이터 파일":"データ ファイル","그림":"画像","데이터 시트에는 서식을 적용할 수 없습니다":"データ シートでは書式を適用できません","데이터 시트에는 붙여넣을 수 없습니다":"データ シートには貼り付けできません",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"選択セルに数式または参照元がありません","현재 셀을 참조하는 수식이 없습니다":"選択セルを参照する数式はありません","찾는 내용이 없습니다.":"一致するデータが見つかりません。","일반 시트만 인쇄할 수 있습니다":"通常のワークシートのみ印刷できます",
  "스파크라인 만들기":"スパークラインの作成","슬라이서를 만들 표 안의 셀을 선택하세요":"スライサーを作成するにはテーブル内のセルを選択してください","데이터 시트 가져오기":"データ シートのインポート",
  "데이터 시트 이름 (수식에서 이름[열]로 참조합니다)":"データ シート名 (数式では名前[列]で参照)","xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"xlsx として保存しました。データ シートは .cellx プロジェクトにのみ保存されます",
  "엑셀 호환 고성능 스프레드시트":"Excel 互換の高性能スプレッドシート","찾기":"検索","바꾸기":"置換","가로":"横","세로":"縦","위쪽":"上","왼쪽":"左","가운데":"中央","오른쪽":"右","아래쪽":"下","일반":"標準",
});
Object.assign(simplifiedChinese, {
  "계산 중":"正在计算…","저장 중":"正在保存…","붙여넣는 중":"正在粘贴…","지우는 중":"正在清除…","가져오는 중":"正在导入…","내보내는 중":"正在导出…","여는 중":"正在打开…","복구 중":"正在恢复…","바꾸는 중":"正在替换…",
  "서식":"格式","행 삽입":"插入行","열 삽입":"插入列","행 삭제":"删除行","열 삭제":"删除列","행 높이":"行高","열 너비":"列宽","행 높이 (픽셀)":"行高（像素）","열 너비 (픽셀)":"列宽（像素）",
  "모든 파일":"所有文件","Excel 통합 문서":"Excel 工作簿","데이터 파일":"数据文件","그림":"图片","데이터 시트에는 서식을 적용할 수 없습니다":"数据工作表不支持设置格式","데이터 시트에는 붙여넣을 수 없습니다":"数据工作表不支持粘贴",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"当前单元格没有公式或引用单元格","현재 셀을 참조하는 수식이 없습니다":"没有公式引用当前单元格","찾는 내용이 없습니다.":"未找到匹配项。","일반 시트만 인쇄할 수 있습니다":"只能打印普通工作表",
  "스파크라인 만들기":"创建迷你图","슬라이서를 만들 표 안의 셀을 선택하세요":"请选择表格中的单元格以创建切片器","데이터 시트 가져오기":"导入数据工作表",
  "데이터 시트 이름 (수식에서 이름[열]로 참조합니다)":"数据工作表名称（在公式中使用 名称[列] 引用）","xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"已保存为 xlsx。数据工作表仅保存在 .cellx 项目中",
  "엑셀 호환 고성능 스프레드시트":"兼容 Excel 的高性能电子表格","찾기":"查找","바꾸기":"替换","가로":"水平","세로":"垂直","위쪽":"上","왼쪽":"左","가운데":"居中","오른쪽":"右","아래쪽":"下","일반":"常规",
});
Object.assign(traditionalChinese, {
  "계산 중":"正在計算…","저장 중":"正在儲存…","붙여넣는 중":"正在貼上…","지우는 중":"正在清除…","가져오는 중":"正在匯入…","내보내는 중":"正在匯出…","여는 중":"正在開啟…","복구 중":"正在復原…","바꾸는 중":"正在取代…",
  "서식":"格式","행 삽입":"插入列","열 삽입":"插入欄","행 삭제":"刪除列","열 삭제":"刪除欄","행 높이":"列高","열 너비":"欄寬","행 높이 (픽셀)":"列高（像素）","열 너비 (픽셀)":"欄寬（像素）",
  "모든 파일":"所有檔案","Excel 통합 문서":"Excel 活頁簿","데이터 파일":"資料檔案","그림":"圖片","데이터 시트에는 서식을 적용할 수 없습니다":"資料工作表不支援套用格式","데이터 시트에는 붙여넣을 수 없습니다":"資料工作表不支援貼上",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"目前儲存格沒有公式或參照儲存格","현재 셀을 참조하는 수식이 없습니다":"沒有公式參照目前儲存格","찾는 내용이 없습니다.":"找不到符合的項目。","일반 시트만 인쇄할 수 있습니다":"只能列印一般工作表",
  "스파크라인 만들기":"建立走勢圖","슬라이서를 만들 표 안의 셀을 선택하세요":"請選取表格中的儲存格以建立交叉分析篩選器","데이터 시트 가져오기":"匯入資料工作表",
  "데이터 시트 이름 (수식에서 이름[열]로 참조합니다)":"資料工作表名稱（在公式中使用 名稱[欄] 參照）","xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"已儲存為 xlsx。資料工作表僅儲存在 .cellx 專案中",
  "엑셀 호환 고성능 스프레드시트":"相容 Excel 的高效能試算表","찾기":"尋找","바꾸기":"取代","가로":"水平","세로":"垂直","위쪽":"上","왼쪽":"左","가운데":"置中","오른쪽":"右","아래쪽":"下","일반":"一般",
});


Object.assign(english, {
  "시트를 옮길 위치":"Move the sheet to","미리 설정된":"Preset","숨김":"Hidden","(끝으로 이동)":"(Move to end)","밝게 1":"Light 1","밝게 9":"Light 9",
  "보통 2":"Medium 2","보통 3":"Medium 3","보통 4":"Medium 4","보통 5":"Medium 5","보통 6":"Medium 6","보통 7":"Medium 7","보통 9":"Medium 9","어둡게 1":"Dark 1",
  "페이지 번호":"Page Number","페이지 수":"Number of Pages","현재 날짜":"Current Date","현재 시간":"Current Time","파일 이름":"File Name","시트 이름":"Sheet Name",
  "1 / 3 페이지":"Page 1 of 3","&P 페이지":"Page &P","시트 이름 · 페이지":"Sheet Name · Page","날짜 · 페이지":"Date · Page",
  "왼쪽 구역":"Left Section","가운데 구역":"Center Section","오른쪽 구역":"Right Section",
  "모든 값":"Any Value","해당 범위":"Between","제외 범위":"Not Between","포함하지 않음":"Does Not Contain","시작 문자":"Begins With","끝 문자":"Ends With",
  "숫자 필터":"Number Filters","텍스트 필터":"Text Filters","시트 범위":"Sheet Range",
  "수식 분석":"Formula Auditing","참조되는 셀 추적":"Trace Precedents","참조하는 셀 추적":"Trace Dependents","연결선 제거":"Remove Arrows","수식 표시":"Show Formulas",
  "자동 합계":"AutoSum","정의된 이름":"Defined Names","이름 정의":"Define Name","표 서식":"Format as Table","표 서식 (Ctrl+T)":"Format as Table (Ctrl+T)",
  "표의 열로 필터링하는 슬라이서를 삽입합니다":"Insert a slicer to filter a table by column","텍스트 상자 삽입":"Insert Text Box","텍스트 상자":"Text Box",
  "데이터 도구":"Data Tools","쿼리 실행 중":"Running query…","피벗 테이블을 새로 고쳤습니다":"PivotTable refreshed","모든 메모 표시":"Show All Notes","메모 삭제":"Delete Note",
});
Object.assign(japanese, {
  "시트를 옮길 위치":"シートの移動先","미리 설정된":"プリセット","숨김":"非表示","(끝으로 이동)":"(末尾へ移動)","밝게 1":"ライト 1","밝게 9":"ライト 9",
  "보통 2":"ミディアム 2","보통 3":"ミディアム 3","보통 4":"ミディアム 4","보통 5":"ミディアム 5","보통 6":"ミディアム 6","보통 7":"ミディアム 7","보통 9":"ミディアム 9","어둡게 1":"ダーク 1",
  "페이지 번호":"ページ番号","페이지 수":"ページ数","현재 날짜":"現在の日付","현재 시간":"現在の時刻","파일 이름":"ファイル名","시트 이름":"シート名",
  "1 / 3 페이지":"1 / 3 ページ","&P 페이지":"&P ページ","시트 이름 · 페이지":"シート名 · ページ","날짜 · 페이지":"日付 · ページ",
  "왼쪽 구역":"左側","가운데 구역":"中央","오른쪽 구역":"右側",
  "수식 분석":"数式の検証","참조되는 셀 추적":"参照元のトレース","참조하는 셀 추적":"参照先のトレース","연결선 제거":"矢印の削除","수식 표시":"数式の表示",
  "자동 합계":"オートSUM","정의된 이름":"定義された名前","이름 정의":"名前の定義","표 서식":"テーブルとして書式設定","텍스트 상자 삽입":"テキスト ボックスの挿入","텍스트 상자":"テキスト ボックス",
});
Object.assign(simplifiedChinese, {
  "시트를 옮길 위치":"将工作表移至","미리 설정된":"预设","숨김":"隐藏","(끝으로 이동)":"(移至末尾)","밝게 1":"浅色 1","밝게 9":"浅色 9",
  "보통 2":"中等 2","보통 3":"中等 3","보통 4":"中等 4","보통 5":"中等 5","보통 6":"中等 6","보통 7":"中等 7","보통 9":"中等 9","어둡게 1":"深色 1",
  "페이지 번호":"页码","페이지 수":"总页数","현재 날짜":"当前日期","현재 시간":"当前时间","파일 이름":"文件名","시트 이름":"工作表名称",
  "1 / 3 페이지":"第 1 / 3 页","&P 페이지":"第 &P 页","시트 이름 · 페이지":"工作表名称 · 页码","날짜 · 페이지":"日期 · 页码",
  "왼쪽 구역":"左侧区域","가운데 구역":"中间区域","오른쪽 구역":"右侧区域","수식 분석":"公式审核","참조되는 셀 추적":"追踪引用单元格","참조하는 셀 추적":"追踪从属单元格","연결선 제거":"删除箭头","수식 표시":"显示公式",
  "자동 합계":"自动求和","정의된 이름":"定义的名称","이름 정의":"定义名称","표 서식":"套用表格格式","텍스트 상자 삽입":"插入文本框","텍스트 상자":"文本框",
});
Object.assign(traditionalChinese, {
  "시트를 옮길 위치":"工作表移至","미리 설정된":"預設","숨김":"隱藏","(끝으로 이동)":"(移至最後)","밝게 1":"淺色 1","밝게 9":"淺色 9",
  "보통 2":"中等 2","보통 3":"中等 3","보통 4":"中等 4","보통 5":"中等 5","보통 6":"中等 6","보통 7":"中等 7","보통 9":"中等 9","어둡게 1":"深色 1",
  "페이지 번호":"頁碼","페이지 수":"總頁數","현재 날짜":"目前日期","현재 시간":"目前時間","파일 이름":"檔案名稱","시트 이름":"工作表名稱",
  "1 / 3 페이지":"第 1 / 3 頁","&P 페이지":"第 &P 頁","시트 이름 · 페이지":"工作表名稱 · 頁碼","날짜 · 페이지":"日期 · 頁碼",
  "왼쪽 구역":"左側區域","가운데 구역":"中央區域","오른쪽 구역":"右側區域","수식 분석":"公式稽核","참조되는 셀 추적":"追蹤前導參照","참조하는 셀 추적":"追蹤從屬參照","연결선 제거":"移除箭號","수식 표시":"顯示公式",
  "자동 합계":"自動加總","정의된 이름":"已定義名稱","이름 정의":"定義名稱","표 서식":"套用表格格式","텍스트 상자 삽입":"插入文字方塊","텍스트 상자":"文字方塊",
});


Object.assign(english, {
  "조건: 서울, >=100, *카드*, <>취소":"Criteria: Seoul, >=100, *Card*, <>Canceled",
  "서울,부산,대구 또는 =$E$1:$E$10":"Seoul, Busan, Daegu or =$E$1:$E$10",
  "(필드 값 없음)":"(No field value)",
});
Object.assign(japanese, {
  "조건: 서울, >=100, *카드*, <>취소":"条件: ソウル, >=100, *カード*, <>取消",
  "서울,부산,대구 또는 =$E$1:$E$10":"ソウル、釜山、大邱 または =$E$1:$E$10",
  "(필드 값 없음)":"(フィールド値なし)",
});
Object.assign(simplifiedChinese, {
  "조건: 서울, >=100, *카드*, <>취소":"条件: 首尔, >=100, *银行卡*, <>取消",
  "서울,부산,대구 또는 =$E$1:$E$10":"首尔、釜山、大邱或 =$E$1:$E$10",
  "(필드 값 없음)":"(无字段值)",
});
Object.assign(traditionalChinese, {
  "조건: 서울, >=100, *카드*, <>취소":"條件: 首爾, >=100, *卡片*, <>取消",
  "서울,부산,대구 또는 =$E$1:$E$10":"首爾、釜山、大邱或 =$E$1:$E$10",
  "(필드 값 없음)":"(沒有欄位值)",
});


Object.assign(english, {
  "테마 색":"Theme Colors","표준 색":"Standard Colors","도형 서식":"Format Shape","선 두께(px)":"Line Width (px)","글꼴 크기(pt)":"Font Size (pt)","가로 맞춤":"Horizontal Alignment","세로 맞춤":"Vertical Alignment",
  "의 변경 내용을 저장하시겠습니까?":"Do you want to save changes to","저장하지 않으면 변경 내용이 사라집니다.":"If you do not save, your changes will be lost.","저장 안 함":"Don't Save",
  "CellX가 정상적으로 종료되지 않았습니다. 자동으로 저장해 둔 다음 파일을 복구할 수 있습니다.":"CellX did not shut down properly. You can recover the following automatically saved files.",
  "자동 저장":"Autosaved","원래 위치:":"Original location:","인쇄 대상":"Print what","활성 시트 인쇄":"Print Active Sheet","선택 영역 인쇄":"Print Selection",
  "용지 방향":"Orientation","세로 방향":"Portrait","가로 방향":"Landscape","현재 크기":"No Scaling","모든 열을 한 페이지에 맞추기":"Fit All Columns on One Page","한 페이지에 시트 맞추기":"Fit Sheet on One Page",
  "인쇄 제목 (반복할 행)":"Print Titles (Rows to Repeat)","눈금선 인쇄":"Print Gridlines","행/열 머리글 인쇄":"Print Row and Column Headings","바닥글에 페이지 번호":"Page Numbers in Footer",
  "PDF는 인쇄 창에서 「PDF로 저장」(Windows: Microsoft Print to PDF)을 고르세요.":"To save as PDF, choose “Save as PDF” in the print dialog (Windows: Microsoft Print to PDF).","페이지":"Page",
  "예: 1:1":"e.g. 1:1","선":"Line","화살표":"Arrow","직사각형":"Rectangle","사각형: 둥근 모서리":"Rounded Rectangle","타원":"Oval","이등변 삼각형":"Isosceles Triangle","다이아몬드":"Diamond","화살표: 오른쪽":"Right Arrow","화살표: 오각형":"Pentagon",
  "없음":"None","채우기":"Fill","글꼴 색":"Font Color","굵게":"Bold","왼쪽":"Left","가운데":"Center","오른쪽":"Right","위쪽":"Top","아래쪽":"Bottom",
});
Object.assign(japanese, {
  "테마 색":"テーマの色","표준 색":"標準の色","도형 서식":"図形の書式設定","선 두께(px)":"線の太さ (px)","글꼴 크기(pt)":"フォント サイズ (pt)","가로 맞춤":"横位置","세로 맞춤":"縦位置",
  "의 변경 내용을 저장하시겠습니까?":"の変更内容を保存しますか？","저장하지 않으면 변경 내용이 사라집니다.":"保存しないと変更内容は失われます。","저장 안 함":"保存しない",
  "CellX가 정상적으로 종료되지 않았습니다. 자동으로 저장해 둔 다음 파일을 복구할 수 있습니다.":"CellX は正常に終了しませんでした。自動保存された次のファイルを復元できます。",
  "자동 저장":"自動保存","원래 위치:":"元の場所:","인쇄 대상":"印刷対象","활성 시트 인쇄":"作業中のシートを印刷","선택 영역 인쇄":"選択範囲を印刷",
  "용지 방향":"印刷の向き","세로 방향":"縦","가로 방향":"横","현재 크기":"拡大縮小なし","모든 열을 한 페이지에 맞추기":"すべての列を1ページに印刷","한 페이지에 시트 맞추기":"シートを1ページに印刷",
  "인쇄 제목 (반복할 행)":"印刷タイトル (繰り返す行)","눈금선 인쇄":"枠線を印刷","행/열 머리글 인쇄":"行列番号を印刷","바닥글에 페이지 번호":"フッターにページ番号を表示",
  "PDF는 인쇄 창에서 「PDF로 저장」(Windows: Microsoft Print to PDF)을 고르세요.":"PDF は印刷ダイアログで「PDF として保存」(Windows: Microsoft Print to PDF) を選択してください。","페이지":"ページ",
  "예: 1:1":"例: 1:1","선":"線","화살표":"矢印","직사각형":"四角形","사각형: 둥근 모서리":"角丸四角形","타원":"楕円","이등변 삼각형":"二等辺三角形","다이아몬드":"ひし形","화살표: 오른쪽":"右矢印","화살표: 오각형":"五角形",
  "없음":"なし","채우기":"塗りつぶし","굵게":"太字","왼쪽":"左","가운데":"中央","오른쪽":"右","위쪽":"上","아래쪽":"下",
});
Object.assign(simplifiedChinese, {
  "테마 색":"主题颜色","표준 색":"标准颜色","도형 서식":"设置形状格式","선 두께(px)":"线条宽度 (px)","글꼴 크기(pt)":"字体大小 (pt)","가로 맞춤":"水平对齐","세로 맞춤":"垂直对齐",
  "의 변경 내용을 저장하시겠습니까?":"的更改要保存吗？","저장하지 않으면 변경 내용이 사라집니다.":"如果不保存，更改将会丢失。","저장 안 함":"不保存",
  "CellX가 정상적으로 종료되지 않았습니다. 자동으로 저장해 둔 다음 파일을 복구할 수 있습니다.":"CellX 未正常关闭。您可以恢复以下自动保存的文件。",
  "자동 저장":"自动保存","원래 위치:":"原始位置：","인쇄 대상":"打印内容","활성 시트 인쇄":"打印活动工作表","선택 영역 인쇄":"打印选定区域",
  "용지 방향":"页面方向","세로 방향":"纵向","가로 방향":"横向","현재 크기":"无缩放","모든 열을 한 페이지에 맞추기":"将所有列调整为一页","한 페이지에 시트 맞추기":"将工作表调整为一页",
  "인쇄 제목 (반복할 행)":"打印标题（重复行）","눈금선 인쇄":"打印网格线","행/열 머리글 인쇄":"打印行列标题","바닥글에 페이지 번호":"在页脚显示页码",
  "PDF는 인쇄 창에서 「PDF로 저장」(Windows: Microsoft Print to PDF)을 고르세요.":"要保存为 PDF，请在打印对话框中选择“另存为 PDF”（Windows: Microsoft Print to PDF）。","페이지":"页",
  "예: 1:1":"例如：1:1","선":"直线","화살표":"箭头","직사각형":"矩形","사각형: 둥근 모서리":"圆角矩形","타원":"椭圆","이등변 삼각형":"等腰三角形","다이아몬드":"菱形","화살표: 오른쪽":"右箭头","화살표: 오각형":"五边形",
  "없음":"无","채우기":"填充","굵게":"加粗","왼쪽":"左","가운데":"居中","오른쪽":"右","위쪽":"上","아래쪽":"下",
});
Object.assign(traditionalChinese, {
  "테마 색":"佈景主題色彩","표준 색":"標準色彩","도형 서식":"設定圖案格式","선 두께(px)":"線條粗細 (px)","글꼴 크기(pt)":"字型大小 (pt)","가로 맞춤":"水平對齊","세로 맞춤":"垂直對齊",
  "의 변경 내용을 저장하시겠습니까?":"的變更要儲存嗎？","저장하지 않으면 변경 내용이 사라집니다.":"如果不儲存，變更將會遺失。","저장 안 함":"不儲存",
  "CellX가 정상적으로 종료되지 않았습니다. 자동으로 저장해 둔 다음 파일을 복구할 수 있습니다.":"CellX 未正常結束。您可以復原下列自動儲存的檔案。",
  "자동 저장":"自動儲存","원래 위치:":"原始位置：","인쇄 대상":"列印內容","활성 시트 인쇄":"列印使用中的工作表","선택 영역 인쇄":"列印選取範圍",
  "용지 방향":"版面方向","세로 방향":"直向","가로 방향":"橫向","현재 크기":"不縮放","모든 열을 한 페이지에 맞추기":"將所有欄調整成一頁","한 페이지에 시트 맞추기":"將工作表調整成一頁",
  "인쇄 제목 (반복할 행)":"列印標題（重複列）","눈금선 인쇄":"列印格線","행/열 머리글 인쇄":"列印列與欄標題","바닥글에 페이지 번호":"在頁尾顯示頁碼",
  "PDF는 인쇄 창에서 「PDF로 저장」(Windows: Microsoft Print to PDF)을 고르세요.":"若要儲存為 PDF，請在列印對話方塊中選擇「另存為 PDF」（Windows: Microsoft Print to PDF）。","페이지":"頁",
  "예: 1:1":"例如：1:1","선":"直線","화살표":"箭頭","직사각형":"矩形","사각형: 둥근 모서리":"圓角矩形","타원":"橢圓","이등변 삼각형":"等腰三角形","다이아몬드":"菱形","화살표: 오른쪽":"向右箭頭","화살표: 오각형":"五邊形",
  "없음":"無","채우기":"填滿","굵게":"粗體","왼쪽":"靠左","가운데":"置中","오른쪽":"靠右","위쪽":"靠上","아래쪽":"靠下",
});


Object.assign(english, {
  "한국어":"Korean","로 실행하세요.":" to run it.","개의 레코드가 있습니다":" records",
  "수식에서":"In a formula,","· 수식 입력 중 열을 클릭하면 참조가 들어갑니다":"· Click a column while editing a formula to insert a reference",
  "SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.":"Only SELECT statements are supported. Results are placed in a new data sheet.",
  "결과 시트 이름":"Result sheet name","실행":"Run","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"No data yet. Open a CSV from Data → Import Data.",
  "시트에서 쓰는 법":"How to use in a sheet","데이터 없음":"No data","+ 기준 추가":"+ Add Level","× 기준 삭제":"× Delete Level",
  "?는 한 글자, *는 여러 글자를 나타냅니다.":"? matches one character; * matches multiple characters.","(모두 선택)":"(Select all)",
  "중복 값을 삭제하려면 중복 값이 포함된 열을 하나 이상 선택하세요.":"Select at least one column to identify duplicate values.",
  "셀을 선택하면 설명 메시지 표시":"Show input message when cell is selected","유효하지 않은 데이터를 입력하면 오류 메시지 표시":"Show error alert when invalid data is entered",
  "모두 지우기":"Clear All","찾기 및 바꾸기":"Find and Replace","전체 셀 내용 일치":"Match entire cell contents","범위: 시트":"Within: Sheet","범위: 통합 문서":"Within: Workbook",
  "새로 만들기...":"New...","웹 페이지":"Web page","현재 문서":"Current document","수식 셀":"Formula cells","찾는 값":"Find what","값을 바꿀 셀":"Replace with","예: B2":"e.g. B2",
  "그룹화할 항목":"At each change in","사용할 함수":"Use function","합계":"Sum","개수":"Count","평균":"Average","최대":"Max","최소":"Min",
  "부분합 계산 항목":"Add subtotal to","새로운 값으로 대치":"Replace with new value","먼저 그룹화할 열로 데이터를 정렬해 두세요.":"Sort the data by the grouping column first.",
  "모두 제거":"Remove All","표에 사용할 데이터를 지정하십시오.":"Specify the data for the table.","머리글 포함":"My table has headers",
  "시트 보호 해제 암호 (선택)":"Unprotect sheet password (optional)","워크시트의 모든 사용자에게 허용할 내용 (잠긴 셀 내용 변경은 항상 막힙니다):":"Allow all users of this worksheet to:",
  "계속하려면 암호를 다시 입력하십시오.":"Re-enter the password to continue.","암호를 잊어버리면 복구할 수 없습니다. Excel과 같은 방식의 보호여서 강력한 보안 수단은 아닙니다.":"If you forget the password, it cannot be recovered. This protection works like Excel and is not a strong security measure.",
  "암호":"Password","형식 코드 (사용자 지정)":"Format code (Custom)","크기":"Size","잠금":"Locked",
  "선택하여 붙여넣기":"Paste Special","연산":"Operation","내용 있는 셀만 붙여넣기":"Skip blanks","행/열 바꿈":"Transpose","가나다 AaBbCc 123":"가나다 AaBbCc 123",
  "데이터 시트:":"Data sheet:","머리글 행이 있는 범위를 지정하세요.":"Select a range that includes a header row.",
  "배율":"Scaling","다음 시트의 앞에:":"Before sheet:","복사본 만들기":"Create a copy","숨기기 취소할 시트:":"Sheets to unhide:",
  "머리글/바닥글":"Header/Footer","구역을 누른 뒤 단추를 누르면 그 자리에 코드가 들어갑니다. 인쇄할 때 실제 값으로 바뀝니다.":"Select a section, then click a button to insert a code. It is replaced with the actual value when printed.",
  "슬라이서 설정":"Slicer Settings","스파크라인 디자인":"Sparkline Design","표식":"Markers","높은 점·낮은 점":"High Point and Low Point",
  "다시 시작":"Restart","계산":"Evaluate","데이터 표":"Data Table","행 입력 셀(R):":"Row input cell (R):","열 입력 셀(C):":"Column input cell (C):",
  "선택한 범위의 첫 행에는 행 입력 셀에 넣을 값, 첫 열에는 열 입력 셀에 넣을 값을 둡니다.":"Put row-input values in the first row and column-input values in the first column of the selected range.",
  "시나리오 이름(N):":"Scenario name (N):","변경 셀(C):":"Changing cells (C):","인접하지 않은 셀은 쉼표로 구분합니다.":"Separate non-adjacent cells with commas.",
  "설명(O):":"Comment (O):","각 변경 셀의 값을 입력하십시오.":"Enter a value for each changing cell.","보고서 종류: 시나리오 요약":"Report type: Scenario summary",
  "결과 셀(R):":"Result cell (R):","정의된 시나리오가 없습니다. 시나리오를 추가하려면 [추가]를 선택하십시오.":"No scenarios are defined. Select [Add] to create a scenario.",
  "추가(A)...":"Add (A)...","삭제(D)":"Delete (D)","편집(E)...":"Edit (E)...","요약(U)...":"Summary (U)...","표시(S)":"Show (S)",
  "구조 및 창 보호":"Protect Workbook Structure and Windows","보호할 대상: 구조 — 시트 추가, 삭제, 이름 바꾸기, 이동, 숨기기를 막습니다.":"Protect: Structure — prevents adding, deleting, renaming, moving, or hiding sheets.",
  "암호(선택):":"Password (optional):","암호 확인:":"Confirm password:","보기 추가":"Add View","이름(N):":"Name (N):","보기에 포함: 숨겨진 행·열, 필터 설정":"Include in view: hidden rows/columns and filter settings","사용자 지정 보기":"Custom Views",
  "원본 이름:":"Source name:","(비어 있음)":"(blank)","계산이 끝났습니다.":"Evaluation is complete.","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"Click [Evaluate] to evaluate the underlined expression. The most recent result is shown in italics.",
  "가운데":"Center","버전":"Version",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"The active cell has no formula or precedents.",
  "현재 셀을 참조하는 수식이 없습니다":"No formulas refer to the active cell.",
  "데이터 시트에는 서식을 적용할 수 없습니다":"Formatting cannot be applied to a data sheet.",
  "데이터 시트에는 붙여넣을 수 없습니다":"You cannot paste into a data sheet.",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"This file was recovered. Review it and save it.",
  "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"Saved as XLSX. Data sheets are saved only in a .cellx project.",
  "찾는 내용이 없습니다.":"No matches found.",
  "이 기능은 일반 시트에서 씁니다. 데이터 시트는 아래 필터 줄과 열 머리글로 정렬·필터합니다.":"This feature is for regular sheets. Sort and filter data sheets using the filter row and column headers.",
  "일반 시트만 인쇄할 수 있습니다":"Only regular sheets can be printed.",
  "스파크라인은 일반 시트에 넣습니다":"Sparklines can only be inserted into regular sheets.",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"The location range is invalid (e.g. F2:F10).",
  "슬라이서를 만들 표 안의 셀을 선택하세요":"Select a cell inside the table for which you want to create a slicer.",
  "도형은 일반 시트에 넣습니다":"Shapes can only be inserted into regular sheets.",
  "그림은 일반 시트에 넣습니다":"Pictures can only be inserted into regular sheets.",
  "차트는 일반 시트의 범위로 만듭니다":"Charts are created from ranges on regular sheets.",
  "차트로 만들 데이터 범위를 선택하세요":"Select a data range to create a chart.",
  "먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"Copy cells in CellX first (Ctrl+C).",
  "클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"There is no content to paste from the clipboard. Try Ctrl+V.",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"Select the entire range containing the input values and formulas before creating a data table.",
  "피벗 테이블을 새로 고쳤습니다":"PivotTable refreshed.",
  "메모는 일반 시트에 넣을 수 있습니다":"Notes can only be added to regular sheets.",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"Text to Columns can process only one column at a time.",
  "수식이 있는 셀을 선택하세요":"Select a cell containing a formula.",
  "그룹 안의 셀을 선택하세요":"Select a cell inside a group.",
  "1904 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 뒤의 날짜로 표시됩니다. 계속할까요?":"Switch to the 1904 date system? Date serial values stay unchanged, so dates will display 4 years and 1 day later. Continue?",
  "1900 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 앞의 날짜로 표시됩니다. 계속할까요?":"Switch to the 1900 date system? Date serial values stay unchanged, so dates will display 4 years and 1 day earlier. Continue?",
  "인쇄 영역을 해제했습니다":"Print area cleared.",
  "셀 주소나 이름이 올바르지 않습니다:":"The cell reference or name is invalid:",
  "쿼리 실행 중":"Running query","시트 이동":"Move Sheet","시트 복사":"Copy Sheet","시트 숨기기":"Hide Sheet","이름 바꾸기":"Rename","스파크라인 지우기":"Clear Sparklines","통합 문서 보호 해제":"Unprotect Workbook","날짜 체계 변경":"Change Date System",
  "저장했습니다:":"Saved:","내보냈습니다:":"Exported:","자동 복구 파일을 저장하지 못했습니다:":"Could not save the recovery file:","빠른 채우기:":"Flash Fill:",
});
Object.assign(japanese, {
  "한국어":"韓国語","로 실행하세요。":"を実行してください。","개의 레코드가 있습니다":"件のレコードがあります",
  "수식에서":"数式内で","· 수식 입력 중 열을 클릭하면 참조가 들어갑니다":"· 数式の入力中に列をクリックすると参照が挿入されます",
  "SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다。":"SELECT 文のみ実行できます。結果は新しいデータシートになります。",
  "결과 시트 이름":"結果シート名","실행":"実行","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"まだありません。[データ] タブの [データの取得] から CSV を開いてください。",
  "시트에서 쓰는 법":"シートでの使い方","데이터 없음":"データなし","+ 기준 추가":"+ レベルの追加","× 기준 삭제":"× レベルの削除",
  "?는 한 글자, *는 여러 글자를 나타냅니다。":"? は1文字、* は複数文字を表します。","(모두 선택)":"(すべて選択)",
  "중복 값을 삭제하려면 중복 값이 포함된 열을 하나 이상 선택하세요.":"重複値を削除するには、重複を含む列を1つ以上選択してください。",
  "셀을 선택하면 설명 메시지 표시":"セルを選択したときに入力メッセージを表示","유효하지 않은 데이터를 입력하면 오류 메시지 표시":"無効なデータが入力されたときにエラーメッセージを表示",
  "모두 지우기":"すべてクリア","찾기 및 바꾸기":"検索と置換","전체 셀 내용 일치":"セル内容全体と一致","범위: 시트":"検索対象: シート","범위: 통합 문서":"検索対象: ブック",
  "새로 만들기...":"新規作成...","웹 페이지":"Web ページ","현재 문서":"現在のドキュメント","수식 셀":"数式セル","찾는 값":"検索する文字列","값을 바꿀 셀":"置換後の文字列","예: B2":"例: B2",
  "그룹화할 항목":"グループ化する項目","사용할 함수":"集計方法","합계":"合計","개수":"個数","평균":"平均","최대":"最大","최소":"最小",
  "부분합 계산 항목":"小計を追加する項目","새로운 값으로 대치":"新しい値に置換","먼저 그룹화할 열로 데이터를 정렬해 두세요。":"先にグループ化する列でデータを並べ替えてください。",
  "모두 제거":"すべて削除","표에 사용할 데이터를 지정하십시오。":"テーブルに使用するデータを指定してください。","머리글 포함":"先頭行をテーブルの見出しとして使用",
  "시트 보호 해제 암호 (선택)":"シート保護解除パスワード (任意)","워크시트의 모든 사용자에게 허용할 내용 (잠긴 셀 내용 변경은 항상 막힙니다):":"このワークシートのすべてのユーザーに許可する操作:",
  "계속하려면 암호를 다시 입력하십시오。":"続行するにはパスワードを再入力してください。","암호를 잊어버리면 복구할 수 없습니다. Excel과 같은 방식의 보호여서 강력한 보안 수단은 아닙니다.":"パスワードを忘れると復元できません。Excel と同様の保護であり、強固なセキュリティ機能ではありません。",
  "암호":"パスワード","형식 코드 (사용자 지정)":"書式コード (ユーザー設定)","크기":"サイズ","잠금":"ロック",
  "선택하여 붙여넣기":"形式を選択して貼り付け","연산":"演算","내용 있는 셀만 붙여넣기":"空白セルを無視","행/열 바꿈":"行/列の入れ替え","가나다 AaBbCc 123":"あいう AaBbCc 123",
  "데이터 시트:":"データシート:","머리글 행이 있는 범위를 지정하세요。":"見出し行を含む範囲を選択してください。",
  "배율":"拡大縮小","다음 시트의 앞에:":"次のシートの前に:","복사본 만들기":"コピーを作成","숨기기 취소할 시트:":"再表示するシート:",
  "머리글/바닥글":"ヘッダー/フッター","구역을 누른 뒤 단추를 누르면 그 자리에 코드가 들어갑니다. 인쇄할 때 실제 값으로 바뀝니다.":"領域を選択してボタンを押すとコードが挿入されます。印刷時に実際の値に置き換わります。",
  "슬라이서 설정":"スライサーの設定","스파크라인 디자인":"スパークラインのデザイン","표식":"マーカー","높은 점·낮은 점":"最高点・最低点",
  "다시 시작":"最初から","계산":"計算","데이터 표":"データ テーブル","행 입력 셀(R):":"行の代入セル (R):","열 입력 셀(C):":"列の代入セル (C):",
  "선택한 범위의 첫 행에는 행 입력 셀에 넣을 값, 첫 열에는 열 입력 셀에 넣을 값을 둡니다.":"選択範囲の先頭行に行の代入値、先頭列に列の代入値を配置します。",
  "시나리오 이름(N):":"シナリオ名 (N):","변경 셀(C):":"変化させるセル (C):","인접하지 않은 셀은 쉼표로 구분합니다.":"隣接していないセルはカンマで区切ります。",
  "설명(O):":"コメント (O):","각 변경 셀의 값을 입력하십시오。":"各変化セルの値を入力してください。","보고서 종류: 시나리오 요약":"レポートの種類: シナリオの概要",
  "결과 셀(R):":"結果セル (R):","정의된 시나리오가 없습니다. 시나리오를 추가하려면 [추가]를 선택하십시오.":"シナリオが定義されていません。[追加] を選択してシナリオを作成してください。",
  "추가(A)...":"追加 (A)...","삭제(D)":"削除 (D)","편집(E)...":"編集 (E)...","요약(U)...":"概要 (U)...","표시(S)":"表示 (S)",
  "구조 및 창 보호":"ブックの構造とウィンドウの保護","보호할 대상: 구조 — 시트 추가, 삭제, 이름 바꾸기, 이동, 숨기기를 막습니다.":"保護対象: 構造 — シートの追加、削除、名前変更、移動、非表示を禁止します。",
  "암호(선택):":"パスワード (任意):","암호 확인:":"パスワードの確認:","보기 추가":"ビューの追加","이름(N):":"名前 (N):","보기에 포함: 숨겨진 행·열, 필터 설정":"ビューに含める: 非表示の行/列、フィルター設定","사용자 지정 보기":"ユーザー設定のビュー",
  "원본 이름:":"元の名前:","(비어 있음)":"(空白)","계산이 끝났습니다.":"計算が完了しました。","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"下線付きの式を計算するには [計算] をクリックしてください。直近の結果は斜体で表示されます。",
  "가운데":"中央","버전":"バージョン",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"選択したセルに数式がないか、参照元セルがありません。",
  "현재 셀을 참조하는 수식이 없습니다":"選択したセルを参照する数式はありません。",
  "데이터 시트에는 서식을 적용할 수 없습니다":"データシートには書式を適用できません。","데이터 시트에는 붙여넣을 수 없습니다":"データシートには貼り付けできません。",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"復元されたファイルです。内容を確認して保存してください。",
  "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"XLSX として保存しました。データシートは .cellx プロジェクトにのみ保存されます。",
  "찾는 내용이 없습니다.":"一致する項目がありません。","이 기능은 일반 시트에서 씁니다. 데이터 시트는 아래 필터 줄과 열 머리글로 정렬·필터합니다.":"この機能は通常のシートで使用します。データシートはフィルター行と列見出しで並べ替え・フィルターします。",
  "일반 시트만 인쇄할 수 있습니다":"通常のシートのみ印刷できます。","스파크라인은 일반 시트에 넣습니다":"スパークラインは通常のシートに挿入します。",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"配置範囲が正しくありません (例: F2:F10)。","슬라이서를 만들 표 안의 셀을 선택하세요":"スライサーを作成するテーブル内のセルを選択してください。",
  "도형은 일반 시트에 넣습니다":"図形は通常のシートに挿入します。","그림은 일반 시트에 넣습니다":"画像は通常のシートに挿入します。","차트는 일반 시트의 범위로 만듭니다":"グラフは通常のシートの範囲から作成します。",
  "차트로 만들 데이터 범위를 선택하세요":"グラフにするデータ範囲を選択してください。","먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"先に CellX でセルをコピーしてください (Ctrl+C)。",
  "클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"クリップボードに貼り付ける内容がありません。Ctrl+V をお試しください。",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"入力値と数式を含む範囲全体を選択してからデータ テーブルを作成してください。",
  "피벗 테이블을 새로 고쳤습니다":"ピボットテーブルを更新しました。","메모는 일반 시트에 넣을 수 있습니다":"メモは通常のシートにのみ追加できます。",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"区切り位置は一度に1列のみ処理できます。","수식이 있는 셀을 선택하세요":"数式が含まれるセルを選択してください。","그룹 안의 셀을 선택하세요":"グループ内のセルを選択してください。",
  "1904 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 뒤의 날짜로 표시됩니다. 계속할까요?":"1904 年日付システムに変更しますか？日付シリアル値は変わらないため、日付は 4 年 1 日後として表示されます。続行しますか？",
  "1900 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 앞의 날짜로 표시됩니다. 계속할까요?":"1900 年日付システムに変更しますか？日付シリアル値は変わらないため、日付は 4 年 1 日前として表示されます。続行しますか？",
  "인쇄 영역을 해제했습니다":"印刷範囲を解除しました。","쿼리 실행 중":"クエリを実行中","시트 이동":"シートの移動","시트 복사":"シートのコピー","시트 숨기기":"シートを非表示","이름 바꾸기":"名前の変更","스파크라인 지우기":"スパークラインのクリア","통합 문서 보호 해제":"ブック保護の解除","날짜 체계 변경":"日付システムの変更",
  "저장했습니다:":"保存しました:","내보냈습니다:":"エクスポートしました:","자동 복구 파일을 저장하지 못했습니다:":"自動回復ファイルを保存できませんでした:","빠른 채우기:":"フラッシュ フィル:",
});
Object.assign(simplifiedChinese, {
  "한국어":"韩语","로 실행하세요.":"运行。","개의 레코드가 있습니다":"条记录",
  "수식에서":"在公式中，","· 수식 입력 중 열을 클릭하면 참조가 들어갑니다":"· 编辑公式时单击列即可插入引用",
  "SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다。":"仅支持执行 SELECT 语句。结果将放入新的数据工作表。",
  "결과 시트 이름":"结果工作表名称","실행":"运行","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"尚无数据。请从“数据”选项卡的“导入数据”打开 CSV。",
  "시트에서 쓰는 법":"在工作表中使用","데이터 없음":"无数据","+ 기준 추가":"+ 添加级别","× 기준 삭제":"× 删除级别",
  "?는 한 글자, *는 여러 글자를 나타냅니다.":"? 表示一个字符；* 表示多个字符。","(모두 선택)":"(全选)",
  "중복 값을 삭제하려면 중복 값이 포함된 열을 하나 이상 선택하세요.":"请选择至少一列以识别重复值。",
  "셀을 선택하면 설명 메시지 표시":"选中单元格时显示输入信息","유효하지 않은 데이터를 입력하면 오류 메시지 표시":"输入无效数据时显示错误警告",
  "모두 지우기":"全部清除","찾기 및 바꾸기":"查找和替换","전체 셀 내용 일치":"单元格内容完全匹配","범위: 시트":"范围：工作表","범위: 통합 문서":"范围：工作簿",
  "새로 만들기...":"新建...","웹 페이지":"网页","현재 문서":"当前文档","수식 셀":"公式单元格","찾는 값":"查找内容","값을 바꿀 셀":"替换为","예: B2":"例如：B2",
  "그룹화할 항목":"每当此字段变化时","사용할 함수":"使用函数","합계":"求和","개수":"计数","평균":"平均值","최대":"最大值","최소":"最小值",
  "부분합 계산 항목":"添加小计到","새로운 값으로 대치":"替换为新值","먼저 그룹화할 열로 데이터를 정렬해 두세요.":"请先按分组列对数据排序。",
  "모두 제거":"全部删除","표에 사용할 데이터를 지정하십시오.":"请指定要用于表格的数据。","머리글 포함":"表包含标题",
  "시트 보호 해제 암호 (선택)":"取消工作表保护密码（可选）","워크시트의 모든 사용자에게 허용할 내용 (잠긴 셀 내용 변경은 항상 막힙니다):":"允许此工作表的所有用户执行以下操作：",
  "계속하려면 암호를 다시 입력하십시오.":"请重新输入密码以继续。","암호를 잊어버리면 복구할 수 없습니다. Excel과 같은 방식의 보호여서 강력한 보안 수단은 아닙니다.":"如果忘记密码，将无法恢复。此保护方式与 Excel 相同，并非强安全措施。",
  "암호":"密码","형식 코드 (사용자 지정)":"格式代码（自定义）","크기":"大小","잠금":"锁定",
  "선택하여 붙여넣기":"选择性粘贴","연산":"运算","내용 있는 셀만 붙여넣기":"跳过空单元格","행/열 바꿈":"转置","가나다 AaBbCc 123":"汉字 AaBbCc 123",
  "데이터 시트:":"数据工作表：","머리글 행이 있는 범위를 지정하세요.":"请选择包含标题行的区域。",
  "배율":"缩放","다음 시트의 앞에:":"在工作表之前：","복사본 만들기":"建立副本","숨기기 취소할 시트:":"要取消隐藏的工作表：",
  "머리글/바닥글":"页眉/页脚","구역을 누른 뒤 단추를 누르면 그 자리에 코드가 들어갑니다. 인쇄할 때 실제 값으로 바뀝니다.":"选择区域后单击按钮即可插入代码。打印时会替换为实际值。",
  "슬라이서 설정":"切片器设置","스파크라인 디자인":"迷你图设计","표식":"标记","높은 점·낮은 점":"最高点和最低点",
  "다시 시작":"重新开始","계산":"求值","데이터 표":"模拟运算表","행 입력 셀(R):":"行输入单元格 (R)：","열 입력 셀(C):":"列输入单元格 (C)：",
  "선택한 범위의 첫 행에는 행 입력 셀에 넣을 값, 첫 열에는 열 입력 셀에 넣을 값을 둡니다.":"在所选区域的第一行放置行输入值，在第一列放置列输入值。",
  "시나리오 이름(N):":"方案名称 (N)：","변경 셀(C):":"可变单元格 (C)：","인접하지 않은 셀은 쉼표로 구분합니다.":"不相邻的单元格请用逗号分隔。",
  "설명(O):":"备注 (O)：","각 변경 셀의 값을 입력하십시오.":"请输入每个可变单元格的值。","보고서 종류: 시나리오 요약":"报告类型：方案摘要",
  "결과 셀(R):":"目标单元格 (R)：","정의된 시나리오가 없습니다. 시나리오를 추가하려면 [추가]를 선택하십시오.":"尚未定义方案。请选择[添加]以创建方案。",
  "추가(A)...":"添加 (A)...","삭제(D)":"删除 (D)","편집(E)...":"编辑 (E)...","요약(U)...":"摘要 (U)...","표시(S)":"显示 (S)",
  "구조 및 창 보호":"保护工作簿结构和窗口","보호할 대상: 구조 — 시트 추가, 삭제, 이름 바꾸기, 이동, 숨기기를 막습니다.":"保护对象：结构 — 禁止添加、删除、重命名、移动或隐藏工作表。",
  "암호(선택):":"密码（可选）：","암호 확인:":"确认密码：","보기 추가":"添加视图","이름(N):":"名称 (N)：","보기에 포함: 숨겨진 행·열, 필터 설정":"视图包含：隐藏的行/列和筛选设置","사용자 지정 보기":"自定义视图",
  "원본 이름:":"源名称：","(비어 있음)":"(空白)","계산이 끝났습니다.":"计算完成。","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"单击[求值]以计算带下划线的表达式。最近一次结果以斜体显示。",
  "가운데":"居中","버전":"版本",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"当前单元格没有公式或前置引用。","현재 셀을 참조하는 수식이 없습니다":"没有公式引用当前单元格。",
  "데이터 시트에는 서식을 적용할 수 없습니다":"无法对数据工作表应用格式。","데이터 시트에는 붙여넣을 수 없습니다":"无法粘贴到数据工作表。",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"此文件已恢复。请检查并保存。",
  "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"已保存为 XLSX。数据工作表仅保存在 .cellx 项目中。",
  "찾는 내용이 없습니다.":"未找到匹配项。","이 기능은 일반 시트에서 씁니다. 데이터 시트는 아래 필터 줄과 열 머리글로 정렬·필터합니다.":"此功能适用于普通工作表。数据工作表请使用筛选行和列标题进行排序和筛选。",
  "일반 시트만 인쇄할 수 있습니다":"只能打印普通工作表。","스파크라인은 일반 시트에 넣습니다":"迷你图只能插入普通工作表。",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"位置区域无效（例如 F2:F10）。","슬라이서를 만들 표 안의 셀을 선택하세요":"请选择要创建切片器的表格中的单元格。",
  "도형은 일반 시트에 넣습니다":"形状只能插入普通工作表。","그림은 일반 시트에 넣습니다":"图片只能插入普通工作表。","차트는 일반 시트의 범위로 만듭니다":"图表由普通工作表中的区域创建。",
  "차트로 만들 데이터 범위를 선택하세요":"请选择要创建图表的数据区域。","먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"请先在 CellX 中复制单元格 (Ctrl+C)。",
  "클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"剪贴板中没有可粘贴的内容。请尝试 Ctrl+V。",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"创建模拟运算表前，请选择包含输入值和公式的整个区域。",
  "피벗 테이블을 새로 고쳤습니다":"数据透视表已刷新。","메모는 일반 시트에 넣을 수 있습니다":"备注只能添加到普通工作表。",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"分列一次只能处理一列。","수식이 있는 셀을 선택하세요":"请选择包含公式的单元格。","그룹 안의 셀을 선택하세요":"请选择组内的单元格。",
  "1904 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 뒤의 날짜로 표시됩니다. 계속할까요?":"切换到 1904 日期系统？日期序列值保持不变，因此日期将显示为晚 4 年零 1 天。是否继续？",
  "1900 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 앞의 날짜로 표시됩니다. 계속할까요?":"切换到 1900 日期系统？日期序列值保持不变，因此日期将显示为早 4 年零 1 天。是否继续？",
  "인쇄 영역을 해제했습니다":"已清除打印区域。","쿼리 실행 중":"正在运行查询","시트 이동":"移动工作表","시트 복사":"复制工作表","시트 숨기기":"隐藏工作表","이름 바꾸기":"重命名","스파크라인 지우기":"清除迷你图","통합 문서 보호 해제":"取消工作簿保护","날짜 체계 변경":"更改日期系统",
  "저장했습니다:":"已保存：","내보냈습니다:":"已导出：","자동 복구 파일을 저장하지 못했습니다:":"无法保存自动恢复文件：","빠른 채우기:":"快速填充：",
});
Object.assign(traditionalChinese, {
  "한국어":"韓語","로 실행하세요.":"執行。","개의 레코드가 있습니다":"筆記錄",
  "수식에서":"在公式中，","· 수식 입력 중 열을 클릭하면 참조가 들어갑니다":"· 編輯公式時按一下欄即可插入參照",
  "SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.":"僅支援執行 SELECT 陳述式。結果會放入新的資料工作表。",
  "결과 시트 이름":"結果工作表名稱","실행":"執行","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"尚無資料。請從「資料」索引標籤的「匯入資料」開啟 CSV。",
  "시트에서 쓰는 법":"在工作表中使用","데이터 없음":"沒有資料","+ 기준 추가":"+ 新增層級","× 기준 삭제":"× 刪除層級",
  "?는 한 글자, *는 여러 글자를 나타냅니다.":"? 代表一個字元；* 代表多個字元。","(모두 선택)":"(全選)",
  "중복 값을 삭제하려면 중복 값이 포함된 열을 하나 이상 선택하세요.":"請至少選取一個包含重複值的欄。",
  "셀을 선택하면 설명 메시지 표시":"選取儲存格時顯示輸入訊息","유효하지 않은 데이터를 입력하면 오류 메시지 표시":"輸入無效資料時顯示錯誤警示",
  "모두 지우기":"全部清除","찾기 및 바꾸기":"尋找及取代","전체 셀 내용 일치":"完全符合儲存格內容","범위: 시트":"範圍：工作表","범위: 통합 문서":"範圍：活頁簿",
  "새로 만들기...":"新增...","웹 페이지":"網頁","현재 문서":"目前文件","수식 셀":"公式儲存格","찾는 값":"尋找目標","값을 바꿀 셀":"取代為","예: B2":"例如：B2",
  "그룹화할 항목":"每當此欄位變更時","사용할 함수":"使用函數","합계":"加總","개수":"計數","평균":"平均","최대":"最大值","최소":"最小值",
  "부분합 계산 항목":"新增小計至","새로운 값으로 대치":"以新值取代","먼저 그룹화할 열로 데이터를 정렬해 두세요.":"請先依群組欄排序資料。",
  "모두 제거":"全部移除","표에 사용할 데이터를 지정하십시오.":"請指定要用於表格的資料。","머리글 포함":"我的表格有標題",
  "시트 보호 해제 암호 (선택)":"取消工作表保護密碼（選用）","워크시트의 모든 사용자에게 허용할 내용 (잠긴 셀 내용 변경은 항상 막힙니다):":"允許此工作表的所有使用者執行：",
  "계속하려면 암호를 다시 입력하십시오.":"請重新輸入密碼以繼續。","암호를 잊어버리면 복구할 수 없습니다. Excel과 같은 방식의 보호여서 강력한 보안 수단은 아닙니다.":"若忘記密碼將無法復原。此保護方式與 Excel 相同，並非強大的安全防護。",
  "암호":"密碼","형식 코드 (사용자 지정)":"格式代碼（自訂）","크기":"大小","잠금":"鎖定",
  "선택하여 붙여넣기":"選擇性貼上","연산":"運算","내용 있는 셀만 붙여넣기":"略過空白儲存格","행/열 바꿈":"轉置","가나다 AaBbCc 123":"漢字 AaBbCc 123",
  "데이터 시트:":"資料工作表：","머리글 행이 있는 범위를 지정하세요.":"請選取包含標題列的範圍。",
  "배율":"縮放","다음 시트의 앞에:":"在下列工作表之前：","복사본 만들기":"建立副本","숨기기 취소할 시트:":"要取消隱藏的工作表：",
  "머리글/바닥글":"頁首/頁尾","구역을 누른 뒤 단추를 누르면 그 자리에 코드가 들어갑니다. 인쇄할 때 실제 값으로 바뀝니다.":"選取區域後按一下按鈕即可插入代碼。列印時會替換成實際值。",
  "슬라이서 설정":"交叉分析篩選器設定","스파크라인 디자인":"走勢圖設計","표식":"標記","높은 점·낮은 점":"最高點與最低點",
  "다시 시작":"重新開始","계산":"計算","데이터 표":"資料表","행 입력 셀(R):":"列輸入儲存格 (R)：","열 입력 셀(C):":"欄輸入儲存格 (C)：",
  "선택한 범위의 첫 행에는 행 입력 셀에 넣을 값, 첫 열에는 열 입력 셀에 넣을 값을 둡니다.":"在所選範圍的第一列放置列輸入值，在第一欄放置欄輸入值。",
  "시나리오 이름(N):":"案例名稱 (N)：","변경 셀(C):":"變更儲存格 (C)：","인접하지 않은 셀은 쉼표로 구분합니다.":"不相鄰的儲存格請以逗號分隔。",
  "설명(O):":"註解 (O)：","각 변경 셀의 값을 입력하십시오.":"請輸入每個變更儲存格的值。","보고서 종류: 시나리오 요약":"報表類型：案例摘要",
  "결과 셀(R):":"結果儲存格 (R)：","정의된 시나리오가 없습니다. 시나리오를 추가하려면 [추가]를 선택하십시오.":"尚未定義案例。請選取 [新增] 以建立案例。",
  "추가(A)...":"新增 (A)...","삭제(D)":"刪除 (D)","편집(E)...":"編輯 (E)...","요약(U)...":"摘要 (U)...","표시(S)":"顯示 (S)",
  "구조 및 창 보호":"保護活頁簿結構與視窗","보호할 대상: 구조 — 시트 추가, 삭제, 이름 바꾸기, 이동, 숨기기를 막습니다.":"保護對象：結構 — 禁止新增、刪除、重新命名、移動或隱藏工作表。",
  "암호(선택):":"密碼（選用）：","암호 확인:":"確認密碼：","보기 추가":"新增檢視","이름(N):":"名稱 (N)：","보기에 포함: 숨겨진 행·열, 필터 설정":"檢視包含：隱藏的列/欄與篩選設定","사용자 지정 보기":"自訂檢視",
  "원본 이름:":"來源名稱：","(비어 있음)":"(空白)","계산이 끝났습니다.":"計算完成。","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"按一下 [計算] 以計算底線標示的運算式。最近一次結果會以斜體顯示。",
  "가운데":"置中","버전":"版本",
  "현재 셀에 수식이 없거나 참조하는 셀이 없습니다":"目前儲存格沒有公式或前置參照。","현재 셀을 참조하는 수식이 없습니다":"沒有公式參照目前儲存格。",
  "데이터 시트에는 서식을 적용할 수 없습니다":"無法對資料工作表套用格式。","데이터 시트에는 붙여넣을 수 없습니다":"無法貼上至資料工作表。",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"此檔案已復原。請檢查並儲存。",
  "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다":"已儲存為 XLSX。資料工作表僅儲存於 .cellx 專案。",
  "찾는 내용이 없습니다.":"找不到符合的項目。","이 기능은 일반 시트에서 씁니다. 데이터 시트는 아래 필터 줄과 열 머리글로 정렬·필터합니다.":"此功能適用於一般工作表。資料工作表請使用篩選列與欄標題進行排序及篩選。",
  "일반 시트만 인쇄할 수 있습니다":"只能列印一般工作表。","스파크라인은 일반 시트에 넣습니다":"走勢圖只能插入一般工作表。",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"位置範圍無效（例如 F2:F10）。","슬라이서를 만들 표 안의 셀을 선택하세요":"請選取要建立交叉分析篩選器的表格內儲存格。",
  "도형은 일반 시트에 넣습니다":"圖案只能插入一般工作表。","그림은 일반 시트에 넣습니다":"圖片只能插入一般工作表。","차트는 일반 시트의 범위로 만듭니다":"圖表由一般工作表中的範圍建立。",
  "차트로 만들 데이터 범위를 선택하세요":"請選取要建立圖表的資料範圍。","먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"請先在 CellX 中複製儲存格 (Ctrl+C)。",
  "클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"剪貼簿沒有可貼上的內容。請嘗試 Ctrl+V。",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"建立資料表前，請選取包含輸入值與公式的整個範圍。",
  "피벗 테이블을 새로 고쳤습니다":"樞紐分析表已重新整理。","메모는 일반 시트에 넣을 수 있습니다":"備註只能新增至一般工作表。",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"分割文字一次只能處理一欄。","수식이 있는 셀을 선택하세요":"請選取包含公式的儲存格。","그룹 안의 셀을 선택하세요":"請選取群組內的儲存格。",
  "1904 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 뒤의 날짜로 표시됩니다. 계속할까요?":"要切換至 1904 日期系統嗎？日期序號值維持不變，因此日期會顯示為晚 4 年又 1 天。是否繼續？",
  "1900 날짜 체계로 바꿉니다. 날짜 셀은 값이 그대로라 4년 1일 앞의 날짜로 표시됩니다. 계속할까요?":"要切換至 1900 日期系統嗎？日期序號值維持不變，因此日期會顯示為早 4 年又 1 天。是否繼續？",
  "인쇄 영역을 해제했습니다":"已清除列印範圍。","쿼리 실행 중":"正在執行查詢","시트 이동":"移動工作表","시트 복사":"複製工作表","시트 숨기기":"隱藏工作表","이름 바꾸기":"重新命名","스파크라인 지우기":"清除走勢圖","통합 문서 보호 해제":"取消活頁簿保護","날짜 체계 변경":"變更日期系統",
  "저장했습니다:":"已儲存：","내보냈습니다:":"已匯出：","자동 복구 파일을 저장하지 못했습니다:":"無法儲存自動復原檔案：","빠른 채우기:":"快速填入：",
});


Object.assign(english, {
  "로 실행하세요.":" to run it.","개의 레코드가 있습니다":" records","시트에서 쓰는 법":"How to use in a sheet",
  "데이터 없음":"No data","SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.":"Only SELECT statements are supported. Results are placed in a new data sheet.",
  "결과 시트 이름":"Result sheet name","실행":"Run","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"No data yet. Open a CSV from Data → Import Data.",
  "형식 코드 (사용자 지정)":"Format code (Custom)","크기":"Size","잠금":"Locked","배율":"Scaling",
  "변경 셀:":"Changing cells:","설명:":"Comment:","원본 이름:":"Source name:","(비어 있음)":"(blank)",
  "계산이 끝났습니다.":"Evaluation is complete.","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"Click [Evaluate] to evaluate the underlined expression. The most recent result is shown in italics.",
  "데이터 시트에는 서식을 적용할 수 없습니다":"Formatting cannot be applied to a data sheet.","데이터 시트에는 붙여넣을 수 없습니다":"You cannot paste into a data sheet.",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"This file was recovered. Review it and save it.","찾는 내용이 없습니다.":"No matches found.",
  "일반 시트만 인쇄할 수 있습니다":"Only regular sheets can be printed.","스파크라인은 일반 시트에 넣습니다":"Sparklines can only be inserted into regular sheets.",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"The location range is invalid (e.g. F2:F10).","슬라이서를 만들 표 안의 셀을 선택하세요":"Select a cell inside the table for which you want to create a slicer.",
  "도형은 일반 시트에 넣습니다":"Shapes can only be inserted into regular sheets.","그림은 일반 시트에 넣습니다":"Pictures can only be inserted into regular sheets.",
  "차트는 일반 시트의 범위로 만듭니다":"Charts are created from ranges on regular sheets.","차트로 만들 데이터 범위를 선택하세요":"Select a data range to create a chart.",
  "먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"Copy cells in CellX first (Ctrl+C).","클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"There is no content to paste from the clipboard. Try Ctrl+V.",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"Select the entire range containing input values and formulas before creating a data table.",
  "피벗 테이블을 새로 고쳤습니다":"PivotTable refreshed.","메모는 일반 시트에 넣을 수 있습니다":"Notes can only be added to regular sheets.",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"Text to Columns can process only one column at a time.","수식이 있는 셀을 선택하세요":"Select a cell containing a formula.","그룹 안의 셀을 선택하세요":"Select a cell inside a group.",
  "쿼리 실행 중":"Running query","시트 이동":"Move Sheet","시트 복사":"Copy Sheet","시트 숨기기":"Hide Sheet","이름 바꾸기":"Rename","스파크라인 지우기":"Clear Sparklines","통합 문서 보호 해제":"Unprotect Workbook","날짜 체계 변경":"Change Date System",
  "저장했습니다:":"Saved:","내보냈습니다:":"Exported:","자동 복구 파일을 저장하지 못했습니다:":"Could not save the recovery file:","빠른 채우기:":"Flash Fill:",
  "차트 제목":"Chart Title","차트 데이터":"Chart Data","필드":"Field","값":"Values","열":"Column","행":"Row","시트":"Sheet",
  "열 입력 셀(C):":"Column input cell (C):","행 입력 셀(R):":"Row input cell (R):","시나리오 이름(N):":"Scenario name (N):",
  "데이터 시트:":"Data sheet:","머리글 행이 있는 범위를 지정하세요.":"Select a range that includes a header row.",
});
Object.assign(japanese, {
  "로 실행하세요.":"を実行してください。","개의 레코드가 있습니다":"件のレコードがあります","시트에서 쓰는 법":"シートでの使い方",
  "데이터 없음":"データなし","SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.":"SELECT 文のみ実行できます。結果は新しいデータシートになります。",
  "결과 시트 이름":"結果シート名","실행":"実行","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"まだありません。[データ] タブの [データの取得] から CSV を開いてください。",
  "형식 코드 (사용자 지정)":"書式コード (ユーザー設定)","크기":"サイズ","잠금":"ロック","배율":"拡大縮小",
  "계산이 끝났습니다.":"計算が完了しました。","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"下線付きの式を計算するには [計算] をクリックしてください。直近の結果は斜体で表示されます。",
  "데이터 시트에는 서식을 적용할 수 없습니다":"データシートには書式を適用できません。","데이터 시트에는 붙여넣을 수 없습니다":"データシートには貼り付けできません。",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"復元されたファイルです。内容を確認して保存してください。","찾는 내용이 없습니다.":"一致する項目がありません。",
  "일반 시트만 인쇄할 수 있습니다":"通常のシートのみ印刷できます。","스파크라인은 일반 시트에 넣습니다":"スパークラインは通常のシートに挿入します。",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"配置範囲が正しくありません (例: F2:F10)。","슬라이서를 만들 표 안의 셀을 선택하세요":"スライサーを作成するテーブル内のセルを選択してください。",
  "도형은 일반 시트에 넣습니다":"図形は通常のシートに挿入します。","그림은 일반 시트에 넣습니다":"画像は通常のシートに挿入します。",
  "차트는 일반 시트의 범위로 만듭니다":"グラフは通常のシートの範囲から作成します。","차트로 만들 데이터 범위를 선택하세요":"グラフにするデータ範囲を選択してください。",
  "먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"先に CellX でセルをコピーしてください (Ctrl+C)。","클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"クリップボードに貼り付ける内容がありません。Ctrl+V をお試しください。",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"入力値と数式を含む範囲全体を選択してからデータ テーブルを作成してください。",
  "피벗 테이블을 새로 고쳤습니다":"ピボットテーブルを更新しました。","메모는 일반 시트에 넣을 수 있습니다":"メモは通常のシートにのみ追加できます。",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"区切り位置は一度に1列のみ処理できます。","수식이 있는 셀을 선택하세요":"数式が含まれるセルを選択してください。","그룹 안의 셀을 선택하세요":"グループ内のセルを選択してください。",
  "쿼리 실행 중":"クエリを実行中","시트 이동":"シートの移動","시트 복사":"シートのコピー","시트 숨기기":"シートを非表示","이름 바꾸기":"名前の変更","스파크라인 지우기":"スパークラインのクリア","통합 문서 보호 해제":"ブック保護の解除","날짜 체계 변경":"日付システムの変更",
  "저장했습니다:":"保存しました:","내보냈습니다:":"エクスポートしました:","자동 복구 파일을 저장하지 못했습니다:":"自動回復ファイルを保存できませんでした:","빠른 채우기:":"フラッシュ フィル:",
  "변경 셀:":"変化させるセル:","설명:":"コメント:","원본 이름:":"元の名前:","(비어 있음)":"(空白)",
});
Object.assign(simplifiedChinese, {
  "로 실행하세요.":"运行。","개의 레코드가 있습니다":"条记录","시트에서 쓰는 법":"在工作表中使用",
  "데이터 없음":"无数据","SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.":"仅支持执行 SELECT 语句。结果将放入新的数据工作表。",
  "결과 시트 이름":"结果工作表名称","실행":"运行","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"尚无数据。请从“数据”选项卡的“导入数据”打开 CSV。",
  "형식 코드 (사용자 지정)":"格式代码（自定义）","크기":"大小","잠금":"锁定","배율":"缩放",
  "계산이 끝났습니다.":"计算完成。","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"单击[求值]以计算带下划线的表达式。最近一次结果以斜体显示。",
  "데이터 시트에는 서식을 적용할 수 없습니다":"无法对数据工作表应用格式。","데이터 시트에는 붙여넣을 수 없습니다":"无法粘贴到数据工作表。",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"此文件已恢复。请检查并保存。","찾는 내용이 없습니다.":"未找到匹配项。",
  "일반 시트만 인쇄할 수 있습니다":"只能打印普通工作表。","스파크라인은 일반 시트에 넣습니다":"迷你图只能插入普通工作表。",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"位置区域无效（例如 F2:F10）。","슬라이서를 만들 표 안의 셀을 선택하세요":"请选择要创建切片器的表格中的单元格。",
  "도형은 일반 시트에 넣습니다":"形状只能插入普通工作表。","그림은 일반 시트에 넣습니다":"图片只能插入普通工作表。",
  "차트는 일반 시트의 범위로 만듭니다":"图表由普通工作表中的区域创建。","차트로 만들 데이터 범위를 선택하세요":"请选择要创建图表的数据区域。",
  "먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"请先在 CellX 中复制单元格 (Ctrl+C)。","클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"剪贴板中没有可粘贴的内容。请尝试 Ctrl+V。",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"创建模拟运算表前，请选择包含输入值和公式的整个区域。",
  "피벗 테이블을 새로 고쳤습니다":"数据透视表已刷新。","메모는 일반 시트에 넣을 수 있습니다":"备注只能添加到普通工作表。",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"分列一次只能处理一列。","수식이 있는 셀을 선택하세요":"请选择包含公式的单元格。","그룹 안의 셀을 선택하세요":"请选择组内的单元格。",
  "쿼리 실행 중":"正在运行查询","시트 이동":"移动工作表","시트 복사":"复制工作表","시트 숨기기":"隐藏工作表","이름 바꾸기":"重命名","스파크라인 지우기":"清除迷你图","통합 문서 보호 해제":"取消工作簿保护","날짜 체계 변경":"更改日期系统",
  "저장했습니다:":"已保存：","내보냈습니다:":"已导出：","자동 복구 파일을 저장하지 못했습니다:":"无法保存自动恢复文件：","빠른 채우기:":"快速填充：",
  "변경 셀:":"可变单元格：","설명:":"备注：","원본 이름:":"源名称：","(비어 있음)":"(空白)",
});
Object.assign(traditionalChinese, {
  "로 실행하세요.":"執行。","개의 레코드가 있습니다":"筆記錄","시트에서 쓰는 법":"在工作表中使用",
  "데이터 없음":"沒有資料","SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.":"僅支援執行 SELECT 陳述式。結果會放入新的資料工作表。",
  "결과 시트 이름":"結果工作表名稱","실행":"執行","아직 없습니다. 데이터 탭의 「데이터 가져오기」로 CSV를 여세요.":"尚無資料。請從「資料」索引標籤的「匯入資料」開啟 CSV。",
  "형식 코드 (사용자 지정)":"格式代碼（自訂）","크기":"大小","잠금":"鎖定","배율":"縮放",
  "변경 셀:":"變更儲存格：","설명:":"註解：","원본 이름:":"來源名稱：","(비어 있음)":"(空白)",
  "계산이 끝났습니다.":"計算完成。","밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.":"按一下 [計算] 以計算底線標示的運算式。最近一次結果會以斜體顯示。",
  "데이터 시트에는 서식을 적용할 수 없습니다":"無法對資料工作表套用格式。","데이터 시트에는 붙여넣을 수 없습니다":"無法貼上至資料工作表。",
  "복구한 파일입니다. 확인한 뒤 저장하세요.":"此檔案已復原。請檢查並儲存。","찾는 내용이 없습니다.":"找不到符合的項目。",
  "일반 시트만 인쇄할 수 있습니다":"只能列印一般工作表。","스파크라인은 일반 시트에 넣습니다":"走勢圖只能插入一般工作表。",
  "위치 범위가 올바르지 않습니다 (예: F2:F10)":"位置範圍無效（例如 F2:F10）。","슬라이서를 만들 표 안의 셀을 선택하세요":"請選取要建立交叉分析篩選器的表格內儲存格。",
  "도형은 일반 시트에 넣습니다":"圖案只能插入一般工作表。","그림은 일반 시트에 넣습니다":"圖片只能插入一般工作表。",
  "차트는 일반 시트의 범위로 만듭니다":"圖表由一般工作表中的範圍建立。","차트로 만들 데이터 범위를 선택하세요":"請選取要建立圖表的資料範圍。",
  "먼저 CellX에서 셀을 복사(Ctrl+C)하세요":"請先在 CellX 中複製儲存格 (Ctrl+C)。","클립보드에 붙여넣을 내용이 없습니다. Ctrl+V를 사용해 보세요.":"剪貼簿沒有可貼上的內容。請嘗試 Ctrl+V。",
  "입력 값과 수식이 들어 있는 범위 전체를 선택한 뒤 데이터 표를 만드세요":"建立資料表前，請選取包含輸入值與公式的整個範圍。",
  "피벗 테이블을 새로 고쳤습니다":"樞紐分析表已重新整理。","메모는 일반 시트에 넣을 수 있습니다":"備註只能新增至一般工作表。",
  "텍스트 나누기는 한 번에 한 열만 할 수 있습니다":"分割文字一次只能處理一欄。","수식이 있는 셀을 선택하세요":"請選取包含公式的儲存格。","그룹 안의 셀을 선택하세요":"請選取群組內的儲存格。",
  "쿼리 실행 중":"正在執行查詢","시트 이동":"移動工作表","시트 복사":"複製工作表","시트 숨기기":"隱藏工作表","이름 바꾸기":"重新命名","스파크라인 지우기":"清除走勢圖","통합 문서 보호 해제":"取消活頁簿保護","날짜 체계 변경":"變更日期系統",
  "저장했습니다:":"已儲存：","내보냈습니다:":"已匯出：","자동 복구 파일을 저장하지 못했습니다:":"無法儲存自動復原檔案：","빠른 채우기:":"快速填入：",
});


Object.assign(english, {
  "편집":"Edit","잘라내기":"Cut","복사":"Copy","내용 지우기":"Clear Contents","행 높이...":"Row Height...","열 너비...":"Column Width...",
  "새로 고침":"Refresh","피벗 테이블 필드...":"PivotTable Fields...","피벗 테이블 삭제":"Delete PivotTable","선택하여 붙여넣기...":"Paste Special...",
  "스파크라인 편집...":"Edit Sparkline...","스파크라인 지우기":"Clear Sparklines","메모 편집":"Edit Note","새 메모":"New Note","메모 삭제":"Delete Note",
  "범례":"Legend","차트 편집...":"Edit Chart...","원래 크기로":"Reset to Original Size","가로세로 비율 맞추기":"Lock Aspect Ratio","도형 서식...":"Format Shape...",
  "행 삽입":"Insert Rows","열 삽입":"Insert Columns","행 삭제":"Delete Rows","열 삭제":"Delete Columns","숨기기":"Hide","숨기기 취소":"Unhide",
  "찾기...":"Find...","바꾸기...":"Replace...","이동...":"Go To...","시트 삽입 (Shift+F11)":"Insert Sheet (Shift+F11)",
  "일러스트레이션":"Illustrations","그림 삽입 (PNG, JPEG, GIF, BMP). 스크린샷은 Ctrl+V로 붙여넣을 수 있습니다.":"Insert Picture (PNG, JPEG, GIF, BMP). Paste screenshots with Ctrl+V.",
  "도형":"Shapes","링크":"Links","하이퍼링크 (Ctrl+K)":"Hyperlink (Ctrl+K)","CSV·Parquet을 데이터 시트로":"CSV/Parquet to Data Sheet",
  "빠른 채우기 (Ctrl+E)":"Flash Fill (Ctrl+E)","중복된 항목 제거":"Remove Duplicates","데이터 유효성 검사":"Data Validation",
  "목표값 찾기...":"Goal Seek...","시나리오 관리자...":"Scenario Manager...","데이터 표...":"Data Table...","그룹":"Group","그룹 해제":"Ungroup",
  "인쇄 영역":"Print Area","머리글과 바닥글 (페이지 번호, 날짜, 시트 이름…)":"Headers and Footers (page number, date, sheet name…)",
  "페이지 나누기":"Page Breaks","첫 행 고정":"Freeze Top Row","첫 열 고정":"Freeze First Column","자동 합계":"AutoSum",
  "붙여넣기 옵션":"Paste Options","시트 옵션":"Sheet Options","창":"Window","데이터 시트로 내보내기":"Export to Data Sheet",
  "표 이름:":"Table Name:","범위로 변환":"Convert to Range","요약 행":"Total Row","줄무늬 행":"Banded Rows","필터 단추":"Filter Buttons",
});
Object.assign(japanese, {
  "편집":"編集","잘라내기":"切り取り","복사":"コピー","내용 지우기":"内容のクリア","행 높이...":"行の高さ...","열 너비...":"列の幅...",
  "새로 고침":"更新","피벗 테이블 필드...":"ピボットテーブルのフィールド...","피벗 테이블 삭제":"ピボットテーブルの削除","선택하여 붙여넣기...":"形式を選択して貼り付け...",
  "스파크라인 편집...":"スパークラインの編集...","스파크라인 지우기":"スパークラインのクリア","메모 편집":"メモの編集","새 메모":"新しいメモ","메모 삭제":"メモの削除",
  "범례":"凡例","차트 편집...":"グラフの編集...","원래 크기로":"元のサイズに戻す","가로세로 비율 맞추기":"縦横比を固定","도형 서식...":"図形の書式設定...",
  "행 삽입":"行の挿入","열 삽입":"列の挿入","행 삭제":"行の削除","열 삭제":"列の削除","숨기기":"非表示","숨기기 취소":"再表示",
  "찾기...":"検索...","바꾸기...":"置換...","이동...":"ジャンプ...","시트 삽입 (Shift+F11)":"シートの挿入 (Shift+F11)",
  "일러스트레이션":"図","그림 삽입 (PNG, JPEG, GIF, BMP). 스크린샷은 Ctrl+V로 붙여넣을 수 있습니다.":"画像の挿入 (PNG、JPEG、GIF、BMP)。スクリーンショットは Ctrl+V で貼り付けできます。",
  "도형":"図形","링크":"リンク","하이퍼링크 (Ctrl+K)":"ハイパーリンク (Ctrl+K)","CSV·Parquet을 데이터 시트로":"CSV/Parquet をデータシートに",
  "빠른 채우기 (Ctrl+E)":"フラッシュ フィル (Ctrl+E)","중복된 항목 제거":"重複の削除","데이터 유효성 검사":"データの入力規則",
  "목표값 찾기...":"ゴール シーク...","시나리오 관리자...":"シナリオの管理...","데이터 표...":"データ テーブル...","그룹":"グループ化","그룹 해제":"グループ解除",
  "인쇄 영역":"印刷範囲","머리글과 바닥글 (페이지 번호, 날짜, 시트 이름…)":"ヘッダーとフッター (ページ番号、日付、シート名…)","페이지 나누기":"改ページ",
  "첫 행 고정":"先頭行の固定","첫 열 고정":"先頭列の固定","자동 합계":"オート SUM","붙여넣기 옵션":"貼り付けオプション","시트 옵션":"シート オプション","창":"ウィンドウ",
  "표 이름:":"テーブル名:","범위로 변환":"範囲に変換","요약 행":"集計行","줄무늬 행":"縞模様の行","필터 단추":"フィルター ボタン",
});
Object.assign(simplifiedChinese, {
  "편집":"编辑","잘라내기":"剪切","복사":"复制","내용 지우기":"清除内容","행 높이...":"行高...","열 너비...":"列宽...",
  "새로 고침":"刷新","피벗 테이블 필드...":"数据透视表字段...","피벗 테이블 삭제":"删除数据透视表","선택하여 붙여넣기...":"选择性粘贴...",
  "스파크라인 편집...":"编辑迷你图...","스파크라인 지우기":"清除迷你图","메모 편집":"编辑批注","새 메모":"新建批注","메모 삭제":"删除批注",
  "범례":"图例","차트 편집...":"编辑图表...","원래 크기로":"恢复原始大小","가로세로 비율 맞추기":"锁定纵横比","도형 서식...":"设置形状格式...",
  "행 삽입":"插入行","열 삽입":"插入列","행 삭제":"删除行","열 삭제":"删除列","숨기기":"隐藏","숨기기 취소":"取消隐藏",
  "찾기...":"查找...","바꾸기...":"替换...","이동...":"定位...","시트 삽입 (Shift+F11)":"插入工作表 (Shift+F11)",
  "일러스트레이션":"插图","그림 삽입 (PNG, JPEG, GIF, BMP). 스크린샷은 Ctrl+V로 붙여넣을 수 있습니다.":"插入图片 (PNG、JPEG、GIF、BMP)。可使用 Ctrl+V 粘贴屏幕截图。",
  "도형":"形状","링크":"链接","하이퍼링크 (Ctrl+K)":"超链接 (Ctrl+K)","CSV·Parquet을 데이터 시트로":"将 CSV/Parquet 导入数据工作表",
  "빠른 채우기 (Ctrl+E)":"快速填充 (Ctrl+E)","중복된 항목 제거":"删除重复项","데이터 유효성 검사":"数据验证",
  "목표값 찾기...":"单变量求解...","시나리오 관리자...":"方案管理器...","데이터 표...":"模拟运算表...","그룹":"组合","그룹 해제":"取消组合",
  "인쇄 영역":"打印区域","머리글과 바닥글 (페이지 번호, 날짜, 시트 이름…)":"页眉和页脚（页码、日期、工作表名称…）","페이지 나누기":"分页符",
  "첫 행 고정":"冻结首行","첫 열 고정":"冻结首列","자동 합계":"自动求和","붙여넣기 옵션":"粘贴选项","시트 옵션":"工作表选项","창":"窗口",
  "표 이름:":"表名称：","범위로 변환":"转换为区域","요약 행":"汇总行","줄무늬 행":"镶边行","필터 단추":"筛选按钮",
});
Object.assign(traditionalChinese, {
  "편집":"編輯","잘라내기":"剪下","복사":"複製","내용 지우기":"清除內容","행 높이...":"列高...","열 너비...":"欄寬...",
  "새로 고침":"重新整理","피벗 테이블 필드...":"樞紐分析表欄位...","피벗 테이블 삭제":"刪除樞紐分析表","선택하여 붙여넣기...":"選擇性貼上...",
  "스파크라인 편집...":"編輯走勢圖...","스파크라인 지우기":"清除走勢圖","메모 편집":"編輯備註","새 메모":"新增備註","메모 삭제":"刪除備註",
  "범례":"圖例","차트 편집...":"編輯圖表...","원래 크기로":"還原原始大小","가로세로 비율 맞추기":"鎖定長寬比","도형 서식...":"設定圖案格式...",
  "행 삽입":"插入列","열 삽입":"插入欄","행 삭제":"刪除列","열 삭제":"刪除欄","숨기기":"隱藏","숨기기 취소":"取消隱藏",
  "찾기...":"尋找...","바꾸기...":"取代...","이동...":"移至...","시트 삽입 (Shift+F11)":"插入工作表 (Shift+F11)",
  "일러스트레이션":"插圖","그림 삽입 (PNG, JPEG, GIF, BMP). 스크린샷은 Ctrl+V로 붙여넣을 수 있습니다.":"插入圖片 (PNG、JPEG、GIF、BMP)。可使用 Ctrl+V 貼上螢幕擷取畫面。",
  "도형":"圖案","링크":"連結","하이퍼링크 (Ctrl+K)":"超連結 (Ctrl+K)","CSV·Parquet을 데이터 시트로":"將 CSV/Parquet 匯入資料工作表",
  "빠른 채우기 (Ctrl+E)":"快速填入 (Ctrl+E)","중복된 항목 제거":"移除重複","데이터 유효성 검사":"資料驗證",
  "목표값 찾기...":"目標搜尋...","시나리오 관리자...":"案例管理員...","데이터 표...":"資料表...","그룹":"群組","그룹 해제":"取消群組",
  "인쇄 영역":"列印範圍","머리글과 바닥글 (페이지 번호, 날짜, 시트 이름…)":"頁首及頁尾（頁碼、日期、工作表名稱…）","페이지 나누기":"分頁符號",
  "첫 행 고정":"凍結頂端列","첫 열 고정":"凍結首欄","자동 합계":"自動加總","붙여넣기 옵션":"貼上選項","시트 옵션":"工作表選項","창":"視窗",
  "표 이름:":"表格名稱：","범위로 변환":"轉換成範圍","요약 행":"摘要列","줄무늬 행":"帶狀列","필터 단추":"篩選按鈕",
});

const dictionary: Record<Locale, Record<string, string>> = {
  "ko-KR": {},
  "en-US": english,
  "ja-JP": japanese,
  "zh-CN": simplifiedChinese,
  "zh-TW": traditionalChinese,
};

export function translate(locale: Locale, key: MessageKey): string {
  return messages[locale][key] ?? messages["en-US"][key];
}

/**
 * The language on first launch, until the user picks one. Korean for now:
 * the other languages are only partly translated, and detecting the OS
 * language would start most non-Korean PCs in a half-translated English UI.
 */
export const DEFAULT_LOCALE: Locale = "ko-KR";

let activeLocale: Locale = DEFAULT_LOCALE;

export function translateText(locale: Locale, source: string): string {
  if (locale === "ko-KR") return source;
  const exact = dictionary[locale][source] ?? dictionary["en-US"][source];
  if (exact !== undefined) return exact;
  const prefixes = Object.keys(dictionary[locale]).filter((key) => key.endsWith(":") && source.startsWith(key)).sort((a, b) => b.length - a.length);
  const prefix = prefixes[0];
  if (prefix) return `${dictionary[locale][prefix]}${source.slice(prefix.length)}`;
  const englishPrefix = Object.keys(dictionary["en-US"]).filter((key) => key.endsWith(":") && source.startsWith(key)).sort((a, b) => b.length - a.length)[0];
  if (englishPrefix) return `${dictionary["en-US"][englishPrefix]}${source.slice(englishPrefix.length)}`;
  return source;
}

export function getActiveLocale(): Locale {
  return activeLocale;
}

export function tr(source: string): string {
  return translateText(activeLocale, source);
}

export function formatLocaleNumber(value: number, locale: Locale, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(locale, options).format(value);
}

export function formatLocaleDate(value: Date | number, locale: Locale, options?: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(locale, options ?? { year: "numeric", month: "2-digit", day: "2-digit" }).format(value);
}

export function formatLocaleTime(value: Date | number, locale: Locale, options?: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(locale, options ?? { hour: "2-digit", minute: "2-digit" }).format(value);
}

export function useI18n() {
  const [locale, setLocaleState] = useState<Locale>(() => {
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      const initial = saved && LOCALES.includes(saved as Locale) ? (saved as Locale) : DEFAULT_LOCALE;
      activeLocale = initial;
      return initial;
    } catch {
      activeLocale = DEFAULT_LOCALE;
      return DEFAULT_LOCALE;
    }
  });

  const setLocale = (next: Locale) => {
    activeLocale = next;
    setLocaleState(next);
  };

  useEffect(() => {
    activeLocale = locale;
    try {
      localStorage.setItem(STORAGE_KEY, locale);
    } catch {
      // Language selection still works for this session when storage is unavailable.
    }
    document.documentElement.lang = locale;
  }, [locale]);

  return {
    locale,
    setLocale,
    t: (key: MessageKey) => translate(locale, key),
  };
}
