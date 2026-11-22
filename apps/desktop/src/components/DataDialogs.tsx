// Dialogs of the 데이터 tab and friends: sort, AutoFilter menu, text to columns,
// remove duplicates, data validation, find & replace, names, hyperlinks, goal seek.
import { useEffect, useMemo, useRef, useState } from "react";
import type {
  ProtectOptions,
  ErrorStyle,
  FilterOp,
  FilterValue,
  NameView,
  SheetColumnFilter,
  SortKey,
  SplitOptions,
  Validation,
  ValidationFailure,
  ValidationKind,
  ValidationOp,
} from "../api";
import { Modal } from "./Dialogs";

export interface ColumnChoice {
  col: number;
  label: string;
}

// ---------------------------------------------------------------- 정렬

export function SortDialog(props: {
  columns: (hasHeader: boolean) => ColumnChoice[];
  initialHeader: boolean;
  initialCol: number;
  onDone(r: { keys: SortKey[]; hasHeader: boolean } | null): void;
}) {
  const [hasHeader, setHasHeader] = useState(props.initialHeader);
  const [keys, setKeys] = useState<SortKey[]>([{ col: props.initialCol, descending: false }]);
  const cols = props.columns(hasHeader);
  return (
    <Modal title="정렬" wide onClose={() => props.onDone(null)}>
      <div className="modal-body">
        <div className="cf-row">
          <button onClick={() => setKeys([...keys, { col: cols.find((c) => !keys.some((k) => k.col === c.col))?.col ?? cols[0].col, descending: false }])}>
            + 기준 추가
          </button>
          <button disabled={keys.length <= 1} onClick={() => setKeys(keys.slice(0, -1))}>
            × 기준 삭제
          </button>
          <span className="spacer" />
          <label className="check">
            <input type="checkbox" checked={hasHeader} onChange={(e) => setHasHeader(e.target.checked)} /> 내 데이터에 머리글 표시
          </label>
        </div>
        <table className="cf-table">
          <thead>
            <tr>
              <th />
              <th>열</th>
              <th>정렬 기준</th>
              <th>정렬</th>
            </tr>
          </thead>
          <tbody>
            {keys.map((k, i) => (
              <tr key={i}>
                <td>{i === 0 ? "정렬 기준" : "다음 기준"}</td>
                <td>
                  <select value={k.col} onChange={(e) => setKeys(keys.map((x, j) => (j === i ? { ...x, col: Number(e.target.value) } : x)))}>
                    {cols.map((c) => (
                      <option key={c.col} value={c.col}>
                        {c.label}
                      </option>
                    ))}
                  </select>
                </td>
                <td>셀 값</td>
                <td>
                  <select value={k.descending ? "d" : "a"} onChange={(e) => setKeys(keys.map((x, j) => (j === i ? { ...x, descending: e.target.value === "d" } : x)))}>
                    <option value="a">오름차순</option>
                    <option value="d">내림차순</option>
                  </select>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="actions">
          <button className="primary" onClick={() => props.onDone({ keys, hasHeader })}>
            확인
          </button>
          <button onClick={() => props.onDone(null)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 필터 단추 메뉴

const TEXT_OPS: [FilterOp, string][] = [
  ["Equal", "같음"],
  ["NotEqual", "같지 않음"],
  ["BeginsWith", "시작 문자"],
  ["EndsWith", "끝 문자"],
  ["Contains", "포함"],
  ["NotContains", "포함하지 않음"],
];
const NUM_OPS: [FilterOp, string][] = [
  ["Equal", "같음"],
  ["NotEqual", "같지 않음"],
  ["GreaterThan", "보다 큼"],
  ["GreaterThanOrEqual", "크거나 같음"],
  ["LessThan", "보다 작음"],
  ["LessThanOrEqual", "작거나 같음"],
];

export function FilterMenu(props: {
  x: number;
  y: number;
  col: number;
  values: FilterValue[];
  current: SheetColumnFilter | null;
  numeric: boolean;
  onSort(descending: boolean): void;
  onApply(f: SheetColumnFilter | null): void;
  onClose(): void;
}) {
  const [checked, setChecked] = useState(() => new Set(props.values.filter((v) => v.checked).map((v) => v.text)));
  const [search, setSearch] = useState("");
  const [custom, setCustom] = useState(!!props.current?.conditions.length);
  const ops = props.numeric ? NUM_OPS : TEXT_OPS;
  const [c1, setC1] = useState(props.current?.conditions[0] ?? { op: ops[props.numeric ? 2 : 4][0], value: "" });
  const [c2, setC2] = useState(props.current?.conditions[1] ?? { op: ops[0][0], value: "" });
  const [and, setAnd] = useState(props.current?.and ?? true);
  const ref = useRef<HTMLDivElement>(null);
  const opened = useRef(performance.now());
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (e.timeStamp <= opened.current) return;
      if (!ref.current?.contains(e.target as Node)) props.onClose();
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  });
  const shown = props.values.filter((v) => !search || v.text.toLowerCase().includes(search.toLowerCase()));
  const label = (t: string) => (t === "" ? "(필드 값 없음)" : t);
  const all = shown.every((v) => checked.has(v.text));
  const left = Math.min(props.x, window.innerWidth - 270);
  const top = Math.min(props.y, window.innerHeight - 440);
  return (
    <div className="filter-menu" ref={ref} style={{ left, top }}>
      <button className="menu-item" onClick={() => props.onSort(false)}>
        <span className="menu-icon">↑</span>
        {props.numeric ? "숫자 오름차순 정렬" : "텍스트 오름차순 정렬"}
      </button>
      <button className="menu-item" onClick={() => props.onSort(true)}>
        <span className="menu-icon">↓</span>
        {props.numeric ? "숫자 내림차순 정렬" : "텍스트 내림차순 정렬"}
      </button>
      <div className="menu-sep" />
      <button className="menu-item" disabled={!props.current} onClick={() => props.onApply(null)}>
        <span className="menu-icon">⊘</span>
        필터 해제
      </button>
      <button className="menu-item" onClick={() => setCustom((v) => !v)}>
        <span className="menu-icon">{custom ? "▾" : "▸"}</span>
        {props.numeric ? "숫자 필터" : "텍스트 필터"}
      </button>
      {custom ? (
        <div className="filter-custom">
          {[c1, c2].map((c, i) => (
            <div className="cf-row" key={i}>
              <select value={c.op} onChange={(e) => (i ? setC2 : setC1)({ ...c, op: e.target.value as FilterOp })}>
                {ops.map(([o, l]) => (
                  <option key={o} value={o}>
                    {l}
                  </option>
                ))}
              </select>
              <input value={c.value} autoFocus={i === 0} onChange={(e) => (i ? setC2 : setC1)({ ...c, value: e.target.value })} />
              {i === 0 && (
                <span className="and-or">
                  <label className="check">
                    <input type="radio" checked={and} onChange={() => setAnd(true)} /> 그리고
                  </label>
                  <label className="check">
                    <input type="radio" checked={!and} onChange={() => setAnd(false)} /> 또는
                  </label>
                </span>
              )}
            </div>
          ))}
          <div className="muted">?는 한 글자, *는 여러 글자를 나타냅니다.</div>
        </div>
      ) : (
        <>
          <input className="filter-search" placeholder="검색" value={search} onChange={(e) => setSearch(e.target.value)} />
          <div className="filter-list">
            <label className="check">
              <input
                type="checkbox"
                checked={all}
                onChange={(e) => {
                  const next = new Set(checked);
                  for (const v of shown) {
                    if (e.target.checked) next.add(v.text);
                    else next.delete(v.text);
                  }
                  setChecked(next);
                }}
              />
              (모두 선택)
            </label>
            {shown.slice(0, 2000).map((v) => (
              <label className="check" key={v.text}>
                <input
                  type="checkbox"
                  checked={checked.has(v.text)}
                  onChange={(e) => {
                    const next = new Set(checked);
                    if (e.target.checked) next.add(v.text);
                    else next.delete(v.text);
                    setChecked(next);
                  }}
                />
                {label(v.text)}
              </label>
            ))}
          </div>
        </>
      )}
      <div className="actions">
        <button
          className="primary"
          onClick={() => {
            if (custom) {
              const conds = [c1, c2].filter((c) => c.value.trim() !== "");
              props.onApply(conds.length ? { col: props.col, values: null, conditions: conds, and } : null);
              return;
            }
            const everything = props.values.every((v) => checked.has(v.text));
            if (everything) props.onApply(null);
            else if (search) {
              // With a search, Excel keeps only the matching checked items.
              props.onApply({ col: props.col, values: shown.filter((v) => checked.has(v.text)).map((v) => v.text), conditions: [], and: true });
            } else props.onApply({ col: props.col, values: [...checked], conditions: [], and: true });
          }}
        >
          확인
        </button>
        <button onClick={props.onClose}>취소</button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- 텍스트 나누기

/** The same split as the engine's, for the preview. */
export function splitPreview(text: string, o: SplitOptions): string[] {
  if (o.widths) {
    const out: string[] = [];
    let start = 0;
    for (const b of [...[...o.widths].sort((a, b) => a - b), text.length]) {
      const end = Math.min(b, text.length);
      if (end > start || out.length === 0) out.push(text.slice(Math.min(start, end), end).trim());
      start = end;
    }
    return out;
  }
  const out: string[] = [];
  let cur = "";
  let quoted = false;
  let lastDelim = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (o.qualifier && ch === o.qualifier) {
      if (quoted && text[i + 1] === ch) {
        cur += ch;
        i++;
      } else quoted = !quoted;
      lastDelim = false;
      continue;
    }
    if (!quoted && o.delimiters.includes(ch)) {
      if (!(o.consecutive && lastDelim)) {
        out.push(cur);
        cur = "";
      }
      lastDelim = true;
      continue;
    }
    lastDelim = false;
    cur += ch;
  }
  out.push(cur);
  return out;
}

export function TextToColumnsDialog(props: { samples: string[]; dest: string; onDone(o: (SplitOptions & { destText: string }) | null): void }) {
  const [mode, setMode] = useState<"delim" | "fixed">("delim");
  const [d, setD] = useState({ tab: true, semi: false, comma: false, space: false, other: false });
  const [other, setOther] = useState("");
  const [consecutive, setConsecutive] = useState(false);
  const [qualifier, setQualifier] = useState<string>('"');
  const [widths, setWidths] = useState("");
  const [dest, setDest] = useState(props.dest);
  const opts: SplitOptions = {
    delimiters: (d.tab ? "\t" : "") + (d.semi ? ";" : "") + (d.comma ? "," : "") + (d.space ? " " : "") + (d.other ? other : ""),
    consecutive,
    qualifier: qualifier || null,
    widths:
      mode === "fixed"
        ? widths
            .split(/[,\s]+/)
            .map(Number)
            .filter((n) => Number.isFinite(n) && n > 0)
        : null,
    dest_row: 0,
    dest_col: 0,
  };
  const preview = props.samples.slice(0, 8).map((s) => splitPreview(s, opts));
  const width = Math.max(1, ...preview.map((r) => r.length));
  return (
    <Modal title="텍스트 마법사" wide onClose={() => props.onDone(null)}>
      <div className="modal-body">
        <div className="cf-row">
          <label className="check">
            <input type="radio" checked={mode === "delim"} onChange={() => setMode("delim")} /> 구분 기호로 분리됨
          </label>
          <label className="check">
            <input type="radio" checked={mode === "fixed"} onChange={() => setMode("fixed")} /> 너비가 일정함
          </label>
        </div>
        {mode === "delim" ? (
          <fieldset className="radio-group">
            <legend>구분 기호</legend>
            {(
              [
                ["tab", "탭"],
                ["semi", "세미콜론"],
                ["comma", "쉼표"],
                ["space", "공백"],
              ] as const
            ).map(([k, l]) => (
              <label className="check" key={k}>
                <input type="checkbox" checked={d[k]} onChange={(e) => setD({ ...d, [k]: e.target.checked })} /> {l}
              </label>
            ))}
            <label className="check">
              <input type="checkbox" checked={d.other} onChange={(e) => setD({ ...d, other: e.target.checked })} /> 기타
              <input className="tiny" maxLength={1} value={other} onChange={(e) => setOther(e.target.value)} />
            </label>
            <label className="check">
              <input type="checkbox" checked={consecutive} onChange={(e) => setConsecutive(e.target.checked)} /> 연속된 구분 기호를 하나로 처리
            </label>
            <label className="check">
              텍스트 한정자
              <select value={qualifier} onChange={(e) => setQualifier(e.target.value)}>
                <option value={'"'}>"</option>
                <option value="'">'</option>
                <option value="">{"{없음}"}</option>
              </select>
            </label>
          </fieldset>
        ) : (
          <label>
            나눌 위치 (글자 수, 쉼표로 구분)
            <input value={widths} onChange={(e) => setWidths(e.target.value)} placeholder="예: 4, 6" />
          </label>
        )}
        <label>
          대상
          <input value={dest} onChange={(e) => setDest(e.target.value)} />
        </label>
        <div className="split-preview">
          <table>
            <tbody>
              {preview.map((r, i) => (
                <tr key={i}>
                  {Array.from({ length: width }, (_, j) => (
                    <td key={j}>{r[j] ?? ""}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="actions">
          <button className="primary" onClick={() => props.onDone({ ...opts, destText: dest })}>
            마침
          </button>
          <button onClick={() => props.onDone(null)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 중복된 항목 제거

export function DuplicatesDialog(props: {
  columns: (hasHeader: boolean) => ColumnChoice[];
  initialHeader: boolean;
  onDone(r: { cols: number[]; hasHeader: boolean } | null): void;
}) {
  const [hasHeader, setHasHeader] = useState(props.initialHeader);
  const cols = props.columns(hasHeader);
  const [picked, setPicked] = useState(() => new Set(cols.map((c) => c.col)));
  return (
    <Modal title="중복 값 제거" onClose={() => props.onDone(null)}>
      <div className="modal-body">
        <div>중복 값을 삭제하려면 중복 값이 포함된 열을 하나 이상 선택하세요.</div>
        <div className="cf-row">
          <button onClick={() => setPicked(new Set(cols.map((c) => c.col)))}>모두 선택</button>
          <button onClick={() => setPicked(new Set())}>모두 선택 취소</button>
          <span className="spacer" />
          <label className="check">
            <input type="checkbox" checked={hasHeader} onChange={(e) => setHasHeader(e.target.checked)} /> 내 데이터에 머리글 표시
          </label>
        </div>
        <div className="filter-list tall">
          {cols.map((c) => (
            <label className="check" key={c.col}>
              <input
                type="checkbox"
                checked={picked.has(c.col)}
                onChange={(e) => {
                  const n = new Set(picked);
                  if (e.target.checked) n.add(c.col);
                  else n.delete(c.col);
                  setPicked(n);
                }}
              />
              {c.label}
            </label>
          ))}
        </div>
        <div className="actions">
          <button className="primary" disabled={picked.size === 0} onClick={() => props.onDone({ cols: [...picked].sort((a, b) => a - b), hasHeader })}>
            확인
          </button>
          <button onClick={() => props.onDone(null)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 데이터 유효성 검사

const KINDS: [ValidationKind, string][] = [
  ["Any", "모든 값"],
  ["WholeNumber", "정수"],
  ["Decimal", "소수점"],
  ["List", "목록"],
  ["Date", "날짜"],
  ["Time", "시간"],
  ["TextLength", "텍스트 길이"],
  ["Custom", "사용자 지정"],
];
const VOPS: [ValidationOp, string][] = [
  ["Between", "해당 범위"],
  ["NotBetween", "제외 범위"],
  ["Equal", "="],
  ["NotEqual", "<>"],
  ["GreaterThan", ">"],
  ["LessThan", "<"],
  ["GreaterThanOrEqual", ">="],
  ["LessThanOrEqual", "<="],
];

export function serialToIso(serial: number): string {
  const d = new Date(Math.round((serial - 25569) * 86400000));
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}-${String(d.getUTCDate()).padStart(2, "0")}`;
}

export function isoToSerial(text: string): number | null {
  const m = /^(\d{4})[-./](\d{1,2})[-./](\d{1,2})$/.exec(text.trim());
  if (!m) return null;
  return Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])) / 86400000 + 25569;
}

export const EMPTY_VALIDATION: Validation = {
  sheet: 0,
  range: "",
  kind: "Any",
  op: "Between",
  formula1: "",
  formula2: "",
  allow_blank: true,
  in_cell_dropdown: true,
  show_input: true,
  input_title: "",
  input_message: "",
  show_error: true,
  error_style: "Stop",
  error_title: "",
  error_message: "",
};

/** How a stored formula is shown in the dialog (dates as yyyy-mm-dd, lists without quotes). */
function toField(kind: ValidationKind, f: string): string {
  if (!f) return "";
  if (kind === "List") return f.startsWith('"') ? f.slice(1, -1) : `=${f}`;
  if (kind === "Date" && /^\d+(\.\d+)?$/.test(f)) return serialToIso(Number(f));
  if (kind === "Custom") return `=${f}`;
  return f;
}

function fromField(kind: ValidationKind, v: string): string {
  const t = v.trim();
  if (!t) return "";
  if (t.startsWith("=")) return t.slice(1);
  if (kind === "List") return `"${t}"`;
  if (kind === "Date") return String(isoToSerial(t) ?? t);
  return t;
}

export function ValidationDialog(props: { initial: Validation | null; onDone(r: Validation | "clear" | null): void }) {
  const init = props.initial ?? EMPTY_VALIDATION;
  const [tab, setTab] = useState<"설정" | "설명 메시지" | "오류 메시지">("설정");
  const [v, setV] = useState<Validation>(init);
  const [f1, setF1] = useState(toField(init.kind, init.formula1));
  const [f2, setF2] = useState(toField(init.kind, init.formula2));
  const usesOp = !["Any", "List", "Custom"].includes(v.kind);
  const two = usesOp && (v.op === "Between" || v.op === "NotBetween");
  const label1 = v.kind === "List" ? "원본" : v.kind === "Custom" ? "수식" : two ? "최소값" : "값";
  return (
    <Modal title="데이터 유효성" wide onClose={() => props.onDone(null)}>
      <div className="tabs-head">
        {(["설정", "설명 메시지", "오류 메시지"] as const).map((t) => (
          <button key={t} className={t === tab ? "on" : ""} onClick={() => setTab(t)}>
            {t}
          </button>
        ))}
      </div>
      <div className="modal-body fmt-body">
        {tab === "설정" && (
          <div className="fmt-form">
            <div className="cf-row">
              <label>
                제한 대상
                <select value={v.kind} onChange={(e) => setV({ ...v, kind: e.target.value as ValidationKind })}>
                  {KINDS.map(([k, l]) => (
                    <option key={k} value={k}>
                      {l}
                    </option>
                  ))}
                </select>
              </label>
              <label className="check">
                <input type="checkbox" checked={v.allow_blank} onChange={(e) => setV({ ...v, allow_blank: e.target.checked })} /> 공백 무시
              </label>
              {v.kind === "List" && (
                <label className="check">
                  <input type="checkbox" checked={v.in_cell_dropdown} onChange={(e) => setV({ ...v, in_cell_dropdown: e.target.checked })} /> 드롭다운 표시
                </label>
              )}
            </div>
            {usesOp && (
              <label>
                제한 방법
                <select value={v.op} onChange={(e) => setV({ ...v, op: e.target.value as ValidationOp })}>
                  {VOPS.map(([o, l]) => (
                    <option key={o} value={o}>
                      {l}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {v.kind !== "Any" && (
              <label>
                {label1}
                <input
                  value={f1}
                  onChange={(e) => setF1(e.target.value)}
                  placeholder={v.kind === "List" ? "서울,부산,대구 또는 =$E$1:$E$10" : v.kind === "Date" ? "2026-01-01" : ""}
                />
              </label>
            )}
            {two && (
              <label>
                최대값
                <input value={f2} onChange={(e) => setF2(e.target.value)} placeholder={v.kind === "Date" ? "2026-12-31" : ""} />
              </label>
            )}
          </div>
        )}
        {tab === "설명 메시지" && (
          <div className="fmt-form">
            <label className="check">
              <input type="checkbox" checked={v.show_input} onChange={(e) => setV({ ...v, show_input: e.target.checked })} /> 셀을 선택하면 설명 메시지 표시
            </label>
            <label>
              제목
              <input value={v.input_title} onChange={(e) => setV({ ...v, input_title: e.target.value })} />
            </label>
            <label>
              설명 메시지
              <textarea value={v.input_message} onChange={(e) => setV({ ...v, input_message: e.target.value })} />
            </label>
          </div>
        )}
        {tab === "오류 메시지" && (
          <div className="fmt-form">
            <label className="check">
              <input type="checkbox" checked={v.show_error} onChange={(e) => setV({ ...v, show_error: e.target.checked })} /> 유효하지 않은 데이터를 입력하면 오류 메시지 표시
            </label>
            <label>
              스타일
              <select value={v.error_style} onChange={(e) => setV({ ...v, error_style: e.target.value as ErrorStyle })}>
                <option value="Stop">중지</option>
                <option value="Warning">경고</option>
                <option value="Information">정보</option>
              </select>
            </label>
            <label>
              제목
              <input value={v.error_title} onChange={(e) => setV({ ...v, error_title: e.target.value })} />
            </label>
            <label>
              오류 메시지
              <textarea value={v.error_message} onChange={(e) => setV({ ...v, error_message: e.target.value })} />
            </label>
          </div>
        )}
        <div className="actions">
          <button onClick={() => props.onDone("clear")}>모두 지우기</button>
          <span className="spacer" />
          <button
            className="primary"
            onClick={() => props.onDone({ ...v, formula1: fromField(v.kind, f1), formula2: two ? fromField(v.kind, f2) : "" })}
          >
            확인
          </button>
          <button onClick={() => props.onDone(null)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

/** Excel's alert after an entry that breaks a validation rule. */
export function ValidationAlert(props: { failure: ValidationFailure; onDone(choice: "retry" | "accept" | "cancel"): void }) {
  const f = props.failure;
  const icon = f.style === "Stop" ? "⛔" : f.style === "Warning" ? "⚠" : "ℹ";
  return (
    <Modal title={f.title} onClose={() => props.onDone("cancel")}>
      <div className="modal-body">
        <div className="alert-row">
          <span className={`alert-icon ${f.style.toLowerCase()}`}>{icon}</span>
          <div>
            {f.message}
            {f.style === "Warning" && <div className="muted">계속하시겠습니까?</div>}
          </div>
        </div>
        <div className="actions">
          {f.style === "Stop" && (
            <>
              <button className="primary" autoFocus onClick={() => props.onDone("retry")}>
                다시 시도
              </button>
              <button onClick={() => props.onDone("cancel")}>취소</button>
            </>
          )}
          {f.style === "Warning" && (
            <>
              <button className="primary" autoFocus onClick={() => props.onDone("accept")}>
                예
              </button>
              <button onClick={() => props.onDone("retry")}>아니요</button>
              <button onClick={() => props.onDone("cancel")}>취소</button>
            </>
          )}
          {f.style === "Information" && (
            <>
              <button className="primary" autoFocus onClick={() => props.onDone("accept")}>
                확인
              </button>
              <button onClick={() => props.onDone("cancel")}>취소</button>
            </>
          )}
        </div>
      </div>
    </Modal>
  );
}

/** The in-cell dropdown of a list rule. */
export function ListDropdown(props: { x: number; y: number; items: string[]; current: string; onPick(v: string): void; onClose(): void }) {
  const ref = useRef<HTMLDivElement>(null);
  const opened = useRef(performance.now());
  const [i, setI] = useState(Math.max(0, props.items.indexOf(props.current)));
  useEffect(() => {
    const close = (e: MouseEvent) => {
      // The click that opened the list is still bubbling: not a click outside.
      if (e.timeStamp <= opened.current) return;
      if (!ref.current?.contains(e.target as Node)) props.onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
      else if (e.key === "ArrowDown") setI((x) => Math.min(props.items.length - 1, x + 1));
      else if (e.key === "ArrowUp") setI((x) => Math.max(0, x - 1));
      else if (e.key === "Enter") props.onPick(props.items[i]);
      else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key, true);
    };
  });
  return (
    <div className="list-dropdown" ref={ref} style={{ left: props.x, top: props.y }}>
      {props.items.map((it, j) => (
        <div key={it} className={j === i ? "on" : ""} onMouseEnter={() => setI(j)} onMouseDown={(e) => (e.preventDefault(), props.onPick(it))}>
          {it}
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- 찾기 및 바꾸기

export interface FindOptions {
  find: string;
  replace: string;
  matchCase: boolean;
  wholeCell: boolean;
  workbook: boolean;
}

export function FindReplaceDialog(props: {
  initialTab: "찾기" | "바꾸기";
  message: string | null;
  onFind(o: FindOptions): void;
  onReplace(o: FindOptions): void;
  onReplaceAll(o: FindOptions): void;
  onClose(): void;
}) {
  const [tab, setTab] = useState(props.initialTab);
  const [o, setO] = useState<FindOptions>({ find: "", replace: "", matchCase: false, wholeCell: false, workbook: false });
  useEffect(() => setTab(props.initialTab), [props.initialTab]);
  return (
    <div className="find-box wide" onMouseDown={(e) => e.stopPropagation()}>
      <div className="modal-title">
        <span>찾기 및 바꾸기</span>
        <button className="x" onClick={props.onClose} aria-label="닫기">
          ✕
        </button>
      </div>
      <div className="tabs-head">
        {(["찾기", "바꾸기"] as const).map((t) => (
          <button key={t} className={t === tab ? "on" : ""} onClick={() => setTab(t)}>
            {t}
          </button>
        ))}
      </div>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onFind(o);
        }}
      >
        <label>
          찾을 내용
          <input autoFocus value={o.find} onChange={(e) => setO({ ...o, find: e.target.value })} />
        </label>
        {tab === "바꾸기" && (
          <label>
            바꿀 내용
            <input value={o.replace} onChange={(e) => setO({ ...o, replace: e.target.value })} />
          </label>
        )}
        <div className="cf-row">
          <label className="check">
            <input type="checkbox" checked={o.matchCase} onChange={(e) => setO({ ...o, matchCase: e.target.checked })} /> 대/소문자 구분
          </label>
          <label className="check">
            <input type="checkbox" checked={o.wholeCell} onChange={(e) => setO({ ...o, wholeCell: e.target.checked })} /> 전체 셀 내용 일치
          </label>
          <select value={o.workbook ? "b" : "s"} onChange={(e) => setO({ ...o, workbook: e.target.value === "b" })}>
            <option value="s">범위: 시트</option>
            <option value="b">범위: 통합 문서</option>
          </select>
        </div>
        {props.message && <div className="muted">{props.message}</div>}
        <div className="actions">
          {tab === "바꾸기" && (
            <>
              <button type="button" onClick={() => props.onReplaceAll(o)}>
                모두 바꾸기
              </button>
              <button type="button" onClick={() => props.onReplace(o)}>
                바꾸기
              </button>
            </>
          )}
          <button type="submit" className="primary">
            다음 찾기
          </button>
          <button type="button" onClick={props.onClose}>
            닫기
          </button>
        </div>
      </form>
    </div>
  );
}

// ---------------------------------------------------------------- 이름 관리자

export function NameManagerDialog(props: {
  names: NameView[];
  sheets: string[];
  suggestion: string;
  onAdd(name: string, scope: number | null, formula: string): Promise<boolean>;
  onUpdate(old: NameView, name: string, scope: number | null, formula: string): Promise<boolean>;
  onDelete(n: NameView): void;
  onClose(): void;
  startNew?: boolean;
}) {
  const [sel, setSel] = useState<number | null>(null);
  const [edit, setEdit] = useState<{ old: NameView | null; name: string; scope: number | null; formula: string } | null>(
    props.startNew ? { old: null, name: "", scope: null, formula: props.suggestion } : null,
  );
  const list = props.names.filter((n) => !n.name.startsWith("_xlnm."));
  const scopeLabel = (s: number | null) => (s === null ? "통합 문서" : props.sheets[s] ?? `시트 ${s + 1}`);
  return (
    <Modal title={edit ? (edit.old ? "이름 편집" : "새 이름") : "이름 관리자"} wide onClose={props.onClose}>
      <div className="modal-body">
        {edit ? (
          <form
            className="fmt-form"
            onSubmit={async (e) => {
              e.preventDefault();
              const ok = edit.old ? await props.onUpdate(edit.old, edit.name, edit.scope, edit.formula) : await props.onAdd(edit.name, edit.scope, edit.formula);
              if (ok) setEdit(null);
              if (ok && props.startNew) props.onClose();
            }}
          >
            <label>
              이름
              <input autoFocus value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} />
            </label>
            <label>
              범위
              <select value={edit.scope ?? -1} onChange={(e) => setEdit({ ...edit, scope: Number(e.target.value) < 0 ? null : Number(e.target.value) })}>
                <option value={-1}>통합 문서</option>
                {props.sheets.map((s, i) => (
                  <option key={s} value={i}>
                    {s}
                  </option>
                ))}
              </select>
            </label>
            <label>
              참조 대상
              <input value={edit.formula} onChange={(e) => setEdit({ ...edit, formula: e.target.value })} />
            </label>
            <div className="actions">
              <button type="submit" className="primary">
                확인
              </button>
              <button type="button" onClick={() => (props.startNew ? props.onClose() : setEdit(null))}>
                취소
              </button>
            </div>
          </form>
        ) : (
          <>
            <div className="cf-row">
              <button onClick={() => setEdit({ old: null, name: "", scope: null, formula: props.suggestion })}>새로 만들기...</button>
              <button
                disabled={sel === null}
                onClick={() => {
                  const n = list[sel!];
                  setEdit({ old: n, name: n.name, scope: n.scope, formula: `=${n.formula.replace(/^=/, "")}` });
                }}
              >
                편집...
              </button>
              <button disabled={sel === null} onClick={() => (props.onDelete(list[sel!]), setSel(null))}>
                삭제
              </button>
            </div>
            <table className="cf-table">
              <thead>
                <tr>
                  <th>이름</th>
                  <th>참조 대상</th>
                  <th>범위</th>
                </tr>
              </thead>
              <tbody>
                {list.length === 0 && (
                  <tr>
                    <td colSpan={3} className="muted">
                      정의된 이름이 없습니다. 이름 상자에 이름을 입력해도 만들 수 있습니다.
                    </td>
                  </tr>
                )}
                {list.map((n, i) => (
                  <tr key={`${n.name}:${n.scope}`} className={sel === i ? "on" : ""} onClick={() => setSel(i)}>
                    <td>{n.name}</td>
                    <td>={n.formula.replace(/^=/, "")}</td>
                    <td>{scopeLabel(n.scope)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="actions">
              <button className="primary" onClick={props.onClose}>
                닫기
              </button>
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 하이퍼링크

export function HyperlinkDialog(props: {
  text: string;
  current: { target: string; internal: boolean } | null;
  sheets: string[];
  onDone(r: { target: string; internal: boolean; text: string } | "remove" | null): void;
}) {
  const cur = props.current;
  const [kind, setKind] = useState<"web" | "doc" | "mail">(cur?.internal ? "doc" : cur?.target.startsWith("mailto:") ? "mail" : "web");
  const [url, setUrl] = useState(cur && !cur.internal && !cur.target.startsWith("mailto:") ? cur.target : "");
  const [mail, setMail] = useState(cur?.target.startsWith("mailto:") ? cur.target.slice(7) : "");
  const internal = cur?.internal ? cur.target.split("!") : null;
  const [sheet, setSheet] = useState(internal?.[0]?.replace(/^'|'$/g, "") ?? props.sheets[0] ?? "Sheet1");
  const [cell, setCell] = useState(internal?.[1] ?? "A1");
  const [text, setText] = useState(props.text);
  const target = useMemo(() => {
    if (kind === "web") return /^[a-z]+:/i.test(url.trim()) || !url.trim() ? url.trim() : `https://${url.trim()}`;
    if (kind === "mail") return mail.trim() ? `mailto:${mail.trim()}` : "";
    const q = /^[\p{L}\p{N}_]+$/u.test(sheet) ? sheet : `'${sheet.replace(/'/g, "''")}'`;
    return `${q}!${cell.trim() || "A1"}`;
  }, [kind, url, mail, sheet, cell]);
  return (
    <Modal title={cur ? "하이퍼링크 편집" : "하이퍼링크 삽입"} onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          if (target) props.onDone({ target, internal: kind === "doc", text });
        }}
      >
        <div className="cf-row">
          <label className="check">
            <input type="radio" checked={kind === "web"} onChange={() => setKind("web")} /> 웹 페이지
          </label>
          <label className="check">
            <input type="radio" checked={kind === "doc"} onChange={() => setKind("doc")} /> 현재 문서
          </label>
          <label className="check">
            <input type="radio" checked={kind === "mail"} onChange={() => setKind("mail")} /> 전자 메일 주소
          </label>
        </div>
        <label>
          표시할 텍스트
          <input value={text} onChange={(e) => setText(e.target.value)} />
        </label>
        {kind === "web" && (
          <label>
            주소
            <input autoFocus value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://" />
          </label>
        )}
        {kind === "mail" && (
          <label>
            전자 메일 주소
            <input autoFocus value={mail} onChange={(e) => setMail(e.target.value)} />
          </label>
        )}
        {kind === "doc" && (
          <div className="cf-row">
            <label>
              시트
              <select value={sheet} onChange={(e) => setSheet(e.target.value)}>
                {props.sheets.map((s) => (
                  <option key={s}>{s}</option>
                ))}
              </select>
            </label>
            <label>
              셀 참조
              <input value={cell} onChange={(e) => setCell(e.target.value)} />
            </label>
          </div>
        )}
        <div className="actions">
          {cur && (
            <button type="button" onClick={() => props.onDone("remove")}>
              링크 제거
            </button>
          )}
          <span className="spacer" />
          <button type="submit" className="primary" disabled={!target}>
            확인
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            취소
          </button>
        </div>
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------- 목표값 찾기

export function GoalSeekDialog(props: { setCell: string; onDone(r: { setCell: string; target: number; changing: string } | null): void }) {
  const [setCell, setSetCell] = useState(props.setCell);
  const [target, setTarget] = useState("");
  const [changing, setChanging] = useState("");
  return (
    <Modal title="목표값 찾기" onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          const t = Number(target);
          if (!setCell.trim() || !changing.trim() || !Number.isFinite(t)) return;
          props.onDone({ setCell: setCell.trim(), target: t, changing: changing.trim() });
        }}
      >
        <label>
          수식 셀
          <input value={setCell} onChange={(e) => setSetCell(e.target.value)} />
        </label>
        <label>
          찾는 값
          <input autoFocus value={target} onChange={(e) => setTarget(e.target.value)} />
        </label>
        <label>
          값을 바꿀 셀
          <input value={changing} onChange={(e) => setChanging(e.target.value)} placeholder="예: B2" />
        </label>
        <div className="actions">
          <button type="submit" className="primary">
            확인
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            취소
          </button>
        </div>
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------- 부분합

export function SubtotalDialog(props: {
  columns: ColumnChoice[];
  initialGroup: number;
  onDone(r: { groupCol: number; func: "Sum" | "Count" | "Average" | "Max" | "Min"; valueCols: number[]; replace: boolean } | "removeAll" | null): void;
}) {
  const [group, setGroup] = useState(props.initialGroup);
  const [func, setFunc] = useState<"Sum" | "Count" | "Average" | "Max" | "Min">("Sum");
  const [vals, setVals] = useState(() => new Set([props.columns[props.columns.length - 1]?.col].filter((x): x is number => x !== undefined)));
  const [replace, setReplace] = useState(true);
  return (
    <Modal title="부분합" onClose={() => props.onDone(null)}>
      <div className="modal-body">
        <label>
          그룹화할 항목
          <select value={group} onChange={(e) => setGroup(Number(e.target.value))}>
            {props.columns.map((c) => (
              <option key={c.col} value={c.col}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          사용할 함수
          <select value={func} onChange={(e) => setFunc(e.target.value as typeof func)}>
            <option value="Sum">합계</option>
            <option value="Count">개수</option>
            <option value="Average">평균</option>
            <option value="Max">최대</option>
            <option value="Min">최소</option>
          </select>
        </label>
        <div>부분합 계산 항목</div>
        <div className="filter-list tall">
          {props.columns.map((c) => (
            <label className="check" key={c.col}>
              <input
                type="checkbox"
                checked={vals.has(c.col)}
                onChange={(e) => {
                  const n = new Set(vals);
                  if (e.target.checked) n.add(c.col);
                  else n.delete(c.col);
                  setVals(n);
                }}
              />
              {c.label}
            </label>
          ))}
        </div>
        <label className="check">
          <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.target.checked)} /> 새로운 값으로 대치
        </label>
        <div className="muted">먼저 그룹화할 열로 데이터를 정렬해 두세요.</div>
        <div className="actions">
          <button onClick={() => props.onDone("removeAll")}>모두 제거</button>
          <span className="spacer" />
          <button className="primary" disabled={vals.size === 0} onClick={() => props.onDone({ groupCol: group, func, valueCols: [...vals].sort((a, b) => a - b), replace })}>
            확인
          </button>
          <button onClick={() => props.onDone(null)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 표 만들기

export const TABLE_STYLES: { name: string; label: string; header: string; stripe: string }[] = [
  { name: "TableStyleLight1", label: "밝게 1", header: "#ffffff", stripe: "#D9D9D9" },
  { name: "TableStyleLight9", label: "밝게 9", header: "#4472C4", stripe: "#ffffff" },
  { name: "TableStyleMedium2", label: "보통 2", header: "#4472C4", stripe: "#D9E1F2" },
  { name: "TableStyleMedium3", label: "보통 3", header: "#ED7D31", stripe: "#FCE4D6" },
  { name: "TableStyleMedium4", label: "보통 4", header: "#A5A5A5", stripe: "#EDEDED" },
  { name: "TableStyleMedium5", label: "보통 5", header: "#FFC000", stripe: "#FFF2CC" },
  { name: "TableStyleMedium6", label: "보통 6", header: "#5B9BD5", stripe: "#DDEBF7" },
  { name: "TableStyleMedium7", label: "보통 7", header: "#70AD47", stripe: "#E2EFDA" },
  { name: "TableStyleMedium9", label: "보통 9", header: "#4472C4", stripe: "#B4C6E7" },
  { name: "TableStyleDark1", label: "어둡게 1", header: "#000000", stripe: "#737373" },
];

export function TableStyleSwatch(props: { name: string }) {
  const s = TABLE_STYLES.find((x) => x.name === props.name) ?? TABLE_STYLES[2];
  return (
    <span className="table-swatch" title={s.label}>
      <span style={{ background: s.header }} />
      <span style={{ background: s.stripe }} />
      <span style={{ background: "#ffffff" }} />
      <span style={{ background: s.stripe }} />
    </span>
  );
}

export function CreateTableDialog(props: { range: string; header: boolean; onDone(r: { range: string; header: boolean } | null): void }) {
  const [range, setRange] = useState(props.range);
  const [header, setHeader] = useState(props.header);
  return (
    <Modal title="표 만들기" onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone({ range, header });
        }}
      >
        <label>
          표에 사용할 데이터를 지정하십시오.
          <input autoFocus value={range} onChange={(e) => setRange(e.target.value)} />
        </label>
        <label className="check">
          <input type="checkbox" checked={header} onChange={(e) => setHeader(e.target.checked)} /> 머리글 포함
        </label>
        <div className="actions">
          <button type="submit" className="primary">
            확인
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            취소
          </button>
        </div>
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------- 시트 보호


const PROTECT_LABELS: [keyof ProtectOptions, string][] = [
  ["format_cells", "셀 서식"],
  ["format_columns", "열 서식"],
  ["format_rows", "행 서식"],
  ["insert_columns", "열 삽입"],
  ["insert_rows", "행 삽입"],
  ["delete_columns", "열 삭제"],
  ["delete_rows", "행 삭제"],
  ["sort", "정렬"],
  ["auto_filter", "자동 필터 사용"],
];

export function ProtectDialog(props: { onDone(r: { password: string | null; options: ProtectOptions } | null): void }) {
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [step, setStep] = useState<"opts" | "confirm">("opts");
  const [o, setO] = useState<ProtectOptions>({
    format_cells: false,
    format_columns: false,
    format_rows: false,
    insert_rows: false,
    insert_columns: false,
    delete_rows: false,
    delete_columns: false,
    sort: false,
    auto_filter: false,
  });
  const [err, setErr] = useState<string | null>(null);
  return (
    <Modal title={step === "opts" ? "시트 보호" : "암호 확인"} onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          if (step === "opts" && pw) {
            setStep("confirm");
            return;
          }
          if (step === "confirm" && pw !== pw2) {
            setErr("확인 암호가 일치하지 않습니다.");
            return;
          }
          props.onDone({ password: pw || null, options: o });
        }}
      >
        {step === "opts" ? (
          <>
            <label>
              시트 보호 해제 암호 (선택)
              <input type="password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} />
            </label>
            <div>워크시트의 모든 사용자에게 허용할 내용 (잠긴 셀 내용 변경은 항상 막힙니다):</div>
            <div className="filter-list tall">
              {PROTECT_LABELS.map(([k, l]) => (
                <label className="check" key={k}>
                  <input type="checkbox" checked={o[k]} onChange={(e) => setO({ ...o, [k]: e.target.checked })} /> {l}
                </label>
              ))}
            </div>
          </>
        ) : (
          <label>
            계속하려면 암호를 다시 입력하십시오.
            <input type="password" autoFocus value={pw2} onChange={(e) => setPw2(e.target.value)} />
          </label>
        )}
        {err && <div className="err-text">{err}</div>}
        <div className="muted">암호를 잊어버리면 복구할 수 없습니다. Excel과 같은 방식의 보호여서 강력한 보안 수단은 아닙니다.</div>
        <div className="actions">
          <button type="submit" className="primary">
            확인
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            취소
          </button>
        </div>
      </form>
    </Modal>
  );
}

export function PasswordDialog(props: { title: string; onDone(pw: string | null): void }) {
  const [pw, setPw] = useState("");
  return (
    <Modal title={props.title} onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(pw);
        }}
      >
        <label>
          암호
          <input type="password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} />
        </label>
        <div className="actions">
          <button type="submit" className="primary">
            확인
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            취소
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** Excel's message box. */
export function MessageBox(props: { title?: string; message: string; onClose(): void }) {
  return (
    <Modal title={props.title ?? "CellX"} onClose={props.onClose}>
      <div className="modal-body">
        <div className="alert-row">
          <span className="alert-icon warning">⚠</span>
          <div>{props.message}</div>
        </div>
        <div className="actions">
          <button className="primary" autoFocus onClick={props.onClose}>
            확인
          </button>
        </div>
      </div>
    </Modal>
  );
}
