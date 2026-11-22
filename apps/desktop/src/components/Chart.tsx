// Charts: an SVG renderer for Excel's basic chart types and the layer that places,
// moves and resizes them over the grid.
import { useEffect, useRef, useState } from "react";
import { api, type Chart, type ChartData, type ChartKind } from "../api";
import type { OverlayGeometry } from "../grid/Grid";

/** Office theme accent colors, Excel's default series colors. */
export const SERIES_COLORS = ["#4472C4", "#ED7D31", "#A5A5A5", "#FFC000", "#5B9BD5", "#70AD47", "#264478", "#9E480E"];

export const CHART_KINDS: { kind: ChartKind; label: string; icon: string }[] = [
  { kind: "Column", label: "세로 막대형", icon: "▮▮" },
  { kind: "Bar", label: "가로 막대형", icon: "▬" },
  { kind: "Line", label: "꺾은선형", icon: "📈" },
  { kind: "Pie", label: "원형", icon: "◔" },
  { kind: "Doughnut", label: "도넛형", icon: "◎" },
  { kind: "Area", label: "영역형", icon: "◭" },
  { kind: "Scatter", label: "분산형", icon: "⁘" },
];

/** Round axis bounds and a step like Excel's automatic scale. */
export function niceScale(min: number, max: number, ticks = 5): { min: number; max: number; step: number } {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return { min: 0, max: 1, step: 0.2 };
  if (min > 0) min = 0;
  if (max < 0) max = 0;
  if (min === max) max = min + 1;
  const raw = (max - min) / ticks;
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const norm = raw / mag;
  const step = (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 2.5 ? 2.5 : norm <= 5 ? 5 : 10) * mag;
  return { min: Math.floor(min / step) * step, max: Math.ceil(max / step) * step, step };
}

function fmt(n: number): string {
  const a = Math.abs(n);
  if (a >= 1e9) return `${+(n / 1e9).toFixed(1)}B`;
  if (a >= 1e6) return `${+(n / 1e6).toFixed(1)}M`;
  return n.toLocaleString("ko-KR", { maximumFractionDigits: 2 });
}

const FONT = '"맑은 고딕", "Malgun Gothic", sans-serif';

export function ChartSvg(props: { kind: ChartKind; data: ChartData; width: number; height: number; legend: boolean; zoom?: number }) {
  const { kind, data, width: W, height: H } = props;
  const z = props.zoom ?? 1;
  const fs = 11 * z;
  const titleH = data.title ? 30 * z : 10 * z;
  const legendH = props.legend ? 22 * z : 0;
  const pie = kind === "Pie" || kind === "Doughnut";
  const items = pie
    ? data.categories.map((c, i) => ({ name: c, color: SERIES_COLORS[i % SERIES_COLORS.length] }))
    : data.series.map((s, i) => ({ name: s.name, color: SERIES_COLORS[i % SERIES_COLORS.length] }));
  const out: React.ReactNode[] = [];

  if (pie) {
    const s = data.series[0];
    const vals = (s?.values ?? []).map((v) => Math.max(0, v ?? 0));
    const total = vals.reduce((a, b) => a + b, 0) || 1;
    const cx = W / 2;
    const cy = titleH + (H - titleH - legendH) / 2;
    const r = Math.max(10, Math.min(W, H - titleH - legendH) / 2 - 12 * z);
    const inner = kind === "Doughnut" ? r * 0.5 : 0;
    let a0 = -Math.PI / 2;
    vals.forEach((v, i) => {
      const a1 = a0 + (v / total) * Math.PI * 2;
      const large = a1 - a0 > Math.PI ? 1 : 0;
      const p = (a: number, rr: number) => `${cx + rr * Math.cos(a)},${cy + rr * Math.sin(a)}`;
      const d =
        vals.length === 1 || v === total
          ? `M${cx - r},${cy}a${r},${r} 0 1,0 ${2 * r},0a${r},${r} 0 1,0 ${-2 * r},0` +
            (inner ? `M${cx - inner},${cy}a${inner},${inner} 0 1,1 ${2 * inner},0a${inner},${inner} 0 1,1 ${-2 * inner},0` : "")
          : inner
            ? `M${p(a0, r)}A${r},${r} 0 ${large} 1 ${p(a1, r)}L${p(a1, inner)}A${inner},${inner} 0 ${large} 0 ${p(a0, inner)}Z`
            : `M${cx},${cy}L${p(a0, r)}A${r},${r} 0 ${large} 1 ${p(a1, r)}Z`;
      out.push(<path key={`p${i}`} d={d} fill={SERIES_COLORS[i % SERIES_COLORS.length]} stroke="#fff" strokeWidth={1.5 * z} fillRule="evenodd" />);
      a0 = a1;
    });
  } else {
    const horizontal = kind === "Bar";
    const all = data.series.flatMap((s) => s.values.filter((v): v is number => v !== null));
    const sc = niceScale(Math.min(0, ...all), Math.max(0, ...all));
    const labelW = Math.max(...[sc.min, sc.max].map((v) => fmt(v).length)) * fs * 0.62 + 10 * z;
    const catLabelW = horizontal ? Math.min(W * 0.3, Math.max(20, ...data.categories.map((c) => c.length * fs * 0.75)) + 8 * z) : 0;
    const left = horizontal ? catLabelW : labelW;
    const plot = { x: left, y: titleH, w: Math.max(10, W - left - 14 * z), h: Math.max(10, H - titleH - legendH - 24 * z) };
    const n = Math.max(1, data.categories.length);
    const ticks: number[] = [];
    for (let v = sc.min; v <= sc.max + sc.step / 2; v += sc.step) ticks.push(v);
    const scatter = kind === "Scatter";
    const xs = scatter ? (data.x_values ?? data.categories.map((_, i) => i + 1)).map((v) => v ?? 0) : [];
    const xsc = scatter ? niceScale(Math.min(...xs), Math.max(...xs)) : null;
    const valuePos = (v: number) =>
      horizontal ? plot.x + ((v - sc.min) / (sc.max - sc.min)) * plot.w : plot.y + plot.h - ((v - sc.min) / (sc.max - sc.min)) * plot.h;
    // Gridlines and value labels.
    ticks.forEach((t, i) => {
      const p = valuePos(t);
      out.push(
        horizontal ? (
          <g key={`t${i}`}>
            <line x1={p} x2={p} y1={plot.y} y2={plot.y + plot.h} stroke="#D9D9D9" strokeWidth={z} />
            <text x={p} y={plot.y + plot.h + 14 * z} textAnchor="middle" fontSize={fs} fill="#595959" fontFamily={FONT}>
              {fmt(t)}
            </text>
          </g>
        ) : (
          <g key={`t${i}`}>
            <line x1={plot.x} x2={plot.x + plot.w} y1={p} y2={p} stroke="#D9D9D9" strokeWidth={z} />
            <text x={plot.x - 5 * z} y={p + fs * 0.35} textAnchor="end" fontSize={fs} fill="#595959" fontFamily={FONT}>
              {fmt(t)}
            </text>
          </g>
        ),
      );
    });
    const zero = valuePos(0);
    const band = (horizontal ? plot.h : plot.w) / n;
    const catPos = (i: number) => (horizontal ? plot.y + band * (i + 0.5) : plot.x + band * (i + 0.5));
    const xPos = (v: number) => (xsc ? plot.x + ((v - xsc.min) / (xsc.max - xsc.min)) * plot.w : 0);
    // Category labels (thinned when crowded).
    if (!scatter) {
      const every = Math.max(1, Math.ceil((n * fs * 2.6) / (horizontal ? plot.h * 2.2 : plot.w)));
      data.categories.forEach((c, i) => {
        if (i % every) return;
        out.push(
          horizontal ? (
            <text key={`c${i}`} x={plot.x - 5 * z} y={catPos(i) + fs * 0.35} textAnchor="end" fontSize={fs} fill="#595959" fontFamily={FONT}>
              {c}
            </text>
          ) : (
            <text key={`c${i}`} x={catPos(i)} y={plot.y + plot.h + 14 * z} textAnchor="middle" fontSize={fs} fill="#595959" fontFamily={FONT}>
              {c}
            </text>
          ),
        );
      });
    } else if (xsc) {
      for (let v = xsc.min; v <= xsc.max + xsc.step / 2; v += xsc.step) {
        out.push(
          <text key={`x${v}`} x={xPos(v)} y={plot.y + plot.h + 14 * z} textAnchor="middle" fontSize={fs} fill="#595959" fontFamily={FONT}>
            {fmt(v)}
          </text>,
        );
      }
    }
    // Axis line.
    out.push(
      horizontal ? (
        <line key="axis" x1={zero} x2={zero} y1={plot.y} y2={plot.y + plot.h} stroke="#BFBFBF" strokeWidth={z} />
      ) : (
        <line key="axis" x1={plot.x} x2={plot.x + plot.w} y1={zero} y2={zero} stroke="#BFBFBF" strokeWidth={z} />
      ),
    );
    const ns = Math.max(1, data.series.length);
    data.series.forEach((s, si) => {
      const color = SERIES_COLORS[si % SERIES_COLORS.length];
      if (kind === "Column" || kind === "Bar") {
        const inner = band * 0.62;
        const bw = inner / ns;
        s.values.forEach((v, i) => {
          if (v === null) return;
          const start = catPos(i) - inner / 2 + bw * si;
          const p = valuePos(v);
          out.push(
            horizontal ? (
              <rect key={`b${si}-${i}`} x={Math.min(p, zero)} y={start} width={Math.abs(p - zero)} height={bw * 0.92} fill={color} />
            ) : (
              <rect key={`b${si}-${i}`} x={start} y={Math.min(p, zero)} width={bw * 0.92} height={Math.abs(p - zero)} fill={color} />
            ),
          );
        });
      } else if (kind === "Line" || kind === "Area") {
        const pts = s.values.map((v, i) => (v === null ? null : [catPos(i), valuePos(v)] as const));
        const segs: string[] = [];
        let cur = "";
        pts.forEach((p) => {
          if (!p) {
            if (cur) segs.push(cur);
            cur = "";
          } else cur += `${cur ? "L" : "M"}${p[0]},${p[1]}`;
        });
        if (cur) segs.push(cur);
        if (kind === "Area") {
          const valid = pts.filter((p): p is readonly [number, number] => !!p);
          if (valid.length) {
            const d = `M${valid[0][0]},${zero}` + valid.map((p) => `L${p[0]},${p[1]}`).join("") + `L${valid[valid.length - 1][0]},${zero}Z`;
            out.push(<path key={`a${si}`} d={d} fill={color} opacity={0.85} />);
          }
        } else {
          segs.forEach((d, k) => out.push(<path key={`l${si}-${k}`} d={d} fill="none" stroke={color} strokeWidth={2.25 * z} strokeLinejoin="round" />));
        }
      } else if (scatter) {
        s.values.forEach((v, i) => {
          if (v === null) return;
          out.push(<circle key={`s${si}-${i}`} cx={xPos(xs[i] ?? 0)} cy={valuePos(v)} r={3.5 * z} fill={color} />);
        });
      }
    });
  }

  // Legend.
  if (props.legend && items.length) {
    const sw = 9 * z;
    const widths = items.map((it) => sw + 4 * z + it.name.length * fs * 0.75 + 12 * z);
    const total = widths.reduce((a, b) => a + b, 0);
    let x = Math.max(4 * z, (W - total) / 2);
    const y = H - legendH / 2 - 2 * z;
    items.forEach((it, i) => {
      out.push(
        <g key={`lg${i}`}>
          <rect x={x} y={y - sw / 2} width={sw} height={sw} fill={it.color} />
          <text x={x + sw + 4 * z} y={y + fs * 0.35} fontSize={fs} fill="#595959" fontFamily={FONT}>
            {it.name}
          </text>
        </g>,
      );
      x += widths[i];
    });
  }

  return (
    <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} style={{ display: "block" }}>
      <rect x={0} y={0} width={W} height={H} fill="#fff" />
      {data.title && (
        <text x={W / 2} y={21 * z} textAnchor="middle" fontSize={14 * z} fill="#595959" fontFamily={FONT}>
          {data.title}
        </text>
      )}
      {out}
    </svg>
  );
}

/** Charts of one sheet, placed over the cells. */
export function ChartLayer(props: {
  charts: Chart[];
  geo: OverlayGeometry;
  /** Bumped whenever cells change, to refetch chart data. */
  version: number;
  selected: number | null;
  onSelect(id: number | null): void;
  onChange(chart: Chart): void;
  onContextMenu(chart: Chart, x: number, y: number): void;
  onEdit(chart: Chart): void;
}) {
  const [data, setData] = useState<Record<number, ChartData>>({});
  const [live, setLive] = useState<Chart | null>(null);
  const drag = useRef<{ id: number; mode: string; x: number; y: number; start: Chart } | null>(null);
  const { geo } = props;
  const z = geo.zoom;

  useEffect(() => {
    let alive = true;
    Promise.all(props.charts.map((c) => api.chartData(c.id).then((d) => [c.id, d] as const).catch(() => null))).then((list) => {
      if (!alive) return;
      const next: Record<number, ChartData> = {};
      for (const e of list) if (e) next[e[0]] = e[1];
      setData(next);
    });
    return () => {
      alive = false;
    };
  }, [props.charts, props.version]);

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
        if (d.mode.includes("e")) a.width = Math.max(80, a.width + dx);
        if (d.mode.includes("s")) a.height = Math.max(60, a.height + dy);
        if (d.mode.includes("w")) {
          const w = Math.max(80, a.width - dx);
          a.dx += a.width - w;
          a.width = w;
        }
        if (d.mode.includes("n")) {
          const h = Math.max(60, a.height - dy);
          a.dy += a.height - h;
          a.height = h;
        }
      }
      setLive({ ...d.start, anchor: a });
    };
    const up = () => {
      const d = drag.current;
      drag.current = null;
      setLive((cur) => {
        if (d && cur && JSON.stringify(cur.anchor) !== JSON.stringify(d.start.anchor)) props.onChange(normalizeAnchor(cur, geo));
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

  const begin = (c: Chart, mode: string, e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    props.onSelect(c.id);
    drag.current = { id: c.id, mode, x: e.clientX, y: e.clientY, start: c };
  };

  return (
    <>
      {props.charts.map((c0) => {
        const c = live && live.id === c0.id ? live : c0;
        const base = geo.cellRect(c.anchor.row, c.anchor.col);
        const x = base.x + c.anchor.dx * z;
        const y = base.y + c.anchor.dy * z;
        const w = c.anchor.width * z;
        const h = c.anchor.height * z;
        if (x > geo.width || y > geo.height || x + w < 0 || y + h < 0) return null;
        const d = data[c.id];
        const sel = props.selected === c.id;
        return (
          <div
            key={c.id}
            className={`chart-box ${sel ? "selected" : ""}`}
            style={{ left: x, top: y, width: w, height: h }}
            onMouseDown={(e) => begin(c0, "move", e)}
            onDoubleClick={() => props.onEdit(c0)}
            onWheel={(e) => {
              // Scrolling over a chart scrolls the sheet, as in Excel.
              document.querySelector(".grid-scroll")?.dispatchEvent(new WheelEvent("wheel", e.nativeEvent));
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              props.onSelect(c0.id);
              props.onContextMenu(c0, e.clientX, e.clientY);
            }}
          >
            {d ? (
              <ChartSvg kind={c.kind} data={d} width={w} height={h} legend={c.legend} zoom={z} />
            ) : (
              <div className="chart-empty">데이터 없음</div>
            )}
            {sel &&
              ["nw", "n", "ne", "e", "se", "s", "sw", "w"].map((m) => (
                <span key={m} className={`chart-handle h-${m}`} onMouseDown={(e) => begin(c0, m, e)} />
              ))}
          </div>
        );
      })}
    </>
  );
}

/** After a move, re-anchor the chart to the cell under its top-left corner (Excel's model). */
export function normalizeAnchor<T extends { anchor: Chart["anchor"] }>(c: T, geo: OverlayGeometry): T {
  const a = { ...c.anchor };
  const z = geo.zoom;
  // Walk left/up when the offset went negative, right/down while it exceeds the cell.
  let guard = 0;
  while (a.dx < 0 && a.col > 1 && guard++ < 2000) {
    a.col -= 1;
    a.dx += geo.cellRect(a.row, a.col).w / z;
  }
  while (a.dy < 0 && a.row > 1 && guard++ < 4000) {
    a.row -= 1;
    a.dy += geo.cellRect(a.row, a.col).h / z;
  }
  for (;;) {
    const cw = geo.cellRect(a.row, a.col).w / z;
    if (cw <= 0 || a.dx < cw || guard++ > 6000) break;
    a.dx -= cw;
    a.col += 1;
  }
  for (;;) {
    const ch = geo.cellRect(a.row, a.col).h / z;
    if (ch <= 0 || a.dy < ch || guard++ > 8000) break;
    a.dy -= ch;
    a.row += 1;
  }
  a.dx = Math.max(0, a.dx);
  a.dy = Math.max(0, a.dy);
  return { ...c, anchor: a };
}
