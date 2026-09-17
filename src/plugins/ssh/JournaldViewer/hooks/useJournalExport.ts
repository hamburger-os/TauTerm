import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import type { JournaldErrorPayload, JournaldFilter } from "../types";
import { formatDateCompact } from "../../../utils/format";
import {
  cancelJournalExport,
  normalizeJournaldError,
  startJournalExport,
} from "./journaldClient";

interface ExportProgressEvent {
  session_id: string;
  loaded: number;
}

interface ExportCompleteEvent {
  session_id: string;
  file_path: string;
  total: number;
}

interface ExportErrorEvent {
  session_id: string;
  error: JournaldErrorPayload;
}

interface ExportCancelledEvent {
  session_id: string;
}

export interface UseJournalExportReturn {
  exporting: boolean;
  exportLoaded: number;
  error: JournaldErrorPayload | null;
  start: (filter: JournaldFilter) => Promise<void>;
  cancel: () => Promise<void>;
  clearError: () => void;
}

function defaultExportName(): string {
  return `journald_export_${formatDateCompact(new Date())}.json`;
}

export function useJournalExport(
  sessionId: string,
  isConnected: boolean,
): UseJournalExportReturn {
  const [exporting, setExporting] = useState(false);
  const [exportLoaded, setExportLoaded] = useState(0);
  const [error, setError] = useState<JournaldErrorPayload | null>(null);

  const exportingRef = useRef(false);
  const unlistenersRef = useRef<UnlistenFn[]>([]);
  const listenersReadyRef = useRef<Promise<void> | null>(null);

  const setExportingState = useCallback((value: boolean) => {
    exportingRef.current = value;
    setExporting(value);
  }, []);

  const clearError = useCallback(() => setError(null), []);

  const ensureListeners = useCallback(async () => {
    if (listenersReadyRef.current) return listenersReadyRef.current;

    listenersReadyRef.current = (async () => {
      const collected: UnlistenFn[] = [];
      try {
        collected.push(
          await listen<ExportProgressEvent>("journald:export-progress", (event) => {
            if (event.payload.session_id !== sessionId) return;
            setExportLoaded(event.payload.loaded);
          }),
        );
        collected.push(
          await listen<ExportCompleteEvent>("journald:export-complete", (event) => {
            if (event.payload.session_id !== sessionId) return;
            setExportLoaded(event.payload.total);
            setExportingState(false);
          }),
        );
        collected.push(
          await listen<ExportErrorEvent>("journald:export-error", (event) => {
            if (event.payload.session_id !== sessionId) return;
            setError(event.payload.error);
            setExportingState(false);
          }),
        );
        collected.push(
          await listen<ExportCancelledEvent>("journald:export-cancelled", (event) => {
            if (event.payload.session_id !== sessionId) return;
            setExportingState(false);
          }),
        );
        unlistenersRef.current = collected;
      } catch (listenerError) {
        collected.forEach((unlisten) => unlisten());
        listenersReadyRef.current = null;
        throw listenerError;
      }
    })();

    return listenersReadyRef.current;
  }, [sessionId, setExportingState]);

  const start = useCallback(
    async (filter: JournaldFilter) => {
      if (!isConnected || exportingRef.current) return;
      const filePath = await save({
        defaultPath: defaultExportName(),
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath) return;

      setError(null);
      setExportLoaded(0);
      setExportingState(true);
      try {
        await ensureListeners();
        await startJournalExport(sessionId, filePath, filter);
      } catch (startError) {
        setError(normalizeJournaldError(startError));
        setExportingState(false);
      }
    },
    [ensureListeners, isConnected, sessionId, setExportingState],
  );

  const cancel = useCallback(async () => {
    if (!exportingRef.current) return;
    try {
      await cancelJournalExport(sessionId);
      // Keep exporting=true until journald:export-cancelled arrives. The worker
      // emits that event only while leaving, so a new export cannot race it.
    } catch (cancelError) {
      setError(normalizeJournaldError(cancelError));
      setExportingState(false);
    }
  }, [sessionId, setExportingState]);

  useEffect(() => {
    void ensureListeners().catch((listenerError) => {
      setError(normalizeJournaldError(listenerError));
    });
    return () => {
      unlistenersRef.current.forEach((unlisten) => unlisten());
      unlistenersRef.current = [];
      listenersReadyRef.current = null;
      if (exportingRef.current) {
        void cancelJournalExport(sessionId).catch(() => undefined);
      }
    };
  }, [ensureListeners, sessionId]);

  useEffect(() => {
    if (!isConnected && exportingRef.current) {
      void cancel();
    }
  }, [cancel, isConnected]);

  return {
    exporting,
    exportLoaded,
    error,
    start,
    cancel,
    clearError,
  };
}
