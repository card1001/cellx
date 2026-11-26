// 스파크라인: small charts inside cells, and the dialogs that make and style them.
import { tr } from "../i18n";
import { type ReactElement, useState } from "react";
import type { SparkKind, SparklineGroup, SparklineView } from "../api";
import type { OverlayGeometry } from "../grid/Grid";
import { Modal } from "./Dialogs";

export const SPARK_KINDS: { kind: SparkKind; label: string; icon: string }[] = [
  { kind: "line", label: "꺾은선형", icon: "📈" },
  { kind: "column", label: "열", icon: "📊" },
  { kind: "winLoss", label: "승패", icon: "±" },
];

/** One sparkline drawn in a `w`×`h` box. */
export function SparklineSvg(props: { s: SparklineView; w: number; h: number }) {
  const { s } = props;
  const pad = 3;
  const w = Math.max(1, props.w - pad * 2);
  const h = Math.max(1, props.h - pad * 2);
  const vals = s.values;
  const nums = vals.filter((v): v is number => v !== null);
  if (!nums.length) return null;
  const n = vals.length;
  const max = Math.max(...nums);
  const min = Math.min(...nums);
  const hi = nums.indexOf(max);
  const lo = nums.indexOf(min);
  const parts: ReactElement[] = [];
  if (s.kind === "line") {
    const span = max - min || 1;
    const x = (i: number) => pad + (n === 1 ? w / 2 : (i * w) / (n - 1));
    const y = (v: number) => pad + h - ((v - min) / span) * h;
    let d = "";
    vals.forEach((v, i) => {
      if (v === null) return;
      d += `${d && vals[i - 1] !== null ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`;
    });
    parts.push(<path key="l" d={d} fill="none" stroke={s.color} strokeWidth={1.2} strokeLinejoin="round" />);
    vals.forEach((v, i) => {
      if (v === null) return;
      const k = nums.indexOf(v);
      const mark = s.markers || (s.high_low && (k === hi || k === lo));
      if (mark) parts.push(<circle key={`m${i}`} cx={x(i)} cy={y(v)} r={1.8} fill={s.high_low && (k === hi || k === lo) ? s.negative_color : s.color} />);
    });
  } else {
    const bw = w / n;
    const top = Math.max(0, max);
    const bottom = Math.min(0, min);
    const span = s.kind === "winLoss" ? 2 : top - bottom || 1;
    const zero = s.kind === "winLoss" ? pad + h / 2 : pad + (top / span) * h;
    vals.forEach((v, i) => {
      if (v === null || (s.kind === "winLoss" && v === 0)) return;
      const len = s.kind === "winLoss" ? h / 2 - 1 : (Math.abs(v) / span) * h;
      const neg = v < 0;
      const highlight = s.high_low && (v === max || v === min);
      parts.push(
        <rect
          key={i}
          x={pad + i * bw + bw * 0.12}
          y={neg ? zero : zero - len}
          width={Math.max(1, bw * 0.76)}
          height={Math.max(0.5, len)}
          fill={neg || highlight ? s.negative_color : s.color}
        />,
      );
    });
  }
  return (
    <svg width={props.w} height={props.h} className="sparkline-svg">
      {parts}
    </svg>
  );
}

export function SparklineLayer(props: { sparklines: SparklineView[]; geo: OverlayGeometry }) {
  const { geo } = props;
  return (
    <>
      {props.sparklines.map((s) => {
        const rc = geo.cellRect(s.row, s.col);
        if (rc.x > geo.width || rc.y > geo.height || rc.x + rc.w < 0 || rc.y + rc.h < 0 || rc.w <= 0 || rc.h <= 0) return null;
        return (
          <div key={`${s.row}:${s.col}`} className="sparkline-cell" style={{ left: rc.x, top: rc.y, width: rc.w, height: rc.h }}>
            <SparklineSvg s={s} w={rc.w} h={rc.h} />
          </div>
        );
      })}
    </>
  );
}

/** 스파크라인 만들기: data range and where to put them. */
export function SparklineCreateDialog(props: { kind: SparkKind; data: string; location: string; onDone(r: { data: string; location: string } | null): void }) {
  const [data, setData] = useState(props.data);
  const [location, setLocation] = useState(props.location);
  return (
    <Modal title={tr("스파크라인 만들기")} onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone({ data, location });
        }}
      >
        <div className="hint">{tr("원하는 데이터 선택")}</div>
        <label className="shape-fmt-row">
          <span>{tr("데이터 범위(D):")}</span>
          <input autoFocus value={data} onChange={(e) => setData(e.target.value)} />
        </label>
        <div className="hint">{tr("스파크라인을 배치할 위치 선택")}</div>
        <label className="shape-fmt-row">
          <span>{tr("위치 범위(L):")}</span>
          <input value={location} onChange={(e) => setLocation(e.target.value)} />
        </label>
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

/** 스파크라인 디자인: kind, colors, markers. */
export function SparklineStyleDialog(props: { group: SparklineGroup; onDone(g: SparklineGroup | null): void }) {
  const [g, setG] = useState(props.group);
  return (
    <Modal title={tr("스파크라인 디자인")} onClose={() => props.onDone(null)}>
      <form
        className="modal-body shape-fmt"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(g);
        }}
      >
        <label className="shape-fmt-row">
          <span>{tr("종류")}</span>
          <select value={g.kind} onChange={(e) => setG({ ...g, kind: e.target.value as SparkKind })}>
            {SPARK_KINDS.map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.label}
              </option>
            ))}
          </select>
        </label>
        <label className="shape-fmt-row">
          <span>{tr("스파크라인 색")}</span>
          <input type="color" value={g.color} onChange={(e) => setG({ ...g, color: e.target.value.toUpperCase() })} />
        </label>
        <label className="shape-fmt-row">
          <span>{tr("음수·강조 색")}</span>
          <input type="color" value={g.negative_color} onChange={(e) => setG({ ...g, negative_color: e.target.value.toUpperCase() })} />
        </label>
        <label className="shape-fmt-row check">
          <input type="checkbox" checked={g.markers} onChange={(e) => setG({ ...g, markers: e.target.checked })} />
          {tr("표식")}
        </label>
        <label className="shape-fmt-row check">
          <input type="checkbox" checked={g.high_low} onChange={(e) => setG({ ...g, high_low: e.target.checked })} />
          {tr("높은 점·낮은 점")}
        </label>
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
