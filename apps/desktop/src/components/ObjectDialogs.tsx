// Dialogs for paste special, conditional formatting, pivot tables, charts and memos.
import { useEffect, useState } from "react";
import { type Aggregate, api, type CfView, type Chart, type ChartKind, type PasteSpecial, type PivotDef, type PivotSource } from "../api";
import { CF_STYLES, type CfStyle, describeRule, HIGHLIGHT, type HighlightKind } from "../cf";
import { CHART_KINDS } from "./Chart";
import { Modal } from "./Dialogs";

// ---------------------------------------------------------------- 선택하여 붙여넣기

export function PasteSpecialDialog(props: { onDone(opts: PasteSpecial | null): void }) {
  const [o, setO] = useState<PasteSpecial>({ what: "All", op: "None", skip_blanks: false, transpose: false });
  const whats: [PasteSpecial["what"], string][] = [
    ["All", "모두"],
    ["Formulas", "수식"],
    ["Values", "값"],
    ["Formats", "서식"],
    ["ColumnWidths", "열 너비"],
  ];
  const ops: [PasteSpecial["op"], string][] = [
    ["None", "없음"],
    ["Add", "더하기"],
    ["Subtract", "빼기"],
    ["Multiply", "곱하기"],
    ["Divide", "나누기"],
  ];
  return (
    <Modal title="선택하여 붙여넣기" onClose={() => props.onDone(null)}>
      <div className="modal-body">
        <fieldset className="radio-group">
          <legend>붙여넣기</legend>
          {whats.map(([v, l]) => (
            <label key={v} className="check">
              <input type="radio" checked={o.what === v} onChange={() => setO({ ...o, what: v })} /> {l}
            </label>
          ))}
        </fieldset>
        <fieldset className="radio-group">
          <legend>연산</legend>
          {ops.map(([v, l]) => (
            <label key={v} className="check">
              <input type="radio" checked={o.op === v} disabled={o.what === "Formats" || o.what === "ColumnWidths"} onChange={() => setO({ ...o, op: v })} /> {l}
            </label>
          ))}
        </fieldset>
        <div className="row-checks">
          <label className="check">
            <input type="checkbox" checked={o.skip_blanks} onChange={(e) => setO({ ...o, skip_blanks: e.target.checked })} /> 내용 있는 셀만 붙여넣기
          </label>
          <label className="check">
            <input type="checkbox" checked={o.transpose} disabled={o.what === "ColumnWidths"} onChange={(e) => setO({ ...o, transpose: e.target.checked })} /> 행/열 바꿈
          </label>
        </div>
        <div className="actions">
          <button className="primary" autoFocus onClick={() => props.onDone(o)}>
            확인
          </button>
          <button onClick={() => props.onDone(null)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 셀 강조 규칙

export function HighlightDialog(props: { kind: HighlightKind; onDone(r: { a: string; b: string; style: CfStyle } | null): void }) {
  const spec = HIGHLIGHT[props.kind];
  const [a, setA] = useState(props.kind === "Top10" || props.kind === "Bottom10" ? "10" : "");
  const [b, setB] = useState("");
  const [style, setStyle] = useState(0);
  return (
    <Modal title={spec.title} onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          if (spec.values > 0 && !a.trim()) return;
          props.onDone({ a, b, style: CF_STYLES[style] });
        }}
      >
        <div>{spec.prompt}</div>
        <div className="cf-row">
          {spec.values >= 1 && <input autoFocus value={a} onChange={(e) => setA(e.target.value)} placeholder="값 또는 =A1" />}
          {spec.values === 2 && (
            <>
              <span>~</span>
              <input value={b} onChange={(e) => setB(e.target.value)} />
            </>
          )}
          <span>적용할 서식:</span>
          <select value={style} onChange={(e) => setStyle(Number(e.target.value))}>
            {CF_STYLES.map((s, i) => (
              <option key={s.label} value={i}>
                {s.label}
              </option>
            ))}
          </select>
        </div>
        <div className="cf-preview" style={{ background: CF_STYLES[style].fill, color: CF_STYLES[style].font }}>
          가나다 AaBbCc 123
        </div>
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

// ---------------------------------------------------------------- 규칙 관리

export function CfManagerDialog(props: { sheet: number; onChanged(): void; onClose(): void }) {
  const [rules, setRules] = useState<CfView[]>([]);
  const [sel, setSel] = useState<number | null>(null);
  const load = () => api.conditionalFormats(props.sheet).then(setRules).catch(() => setRules([]));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.sheet]);
  return (
    <Modal title="조건부 서식 규칙 관리자" wide onClose={props.onClose}>
      <div className="modal-body">
        <table className="cf-table">
          <thead>
            <tr>
              <th>규칙 (우선 순위 순)</th>
              <th>적용 대상</th>
            </tr>
          </thead>
          <tbody>
            {rules.length === 0 && (
              <tr>
                <td colSpan={2} className="muted">
                  이 시트에는 규칙이 없습니다.
                </td>
              </tr>
            )}
            {rules.map((r) => (
              <tr key={r.index} className={sel === r.index ? "on" : ""} onClick={() => setSel(r.index)}>
                <td>{describeRule(r.cf_rule)}</td>
                <td>={r.range}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <div className="actions">
          <button
            disabled={sel === null}
            onClick={async () => {
              if (sel === null) return;
              await api.deleteConditionalFormat(props.sheet, sel);
              setSel(null);
              await load();
              props.onChanged();
            }}
          >
            규칙 삭제
          </button>
          <span className="spacer" />
          <button className="primary" onClick={props.onClose}>
            닫기
          </button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 피벗 테이블

const AGGS: [Aggregate, string][] = [
  ["Sum", "합계"],
  ["Count", "개수"],
  ["Average", "평균"],
  ["Max", "최대"],
  ["Min", "최소"],
];

export function PivotDialog(props: {
  initial: PivotDef;
  tables: string[];
  sheetName(i: number): string;
  onDone(def: PivotDef | null, newSheet: boolean): void;
}) {
  const [def, setDef] = useState<PivotDef>(props.initial);
  const [fields, setFields] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [newSheet, setNewSheet] = useState(props.initial.id === 0);
  const [location, setLocation] = useState("");
  const source = def.source;

  useEffect(() => {
    let live = true;
    api
      .pivotFields(source, def.sheet)
      .then((f) => {
        if (!live) return;
        setFields(f);
        setError(null);
      })
      .catch((e) => live && (setFields([]), setError(String(e))));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(source)]);

  const used = new Set([...def.rows, ...(def.columns ? [def.columns] : []), ...def.values.map((v) => v.field)]);
  const toggle = (f: string, on: boolean) => {
    if (!on) {
      setDef({ ...def, rows: def.rows.filter((x) => x !== f), columns: def.columns === f ? null : def.columns, values: def.values.filter((v) => v.field !== f) });
      return;
    }
    // Excel: numeric-looking fields go to 값, others to 행.
    const numeric = /금액|수량|가격|합계|매출|비용|점수|amount|qty|price|total|sales|count|value/i.test(f);
    if (numeric) setDef({ ...def, values: [...def.values, { field: f, agg: "Sum" }] });
    else setDef({ ...def, rows: [...def.rows, f] });
  };
  const move = (f: string, to: "rows" | "columns" | "values") => {
    const base = { ...def, rows: def.rows.filter((x) => x !== f), columns: def.columns === f ? null : def.columns, values: def.values.filter((v) => v.field !== f) };
    if (to === "rows") base.rows.push(f);
    if (to === "columns") {
      if (base.columns) base.rows.push(base.columns);
      base.columns = f;
    }
    if (to === "values") base.values.push({ field: f, agg: "Sum" });
    setDef(base);
  };
  const area = (f: string) => (def.rows.includes(f) ? "rows" : def.columns === f ? "columns" : def.values.some((v) => v.field === f) ? "values" : "");

  return (
    <Modal title={props.initial.id ? "피벗 테이블 필드" : "피벗 테이블 만들기"} wide onClose={() => props.onDone(null, false)}>
      <div className="modal-body pivot-body">
        <label>
          표 또는 범위
          <div className="cf-row">
            <select
              value={source.kind === "Table" ? `t:${source.name}` : "range"}
              onChange={(e) => {
                const v = e.target.value;
                const s: PivotSource = v === "range" ? { kind: "Range", range: source.kind === "Range" ? source.range : "" } : { kind: "Table", name: v.slice(2) };
                setDef({ ...def, source: s, rows: [], columns: null, values: [] });
              }}
            >
              <option value="range">시트 범위</option>
              {props.tables.map((t) => (
                <option key={t} value={`t:${t}`}>
                  데이터 시트: {t}
                </option>
              ))}
            </select>
            {source.kind === "Range" && (
              <input
                value={source.range}
                onChange={(e) => setDef({ ...def, source: { kind: "Range", range: e.target.value }, rows: [], columns: null, values: [] })}
                placeholder="Sheet1!A1:F500"
              />
            )}
          </div>
        </label>
        {error && <div className="err-text">{error}</div>}
        {props.initial.id === 0 && (
          <div className="cf-row">
            <label className="check">
              <input type="radio" checked={newSheet} onChange={() => setNewSheet(true)} /> 새 워크시트
            </label>
            <label className="check">
              <input type="radio" checked={!newSheet} onChange={() => setNewSheet(false)} /> 기존 워크시트
            </label>
            {!newSheet && <input value={location} onChange={(e) => setLocation(e.target.value)} placeholder={`${props.sheetName(def.sheet)}!H1`} />}
          </div>
        )}
        <div className="pivot-grid">
          <div className="pivot-fields">
            <div className="pivot-head">보고서에 추가할 필드 선택:</div>
            {fields.map((f) => (
              <div key={f} className="pivot-field">
                <label className="check">
                  <input type="checkbox" checked={used.has(f)} onChange={(e) => toggle(f, e.target.checked)} /> {f}
                </label>
                <select value={area(f)} onChange={(e) => e.target.value && move(f, e.target.value as "rows")}>
                  <option value="">—</option>
                  <option value="rows">행</option>
                  <option value="columns">열</option>
                  <option value="values">값</option>
                </select>
              </div>
            ))}
            {fields.length === 0 && !error && <div className="muted">머리글 행이 있는 범위를 지정하세요.</div>}
          </div>
          <div className="pivot-areas">
            <div className="pivot-area">
              <div className="pivot-head">열</div>
              {def.columns && <div className="pivot-chip">{def.columns}</div>}
            </div>
            <div className="pivot-area">
              <div className="pivot-head">행</div>
              {def.rows.map((r) => (
                <div key={r} className="pivot-chip">
                  {r}
                </div>
              ))}
            </div>
            <div className="pivot-area">
              <div className="pivot-head">값</div>
              {def.values.map((v, i) => (
                <div key={v.field} className="pivot-chip">
                  <select
                    value={v.agg}
                    onChange={(e) => {
                      const values = [...def.values];
                      values[i] = { ...v, agg: e.target.value as Aggregate };
                      setDef({ ...def, values });
                    }}
                  >
                    {AGGS.map(([a, l]) => (
                      <option key={a} value={a}>
                        {l}
                      </option>
                    ))}
                  </select>
                  : {v.field}
                </div>
              ))}
            </div>
          </div>
        </div>
        <div className="actions">
          <button
            className="primary"
            onClick={() => {
              let out = def;
              if (!newSheet && location.trim()) {
                const m = /^(?:(.+)!)?\$?([A-Za-z]+)\$?(\d+)$/.exec(location.trim());
                if (!m) return setError("위치를 A1 형식으로 입력하세요");
                let col = 0;
                for (const ch of m[2].toUpperCase()) col = col * 26 + ch.charCodeAt(0) - 64;
                out = { ...def, row: Number(m[3]), col };
              }
              props.onDone(out, newSheet);
            }}
          >
            확인
          </button>
          <button onClick={() => props.onDone(null, false)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- 차트

export function ChartDialog(props: { chart: Chart; onDone(c: Chart | null): void }) {
  const [c, setC] = useState(props.chart);
  return (
    <Modal title="차트 편집" onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(c);
        }}
      >
        <label>
          차트 종류
          <select value={c.kind} onChange={(e) => setC({ ...c, kind: e.target.value as ChartKind })}>
            {CHART_KINDS.map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          차트 제목 (비우면 자동)
          <input value={c.title} onChange={(e) => setC({ ...c, title: e.target.value })} />
        </label>
        <label>
          데이터 범위
          <input value={c.range} onChange={(e) => setC({ ...c, range: e.target.value })} />
        </label>
        <div className="cf-row">
          <label className="check">
            <input type="radio" checked={!c.series_in_rows} onChange={() => setC({ ...c, series_in_rows: false })} /> 열 방향 계열
          </label>
          <label className="check">
            <input type="radio" checked={c.series_in_rows} onChange={() => setC({ ...c, series_in_rows: true })} /> 행 방향 계열
          </label>
          <label className="check">
            <input type="checkbox" checked={c.legend} onChange={(e) => setC({ ...c, legend: e.target.checked })} /> 범례
          </label>
        </div>
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

// ---------------------------------------------------------------- 메모

/** A memo box next to a cell: read-only on hover, editable with Shift+F2. */
export function MemoBox(props: {
  x: number;
  y: number;
  text: string;
  author: string;
  editing: boolean;
  onSave?(text: string): void;
  onCancel?(): void;
}) {
  const [text, setText] = useState(props.text);
  useEffect(() => setText(props.text), [props.text]);
  return (
    <div className="memo-box" style={{ left: props.x, top: props.y }} onMouseDown={(e) => e.stopPropagation()}>
      {props.author && <div className="memo-author">{props.author}:</div>}
      {props.editing ? (
        <textarea
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              props.onCancel?.();
            } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              props.onSave?.(text);
            }
          }}
          onBlur={() => props.onSave?.(text)}
        />
      ) : (
        <div className="memo-text">{props.text}</div>
      )}
    </div>
  );
}
