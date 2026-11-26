import { tr } from "../i18n";
// Shapes and text boxes over the sheet (삽입 → 도형, 텍스트 상자), drawn,
// moved, resized and typed into like Excel's.
import { useEffect, useRef, useState } from "react";
import type { Shape, ShapeKind } from "../api";
import type { OverlayGeometry } from "../grid/Grid";
import { normalizeAnchor } from "./Chart";
import { Modal } from "./Dialogs";

export const SHAPE_KINDS: { kind: ShapeKind; label: string; icon: string }[] = [
  { kind: "line", label: "선", icon: "╲" },
  { kind: "arrow", label: "화살표", icon: "↘" },
  { kind: "rect", label: "직사각형", icon: "▭" },
  { kind: "roundRect", label: "사각형: 둥근 모서리", icon: "▢" },
  { kind: "ellipse", label: "타원", icon: "◯" },
  { kind: "triangle", label: "이등변 삼각형", icon: "△" },
  { kind: "diamond", label: "다이아몬드", icon: "◇" },
  { kind: "rightArrow", label: "화살표: 오른쪽", icon: "➡" },
  { kind: "pentagon", label: "화살표: 오각형", icon: "⬠" },
];

const isLine = (k: ShapeKind) => k === "line" || k === "arrow";
const PAD = 6;

/** The outline of a shape in a `w`×`h` box, as SVG. */
export function ShapeFigure(props: { shape: Shape; w: number; h: number; zoom?: number }) {
  const { shape: s, w, h } = props;
  const z = props.zoom ?? 1;
  const stroke = s.line ?? "none";
  const sw = s.line ? Math.max(0.5, s.line_width * z) : 0;
  const fill = s.fill ?? "none";
  const common = { fill, stroke, strokeWidth: sw, vectorEffect: "non-scaling-stroke" as const };
  const head = Math.min(w, h) * 0.5;
  const poly = (pts: [number, number][]) => <polygon points={pts.map((p) => p.join(",")).join(" ")} {...common} />;
  let figure;
  switch (s.kind) {
    case "line":
    case "arrow": {
      let [x1, y1, x2, y2] = [0, 0, w, h];
      if (s.flip_v) [y1, y2] = [h, 0];
      if (s.flip_h) [x1, x2] = [w, 0];
      figure = (
        <line
          x1={x1 + PAD}
          y1={y1 + PAD}
          x2={x2 + PAD}
          y2={y2 + PAD}
          stroke={stroke}
          strokeWidth={sw}
          markerEnd={s.kind === "arrow" ? `url(#shape-head-${s.id})` : undefined}
        />
      );
      break;
    }
    case "roundRect":
      figure = <rect x={0} y={0} width={w} height={h} rx={Math.min(w, h) * 0.1667} {...common} />;
      break;
    case "ellipse":
      figure = <ellipse cx={w / 2} cy={h / 2} rx={w / 2} ry={h / 2} {...common} />;
      break;
    case "triangle":
      figure = poly([
        [w / 2, 0],
        [w, h],
        [0, h],
      ]);
      break;
    case "diamond":
      figure = poly([
        [w / 2, 0],
        [w, h / 2],
        [w / 2, h],
        [0, h / 2],
      ]);
      break;
    case "rightArrow":
      figure = poly([
        [0, h * 0.25],
        [w - head, h * 0.25],
        [w - head, 0],
        [w, h / 2],
        [w - head, h],
        [w - head, h * 0.75],
        [0, h * 0.75],
      ]);
      break;
    case "pentagon":
      figure = poly([
        [0, 0],
        [w - head, 0],
        [w, h / 2],
        [w - head, h],
        [0, h],
      ]);
      break;
    default:
      figure = <rect x={0} y={0} width={w} height={h} {...common} />;
  }
  const line = isLine(s.kind);
  return (
    <svg
      className="shape-svg"
      width={line ? w + PAD * 2 : w}
      height={line ? h + PAD * 2 : h}
      style={line ? { left: -PAD, top: -PAD } : undefined}
      overflow="visible"
    >
      {s.kind === "arrow" && (
        <defs>
          <marker id={`shape-head-${s.id}`} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="5" markerHeight="5" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" fill={stroke} />
          </marker>
        </defs>
      )}
      {figure}
    </svg>
  );
}

/** The text of a shape, placed as its alignment says. */
export function ShapeText(props: { shape: Shape; zoom?: number }) {
  const s = props.shape;
  const z = props.zoom ?? 1;
  if (isLine(s.kind) || !s.text) return null;
  return (
    <div
      className="shape-text"
      style={{
        justifyContent: s.valign === "ctr" ? "center" : s.valign === "b" ? "flex-end" : "flex-start",
        textAlign: s.align === "ctr" ? "center" : s.align === "r" ? "right" : "left",
        color: s.text_color,
        fontSize: `${s.font_size * z}pt`,
        fontWeight: s.bold ? 700 : 400,
      }}
    >
      {s.text.split("\n").map((l, i) => (
        <div key={i}>{l || " "}</div>
      ))}
    </div>
  );
}

export function ShapeLayer(props: {
  shapes: Shape[];
  geo: OverlayGeometry;
  selected: number | null;
  editing: number | null;
  onSelect(id: number | null): void;
  onEdit(id: number | null): void;
  onChange(s: Shape): void;
  onContextMenu(s: Shape, x: number, y: number): void;
}) {
  const [live, setLive] = useState<Shape | null>(null);
  const drag = useRef<{ mode: string; x: number; y: number; start: Shape } | null>(null);
  const [draft, setDraft] = useState("");
  const { geo } = props;
  const z = geo.zoom;

  useEffect(() => {
    const s = props.shapes.find((x) => x.id === props.editing);
    setDraft(s?.text ?? "");
  }, [props.editing]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const move = (e: MouseEvent) => {
      const d = drag.current;
      if (!d) return;
      const dx = (e.clientX - d.x) / z;
      const dy = (e.clientY - d.y) / z;
      const a = { ...d.start.anchor };
      const s = { ...d.start, anchor: a };
      if (d.mode === "move") {
        a.dx += dx;
        a.dy += dy;
      } else if (isLine(d.start.kind)) {
        // Lines: the box spans both ends; dragging an end can flip it.
        const st = d.start;
        const ends = {
          x1: st.flip_h ? st.anchor.width : 0,
          y1: st.flip_v ? st.anchor.height : 0,
          x2: st.flip_h ? 0 : st.anchor.width,
          y2: st.flip_v ? 0 : st.anchor.height,
        };
        if (d.mode === "start") {
          ends.x1 += dx;
          ends.y1 += dy;
        } else {
          ends.x2 += dx;
          ends.y2 += dy;
        }
        const left = Math.min(ends.x1, ends.x2);
        const top = Math.min(ends.y1, ends.y2);
        a.dx += left;
        a.dy += top;
        a.width = Math.abs(ends.x2 - ends.x1);
        a.height = Math.abs(ends.y2 - ends.y1);
        s.flip_h = ends.x2 < ends.x1;
        s.flip_v = ends.y2 < ends.y1;
      } else {
        let w = a.width;
        let h = a.height;
        if (d.mode.includes("e")) w = a.width + dx;
        if (d.mode.includes("w")) w = a.width - dx;
        if (d.mode.includes("s")) h = a.height + dy;
        if (d.mode.includes("n")) h = a.height - dy;
        if (d.mode.length === 2 && e.shiftKey) {
          const ratio = a.width / Math.max(1, a.height);
          if (Math.abs(w - a.width) > Math.abs(h - a.height)) h = w / ratio;
          else w = h * ratio;
        }
        w = Math.max(6, w);
        h = Math.max(6, h);
        if (d.mode.includes("w")) a.dx += a.width - w;
        if (d.mode.includes("n")) a.dy += a.height - h;
        a.width = w;
        a.height = h;
      }
      setLive(s);
    };
    const up = () => {
      const d = drag.current;
      drag.current = null;
      setLive((cur) => {
        if (d && cur && JSON.stringify(cur) !== JSON.stringify(d.start)) {
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

  const lastDown = useRef<{ id: number; at: number } | null>(null);
  const begin = (s: Shape, mode: string, e: React.MouseEvent) => {
    if (e.button !== 0 || props.editing === s.id) return;
    e.stopPropagation();
    e.preventDefault();
    // A second press soon after the first types into the shape (the grid
    // takes focus between them, so the browser's dblclick is not reliable).
    const now = Date.now();
    const last = lastDown.current;
    lastDown.current = { id: s.id, at: now };
    if (mode === "move" && last && last.id === s.id && now - last.at < 450 && !isLine(s.kind)) {
      lastDown.current = null;
      props.onEdit(s.id);
      return;
    }
    props.onSelect(s.id);
    drag.current = { mode, x: e.clientX, y: e.clientY, start: s };
  };

  const finishEdit = (s: Shape) => {
    if (draft !== s.text) props.onChange({ ...s, text: draft });
    props.onEdit(null);
  };

  return (
    <>
      {props.shapes.map((s0) => {
        const s = live && live.id === s0.id ? live : s0;
        const base = geo.cellRect(s.anchor.row, s.anchor.col);
        const x = base.x + s.anchor.dx * z;
        const y = base.y + s.anchor.dy * z;
        const w = s.anchor.width * z;
        const h = s.anchor.height * z;
        if (x > geo.width + PAD || y > geo.height + PAD || x + w < -PAD || y + h < -PAD) return null;
        const sel = props.selected === s0.id;
        const line = isLine(s.kind);
        const editing = props.editing === s0.id;
        return (
          <div
            key={s0.id}
            className={`shape-box ${sel ? "selected" : ""} ${line ? "line" : ""}`}
            style={{ left: x, top: y, width: w, height: h }}
            onMouseDown={(e) => begin(s0, "move", e)}
            onDoubleClick={(e) => {
              e.stopPropagation();
              if (!line) props.onEdit(s0.id);
            }}
            onWheel={(e) => {
              document.querySelector(".grid-scroll")?.dispatchEvent(new WheelEvent("wheel", e.nativeEvent));
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              props.onSelect(s0.id);
              props.onContextMenu(s0, e.clientX, e.clientY);
            }}
          >
            <ShapeFigure shape={s} w={w} h={h} zoom={z} />
            {editing ? (
              <textarea
                className="shape-editor"
                autoFocus
                value={draft}
                style={{
                  color: s.text_color,
                  fontSize: `${s.font_size * z}pt`,
                  fontWeight: s.bold ? 700 : 400,
                  textAlign: s.align === "ctr" ? "center" : s.align === "r" ? "right" : "left",
                }}
                onChange={(e) => setDraft(e.target.value)}
                onMouseDown={(e) => e.stopPropagation()}
                onBlur={() => finishEdit(s0)}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Escape") {
                    e.preventDefault();
                    finishEdit(s0);
                  }
                }}
              />
            ) : (
              <ShapeText shape={s} zoom={z} />
            )}
            {sel &&
              !editing &&
              (line
                ? (["start", "end"] as const).map((m) => {
                    const atStart = m === "start";
                    const right = atStart ? s.flip_h : !s.flip_h;
                    const bottom = atStart ? s.flip_v : !s.flip_v;
                    return (
                      <span
                        key={m}
                        className="chart-handle line-end"
                        style={{ left: right ? w - 4 : -4, top: bottom ? h - 4 : -4 }}
                        onMouseDown={(e) => begin(s0, m, e)}
                      />
                    );
                  })
                : ["nw", "n", "ne", "e", "se", "s", "sw", "w"].map((m) => (
                    <span key={m} className={`chart-handle h-${m}`} onMouseDown={(e) => begin(s0, m, e)} />
                  )))}
          </div>
        );
      })}
    </>
  );
}

/** 도형 서식: fill, line and text of a shape. */
export function ShapeFormatDialog(props: { shape: Shape; onDone(s: Shape | null): void }) {
  const [s, setS] = useState(props.shape);
  const line = isLine(s.kind);
  const set = (p: Partial<Shape>) => setS((cur) => ({ ...cur, ...p }));
  const color = (label: string, value: string | null, onChange: (v: string | null) => void, allowNone = true) => (
    <label className="shape-fmt-row">
      <span>{tr(label)}</span>
      <input type="color" value={value ?? "#ffffff"} disabled={value === null} onChange={(e) => onChange(e.target.value.toUpperCase())} />
      {allowNone && (
        <label className="check">
          <input type="checkbox" checked={value === null} onChange={(e) => onChange(e.target.checked ? null : "#4472C4")} />
          {tr("없음")}
        </label>
      )}
    </label>
  );
  return (
    <Modal title={tr("도형 서식")} onClose={() => props.onDone(null)}>
      <form
        className="modal-body shape-fmt"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(s);
        }}
      >
        {!line && color("채우기", s.fill, (v) => set({ fill: v }))}
        {color("선", s.line, (v) => set({ line: v }))}
        <label className="shape-fmt-row">
          <span>{tr("선 두께(px)")}</span>
          <input type="number" min={0} max={20} step={0.25} value={s.line_width} onChange={(e) => set({ line_width: Number(e.target.value) || 0 })} />
        </label>
        {!line && (
          <>
            {color("글꼴 색", s.text_color, (v) => set({ text_color: v ?? "#000000" }), false)}
            <label className="shape-fmt-row">
              <span>{tr("글꼴 크기(pt)")}</span>
              <input type="number" min={1} max={409} value={s.font_size} onChange={(e) => set({ font_size: Number(e.target.value) || 11 })} />
              <label className="check">
                <input type="checkbox" checked={s.bold} onChange={(e) => set({ bold: e.target.checked })} />
                {tr("굵게")}
              </label>
            </label>
            <label className="shape-fmt-row">
              <span>{tr("가로 맞춤")}</span>
              <select value={s.align} onChange={(e) => set({ align: e.target.value as Shape["align"] })}>
                <option value="l">{tr("왼쪽")}</option>
                <option value="ctr">{tr("가운데")}</option>
                <option value="r">{tr("오른쪽")}</option>
              </select>
            </label>
            <label className="shape-fmt-row">
              <span>{tr("세로 맞춤")}</span>
              <select value={s.valign} onChange={(e) => set({ valign: e.target.value as Shape["valign"] })}>
                <option value="t">{tr("위쪽")}</option>
                <option value="ctr">가운데</option>
                <option value="b">{tr("아래쪽")}</option>
              </select>
            </label>
          </>
        )}
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
