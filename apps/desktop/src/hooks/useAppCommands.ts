import { useCallback, useState } from "react";
import { errorText } from "../api";

/** Owns app-wide command feedback and async command lifecycle. */
export function useAppCommands(translate: (value: string) => string) {
  const [message, setMessage] = useState<{ text: string; error?: boolean } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [dataJob, setDataJob] = useState(false);

  const say = useCallback((value: string, error = false) => {
    setMessage({ text: translate(value), error });
  }, [translate]);
  const fail = useCallback((cause: unknown) => say(errorText(cause), true), [say]);

  const run = useCallback(async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(translate(label));
    try {
      return await fn();
    } catch (cause) {
      fail(cause);
      return undefined;
    } finally {
      setBusy(null);
    }
  }, [translate, fail]);

  const runDataJob = useCallback(async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setDataJob(true);
    try {
      return await run(label, fn);
    } finally {
      setDataJob(false);
    }
  }, [run]);

  return { message, setMessage, busy, dataJob, say, fail, run, runDataJob };
}
