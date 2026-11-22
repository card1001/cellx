// 이동/복사, 숨기기 취소 and 머리글/바닥글.
import { useState } from "react";
import type { PageSetup, SheetTab } from "../api";
import { Modal } from "./Dialogs";

/** A list box (the native one draws as a single line in some engines). */
function ListBox(props: { items: { value: number; label: string }[]; value: number; onChange(v: number): void; onPick(v: number): void }) {
  return (
    <div className="list-box" role="listbox">
      {props.items.map((it) => (
        <div
          key={it.value}
          role="option"
          aria-selected={it.value === props.value}
          className={`list-item ${it.value === props.value ? "on" : ""}`}
          onMouseDown={() => props.onChange(it.value)}
          onDoubleClick={() => props.onPick(it.value)}
        >
          {it.label}
        </div>
      ))}
    </div>
  );
}

export function MoveCopyDialog(props: { tabs: SheetTab[]; sheet: number; onOk(to: number, copy: boolean): void; onClose(): void }) {
  // Excel lists the sheets to put it before, then 「(끝으로 이동)」.
  const [before, setBefore] = useState(props.sheet + 1 < props.tabs.length ? props.sheet + 1 : props.tabs.length);
  const [copy, setCopy] = useState(false);
  const ok = () => {
    // `before` counts in the current order; moving takes the sheet out first.
    const to = copy ? before : before > props.sheet ? before - 1 : before;
    props.onOk(Math.min(to, copy ? props.tabs.length : props.tabs.length - 1), copy);
  };
  return (
    <Modal title="이동/복사" onClose={props.onClose}>
      <div className="modal-body">
        <div>「{props.tabs[props.sheet]?.name}」 시트를 옮길 위치</div>
        <div className="hint">다음 시트의 앞에:</div>
        <ListBox
          items={[...props.tabs.map((t, i) => ({ value: i, label: `${t.name}${t.hidden ? " (숨김)" : ""}` })), { value: props.tabs.length, label: "(끝으로 이동)" }]}
          value={before}
          onChange={setBefore}
          onPick={(v) => {
            setBefore(v);
          }}
        />
        <label className="check">
          <input type="checkbox" checked={copy} onChange={(e) => setCopy(e.target.checked)} /> 복사본 만들기
        </label>
        <div className="actions">
          <button className="primary" onClick={ok}>
            확인
          </button>
          <button onClick={props.onClose}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

export function UnhideDialog(props: { tabs: SheetTab[]; onOk(sheet: number): void; onClose(): void }) {
  const hidden = props.tabs.map((t, i) => ({ t, i })).filter((x) => x.t.hidden);
  const [pick, setPick] = useState(hidden[0]?.i ?? -1);
  return (
    <Modal title="숨기기 취소" onClose={props.onClose}>
      <div className="modal-body">
        <div className="hint">숨기기 취소할 시트:</div>
        <ListBox items={hidden.map(({ t, i }) => ({ value: i, label: t.name }))} value={pick} onChange={setPick} onPick={(v) => props.onOk(v)} />
        <div className="actions">
          <button className="primary" disabled={pick < 0} onClick={() => props.onOk(pick)}>
            확인
          </button>
          <button onClick={props.onClose}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

const CODES: { label: string; code: string }[] = [
  { label: "페이지 번호", code: "&P" },
  { label: "페이지 수", code: "&N" },
  { label: "현재 날짜", code: "&D" },
  { label: "현재 시간", code: "&T" },
  { label: "파일 이름", code: "&F" },
  { label: "시트 이름", code: "&A" },
];

const PRESETS: { label: string; parts: [string, string, string] }[] = [
  { label: "(없음)", parts: ["", "", ""] },
  { label: "1 / 3 페이지", parts: ["", "&P / &N 페이지", ""] },
  { label: "&P 페이지", parts: ["", "&P 페이지", ""] },
  { label: "시트 이름", parts: ["", "&A", ""] },
  { label: "파일 이름", parts: ["", "&F", ""] },
  { label: "시트 이름 · 페이지", parts: ["&A", "", "&P 페이지"] },
  { label: "날짜 · 페이지", parts: ["&D", "", "&P / &N"] },
];

function Parts(props: { title: string; parts: [string, string, string]; onChange(p: [string, string, string]): void }) {
  const [focus, setFocus] = useState(1);
  const labels = ["왼쪽 구역", "가운데 구역", "오른쪽 구역"];
  const insert = (code: string) => {
    const next = [...props.parts] as [string, string, string];
    next[focus] = `${next[focus]}${code}`;
    props.onChange(next);
  };
  return (
    <fieldset className="hf-group">
      <legend>{props.title}</legend>
      <div className="hf-row">
        <select
          value=""
          onChange={(e) => {
            const p = PRESETS[Number(e.target.value)];
            if (p) props.onChange([...p.parts]);
          }}
        >
          <option value="">미리 설정된 {props.title}...</option>
          {PRESETS.map((p, i) => (
            <option key={i} value={i}>
              {p.label}
            </option>
          ))}
        </select>
        {CODES.map((c) => (
          <button key={c.code} type="button" className="hf-code" title={`${c.label} (${c.code})`} onMouseDown={(e) => e.preventDefault()} onClick={() => insert(c.code)}>
            {c.label}
          </button>
        ))}
      </div>
      <div className="hf-parts">
        {labels.map((l, i) => (
          <label key={i}>
            <span className="hint">{l}</span>
            <textarea
              rows={2}
              value={props.parts[i]}
              onFocus={() => setFocus(i)}
              onChange={(e) => {
                const next = [...props.parts] as [string, string, string];
                next[i] = e.target.value;
                props.onChange(next);
              }}
            />
          </label>
        ))}
      </div>
    </fieldset>
  );
}

export function HeaderFooterDialog(props: { setup: PageSetup; onOk(s: PageSetup): void; onClose(): void }) {
  const footer0: [string, string, string] =
    props.setup.page_numbers && props.setup.footer.every((x) => !x) ? ["", "&P / &N", ""] : [...props.setup.footer];
  const [header, setHeader] = useState<[string, string, string]>([...props.setup.header]);
  const [footer, setFooter] = useState<[string, string, string]>(footer0);
  return (
    <Modal title="머리글/바닥글" onClose={props.onClose} wide>
      <div className="modal-body">
        <Parts title="머리글" parts={header} onChange={setHeader} />
        <Parts title="바닥글" parts={footer} onChange={setFooter} />
        <div className="hint">구역을 누른 뒤 단추를 누르면 그 자리에 코드가 들어갑니다. 인쇄할 때 실제 값으로 바뀝니다.</div>
        <div className="actions">
          <button className="primary" onClick={() => props.onOk({ ...props.setup, header, footer, page_numbers: false })}>
            확인
          </button>
          <button onClick={props.onClose}>취소</button>
        </div>
      </div>
    </Modal>
  );
}
