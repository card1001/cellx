import { useCallback, useState } from "react";
import type { Dialog } from "../appDialogs";

/** Owns dialog state and the promise lifecycle for prompt dialogs. */
export function useDialogController(translate: (value: string) => string) {
  const [dialog, setDialog] = useState<Dialog>(null);
  const ask = useCallback((title: string, label: string, value = "") =>
    new Promise<string | null>((resolve) => setDialog({
      kind: "prompt",
      title: translate(title),
      label: translate(label),
      value,
      resolve,
    })), [translate]);
  return { dialog, setDialog, ask };
}
