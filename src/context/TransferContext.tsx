import { createContext, useContext, useReducer, useCallback, useEffect, useRef, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type TransferDirection = "send" | "receive";
export type TransferStatus = "idle" | "transferring" | "completed" | "failed" | "cancelled";

export interface TransferProgress {
  file_name: string;
  bytes_transferred: number;
  total_bytes: number;
  direction: TransferDirection;
}

export interface TransferHistoryItem {
  id: string;
  file_name: string;
  direction: TransferDirection;
  size: number;
  status: TransferStatus;
  timestamp: number;
  error?: string;
}

interface TransferState {
  status: TransferStatus;
  progress: TransferProgress | null;
  history: TransferHistoryItem[];
  error: string | null;
  isDragging: boolean;
}

type TransferAction =
  | { type: "SET_STATUS"; status: TransferStatus }
  | { type: "SET_PROGRESS"; progress: TransferProgress }
  | { type: "ADD_HISTORY"; item: TransferHistoryItem }
  | { type: "CLEAR_HISTORY" }
  | { type: "SET_ERROR"; error: string | null }
  | { type: "SET_DRAGGING"; dragging: boolean };

const initialState: TransferState = {
  status: "idle",
  progress: null,
  history: [],
  error: null,
  isDragging: false,
};

function transferReducer(state: TransferState, action: TransferAction): TransferState {
  switch (action.type) {
    case "SET_STATUS": return { ...state, status: action.status };
    case "SET_PROGRESS": return { ...state, progress: action.progress };
    case "ADD_HISTORY": return { ...state, history: [action.item, ...state.history] };
    case "CLEAR_HISTORY": return { ...state, history: [] };
    case "SET_ERROR": return { ...state, error: action.error };
    case "SET_DRAGGING": return { ...state, isDragging: action.dragging };
    default: return state;
  }
}

interface TransferContextValue {
  state: TransferState;
  sendFiles: (sessionId: string, filePaths: string[]) => Promise<void>;
  receiveFiles: (sessionId: string, downloadDir: string) => Promise<void>;
  cancelTransfer: (sessionId: string) => Promise<void>;
  clearError: () => void;
  clearHistory: () => void;
  setDragging: (dragging: boolean) => void;
}

const TransferContext = createContext<TransferContextValue | null>(null);

export function TransferProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(transferReducer, initialState);
  const idCounter = useRef(0);

  const addHistory = useCallback((item: Omit<TransferHistoryItem, "id">) => {
    dispatch({
      type: "ADD_HISTORY",
      item: { ...item, id: String(++idCounter.current) },
    });
  }, []);

  const sendFiles = useCallback(async (sessionId: string, filePaths: string[]) => {
    dispatch({ type: "SET_STATUS", status: "transferring" });
    dispatch({ type: "SET_ERROR", error: null });
    try {
      await invoke("send_files_ymodem", { sessionId, filePaths });
    } catch (e) {
      dispatch({ type: "SET_STATUS", status: "failed" });
      dispatch({ type: "SET_ERROR", error: `发送失败: ${e}` });
      addHistory({
        file_name: filePaths.join(", "),
        direction: "send",
        size: 0,
        status: "failed",
        timestamp: Date.now(),
        error: String(e),
      });
    }
  }, [addHistory]);

  const receiveFiles = useCallback(async (sessionId: string, downloadDir: string) => {
    dispatch({ type: "SET_STATUS", status: "transferring" });
    dispatch({ type: "SET_ERROR", error: null });
    try {
      await invoke("receive_files_ymodem", { sessionId, downloadDir });
    } catch (e) {
      dispatch({ type: "SET_STATUS", status: "failed" });
      dispatch({ type: "SET_ERROR", error: `接收失败: ${e}` });
      addHistory({
        file_name: "接收文件",
        direction: "receive",
        size: 0,
        status: "failed",
        timestamp: Date.now(),
        error: String(e),
      });
    }
  }, [addHistory]);

  const cancelTransfer = useCallback(async (sessionId: string) => {
    try {
      await invoke("cancel_transfer", { sessionId });
      dispatch({ type: "SET_STATUS", status: "cancelled" });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `取消失败: ${e}` });
    }
  }, []);

  const clearError = useCallback(() => dispatch({ type: "SET_ERROR", error: null }), []);
  const clearHistory = useCallback(() => dispatch({ type: "CLEAR_HISTORY" }), []);
  const setDragging = useCallback((dragging: boolean) => dispatch({ type: "SET_DRAGGING", dragging }), []);

  // Event listeners
  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    (async () => {
      const u1 = await listen<TransferProgress>("transfer-progress", (event) => {
        dispatch({ type: "SET_PROGRESS", progress: event.payload });
        dispatch({ type: "SET_STATUS", status: "transferring" });
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u2 = await listen<{ success: boolean; files?: number; message?: string }>("transfer-complete", (event) => {
        const payload = event.payload;
        if (payload.success) {
          dispatch({ type: "SET_STATUS", status: "completed" });
        } else {
          dispatch({ type: "SET_STATUS", status: "failed" });
        }
      });
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach(u => u());
    };
  }, []);

  return (
    <TransferContext.Provider value={{
      state,
      sendFiles,
      receiveFiles,
      cancelTransfer,
      clearError,
      clearHistory,
      setDragging,
    }}>
      {children}
    </TransferContext.Provider>
  );
}

export function useTransfer() {
  const ctx = useContext(TransferContext);
  if (!ctx) throw new Error("useTransfer must be used within TransferProvider");
  return ctx;
}
