// Progress window for long data-sheet commands (import, export, query).
// Appears only when the command takes longer than a moment, polls the
// backend without touching the workbook, and offers Cancel (or Esc).

import { useEffect, useRef, useState } from "react";
import { api, type DataJobStatus } from "../api";
import { tr } from "../i18n";

const SHOW_AFTER_MS = 400;
const POLL_MS = 250;

export function DataJobProgress(props: { active: boolean }) {
  const [status, setStatus] = useState<DataJobStatus | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!props.active) {
      setStatus(null);
      setCancelling(false);
      return;
    }
    let stopped = false;
    let timer: number | undefined;
    const poll = async () => {
      try {
        const next = await api.dataJobProgress();
        if (!stopped) setStatus(next);
      } catch {
        // Keep the last status; the command itself reports real errors.
      }
      if (!stopped) timer = window.setTimeout(poll, POLL_MS);
    };
    timer = window.setTimeout(poll, SHOW_AFTER_MS);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [props.active]);

  useEffect(() => {
    if (status) cancelRef.current?.focus();
  }, [status !== null]);

  if (!props.active || !status) return null;

  const cancel = () => {
    setCancelling(true);
    void api.cancelDataJob();
  };
  const p = status.progress;
  const title = status.kind === "import" ? tr("가져오는 중") : status.kind === "export" ? tr("내보내는 중") : tr("쿼리 실행 중");
  const phase =
    cancelling || p.cancelled
      ? tr("취소하는 중")
      : p.phase === "converting"
        ? tr("텍스트 인코딩 변환 중")
        : p.phase === "querying"
          ? status.kind === "export"
            ? tr("파일 쓰는 중")
            : status.kind === "import"
              ? tr("데이터 읽는 중")
              : tr("쿼리 실행 중")
          : p.phase === "finishing"
            ? tr("마무리하는 중")
            : tr("준비하는 중");
  const percent = p.fraction == null ? null : Math.round(p.fraction * 1000) / 10;
  const detail: string[] = [];
  if (p.phase === "converting" && p.bytesTotal > 0) detail.push(`${formatBytes(p.bytesDone)} / ${formatBytes(p.bytesTotal)}`);
  if (p.phase === "querying" && p.rowsProcessed > 0) detail.push(`${p.rowsProcessed.toLocaleString()} ${tr("행 처리")}`);
  detail.push(`${tr("경과")} ${formatElapsed(status.elapsedMs)}`);

  return (
    <div className="modal-back" role="presentation">
      <div
        className="modal job-progress"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            if (!cancelling) cancel();
          }
        }}
      >
        <div className="modal-title">
          <span>{title}</span>
        </div>
        <div className="modal-body">
          <div className="job-label" title={status.label}>{status.label}</div>
          <div className="job-phase">
            {phase}
            {percent != null && !cancelling ? ` — ${percent.toFixed(1)}%` : ""}
          </div>
          <div
            className={`job-bar ${percent == null ? "indeterminate" : ""}`}
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent ?? undefined}
          >
            <div style={percent == null ? undefined : { width: `${percent}%` }} />
          </div>
          <div className="hint">{detail.join(" · ")}</div>
          <div className="actions">
            <button ref={cancelRef} disabled={cancelling} onClick={cancel}>
              {tr("취소")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

export function formatElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  const pad = (x: number) => String(x).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m % 60)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}
