import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  TransferDirection,
  TransferFinishedPayload,
  TransferStartAck,
} from "../types/transfer";

export type TransferRequest = Record<string, unknown>;

function commandFor(direction: TransferDirection): "file_transfer_send" | "file_transfer_receive" {
  return direction === "send" ? "file_transfer_send" : "file_transfer_receive";
}

/**
 * 协议无关的传输启动入口。
 *
 * 后端命令只负责校验/注册任务并返回 transfer_id；任务终态统一由
 * file-transfer:finished 事件表达。调用方不应从 invoke resolve 推断“传输完成”。
 */
export async function startFileTransfer(
  direction: TransferDirection,
  request: TransferRequest,
): Promise<TransferStartAck> {
  return invoke<TransferStartAck>(commandFor(direction), { request });
}

/** 精确取消某个 transfer_id，避免迟到 UI 操作误伤同 Session 的后续任务。 */
export async function cancelFileTransfer(
  sessionId: string,
  transferId: string,
): Promise<void> {
  await invoke("file_transfer_cancel", { sessionId, transferId });
}

/**
 * 启动传输并等待其精确终态。
 *
 * finished 监听器必须先于启动命令注册：极小文件可能在启动 invoke 返回 ack 前
 * 已经完成。此处按 transfer_id 缓冲早到事件，集中消除所有调用方的竞态处理。
 */
export async function startFileTransferAndWait(
  direction: TransferDirection,
  request: TransferRequest,
  sessionId: string,
  protocol?: string,
): Promise<TransferFinishedPayload> {
  let activeTransferId: string | null = null;
  const bufferedFinished = new Map<string, TransferFinishedPayload>();
  let unlisten: (() => void) | undefined;
  let settled = false;
  let resolveFinished!: (payload: TransferFinishedPayload) => void;
  let rejectFinished!: (error: Error) => void;

  const finished = new Promise<TransferFinishedPayload>((resolve, reject) => {
    resolveFinished = resolve;
    rejectFinished = reject;
  });

  const settle = (payload: TransferFinishedPayload) => {
    if (settled) return;
    settled = true;
    if (payload.success) {
      resolveFinished(payload);
      return;
    }
    rejectFinished(new Error(payload.error || (payload.cancelled ? "Transfer cancelled" : "Transfer failed")));
  };

  try {
    unlisten = await listen<TransferFinishedPayload>("file-transfer:finished", (event) => {
      const payload = event.payload;
      if (payload.session_id !== sessionId) return;
      if (protocol && payload.protocol && payload.protocol !== protocol) return;
      if (!payload.transfer_id) return;

      if (!activeTransferId) {
        bufferedFinished.set(payload.transfer_id, payload);
        return;
      }
      if (payload.transfer_id !== activeTransferId) return;
      settle(payload);
    });

    const ack = await startFileTransfer(direction, request);
    activeTransferId = ack.transfer_id;
    const early = bufferedFinished.get(activeTransferId);
    if (early) settle(early);
    return await finished;
  } finally {
    unlisten?.();
  }
}
