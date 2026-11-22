// Pictures over the sheet (삽입 → 그림): drawn, moved and resized like Excel's.
// Corner handles keep the picture's proportions, side handles stretch it.
import { useEffect, useRef, useState } from "react";
import { api, Picture } from "../api";
import { OverlayGeometry } from "../grid/Grid";
import { normalizeAnchor } from "./Chart";

/** Image data by picture id. A picture's image never changes, so this lives as long as the app. */
const urls = new Map<number, string>();

export function pictureUrl(id: number): Promise<string> {
  const known = urls.get(id);
  if (known) return Promise.resolve(known);
  return api.pictureData(id).then((u) => {
    urls.set(id, u);
    return u;
  });
}

export function PictureLayer(props: {
  pictures: Picture[];
  geo: OverlayGeometry;
  selected: number | null;
  onSelect(id: number | null): void;
  onChange(p: Picture): void;
  onContextMenu(p: Picture, x: number, y: number): void;
}) {
  const [, setLoaded] = useState(0);
  const [live, setLive] = useState<Picture | null>(null);
  const drag = useRef<{ mode: string; x: number; y: number; start: Picture } | null>(null);
  const { geo } = props;
  const z = geo.zoom;

  useEffect(() => {
    let alive = true;
    for (const p of props.pictures) {
      if (!urls.has(p.id)) void pictureUrl(p.id).then(() => alive && setLoaded((n) => n + 1)).catch(() => {});
    }
    return () => {
      alive = false;
    };
  }, [props.pictures]);

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
        const corner = d.mode.length === 2;
        let w = a.width;
        let h = a.height;
        if (d.mode.includes("e")) w = a.width + dx;
        if (d.mode.includes("w")) w = a.width - dx;
        if (d.mode.includes("s")) h = a.height + dy;
        if (d.mode.includes("n")) h = a.height - dy;
        if (corner && !e.shiftKey) {
          // Keep the proportions: follow whichever side moved more.
          const ratio = a.width / Math.max(1, a.height);
          if (Math.abs(w - a.width) / Math.max(1, a.width) > Math.abs(h - a.height) / Math.max(1, a.height)) h = w / ratio;
          else w = h * ratio;
        }
        w = Math.max(8, w);
        h = Math.max(8, h);
        if (d.mode.includes("w")) a.dx += a.width - w;
        if (d.mode.includes("n")) a.dy += a.height - h;
        a.width = w;
        a.height = h;
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

  const begin = (p: Picture, mode: string, e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    props.onSelect(p.id);
    drag.current = { mode, x: e.clientX, y: e.clientY, start: p };
  };

  return (
    <>
      {props.pictures.map((p0) => {
        const p = live && live.id === p0.id ? live : p0;
        const base = geo.cellRect(p.anchor.row, p.anchor.col);
        const x = base.x + p.anchor.dx * z;
        const y = base.y + p.anchor.dy * z;
        const w = p.anchor.width * z;
        const h = p.anchor.height * z;
        if (x > geo.width || y > geo.height || x + w < 0 || y + h < 0) return null;
        const sel = props.selected === p0.id;
        const url = urls.get(p0.id);
        return (
          <div
            key={p0.id}
            className={`picture-box ${sel ? "selected" : ""}`}
            style={{ left: x, top: y, width: w, height: h }}
            onMouseDown={(e) => begin(p0, "move", e)}
            onWheel={(e) => {
              document.querySelector(".grid-scroll")?.dispatchEvent(new WheelEvent("wheel", e.nativeEvent));
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              props.onSelect(p0.id);
              props.onContextMenu(p0, e.clientX, e.clientY);
            }}
          >
            {url && <img src={url} alt={p0.name} draggable={false} />}
            {sel &&
              ["nw", "n", "ne", "e", "se", "s", "sw", "w"].map((m) => (
                <span key={m} className={`chart-handle h-${m}`} onMouseDown={(e) => begin(p0, m, e)} />
              ))}
          </div>
        );
      })}
    </>
  );
}
