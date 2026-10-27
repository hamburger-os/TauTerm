import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useReducer,
  useRef,
  type ReactNode,
} from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSession } from "./SessionContext";
import {
  cancelFileTransfer,
  startFileTransfer,
} from "../services/transferService";
import type { BatchFileEntry, FileTransferReceiveRequest, FileTransferSendRequest, ReceiveProtocolOptions, SendProtocolOptions, TransferConfig, TransferDirection, TransferFinishedPayload, TransferHistoryItem, TransferStartedPayload, TransferStartAck, TransferStatus, UnifiedTransferProgressPayload } from "../types/transfer";
import { PROTOCOL_REGISTRY } from "../types/transfer";

export type {
  BatchFileEntry,
  TransferDirection,
  TransferHistoryItem,
  TransferStatus,
};

import { initialTransferState, transferReducer, selectedFileEntries, latestTaskForSession, latestActiveTaskForSession, isManagedTransferTerminalPhase, type TransferState, type ManagedTransferTask } from "../core/transfer-state";
export { isManagedTransferTerminalPhase } from "../core/transfer-state";
export type { ManagedTransferPhase, ManagedTransferTask, TransferState } from "../core/transfer-state";

function sendProtocolOptions(config: TransferConfig): SendProtocolOptions {
  switch (config.protocol) {
    case "ymodem":
      return { protocol: "ymodem", blockSize: config.send.blockSize };
    case "xmodem":
      return { protocol: "xmodem", blockSize: config.send.blockSize };
    case "zmodem":
      return {
        protocol: "zmodem",
        crcPolicy: config.send.crcPolicy,
        maxBlockSize: config.send.maxBlockSize,
      };
    case "sftp":
      return { protocol: "sftp" };
  }
}

function receiveProtocolOptions(config: TransferConfig): ReceiveProtocolOptions {
  switch (config.protocol) {
    case "ymodem":
      return { protocol: "ymodem" };
    case "xmodem":
      return { protocol: "xmodem", checkMode: config.receive.checkMode };
    case "zmodem":
      return { protocol: "zmodem", crcCapability: config.receive.crcCapability };
    case "sftp":
      return { protocol: "sftp" };
  }
}

interface TransferContextValue {
  state: TransferState;
  startTransfer: (
    config: TransferConfig,
    sessionId: string,
    direction: TransferDirection,
    filePaths?: string[],
    downloadDir?: string,
  ) => Promise<TransferStartAck>;
  sendFiles: (sessionId: string, filePaths: string[]) => Promise<void>;
  receiveFiles: (sessionId: string, downloadDir: string) => Promise<void>;
  cancelTransfer: (sessionId: string) => Promise<void>;
  cancelTask: (sessionId: string, transferId: string) => Promise<void>;
  dismissTask: (sessionId: string, transferId: string) => void;
  clearError: (sessionId?: string) => void;
  clearHistory: () => void;
  getTaskForSession: (sessionId: string) => ManagedTransferTask | undefined;
  getActiveTaskForSession: (sessionId: string) => ManagedTransferTask | undefined;
}

const TransferContext = createContext<TransferContextValue | null>(null);

export function TransferProvider({ children }: { children: ReactNode }) {
  const { isSessionRuntimeCurrent, getSessionRuntimeId } = useSession();
  const [state, dispatch] = useReducer(transferReducer, initialTransferState);
  const listenerReady = useRef<Promise<void> | null>(null);

  const startTransfer = useCallback(
    async (
      config: TransferConfig,
      sessionId: string,
      direction: TransferDirection,
      filePaths?: string[],
      downloadDir?: string,
    ): Promise<TransferStartAck> => {
      dispatch({ type: "TASK_CLEAR_ERROR", sessionId });
      const requestedRuntimeId = getSessionRuntimeId(sessionId);

      try {
        if (!listenerReady.current) throw new Error("Transfer listeners are not ready");
        await listenerReady.current;
        if (!requestedRuntimeId || !isSessionRuntimeCurrent(sessionId, requestedRuntimeId)) throw new Error("Session runtime is no longer connected");
        let ack: TransferStartAck;
        if (direction === "send") {
          const request: FileTransferSendRequest = {
            sessionId,
            runtimeId: requestedRuntimeId,
            protocolOptions: sendProtocolOptions(config),
            filePaths: filePaths ?? [],
          };
          if (config.protocol === "sftp") request.remoteDir = config.remotePath;
          ack = await startFileTransfer("send", request);
        } else {
          const request: FileTransferReceiveRequest = {
            sessionId,
            runtimeId: requestedRuntimeId,
            protocolOptions: receiveProtocolOptions(config),
            downloadDir: downloadDir ?? "",
            remotePaths: [],
          };
          ack = await startFileTransfer("receive", request);
        }

        if (!isSessionRuntimeCurrent(sessionId, ack.runtime_id)) return ack;
        dispatch({
          type: "TASK_ACKNOWLEDGED",
          payload: {
            session_id: sessionId,
            runtime_id: ack.runtime_id,
            transfer_id: ack.transfer_id,
            protocol: config.protocol,
            direction,
          },
          initialFiles: direction === "send" ? selectedFileEntries(filePaths) : undefined,
        });
        return ack;
      } catch (error) {
        const message = String(error);
        if (requestedRuntimeId && isSessionRuntimeCurrent(sessionId, requestedRuntimeId)) {
          dispatch({ type: "TASK_START_FAILED", sessionId, error: message });
        }
        throw error;
      }
    },
    [getSessionRuntimeId, isSessionRuntimeCurrent],
  );

  const sendFiles = useCallback(
    async (sessionId: string, filePaths: string[]) => {
      await startTransfer(
        PROTOCOL_REGISTRY.ymodem.defaultConfig,
        sessionId,
        "send",
        filePaths,
      );
    },
    [startTransfer],
  );

  const receiveFiles = useCallback(
    async (sessionId: string, downloadDir: string) => {
      await startTransfer(
        PROTOCOL_REGISTRY.ymodem.defaultConfig,
        sessionId,
        "receive",
        undefined,
        downloadDir,
      );
    },
    [startTransfer],
  );

  const cancelTask = useCallback(async (sessionId: string, transferId: string) => {
    const current = state.tasksById[transferId];
    if (!current || current.sessionId !== sessionId) {
      throw new Error("Transfer task is no longer active");
    }
    if (isManagedTransferTerminalPhase(current.phase) || current.phase === "cancelling") {
      return;
    }

    const previousPhase = current.phase;
    dispatch({ type: "TASK_CANCELLING", sessionId, transferId });
    try {
      await cancelFileTransfer(sessionId, transferId);
    } catch (error) {
      dispatch({
        type: "TASK_CANCEL_REJECTED",
        sessionId,
        transferId,
        previousPhase,
        error: String(error),
      });
      throw error;
    }
  }, [state.tasksById]);

  const cancelTransfer = useCallback(async (sessionId: string) => {
    const task = latestActiveTaskForSession(state, sessionId);
    if (!task) return;
    await cancelTask(sessionId, task.transferId);
  }, [cancelTask, state]);

  const dismissTask = useCallback((sessionId: string, transferId: string) => {
    dispatch({ type: "TASK_DISCARD", sessionId, transferId });
  }, []);

  const clearError = useCallback((sessionId?: string) => {
    dispatch({ type: "TASK_CLEAR_ERROR", sessionId });
  }, []);

  const clearHistory = useCallback(() => {
    dispatch({ type: "CLEAR_HISTORY" });
  }, []);

  const getTaskForSession = useCallback(
    (sessionId: string) => latestTaskForSession(state, sessionId),
    [state],
  );
  const getActiveTaskForSession = useCallback(
    (sessionId: string) => {
      const task = latestActiveTaskForSession(state, sessionId);
      return task && isSessionRuntimeCurrent(sessionId, task.runtimeId) ? task : undefined;
    },
    [state, isSessionRuntimeCurrent],
  );

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    listenerReady.current = (async () => {
      const disconnected = await listen<{ session_id: string; runtime_id: string }>(
        "session-disconnected",
        (event) => {
          dispatch({ type: "TASK_DISCARD_SESSION", sessionId: event.payload.session_id, runtimeId: event.payload.runtime_id });
        },
      );
      if (cancelled) {
        disconnected();
        return;
      }
      unlisteners.push(disconnected);

      const started = await listen<TransferStartedPayload>(
        "file-transfer:started",
        (event) => {
          if (isSessionRuntimeCurrent(event.payload.session_id, event.payload.runtime_id)) dispatch({ type: "TASK_STARTED", payload: event.payload });
        },
      );
      if (cancelled) {
        started();
        return;
      }
      unlisteners.push(started);

      const progress = await listen<UnifiedTransferProgressPayload>(
        "file-transfer:progress",
        (event) => {
          if (isSessionRuntimeCurrent(event.payload.session_id, event.payload.runtime_id)) dispatch({ type: "TASK_PROGRESS", payload: event.payload });
        },
      );
      if (cancelled) {
        progress();
        return;
      }
      unlisteners.push(progress);

      const finished = await listen<TransferFinishedPayload>(
        "file-transfer:finished",
        (event) => dispatch({ type: "TASK_FINISHED", payload: event.payload }),
      );
      if (cancelled) {
        finished();
        return;
      }
      unlisteners.push(finished);
    })();
    void listenerReady.current.catch((error) => {
      console.error("TransferContext: Failed to register event listeners:", error);
    });

    return () => {
      cancelled = true;
      listenerReady.current = null;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [isSessionRuntimeCurrent]);

  return (
    <TransferContext.Provider
      value={{
        state,
        startTransfer,
        sendFiles,
        receiveFiles,
        cancelTransfer,
        cancelTask,
        dismissTask,
        clearError,
        clearHistory,
        getTaskForSession,
        getActiveTaskForSession,
      }}
    >
      {children}
    </TransferContext.Provider>
  );
}

export function useTransfer() {
  const ctx = useContext(TransferContext);
  if (!ctx) {
    throw new Error("useTransfer must be used within TransferProvider");
  }
  return ctx;
}
