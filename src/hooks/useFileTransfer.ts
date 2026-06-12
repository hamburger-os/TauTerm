import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

/** 传输方向 */
export type TransferDirection = "send" | "receive";

/** 传输状态 */
export type TransferStatus = "idle" | "transferring" | "completed" | "failed" | "cancelled";

/** 传输进度信息 */
export interface TransferProgress {
  file_name: string;
  bytes_transferred: number;
  total_bytes: number;
  direction: TransferDirection;
}

/** 传输历史记录 */
export interface TransferHistoryItem {
  id: string;
  file_name: string;
  direction: TransferDirection;
  size: number;
  status: TransferStatus;
  timestamp: number;
  error?: string;
}

/**
 * useFileTransfer Hook
 *
 * 封装文件传输命令，追踪进度事件，管理传输状态。
 */
export function useFileTransfer() {
  const [status, setStatus] = useState<TransferStatus>("idle");
  const [progress, setProgress] = useState<TransferProgress | null>(null);
  const [history, setHistory] = useState<TransferHistoryItem[]>([]);
  const [error, setError] = useState<string | null>(null);

  const idCounter = useRef(0);

  /** 添加历史记录 */
  const addHistory = useCallback(
    (item: Omit<TransferHistoryItem, "id">) => {
      const newItem: TransferHistoryItem = {
        ...item,
        id: String(++idCounter.current),
      };
      setHistory((prev) => [newItem, ...prev]);
    },
    []
  );

  /** 发送文件 */
  const sendFiles = useCallback(
    async (filePaths: string[]) => {
      setStatus("transferring");
      setError(null);
      try {
        await invoke("send_files_ymodem", { filePaths });
      } catch (e) {
        setStatus("failed");
        setError(`发送失败: ${e}`);
        addHistory({
          file_name: filePaths.join(", "),
          direction: "send",
          size: 0,
          status: "failed",
          timestamp: Date.now(),
          error: String(e),
        });
      }
    },
    [addHistory]
  );

  /** 接收文件 */
  const receiveFiles = useCallback(
    async (downloadDir: string) => {
      setStatus("transferring");
      setError(null);
      try {
        await invoke("receive_files_ymodem", { downloadDir });
      } catch (e) {
        setStatus("failed");
        setError(`接收失败: ${e}`);
        addHistory({
          file_name: "接收文件",
          direction: "receive",
          size: 0,
          status: "failed",
          timestamp: Date.now(),
          error: String(e),
        });
      }
    },
    [addHistory]
  );

  /** 取消传输 */
  const cancelTransfer = useCallback(async () => {
    try {
      await invoke("cancel_transfer");
      setStatus("cancelled");
    } catch (e) {
      setError(`取消失败: ${e}`);
    }
  }, []);

  /** 清除错误 */
  const clearError = useCallback(() => setError(null), []);

  /** 清除历史 */
  const clearHistory = useCallback(() => setHistory([]), []);

  // 监听传输事件
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    listen<{
      file_name: string;
      bytes_transferred: number;
      total_bytes: number;
      direction: TransferDirection;
    }>("transfer-progress", (event) => {
      setProgress(event.payload);
      setStatus("transferring");
    }).then((u) => {
      unlisteners.push(u);
    });

    listen<{ success: boolean; files?: number; message?: string }>(
      "transfer-complete",
      (event) => {
        if (event.payload.success) {
          setStatus("completed");
          if (progress) {
            addHistory({
              file_name: progress.file_name,
              direction: progress.direction,
              size: progress.total_bytes,
              status: "completed",
              timestamp: Date.now(),
            });
          }
        } else {
          setStatus("failed");
        }
      }
    ).then((u) => {
      unlisteners.push(u);
    });

    return () => {
      unlisteners.forEach((u) => u());
    };
  }, [addHistory, progress]);

  return {
    status,
    progress,
    history,
    error,
    sendFiles,
    receiveFiles,
    cancelTransfer,
    clearError,
    clearHistory,
  };
}
