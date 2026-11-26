// 슬라이서: a panel of buttons that filters a table by one column.
// Click shows one value, Ctrl+click adds or removes values, ✕ clears the filter.
import { tr } from "../i18n";
import { useEffect, useRef, useState } from "react";
import type { Slicer, SlicerView } from "../api";
import type { OverlayGeometry } from "../grid/Grid";
import { normalizeAnchor } from "./Chart";
import { Modal } from "./Dialogs";

export function SlicerLayer(props: {
  slicers: SlicerView[];
  geo: OverlayGeometry;
  selected: number | null;
  onSelect(id: number | null): void;
  onChange(s: Slicer): void;
  onPick(id: number, values: string[] | null): void;
  onContextMenu(s: Slicer, x: number, y: number): void;
}) {
  const [live, setLive] = useState<Slicer | null>(null);
  const drag = useRef<{ mode: string; x: number; y: number; start: Slicer } | null>(null);
  const { geo } = props;
  const z = geo.zoom;

  useEffect(() => {
    const move = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      const dx = (e.clientX - d.x) / z;
      const dy = (e.clientY - d.y) / z;
      const a = { ...d.start.anchor };
      if (d.mode === "move") {
        a.dx += dx;
        a.dy += dy;
      } else {
        a.width = Math.max(60, a.width + dx);
        a.height = Math.max(60, a.height + dy);
      }
      setLive({ ...d.start, anchor: a });
    };
    const up = () => {
      const d = drag.current;
      drag.current = null;
      setLive((cur) => {
        if (d && cur && JSON.stringify(cur.anchor) !== JSON.stringify(d.start.anchor)) {
          const n = normalizeAnchor(cur, geo);
          props.onChange({ ...n, anchor: { ...n.anchor, width: Math.round(n.anchor.width), height: Math.round(n.anchor.height) } });
        }
        return null;
      });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
  });

  const begin = (s: Slicer, mode: string, e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    props.onSelect(s.id);
    drag.current = { mode, x: e.clientX, y: e.clientY, start: s };
  };

  return (
    <>
      {props.slicers.map((v) => {
        const s = live && live.id === v.slicer.id ? live : v.slicer;
        const base = geo.cellRect(s.anchor.row, s.anchor.col);
        const x = base.x + s.anchor.dx * z;
        const y = base.y + s.anchor.dy * z;
        const w = s.anchor.width * z;
        const h = s.anchor.height * z;
        if (x > geo.width || y > geo.height || x + w < 0 || y + h < 0) return null;
        const sel = props.selected === s.id;
        const pick = (text: string, e: React.MouseEvent) => {
          e.stopPropagation();
          props.onSelect(s.id);
          const chosen = v.items.filter((i) => i.selected).map((i) => i.text);
          let next: string[];
          if (e.ctrlKey || e.metaKey) next = chosen.includes(text) && v.filtered ? chosen.filter((t) => t !== text) : v.filtered ? [...chosen, text] : [text];
          else next = [text];
          props.onPick(s.id, next.length === 0 || next.length === v.items.length ? null : next);
        };
        return (
          <div
            key={s.id}
            className={`slicer ${sel ? "selected" : ""}`}
            style={{ left: x, top: y, width: w, height: h, fontSize: 11 * z }}
            onMouseDown={(e) => e.stopPropagation()}
            onWheel={(e) => e.stopPropagation()}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              props.onSelect(s.id);
              props.onContextMenu(s, e.clientX, e.clientY);
            }}
          >
            <div className="slicer-head" onMouseDown={(e) => begin(s, "move", e)}>
              <span>{s.caption}</span>
              <button
                className="slicer-clear"
                title={tr("필터 지우기 (Alt+C)")}
                disabled={!v.filtered}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={() => props.onPick(s.id, null)}
              >
                ⊘
              </button>
            </div>
            <div className="slicer-items" style={{ gridTemplateColumns: `repeat(${s.columns}, 1fr)` }}>
              {v.items.map((i) => (
                <button
                  key={i.text}
                  className={`slicer-item ${i.selected ? "on" : ""} ${i.available ? "" : "empty"}`}
                  onMouseDown={(e) => e.stopPropagation()}
                  onClick={(e) => pick(i.text, e)}
                >
                  {i.text || "(비어 있음)"}
                </button>
              ))}
            </div>
            {sel && <span className="slicer-resize" onMouseDown={(e) => begin(s, "resize", e)} />}
          </div>
        );
      })}
    </>
  );
}

/** 슬라이서 삽입: which columns of the table get a slicer. */
export function SlicerInsertDialog(props: { table: string; columns: string[]; onDone(cols: string[] | null): void }) {
  const [picked, setPicked] = useState<string[]>([]);
  return (
    <Modal title={tr("슬라이서 삽입")} onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(picked);
        }}
      >
        <div className="hint">{props.table}</div>
        <div className="slicer-pick">
          {props.columns.map((c) => (
            <label key={c} className="check">
              <input
                type="checkbox"
                checked={picked.includes(c)}
                onChange={(e) => setPicked((p) => (e.target.checked ? [...p, c] : p.filter((x) => x !== c)))}
              />
              {c}
            </label>
          ))}
        </div>
        <div className="actions">
          <button type="submit" className="primary" disabled={!picked.length}>
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

/** 슬라이서 설정: caption and button columns. */
export function SlicerSettingsDialog(props: { slicer: Slicer; onDone(s: Slicer | null): void }) {
  const [s, setS] = useState(props.slicer);
  return (
    <Modal title={tr("슬라이서 설정")} onClose={() => props.onDone(null)}>
      <form
        className="modal-body shape-fmt"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(s);
        }}
      >
        <div className="hint">원본 이름: {s.column}</div>
        <label className="shape-fmt-row">
          <span>{tr("머리글")}</span>
          <input autoFocus value={s.caption} onChange={(e) => setS({ ...s, caption: e.target.value })} />
        </label>
        <label className="shape-fmt-row">
          <span>{tr("열 개수")}</span>
          <input type="number" min={1} max={20} value={s.columns} onChange={(e) => setS({ ...s, columns: Number(e.target.value) || 1 })} />
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
