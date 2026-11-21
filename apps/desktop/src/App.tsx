import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type BookInfo, errorText, isTauri, type RecalcStats, scalarText, type TableInfo } from "./api";
import { Grid, type Selection } from "./grid/Grid";
import { addr, formatCount, normalize, parseAddr, type Pos, type Range, rangeLabel, toTsv } from "./grid/model";
import { DataSource, SheetSource } from "./grid/sources";

type Tab = { kind: "sheet"; index: number } | { kind: "data"; name: string };

const tabKey = (t: Tab) => (t.kind === "sheet" ? `s:${t.index}` : `d:${t.name}`);
const START: Selection = { anchor: { r: 1, c: 1 }, focus: { r: 1, c: 1 } };

interface Prompt {
  title: string;
  label: string;
  value: string;
  resolve(v: string | null): void;
}

export default function App() {
  const [info, setInfo] = useState<BookInfo | null>(null);
  const [tab, setTab] = useState<Tab>({ kind: "sheet", index: 0 });
  const [selection, setSelection] = useState<Selection>(START);
  const [formula, setFormula] = useState("");
  const [bridgeError, setBridgeError] = useState<string | null>(null);
  const [message, setMessage] = useState<{ text: string; error?: boolean } | null>(null);
  const [stats, setStats] = useState<RecalcStats | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [showSql, setShowSql] = useState(false);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [, force] = useState(0);
  const sources = useRef(new Map<string, SheetSource | DataSource>());
  const formulaEditing = useRef(false);

  const say = (text: string, error = false) => setMessage({ text, error });
  const fail = (e: unknown) => say(errorText(e), true);

  const ask = (title: string, label: string, value = "") =>
    new Promise<string | null>((resolve) => setPrompt({ title, label, value, resolve }));

  // ---------------------------------------------------------------- sources
  const resetSources = () => sources.current.clear();

  const sourceFor = useCallback(
    async (t: Tab): Promise<SheetSource | DataSource | null> => {
      const k = tabKey(t);
      const existing = sources.current.get(k);
      if (existing) return existing;
      if (t.kind === "sheet") {
        const [rows, cols] = await api.dimensions(t.index);
        const s = new SheetSource(t.index, rows, cols);
        sources.current.set(k, s);
        return s;
      }
      const table = info?.tables.find((x) => x.name === t.name);
      if (!table) return null;
      const s = new DataSource(table);
      sources.current.set(k, s);
      return s;
    },
    [info],
  );

  const [source, setSource] = useState<SheetSource | DataSource | null>(null);
  useEffect(() => {
    let live = true;
    void sourceFor(tab).then((s) => live && setSource(s));
    return () => {
      live = false;
    };
  }, [tab, sourceFor]);

  const invalidateSheets = () => {
    for (const s of sources.current.values()) if (s instanceof SheetSource) s.invalidate();
  };

  // ---------------------------------------------------------------- book
  const refreshInfo = async () => setInfo(await api.bookInfo());

  useEffect(() => {
    if (!isTauri()) return;
    void refreshInfo().catch(fail);
  }, []);

  const loadFocusedCell = useCallback(async () => {
    const f = selection.focus;
    if (formulaEditing.current) return;
    if (tab.kind === "sheet") {
      try {
        const d = await api.cellDetail(tab.index, f.r, f.c);
        setFormula(d.input);
        setBridgeError(d.bridge_error);
      } catch (e) {
        fail(e);
      }
    } else if (source instanceof DataSource) {
      const v = source.rawValue(f.r, f.c);
      setFormula(v ? (source.cell(f.r, f.c)?.text ?? scalarText(v)) : "");
      setBridgeError(null);
    }
  }, [selection.focus, tab, source]);

  useEffect(() => {
    void loadFocusedCell();
  }, [loadFocusedCell]);

  const afterRecalc = (s: RecalcStats) => {
    setStats(s);
    invalidateSheets();
    void loadFocusedCell();
    void refreshInfo();
  };

  const run = async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(label);
    try {
      return await fn();
    } catch (e) {
      fail(e);
      return undefined;
    } finally {
      setBusy(null);
    }
  };

  const newBook = async () => {
    const i = await run("새 문서", api.newBook);
    if (!i) return;
    resetSources();
    setInfo(i);
    setTab({ kind: "sheet", index: 0 });
    setSelection(START);
    say("새 문서를 만들었습니다");
  };

  const openFile = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [
        { name: "스프레드시트", extensions: ["xlsx", "cellx"] },
        { name: "모든 파일", extensions: ["*"] },
      ],
    });
    if (!path || Array.isArray(path)) return;
    const t = performance.now();
    const i = await run("여는 중", () => api.openFile(path));
    if (!i) return;
    resetSources();
    setInfo(i);
    setTab({ kind: "sheet", index: 0 });
    setSelection(START);
    say(`${path} (${Math.round(performance.now() - t)} ms)`);
  };

  const saveAs = async () => {
    const path = await saveDialog({
      defaultPath: info?.path ?? "통합문서.cellx",
      filters: [
        { name: "CellX 프로젝트(데이터 시트 포함)", extensions: ["cellx"] },
        { name: "Excel 통합 문서", extensions: ["xlsx"] },
      ],
    });
    if (!path) return;
    const i = await run("저장 중", () => api.saveFile(path));
    if (i) {
      setInfo(i);
      say(
        path.toLowerCase().endsWith(".xlsx") && i.tables.length > 0
          ? "xlsx로 저장했습니다. 데이터 시트는 .cellx 프로젝트에만 저장됩니다"
          : `저장했습니다: ${path}`,
      );
    }
  };

  const save = async () => {
    if (!info?.path) return saveAs();
    const i = await run("저장 중", () => api.saveFile(info.path!));
    if (i) {
      setInfo(i);
      say(`저장했습니다: ${info.path}`);
    }
  };

  const importData = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: "데이터 파일", extensions: ["csv", "tsv", "txt", "parquet"] }],
    });
    if (!path || Array.isArray(path)) return;
    const base = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "data";
    const suggested = base.replace(/[^\p{L}\p{N}_]/gu, "_").replace(/^(\d)/, "_$1");
    const name = await ask("데이터 시트 가져오기", "데이터 시트 이름 (수식에서 이름[열]로 참조)", suggested);
    if (!name) return;
    const t = performance.now();
    const table = await run("가져오는 중", () => api.importData(path, name));
    if (!table) return;
    await refreshInfo();
    sources.current.delete(tabKey({ kind: "data", name: table.name }));
    setTab({ kind: "data", name: table.name });
    setSelection(START);
    invalidateSheets();
    say(`${formatCount(table.rows)}행을 가져왔습니다 (${((performance.now() - t) / 1000).toFixed(1)}초)`);
  };

  const addSheet = async () => {
    const i = await run("시트 추가", api.addSheet);
    if (i) {
      setInfo(i);
      setTab({ kind: "sheet", index: i.sheets.length - 1 });
      setSelection(START);
    }
  };

  const renameSheet = async (index: number) => {
    const name = await ask("시트 이름 바꾸기", "새 이름", info?.sheets[index] ?? "");
    if (!name) return;
    const i = await run("이름 바꾸기", () => api.renameSheet(index, name));
    if (i) setInfo(i);
  };

  // ---------------------------------------------------------------- editing
  const beginEdit = async (p: Pos) => {
    if (tab.kind === "sheet") return (await api.cellDetail(tab.index, p.r, p.c)).input;
    return source instanceof DataSource ? (source.cell(p.r, p.c)?.text ?? "").replace(/,/g, "") : "";
  };

  const commitEdit = async (p: Pos, text: string) => {
    if (tab.kind === "sheet") {
      const s = await run("계산 중", () => api.setCell(tab.index, p.r, p.c, text));
      if (s) afterRecalc(s);
    } else if (source instanceof DataSource) {
      const rid = source.rowId(p.r);
      const col = source.column(p.c);
      if (rid === undefined || !col) return;
      const s = await run("저장 중", () => api.updateDataCell(tab.name, rid, col.name, text));
      if (s) {
        source.reload();
        afterRecalc(s);
      }
    }
  };

  const onDelete = async (range: Range) => {
    if (tab.kind !== "sheet") return;
    const cells: [number, number, string][] = [];
    for (let r = range.r0; r <= Math.min(range.r1, range.r0 + 10000); r++)
      for (let c = range.c0; c <= Math.min(range.c1, range.c0 + 200); c++) cells.push([r, c, ""]);
    const s = await run("지우는 중", () => api.setCells(tab.index, cells));
    if (s) afterRecalc(s);
  };

  const copyText = (range: Range) => {
    if (!source) return "";
    const rows: string[][] = [];
    const r1 = Math.min(range.r1, range.r0 + 50000);
    for (let r = range.r0; r <= r1; r++) {
      const row: string[] = [];
      for (let c = range.c0; c <= range.c1; c++) row.push(source.cell(r, c)?.text ?? "");
      rows.push(row);
    }
    return toTsv(rows);
  };

  const onPaste = async (at: Pos, rows: string[][]) => {
    if (tab.kind !== "sheet") return say("데이터 시트에는 붙여넣을 수 없습니다", true);
    const cells: [number, number, string][] = [];
    rows.forEach((row, i) => row.forEach((v, j) => cells.push([at.r + i, at.c + j, v])));
    const s = await run("붙여넣는 중", () => api.setCells(tab.index, cells));
    if (s) {
      afterRecalc(s);
      say(`${formatCount(cells.length)}개 셀을 붙여넣었습니다`);
    }
  };

  const undo = async () => {
    const at = await run("실행 취소", api.undo);
    if (at) {
      setTab({ kind: "sheet", index: at[0] });
      setSelection({ anchor: { r: at[1], c: at[2] }, focus: { r: at[1], c: at[2] } });
    }
    invalidateSheets();
    void refreshInfo();
  };

  const redo = async () => {
    const at = await run("다시 실행", api.redo);
    if (at) {
      setTab({ kind: "sheet", index: at[0] });
      setSelection({ anchor: { r: at[1], c: at[2] }, focus: { r: at[1], c: at[2] } });
    }
    invalidateSheets();
    void refreshInfo();
  };

  const onHeaderClick =
    source instanceof DataSource
      ? (c: number) => {
          const col = source.column(c);
          if (!col) return;
          const s = source.sort;
          const next =
            s?.column !== col.name
              ? { column: col.name, descending: false }
              : s.descending
                ? null
                : { column: col.name, descending: true };
          source.setView(next, source.filters);
          force((x) => x + 1);
        }
      : undefined;

  const onColumnResize = (c: number, w: number) => {
    if (!source) return;
    source.setColWidth(c, w);
    if (source instanceof SheetSource) void api.setColumnWidth(source.sheet, c, w);
  };

  // Formula bar
  const commitFormula = async () => {
    formulaEditing.current = false;
    await commitEdit(selection.focus, formula);
  };

  // Global shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (!mod) return;
      const k = e.key.toLowerCase();
      if (k === "s") {
        e.preventDefault();
        void (e.shiftKey ? saveAs() : save());
      } else if (k === "o") {
        e.preventDefault();
        void openFile();
      } else if (k === "n") {
        e.preventDefault();
        void newBook();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const selRange = normalize(selection.anchor, selection.focus);
  const selCount = (selRange.r1 - selRange.r0 + 1) * (selRange.c1 - selRange.c0 + 1);
  const title = useMemo(() => {
    const name = info?.path?.split(/[\\/]/).pop() ?? "통합문서";
    return `${info?.dirty ? "● " : ""}${name}`;
  }, [info]);

  if (!isTauri()) {
    return (
      <div className="not-tauri">
        <h1>CellX</h1>
        <p>
          이 화면은 데스크톱 앱 안에서 실행됩니다. <code>pnpm tauri dev</code>로 실행하세요.
        </p>
      </div>
    );
  }

  return (
    <div className="app">
      <header className="toolbar">
        <div className="brand">
          <span className="logo">CX</span>
          <span className="doc-title">{title}</span>
        </div>
        <div className="group">
          <button onClick={newBook} title="새 문서 (Ctrl+N)">새 문서</button>
          <button onClick={openFile} title="열기 (Ctrl+O)">열기</button>
          <button onClick={save} title="저장 (Ctrl+S)">저장</button>
          <button onClick={saveAs} title="다른 이름으로 저장 (Ctrl+Shift+S)">다른 이름으로</button>
        </div>
        <div className="group">
          <button onClick={undo} title="실행 취소 (Ctrl+Z)">↶</button>
          <button onClick={redo} title="다시 실행 (Ctrl+Y)">↷</button>
        </div>
        <div className="group">
          <button className="primary" onClick={importData}>데이터 가져오기</button>
          <button className={showSql ? "active" : ""} onClick={() => setShowSql((v) => !v)}>SQL</button>
        </div>
        {busy && <span className="busy">{busy}…</span>}
      </header>

      <div className="formula-bar">
        <input
          className="name-box"
          value={rangeLabel(selection.anchor, selection.focus)}
          onChange={() => {}}
          onKeyDown={(e) => {
            if (e.key !== "Enter") return;
            const p = parseAddr((e.target as HTMLInputElement).value);
            if (p) setSelection({ anchor: p, focus: p });
          }}
          onFocus={(e) => e.target.select()}
        />
        <span className="fx">fx</span>
        <input
          className="formula-input"
          value={formula}
          spellCheck={false}
          onFocus={() => (formulaEditing.current = true)}
          onBlur={() => (formulaEditing.current = false)}
          onChange={(e) => {
            formulaEditing.current = true;
            setFormula(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void commitFormula();
            } else if (e.key === "Escape") {
              formulaEditing.current = false;
              void loadFocusedCell();
            }
          }}
        />
      </div>

      {source instanceof DataSource && (
        <DataBar
          source={source}
          onChange={() => force((x) => x + 1)}
          onExport={async () => {
            const path = await saveDialog({
              defaultPath: `${source.info.name}.csv`,
              filters: [
                { name: "CSV", extensions: ["csv"] },
                { name: "Parquet", extensions: ["parquet"] },
              ],
            });
            if (!path) return;
            const ok = await run("내보내는 중", () => api.exportTable(source.info.name, path));
            if (ok !== undefined) say(`내보냈습니다: ${path}`);
          }}
        />
      )}

      <main className="main">
        <div className="grid-host">
          {source && (
            <Grid
              key={tabKey(tab)}
              source={source}
              selection={selection}
              onSelection={setSelection}
              beginEdit={beginEdit}
              commitEdit={commitEdit}
              onEditText={(t) => {
                if (t !== null) setFormula(t);
              }}
              onDelete={onDelete}
              copyText={copyText}
              onPaste={onPaste}
              onUndo={undo}
              onRedo={redo}
              onHeaderClick={onHeaderClick}
              onColumnResize={onColumnResize}
            />
          )}
        </div>
        {showSql && (
          <SqlPanel
            tables={info?.tables ?? []}
            onCreate={async (sql, name) => {
              const table = await run("쿼리 실행 중", () => api.queryToTable(sql, name));
              if (!table) return;
              await refreshInfo();
              sources.current.delete(tabKey({ kind: "data", name: table.name }));
              setTab({ kind: "data", name: table.name });
              setSelection(START);
              say(`${table.name}: ${formatCount(table.rows)}행`);
            }}
          />
        )}
      </main>

      <footer className="bottom">
        <div className="tabs">
          {info?.sheets.map((name, i) => (
            <button
              key={`s${i}`}
              className={`tab ${tab.kind === "sheet" && tab.index === i ? "on" : ""}`}
              onClick={() => {
                setTab({ kind: "sheet", index: i });
                setSelection(START);
              }}
              onDoubleClick={() => renameSheet(i)}
            >
              {name}
            </button>
          ))}
          <button className="tab add" onClick={addSheet} title="시트 추가">＋</button>
          {info?.tables.map((t) => (
            <button
              key={`d${t.name}`}
              className={`tab data ${tab.kind === "data" && tab.name === t.name ? "on" : ""}`}
              onClick={() => {
                setTab({ kind: "data", name: t.name });
                setSelection(START);
              }}
              title={`${t.name}: ${formatCount(t.rows)}행 · 수식에서 ${t.name}[열이름]`}
            >
              <span className="db">DB</span>
              {t.name}
            </button>
          ))}
        </div>
        <div className="status">
          {message && (
            <span className={message.error ? "msg err" : "msg"} onClick={() => setMessage(null)}>
              {message.text}
            </span>
          )}
          {bridgeError && <span className="msg err">{bridgeError}</span>}
          {source instanceof DataSource && (
            <span>
              {formatCount(source.total)}행 · 조회 {source.lastQueryMs.toFixed(0)} ms
            </span>
          )}
          {stats && tab.kind === "sheet" && (
            <span title="계산 엔진 / 데이터 엔진 시간">
              계산 {stats.calc_ms.toFixed(1)} ms
              {stats.bridge_calls > 0 &&
                ` · 데이터 엔진 ${stats.bridge_ms.toFixed(1)} ms (캐시 ${stats.bridge_cache_hits}/${stats.bridge_calls})`}
            </span>
          )}
          <span>{selCount > 1 ? `${formatCount(selCount)}개 선택` : addr(selection.focus)}</span>
        </div>
      </footer>

      {prompt && (
        <div className="modal-back" onMouseDown={() => (prompt.resolve(null), setPrompt(null))}>
          <form
            className="modal"
            onMouseDown={(e) => e.stopPropagation()}
            onSubmit={(e) => {
              e.preventDefault();
              prompt.resolve(prompt.value.trim() || null);
              setPrompt(null);
            }}
          >
            <h3>{prompt.title}</h3>
            <label>
              {prompt.label}
              <input autoFocus value={prompt.value} onChange={(e) => setPrompt({ ...prompt, value: e.target.value })} />
            </label>
            <div className="actions">
              <button type="button" onClick={() => (prompt.resolve(null), setPrompt(null))}>
                취소
              </button>
              <button type="submit" className="primary">확인</button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}

function DataBar(props: { source: DataSource; onChange(): void; onExport(): void }) {
  const { source } = props;
  const [column, setColumn] = useState(source.info.columns[0]?.name ?? "");
  const [criterion, setCriterion] = useState("");
  useEffect(() => setColumn(source.info.columns[0]?.name ?? ""), [source]);
  const apply = (filters: typeof source.filters) => {
    source.setView(source.sort, filters);
    props.onChange();
  };
  return (
    <div className="data-bar">
      <span className="label">필터</span>
      <select value={column} onChange={(e) => setColumn(e.target.value)}>
        {source.info.columns.map((c) => (
          <option key={c.name}>{c.name}</option>
        ))}
      </select>
      <input
        placeholder='조건: 서울, >=100, *카드*, <>취소'
        value={criterion}
        onChange={(e) => setCriterion(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && criterion.trim()) {
            apply([...source.filters, { column, criterion: criterion.trim() }]);
            setCriterion("");
          }
        }}
      />
      {source.filters.map((f, i) => (
        <span key={i} className="chip">
          {f.column} {f.criterion}
          <button onClick={() => apply(source.filters.filter((_, j) => j !== i))}>×</button>
        </span>
      ))}
      <span className="spacer" />
      <span className="hint">
        수식: <code>=SUMIFS({source.info.name}[열], {source.info.name}[열], "조건")</code>
      </span>
      <button onClick={props.onExport}>내보내기</button>
    </div>
  );
}

function SqlPanel(props: { tables: TableInfo[]; onCreate(sql: string, name: string): Promise<void> }) {
  const first = props.tables[0];
  const [sql, setSql] = useState(
    first
      ? `SELECT ${first.columns[0]?.name ?? "*"}, COUNT(*) AS cnt\nFROM ${first.name}\nGROUP BY 1\nORDER BY 2 DESC`
      : "SELECT 1 AS x",
  );
  const [name, setName] = useState("result");
  return (
    <aside className="side">
      <h3>SQL</h3>
      <p className="muted">SELECT 문만 실행합니다. 결과는 새 데이터 시트가 됩니다.</p>
      <textarea value={sql} onChange={(e) => setSql(e.target.value)} spellCheck={false} />
      <label>
        결과 시트 이름
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <button className="primary" onClick={() => props.onCreate(sql, name)}>
        실행
      </button>
      <h4>데이터 시트</h4>
      {props.tables.length === 0 && <p className="muted">아직 없습니다. 「데이터 가져오기」로 CSV를 여세요.</p>}
      {props.tables.map((t) => (
        <details key={t.name}>
          <summary>
            {t.name} <span className="muted">{formatCount(t.rows)}행</span>
          </summary>
          <ul>
            {t.columns.map((c) => (
              <li key={c.name}>
                {c.name} <span className="muted">{c.sql_type}</span>
              </li>
            ))}
          </ul>
        </details>
      ))}
      <h4>시트에서 쓰는 법</h4>
      <ul className="muted help">
        <li>=SUMIFS(표[금액], 표[지역], "서울")</li>
        <li>=COUNTIFS(표[상태], "&lt;&gt;취소")</li>
        <li>=XLOOKUP(A2, 표[주문번호], 표[금액])</li>
        <li>=DQUERY("SELECT ... FROM 표")</li>
      </ul>
    </aside>
  );
}
