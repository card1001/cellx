// Drop-down and context menus in the style of Excel's.
import { useEffect, useRef, useState, type ReactNode } from "react";

export interface MenuItem {
  label?: string;
  shortcut?: string;
  icon?: ReactNode;
  disabled?: boolean;
  checked?: boolean;
  separator?: boolean;
  onClick?(): void;
}

export function MenuList(props: { items: MenuItem[]; onClose(): void }) {
  return (
    <div className="menu" role="menu" onMouseDown={(e) => e.preventDefault()}>
      {props.items.map((it, i) =>
        it.separator ? (
          <div key={i} className="menu-sep" />
        ) : (
          <button
            key={i}
            className="menu-item"
            role="menuitem"
            disabled={it.disabled}
            onClick={() => {
              props.onClose();
              it.onClick?.();
            }}
          >
            <span className="menu-icon">{it.checked ? "✓" : it.icon}</span>
            <span className="menu-label">{it.label}</span>
            {it.shortcut && <span className="menu-shortcut">{it.shortcut}</span>}
          </button>
        ),
      )}
    </div>
  );
}

export function ContextMenu(props: { x: number; y: number; items: MenuItem[]; onClose(): void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: props.x, y: props.y });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      x: Math.min(props.x, window.innerWidth - r.width - 4),
      y: Math.min(props.y, window.innerHeight - r.height - 4),
    });
  }, [props.x, props.y]);
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) props.onClose();
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [props]);
  return (
    <div ref={ref} className="context-menu" style={{ left: pos.x, top: pos.y }}>
      <MenuList items={props.items} onClose={props.onClose} />
    </div>
  );
}

/** A ribbon button with a drop-down menu (or arbitrary panel). */
export function Dropdown(props: {
  label: ReactNode;
  title?: string;
  items?: MenuItem[];
  panel?(close: () => void): ReactNode;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [at, setAt] = useState({ x: 0, y: 0 });
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  return (
    <div className={`dropdown ${props.className ?? ""}`} ref={ref}>
      <button
        className={`rb dd ${open ? "on" : ""}`}
        title={props.title}
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          // The ribbon clips overflowing children, so the panel is placed in window coordinates.
          const r = e.currentTarget.getBoundingClientRect();
          setAt({ x: Math.min(r.left, window.innerWidth - 260), y: r.bottom + 1 });
          setOpen((o) => !o);
        }}
      >
        {props.label}
        <span className="caret">▾</span>
      </button>
      {open && (
        <div className="dropdown-panel" style={{ position: "fixed", left: at.x, top: at.y, maxHeight: window.innerHeight - at.y - 8 }}>
          {props.items ? (
            <MenuList items={props.items} onClose={() => setOpen(false)} />
          ) : (
            props.panel?.(() => setOpen(false))
          )}
        </div>
      )}
    </div>
  );
}
