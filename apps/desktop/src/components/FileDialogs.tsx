// 「변경 내용을 저장하시겠습니까?」 and 문서 복구 (files left by a crash).
import type { RecoveredFile } from "../api";
import { Modal } from "./Dialogs";

export type SaveChoice = "save" | "discard" | "cancel";

export function SaveChangesDialog(props: { name: string; onChoose(c: SaveChoice): void }) {
  return (
    <Modal title="CellX" onClose={() => props.onChoose("cancel")}>
      <div className="modal-body">
        <div className="alert-row">
          <span className="alert-icon warning">⚠</span>
          <div>
            「{props.name}」의 변경 내용을 저장하시겠습니까?
            <div className="hint">저장하지 않으면 변경 내용이 사라집니다.</div>
          </div>
        </div>
        <div className="actions">
          <button className="primary" autoFocus onClick={() => props.onChoose("save")}>
            저장
          </button>
          <button onClick={() => props.onChoose("discard")}>저장 안 함</button>
          <button onClick={() => props.onChoose("cancel")}>취소</button>
        </div>
      </div>
    </Modal>
  );
}

function when(seconds: number): string {
  const d = new Date(seconds * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function RecoveryDialog(props: {
  files: RecoveredFile[];
  onOpen(id: string): void;
  onDiscard(id: string): void;
  onClose(): void;
}) {
  return (
    <Modal title="문서 복구" onClose={props.onClose} wide>
      <div className="modal-body">
        <p>CellX가 정상적으로 종료되지 않았습니다. 자동으로 저장해 둔 다음 파일을 복구할 수 있습니다.</p>
        <div className="recovery-list">
          {props.files.map((f) => (
            <div key={f.id} className="recovery-item">
              <div>
                <b>{f.name}</b> <span className="hint">(자동 저장 {when(f.saved_at)})</span>
                {f.original && <div className="hint">원래 위치: {f.original}</div>}
              </div>
              <div className="recovery-actions">
                <button className="primary" onClick={() => props.onOpen(f.id)}>
                  열기
                </button>
                <button onClick={() => props.onDiscard(f.id)}>삭제</button>
              </div>
            </div>
          ))}
        </div>
        <div className="actions">
          <button onClick={props.onClose}>닫기</button>
        </div>
      </div>
    </Modal>
  );
}
