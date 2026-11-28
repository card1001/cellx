// 수식 분석: 참조되는/참조하는 셀 추적 arrows and the 수식 계산 dialog.

import { tr } from "../i18n";
import { useEffect, useRef, useState } from "react";
import { api, type EvalStep, type FormulaDependencyGraph, type FormulaNode, type TraceArea } from "../api";
import type { OverlayGeometry } from "../grid/Grid";
import { colName } from "../grid/model";
import { Modal } from "./Dialogs";

/** A trace arrow from a range (or another sheet) to a formula cell. */
export interface TraceArrow {
  from: TraceArea;
  to: { sheet: number; row: number; col: number };
}

export const arrowKey = (a: TraceArrow) =>
  `${a.from.sheet}:${a.from.row1}:${a.from.col1}:${a.from.row2}:${a.from.col2}>${a.to.sheet}:${a.to.row}:${a.to.col}`;

const BLUE = "#2b57d9";

export function TraceLayer(props: { arrows: TraceArrow[]; sheet: number; geo: OverlayGeometry }) {
  const { geo } = props;
  const arrows = props.arrows.filter((a) => a.to.sheet === props.sheet || a.from.sheet === props.sheet);
  if (!arrows.length) return null;
  const center = (r: number, c: number) => {
    const rc = geo.cellRect(r, c);
    return { x: rc.x + Math.min(rc.w / 2, 12), y: rc.y + rc.h / 2 };
  };
  const boxes = new Map<string, TraceArea>();
  for (const a of arrows) {
    const f = a.from;
    if (f.sheet === props.sheet && (f.row1 !== f.row2 || f.col1 !== f.col2)) boxes.set(`${f.row1}:${f.col1}:${f.row2}:${f.col2}`, f);
  }
  return (
    <svg className="trace-layer" width={geo.width} height={geo.height}>
      <defs>
        <marker id="trace-head" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" fill={BLUE} />
        </marker>
      </defs>
      {[...boxes.values()].map((b) => {
        const a = geo.cellRect(b.row1, b.col1);
        const z = geo.cellRect(b.row2, b.col2);
        return (
          <rect
            key={`${b.row1}:${b.col1}:${b.row2}:${b.col2}`}
            x={a.x}
            y={a.y}
            width={z.x + z.w - a.x}
            height={z.y + z.h - a.y}
            fill="none"
            stroke={BLUE}
            strokeWidth={1.5}
          />
        );
      })}
      {arrows.map((a) => {
        const end = center(a.to.row, a.to.col);
        if (a.from.sheet !== a.to.sheet) {
          // Another sheet: a dashed arrow from a sheet icon, like Excel's.
          if (a.to.sheet !== props.sheet) return null;
          const start = { x: end.x - 46, y: end.y - 28 };
          return (
            <g key={arrowKey(a)}>
              <line x1={start.x + 8} y1={start.y + 6} x2={end.x} y2={end.y} stroke="#000" strokeDasharray="4 3" markerEnd="url(#trace-head)" />
              <rect x={start.x} y={start.y} width={16} height={12} fill="#fff" stroke="#000" />
              <path d={`M${start.x} ${start.y + 4}h16M${start.x} ${start.y + 8}h16M${start.x + 5} ${start.y}v12M${start.x + 11} ${start.y}v12`} stroke="#000" strokeWidth={0.6} />
            </g>
          );
        }
        const start = center(a.from.row1, a.from.col1);
        return (
          <g key={arrowKey(a)}>
            <circle cx={start.x} cy={start.y} r={3} fill={BLUE} />
            <line x1={start.x} y1={start.y} x2={end.x} y2={end.y} stroke={BLUE} strokeWidth={1.5} markerEnd="url(#trace-head)" />
          </g>
        );
      })}
    </svg>
  );
}

/** A step's text without its marks, and where the marked part is. */
function marked(text: string): { plain: string; mark: [number, number] | null } {
  const a = text.indexOf("\u0002");
  const b = text.indexOf("\u0003");
  if (a < 0 || b < a) return { plain: text, mark: null };
  return { plain: text.slice(0, a) + text.slice(a + 1, b) + text.slice(b + 1), mark: [a, b - 1] };
}

/** The formula with the part done next underlined and the last result in italics, as in Excel. */
export function stepParts(step: EvalStep, previous?: EvalStep): { text: string; under: boolean; italic: boolean }[] {
  const { plain, mark } = marked(step.text);
  const prev = previous ? marked(previous.text).mark : null;
  const italic: [number, number] | null = prev && previous ? [prev[0], prev[0] + previous.value.length] : null;
  const cuts = new Set([0, plain.length]);
  for (const r of [mark, italic]) if (r) r.forEach((x) => cuts.add(Math.max(0, Math.min(plain.length, x))));
  const points = [...cuts].sort((x, y) => x - y);
  const inside = (r: [number, number] | null, x: number) => !!r && x >= r[0] && x < r[1];
  const parts: { text: string; under: boolean; italic: boolean }[] = [];
  for (let k = 0; k + 1 < points.length; k++) {
    const [x, y] = [points[k], points[k + 1]];
    if (y > x) parts.push({ text: plain.slice(x, y), under: inside(mark, x), italic: inside(italic, x) });
  }
  return parts;
}

/** 수식 계산: steps through a formula the way Excel's 계산 button does. */
export function EvaluateDialog(props: { sheet: number; sheetName: string; row: number; col: number; onClose(): void }) {
  const [steps, setSteps] = useState<EvalStep[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [i, setI] = useState(0);
  const button = useRef<HTMLButtonElement>(null);
  const load = () => {
    setI(0);
    api
      .formulaSteps(props.sheet, props.row, props.col)
      .then((s) => setSteps(s))
      .catch((e) => setError(String(e)));
  };
  useEffect(load, [props.sheet, props.row, props.col]);
  const done = !!steps && i >= steps.length - 1;
  useEffect(() => button.current?.focus(), [steps, done]);
  const current = steps?.[i];
  const parts = current ? stepParts(current, i > 0 ? steps?.[i - 1] : undefined) : [];
  const part = parts.some((p) => p.under);
  return (
    <Modal title={tr("수식 계산")} onClose={props.onClose}>
      <div className="modal-body eval-dialog">
        <div className="eval-head">
          <span>{tr("참조:")}</span>
          <span>{tr("계산(V):")}</span>
        </div>
        <div className="eval-row">
          <span className="eval-ref">
            {props.sheetName}!${colName(props.col)}${props.row}
          </span>
          <div className="eval-box" data-testid="eval-text">
            {error ??
              parts.map((p, k) => (
                <span key={k} className={(p.under ? "eval-next " : "") + (p.italic ? "eval-last" : "")}>
                  {p.text}
                </span>
              ))}
          </div>
        </div>
        <div className="hint eval-hint">
          {error
            ? ""
            : done
              ? tr("계산이 끝났습니다.")
              : part
                ? tr("밑줄 친 식을 계산하려면 [계산]을 클릭하십시오. 가장 최근 결과는 기울임꼴로 표시됩니다.")
                : ""}
        </div>
        <div className="actions">
          {done ? (
            <button ref={button} className="primary" onClick={load}>
              {tr("다시 시작")}
            </button>
          ) : (
            <button ref={button} className="primary" disabled={!steps || !!error} onClick={() => setI((x) => x + 1)}>
              {tr("계산")}
            </button>
          )}
          <button onClick={props.onClose}>{tr("닫기")}</button>
        </div>
      </div>
    </Modal>
  );
}


/** Bounded, read-only visualization of the calculation engine's formula graph. */
export function FormulaGraphDialog(props: { onClose(): void; onSelectCell(sheet: number, row: number, col: number): void }) {
  const [graph, setGraph] = useState<FormulaDependencyGraph | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let live = true;
    api.formulaDependencyGraph()
      .then((value) => { if (live) setGraph(value); })
      .catch((cause) => { if (live) setError(String(cause)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, []);

  const nodes = new Map((graph?.nodes ?? []).map((node) => [node.id, node]));
  const visibleEdges = (graph?.edges ?? []).slice(0, 150);
  const visibleNodes = (graph?.nodes ?? []).slice(0, 150);
  const cellName = (node: FormulaNode) => `${colName(node.col)}${node.row}`;
  const nodeLabel = (node: FormulaNode) => `시트 ${node.sheet + 1}! ${cellName(node)}`;

  return (
    <Modal title={tr("수식 의존성 그래프")} onClose={props.onClose}>
      <div className="modal-body" style={{ minWidth: 560, maxWidth: "min(900px, 90vw)" }}>
        {loading && <p>{tr("수식 의존성 분석 중…")}</p>}
        {error && <p role="alert" style={{ color: "var(--danger, #b42318)" }}>{error}</p>}
        {graph && <>
          <p className="muted">
            수식 {graph.nodes.length.toLocaleString()}개 · 의존 관계 {graph.edges.length.toLocaleString()}개
            {graph.truncated ? " · 표시 한도에 도달하여 일부 생략됨" : ""}
          </p>
          <h4>참조 흐름 (선행 수식 → 종속 수식)</h4>
          <div style={{ maxHeight: 320, overflow: "auto", border: "1px solid var(--border)", borderRadius: 6 }}>
            {visibleEdges.length === 0
              ? <p className="muted" style={{ padding: 12 }}>수식 셀 간 의존 관계가 없습니다. 상수 셀을 참조하는 관계는 그래프에 포함하지 않습니다.</p>
              : visibleEdges.map((edge, index) => {
                const from = nodes.get(edge.from);
                const to = nodes.get(edge.to);
                if (!from || !to) return null;
                return <div key={`${edge.from}>${edge.to}:${index}`} style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 28px minmax(0, 1fr)", gap: 8, alignItems: "center", padding: "7px 10px", borderBottom: "1px solid var(--border)" }}>
                  <div style={{ minWidth: 0 }}>
                    <button type="button" className="link-button" onClick={() => props.onSelectCell(from.sheet, from.row, from.col)}>{nodeLabel(from)}</button>
                    <code style={{ display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{from.formula}</code>
                  </div>
                  <span aria-label="depends on" style={{ textAlign: "center", color: "var(--accent, #4f46e5)", fontWeight: 700 }}>→</span>
                  <div style={{ minWidth: 0 }}>
                    <button type="button" className="link-button" onClick={() => props.onSelectCell(to.sheet, to.row, to.col)}>{nodeLabel(to)}</button>
                    <code style={{ display: "block", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{to.formula}</code>
                  </div>
                </div>;
              })}
            {graph.edges.length > visibleEdges.length && <p className="muted" style={{ padding: 8 }}>상위 150개 의존 관계만 표시합니다.</p>}
          </div>
          <h4>수식 셀 목록</h4>
          <div style={{ maxHeight: 180, overflow: "auto", border: "1px solid var(--border)", borderRadius: 6 }}>
            {visibleNodes.map((node) => <div key={node.id} style={{ display: "flex", gap: 12, padding: "5px 10px", borderBottom: "1px solid var(--border)" }}>
              <button type="button" className="link-button" style={{ minWidth: 110 }} onClick={() => props.onSelectCell(node.sheet, node.row, node.col)}>{nodeLabel(node)}</button><code style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{node.formula}</code>
            </div>)}
            {graph.nodes.length > visibleNodes.length && <p className="muted" style={{ padding: 8 }}>상위 150개 수식 셀만 표시합니다.</p>}
          </div>
        </>}
        <div className="actions"><button onClick={props.onClose}>{tr("닫기")}</button></div>
      </div>
    </Modal>
  );
}
