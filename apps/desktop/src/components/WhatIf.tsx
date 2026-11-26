// 가상 분석: 데이터 표 and 시나리오 관리자 dialogs.
import { tr } from "../i18n";
import { useEffect, useState } from "react";
import { api, type Scenario } from "../api";
import { colName, parseAddr } from "../grid/model";
import { Modal } from "./Dialogs";

export function DataTableDialog(props: { onDone(r: { row: string; col: string } | null): void }) {
  const [row, setRow] = useState("");
  const [col, setCol] = useState("");
  return (
    <Modal title={tr("데이터 표")} onClose={() => props.onDone(null)}>
      <form
        className="modal-body shape-fmt"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone({ row, col });
        }}
      >
        <label className="shape-fmt-row">
          <span>{tr("행 입력 셀(R):")}</span>
          <input autoFocus value={row} placeholder={tr("예: $B$1")} onChange={(e) => setRow(e.target.value)} />
        </label>
        <label className="shape-fmt-row">
          <span>{tr("열 입력 셀(C):")}</span>
          <input value={col} placeholder={tr("예: $B$2")} onChange={(e) => setCol(e.target.value)} />
        </label>
        <div className="hint">{tr("선택한 범위의 첫 행에는 행 입력 셀에 넣을 값, 첫 열에는 열 입력 셀에 넣을 값을 둡니다.")}</div>
        <div className="actions">
          <button type="submit" className="primary">
            {tr("확인")}
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            {tr("취소")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** "A1:A3,C2" → cells. */
export function parseCells(text: string): [number, number][] | null {
  const out: [number, number][] = [];
  for (const part of text.split(/[,;]/).map((p) => p.trim().replace(/\$/g, "")).filter(Boolean)) {
    const [a, b] = part.split(":");
    const p0 = parseAddr(a);
    const p1 = b ? parseAddr(b) : p0;
    if (!p0 || !p1) return null;
    for (let r = Math.min(p0.r, p1.r); r <= Math.max(p0.r, p1.r); r++)
      for (let c = Math.min(p0.c, p1.c); c <= Math.max(p0.c, p1.c); c++) if (!out.some(([x, y]) => x === r && y === c)) out.push([r, c]);
  }
  return out.length ? out : null;
}

const cellsText = (cells: [number, number, string][]) => cells.map(([r, c]) => `$${colName(c)}$${r}`).join(",");

type Mode = { kind: "list" } | { kind: "edit"; old: string | null; name: string; cells: string; comment: string } | { kind: "values"; draft: Scenario; old: string | null } | { kind: "summary"; results: string };

export function ScenarioDialog(props: {
  sheet: number;
  selection: string;
  onShown(): void;
  onSummary(sheet: number): void;
  onError(e: unknown): void;
  onClose(): void;
}) {
  const [list, setList] = useState<Scenario[]>([]);
  const [pick, setPick] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const reload = () =>
    api
      .scenarios(props.sheet)
      .then((l) => {
        setList(l);
        setPick((p) => (p && l.some((s) => s.name === p) ? p : (l[0]?.name ?? null)));
      })
      .catch(props.onError);
  useEffect(() => void reload(), [props.sheet]); // eslint-disable-line react-hooks/exhaustive-deps
  const current = list.find((s) => s.name === pick) ?? null;

  const toValues = async () => {
    if (mode.kind !== "edit") return;
    const cells = parseCells(mode.cells);
    if (!mode.name.trim()) return props.onError("시나리오 이름을 입력하세요");
    if (!cells) return props.onError("변경 셀이 올바르지 않습니다 (예: $B$1:$B$3)");
    const old = list.find((s) => s.name === mode.old);
    const values = await Promise.all(
      cells.map(async ([r, c]) => old?.cells.find(([x, y]) => x === r && y === c)?.[2] ?? (await api.cellDetail(props.sheet, r, c).then((d) => d.input).catch(() => ""))),
    );
    setMode({ kind: "values", old: mode.old, draft: { sheet: props.sheet, name: mode.name.trim(), comment: mode.comment, cells: cells.map(([r, c], i) => [r, c, values[i]]) } });
  };

  const save = async (draft: Scenario, old: string | null) => {
    try {
      if (old && old !== draft.name) await api.deleteScenario(props.sheet, old);
      await api.setScenario(draft);
      setPick(draft.name);
      setMode({ kind: "list" });
      await reload();
    } catch (e) {
      props.onError(e);
    }
  };

  let body;
  if (mode.kind === "edit") {
    body = (
      <form
        className="modal-body shape-fmt"
        onSubmit={(e) => {
          e.preventDefault();
          void toValues();
        }}
      >
        <label className="shape-fmt-row">
          <span>{tr("시나리오 이름(N):")}</span>
          <input autoFocus value={mode.name} onChange={(e) => setMode({ ...mode, name: e.target.value })} />
        </label>
        <label className="shape-fmt-row">
          <span>{tr("변경 셀(C):")}</span>
          <input value={mode.cells} onChange={(e) => setMode({ ...mode, cells: e.target.value })} />
        </label>
        <div className="hint">{tr("인접하지 않은 셀은 쉼표로 구분합니다.")}</div>
        <label className="shape-fmt-row">
          <span>{tr("설명(O):")}</span>
          <input value={mode.comment} onChange={(e) => setMode({ ...mode, comment: e.target.value })} />
        </label>
        <div className="actions">
          <button type="submit" className="primary">
            {tr("확인")}
          </button>
          <button type="button" onClick={() => setMode({ kind: "list" })}>
            {tr("취소")}
          </button>
        </div>
      </form>
    );
  } else if (mode.kind === "values") {
    const d = mode.draft;
    body = (
      <form
        className="modal-body shape-fmt"
        onSubmit={(e) => {
          e.preventDefault();
          void save(d, mode.old);
        }}
      >
        <div className="hint">{tr("각 변경 셀의 값을 입력하십시오.")}</div>
        <div className="scenario-values">
          {d.cells.map(([r, c, v], i) => (
            <label key={`${r}:${c}`} className="shape-fmt-row">
              <span>
                ${colName(c)}${r}
              </span>
              <input
                autoFocus={i === 0}
                value={v}
                onChange={(e) => {
                  const cells = d.cells.map((x, k) => (k === i ? ([x[0], x[1], e.target.value] as [number, number, string]) : x));
                  setMode({ kind: "values", old: mode.old, draft: { ...d, cells } });
                }}
              />
            </label>
          ))}
        </div>
        <div className="actions">
          <button type="submit" className="primary">
            {tr("확인")}
          </button>
          <button type="button" onClick={() => setMode({ kind: "list" })}>
            {tr("취소")}
          </button>
        </div>
      </form>
    );
  } else if (mode.kind === "summary") {
    body = (
      <form
        className="modal-body shape-fmt"
        onSubmit={async (e) => {
          e.preventDefault();
          const cells = mode.results.trim() ? parseCells(mode.results) : [];
          if (!cells) return props.onError("결과 셀이 올바르지 않습니다");
          try {
            const s = await api.scenarioSummary(props.sheet, cells);
            props.onSummary(s);
          } catch (err) {
            props.onError(err);
          }
        }}
      >
        <div className="hint">{tr("보고서 종류: 시나리오 요약")}</div>
        <label className="shape-fmt-row">
          <span>{tr("결과 셀(R):")}</span>
          <input autoFocus value={mode.results} onChange={(e) => setMode({ ...mode, results: e.target.value })} />
        </label>
        <div className="actions">
          <button type="submit" className="primary">
            {tr("확인")}
          </button>
          <button type="button" onClick={() => setMode({ kind: "list" })}>
            {tr("취소")}
          </button>
        </div>
      </form>
    );
  } else {
    body = (
      <div className="modal-body scenario-manager">
        <div className="scenario-main">
          <div className="list-box scenario-list" role="listbox">
            {list.length === 0 && <div className="hint">{tr("정의된 시나리오가 없습니다. 시나리오를 추가하려면 [추가]를 선택하십시오.")}</div>}
            {list.map((s) => (
              <div
                key={s.name}
                role="option"
                aria-selected={s.name === pick}
                className={`list-item ${s.name === pick ? "on" : ""}`}
                onClick={() => setPick(s.name)}
                onDoubleClick={async () => {
                  try {
                    await api.showScenario(props.sheet, s.name);
                    props.onShown();
                  } catch (e) {
                    props.onError(e);
                  }
                }}
              >
                {s.name}
              </div>
            ))}
          </div>
          <div className="scenario-buttons">
            <button onClick={() => setMode({ kind: "edit", old: null, name: "", cells: props.selection, comment: "" })}>{tr("추가(A)...")}</button>
            <button
              disabled={!current}
              onClick={async () => {
                if (!current) return;
                await api.deleteScenario(props.sheet, current.name).catch(props.onError);
                await reload();
              }}
            >
              {tr("삭제(D)")}
            </button>
            <button
              disabled={!current}
              onClick={() => current && setMode({ kind: "edit", old: current.name, name: current.name, cells: cellsText(current.cells), comment: current.comment })}
            >
              {tr("편집(E)...")}
            </button>
            <button disabled={!list.length} onClick={() => setMode({ kind: "summary", results: "" })}>
              {tr("요약(U)...")}
            </button>
          </div>
        </div>
        <div className="hint">변경 셀: {current ? cellsText(current.cells) : ""}</div>
        <div className="hint">설명: {current?.comment ?? ""}</div>
        <div className="actions">
          <button
            className="primary"
            disabled={!current}
            onClick={async () => {
              if (!current) return;
              try {
                await api.showScenario(props.sheet, current.name);
                props.onShown();
              } catch (e) {
                props.onError(e);
              }
            }}
          >
            {tr("표시(S)")}
          </button>
          <button onClick={props.onClose}>{tr("닫기")}</button>
        </div>
      </div>
    );
  }
  const title = mode.kind === "edit" ? (mode.old ? "시나리오 편집" : "시나리오 추가") : mode.kind === "values" ? "시나리오 값" : mode.kind === "summary" ? "시나리오 요약" : "시나리오 관리자";
  return (
    <Modal title={title} onClose={props.onClose}>
      {body}
    </Modal>
  );
}

/** 통합 문서 보호: an optional password, typed twice. */
export function BookProtectDialog(props: { onDone(pw: string | null | undefined): void }) {
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [err, setErr] = useState<string | null>(null);
  return (
    <Modal title={tr("구조 및 창 보호")} onClose={() => props.onDone(undefined)}>
      <form
        className="modal-body shape-fmt"
        onSubmit={(e) => {
          e.preventDefault();
          if (pw && pw !== pw2) return setErr("확인 암호가 일치하지 않습니다.");
          props.onDone(pw || null);
        }}
      >
        <div className="hint">{tr("보호할 대상: 구조 — 시트 추가, 삭제, 이름 바꾸기, 이동, 숨기기를 막습니다.")}</div>
        <label className="shape-fmt-row">
          <span>{tr("암호(선택):")}</span>
          <input type="password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} />
        </label>
        <label className="shape-fmt-row">
          <span>{tr("암호 확인:")}</span>
          <input type="password" value={pw2} disabled={!pw} onChange={(e) => setPw2(e.target.value)} />
        </label>
        {err && <div className="err-text">{err}</div>}
        <div className="actions">
          <button type="submit" className="primary">
            {tr("확인")}
          </button>
          <button type="button" onClick={() => props.onDone(undefined)}>
            {tr("취소")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** 사용자 지정 보기: show, add or delete named views. */
export function CustomViewsDialog(props: { active: number; onShown(sheet: number): void; onError(e: unknown): void; onClose(): void }) {
  const [list, setList] = useState<{ name: string }[]>([]);
  const [pick, setPick] = useState<string | null>(null);
  const [adding, setAdding] = useState<string | null>(null);
  const reload = () =>
    api
      .customViews()
      .then((l) => {
        setList(l);
        setPick((p) => (p && l.some((v) => v.name === p) ? p : (l[0]?.name ?? null)));
      })
      .catch(props.onError);
  useEffect(() => void reload(), []); // eslint-disable-line react-hooks/exhaustive-deps
  const show = async (name: string) => {
    try {
      const s = await api.showCustomView(name);
      props.onShown(s);
    } catch (e) {
      props.onError(e);
    }
  };
  if (adding !== null)
    return (
      <Modal title={tr("보기 추가")} onClose={() => setAdding(null)}>
        <form
          className="modal-body shape-fmt"
          onSubmit={async (e) => {
            e.preventDefault();
            try {
              await api.addCustomView(adding, props.active);
              setAdding(null);
              await reload();
            } catch (err) {
              props.onError(err);
            }
          }}
        >
          <label className="shape-fmt-row">
            <span>{tr("이름(N):")}</span>
            <input autoFocus value={adding} onChange={(e) => setAdding(e.target.value)} />
          </label>
          <div className="hint">{tr("보기에 포함: 숨겨진 행·열, 필터 설정")}</div>
          <div className="actions">
            <button type="submit" className="primary" disabled={!adding.trim()}>
              {tr("확인")}
            </button>
            <button type="button" onClick={() => setAdding(null)}>
              {tr("취소")}
            </button>
          </div>
        </form>
      </Modal>
    );
  return (
    <Modal title={tr("사용자 지정 보기")} onClose={props.onClose}>
      <div className="modal-body scenario-manager">
        <div className="scenario-main">
          <div className="list-box scenario-list" role="listbox">
            {list.map((v) => (
              <div
                key={v.name}
                role="option"
                aria-selected={v.name === pick}
                className={`list-item ${v.name === pick ? "on" : ""}`}
                onClick={() => setPick(v.name)}
                onDoubleClick={() => void show(v.name)}
              >
                {v.name}
              </div>
            ))}
          </div>
          <div className="scenario-buttons">
            <button className="primary" disabled={!pick} onClick={() => pick && void show(pick)}>
              {tr("표시(S)")}
            </button>
            <button onClick={props.onClose}>{tr("닫기")}</button>
            <button onClick={() => setAdding("")}>{tr("추가(A)...")}</button>
            <button
              disabled={!pick}
              onClick={async () => {
                if (!pick) return;
                await api.deleteCustomView(pick).catch(props.onError);
                await reload();
              }}
            >
              {tr("삭제(D)")}
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
