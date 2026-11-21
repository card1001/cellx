// Dialogs: name/number prompts, Format Cells (Ctrl+1) and Find (Ctrl+F).
import { useState, type ReactNode } from "react";
import type { CurrentStyle } from "../api";
import { FORMAT_CHOICES } from "../formats";
import { ColorPicker } from "./ColorPicker";

export function Modal(props: { title: string; children: ReactNode; onClose(): void; wide?: boolean }) {
  return (
    <div className="modal-back" onMouseDown={props.onClose}>
      <div className={`modal ${props.wide ? "wide" : ""}`} onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-title">
          <span>{props.title}</span>
          <button className="x" onClick={props.onClose} aria-label="닫기">
            ✕
          </button>
        </div>
        {props.children}
      </div>
    </div>
  );
}

export function PromptDialog(props: { title: string; label: string; value: string; onDone(v: string | null): void }) {
  const [v, setV] = useState(props.value);
  return (
    <Modal title={props.title} onClose={() => props.onDone(null)}>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onDone(v.trim() || null);
        }}
      >
        <label>
          {props.label}
          <input autoFocus value={v} onChange={(e) => setV(e.target.value)} onFocus={(e) => e.target.select()} />
        </label>
        <div className="actions">
          <button type="submit" className="primary">
            확인
          </button>
          <button type="button" onClick={() => props.onDone(null)}>
            취소
          </button>
        </div>
      </form>
    </Modal>
  );
}

export interface FormatResult {
  numFmt?: string;
  align?: string;
  valign?: string;
  wrap?: boolean;
  font?: string;
  size?: number;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  color?: string | null;
  fill?: string | null;
  border?: string;
}

export const FONTS = [
  "맑은 고딕",
  "굴림",
  "굴림체",
  "돋움",
  "돋움체",
  "바탕",
  "바탕체",
  "궁서",
  "궁서체",
  "나눔고딕",
  "Arial",
  "Calibri",
  "Cambria",
  "Consolas",
  "Courier New",
  "Georgia",
  "Segoe UI",
  "Tahoma",
  "Times New Roman",
  "Verdana",
];
export const SIZES = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];

const TABS = ["표시 형식", "맞춤", "글꼴", "테두리", "채우기"] as const;

export function FormatCellsDialog(props: { style: CurrentStyle; onDone(r: FormatResult | null): void }) {
  const s = props.style;
  const [tab, setTab] = useState<(typeof TABS)[number]>("표시 형식");
  const [r, setR] = useState<FormatResult>({});
  const cur = <K extends keyof FormatResult>(k: K, fallback: FormatResult[K]) => (k in r ? r[k] : fallback);
  const numFmt = (cur("numFmt", s.num_fmt) as string) || "general";
  return (
    <Modal title="셀 서식" onClose={() => props.onDone(null)} wide>
      <div className="tabs-head">
        {TABS.map((t) => (
          <button key={t} className={t === tab ? "on" : ""} onClick={() => setTab(t)}>
            {t}
          </button>
        ))}
      </div>
      <div className="modal-body fmt-body">
        {tab === "표시 형식" && (
          <div className="fmt-grid">
            <div className="list">
              {FORMAT_CHOICES.map((f) => (
                <button key={f.code} className={numFmt === f.code ? "on" : ""} onClick={() => setR({ ...r, numFmt: f.code })}>
                  {f.label}
                  <span className="muted">{f.sample}</span>
                </button>
              ))}
            </div>
            <label>
              형식 코드 (사용자 지정)
              <input value={numFmt} onChange={(e) => setR({ ...r, numFmt: e.target.value })} />
            </label>
          </div>
        )}
        {tab === "맞춤" && (
          <div className="fmt-form">
            <label>
              가로
              <select value={cur("align", s.align) as string} onChange={(e) => setR({ ...r, align: e.target.value })}>
                <option value="general">일반</option>
                <option value="left">왼쪽</option>
                <option value="center">가운데</option>
                <option value="right">오른쪽</option>
              </select>
            </label>
            <label>
              세로
              <select value={cur("valign", s.valign) as string} onChange={(e) => setR({ ...r, valign: e.target.value })}>
                <option value="top">위쪽</option>
                <option value="center">가운데</option>
                <option value="bottom">아래쪽</option>
              </select>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={cur("wrap", s.wrap) as boolean}
                onChange={(e) => setR({ ...r, wrap: e.target.checked })}
              />
              자동 줄 바꿈
            </label>
          </div>
        )}
        {tab === "글꼴" && (
          <div className="fmt-form">
            <label>
              글꼴
              <select value={cur("font", s.font) as string} onChange={(e) => setR({ ...r, font: e.target.value })}>
                {[...new Set([s.font, ...FONTS])].map((f) => (
                  <option key={f}>{f}</option>
                ))}
              </select>
            </label>
            <label>
              크기
              <select value={String(cur("size", s.size))} onChange={(e) => setR({ ...r, size: Number(e.target.value) })}>
                {[...new Set([s.size, ...SIZES])].sort((a, b) => a - b).map((n) => (
                  <option key={n}>{n}</option>
                ))}
              </select>
            </label>
            {(
              [
                ["bold", "굵게", s.bold],
                ["italic", "기울임꼴", s.italic],
                ["underline", "밑줄", s.underline],
                ["strike", "취소선", s.strike],
              ] as const
            ).map(([k, label, v]) => (
              <label className="check" key={k}>
                <input type="checkbox" checked={cur(k, v) as boolean} onChange={(e) => setR({ ...r, [k]: e.target.checked })} />
                {label}
              </label>
            ))}
            <div>
              글꼴 색
              <ColorPicker noneLabel="자동" onPick={(c) => setR({ ...r, color: c })} />
            </div>
          </div>
        )}
        {tab === "테두리" && (
          <div className="fmt-form">
            {(
              [
                ["None", "없음"],
                ["Outer", "윤곽선"],
                ["All", "모든 테두리"],
                ["Inner", "안쪽"],
              ] as const
            ).map(([k, label]) => (
              <label className="check" key={k}>
                <input type="radio" name="border" checked={r.border === k} onChange={() => setR({ ...r, border: k })} />
                {label}
              </label>
            ))}
          </div>
        )}
        {tab === "채우기" && (
          <div className="fmt-form">
            <ColorPicker noneLabel="채우기 없음" onPick={(c) => setR({ ...r, fill: c })} />
          </div>
        )}
        <div className="actions">
          <button className="primary" onClick={() => props.onDone(r)}>
            확인
          </button>
          <button onClick={() => props.onDone(null)}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

export function FindDialog(props: { onFind(q: string, matchCase: boolean): void; onClose(): void; message: string | null }) {
  const [q, setQ] = useState("");
  const [mc, setMc] = useState(false);
  return (
    <div className="find-box" onMouseDown={(e) => e.stopPropagation()}>
      <div className="modal-title">
        <span>찾기</span>
        <button className="x" onClick={props.onClose} aria-label="닫기">
          ✕
        </button>
      </div>
      <form
        className="modal-body"
        onSubmit={(e) => {
          e.preventDefault();
          props.onFind(q, mc);
        }}
      >
        <label>
          찾을 내용
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && props.onClose()}
          />
        </label>
        <label className="check">
          <input type="checkbox" checked={mc} onChange={(e) => setMc(e.target.checked)} />
          대/소문자 구분
        </label>
        {props.message && <div className="muted">{props.message}</div>}
        <div className="actions">
          <button type="submit" className="primary">
            다음 찾기
          </button>
          <button type="button" onClick={props.onClose}>
            닫기
          </button>
        </div>
      </form>
    </div>
  );
}
