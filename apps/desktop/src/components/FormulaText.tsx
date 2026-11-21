// A text box that colours the references of a formula, like Excel's cell editor
// and formula bar. A transparent textarea sits over a coloured copy of its text.
import { forwardRef, useLayoutEffect, useRef, type CSSProperties } from "react";
import { colourPieces } from "../formula";

interface Props {
  value: string;
  onChange(text: string, caret: number): void;
  onCaret?(caret: number): void;
  onKeyDown?(e: React.KeyboardEvent<HTMLTextAreaElement>): void;
  onFocus?(): void;
  onBlur?(): void;
  onCopy?(e: React.ClipboardEvent<HTMLTextAreaElement>): void;
  onCut?(e: React.ClipboardEvent<HTMLTextAreaElement>): void;
  onPaste?(e: React.ClipboardEvent<HTMLTextAreaElement>): void;
  onCompositionStart?(): void;
  className?: string;
  style?: CSSProperties;
  font?: string;
  /** Visible text (false: transparent key catcher). */
  visible?: boolean;
  spellCheck?: boolean;
}

export const FormulaText = forwardRef<HTMLTextAreaElement, Props>(function FormulaText(props, ref) {
  const hlRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLTextAreaElement | null>(null);
  const coloured = props.value.startsWith("=");
  const visible = props.visible !== false;

  useLayoutEffect(() => {
    const ta = innerRef.current;
    const hl = hlRef.current;
    if (ta && hl) {
      hl.scrollTop = ta.scrollTop;
      hl.scrollLeft = ta.scrollLeft;
    }
  });

  const textStyle: CSSProperties = { font: props.font };
  return (
    <div className={`ftext ${props.className ?? ""} ${visible ? "" : "ftext-hidden"}`} style={props.style}>
      {visible && coloured && (
        <div ref={hlRef} className="ftext-hl" style={textStyle} aria-hidden>
          {colourPieces(props.value).map((p, i) =>
            p.color ? (
              <span key={i} style={{ color: p.color }}>
                {p.text}
              </span>
            ) : (
              <span key={i}>{p.text}</span>
            ),
          )}
          {"​"}
        </div>
      )}
      <textarea
        ref={(el) => {
          innerRef.current = el;
          if (typeof ref === "function") ref(el);
          else if (ref) ref.current = el;
        }}
        className="ftext-input"
        style={{ ...textStyle, color: visible && !coloured ? undefined : "transparent" }}
        value={props.value}
        spellCheck={props.spellCheck ?? false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        onChange={(e) => props.onChange(e.target.value, e.target.selectionStart ?? e.target.value.length)}
        onSelect={(e) => props.onCaret?.(e.currentTarget.selectionStart ?? 0)}
        onKeyUp={(e) => props.onCaret?.(e.currentTarget.selectionStart ?? 0)}
        onClick={(e) => props.onCaret?.(e.currentTarget.selectionStart ?? 0)}
        onKeyDown={props.onKeyDown}
        onFocus={props.onFocus}
        onBlur={props.onBlur}
        onCopy={props.onCopy}
        onCut={props.onCut}
        onPaste={props.onPaste}
        onCompositionStart={props.onCompositionStart}
        onScroll={(e) => {
          if (hlRef.current) {
            hlRef.current.scrollTop = e.currentTarget.scrollTop;
            hlRef.current.scrollLeft = e.currentTarget.scrollLeft;
          }
        }}
      />
    </div>
  );
});
