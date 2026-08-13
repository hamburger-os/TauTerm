/**
 * TFTP Session 主视图
 *
 * 左右布局：左列（传输参数 + 服务端 + 客户端）+ 右列（传输列表）
 * content_type: "custom" → CustomRenderer → TftpSessionView
 */
import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { usePluginSessionStore, type SessionStoreApi } from "../../hooks/usePluginSessionStore";
import TftpClientPanel from "./TftpClientPanel";
import TftpServerPanel from "./TftpServerPanel";
import TftpTransferList from "./TftpTransferList";
import styles from "./TftpSessionView.module.css";

export interface TransferState {
  id: string;
  filename: string;
  remoteAddr: string;
  direction: "download" | "upload";
  bytesTransferred: number;
  totalBytes: number;
  bytesPerSecond: number;
  avgBytesPerSecond: number;
  status: "pending" | "transferring" | "completed" | "failed" | "cancelled";
  error?: string;
  checksum?: string;
  isServer: boolean;
}

export interface PendingRequest {
  id: string;
  remote_addr: string;
  filename: string;
  is_write: boolean;
  file_size?: number;
}

export interface TftpParams {
  blksize: number;
  timeout_secs: number;
  windowsize: number;
  max_retries: number;
  rollover: string;
  window_wait: number;
  clean_on_error: boolean;
  repeat_count: number;
}

export interface ClientFormState {
  remoteIp: string;
  remotePort: number;
  remoteFile: string;
  localPath: string;
}

interface Props {
  sessionId: string;
}

// ═══════════════════════════════════════════════════════════════════
// 会话级持久状态（usePluginSessionStore，keepAlive：监听器存活于进程
// 生命周期——组件卸载后传输在后台持续进行，done 事件永不丢失；
// 断连时仅清空状态、不注销监听器，重连复用（修复旧实现"断连删
// inited 后重复注册监听器"的缺陷）
// ═══════════════════════════════════════════════════════════════════

interface CachedState {
  transfers: TransferState[];
  serverRunning: boolean;
  fileRoot: string;
  listenAddr: string;
  params: TftpParams;
  clientForm: ClientFormState;
}

function createState(): CachedState {
  return {
    transfers: [],
    serverRunning: false,
    fileRoot: "",
    listenAddr: "",
    params: { blksize: 512, timeout_secs: 5, windowsize: 1, max_retries: 6, rollover: "Enforce0", window_wait: 0, clean_on_error: true, repeat_count: 1 },
    clientForm: { remoteIp: "", remotePort: 69, remoteFile: "", localPath: "" },
  };
}

async function initListeners(
  sessionId: string,
  api: SessionStoreApi<CachedState>
) {
  // ── 事件匹配：按 transfer_id 精确查找 ──
  // 每个传输（客户端 GET/PUT 或服务端 RRQ/WRQ）使用全局唯一 transfer_id。
  // 客户端和服务端事件各自创建独立条目（isServer 区分），不再合并。
  function findMatch(s: CachedState, transferId: string): number {
    return s.transfers.findIndex((t) => t.id === transferId);
  }

  const unProgress = await listen("tftp-transfer-progress", (event: any) => {
    const e = event.payload as any;
    if (e.session_id !== sessionId) return;
    const tid = e.transfer_id;
    if (!tid) return;
    const s = api.getState();
    const ix = findMatch(s, tid);
    const entry: TransferState = {
      id: tid,
      filename: e.filename,
      remoteAddr: e.remote_addr || "",
      direction: e.direction || (e.is_server ? "upload" : "download"),
      bytesTransferred: e.bytes_transferred,
      totalBytes: e.total_bytes,
      bytesPerSecond: e.bytes_per_second ?? 0,
      avgBytesPerSecond: ix >= 0 ? s.transfers[ix].avgBytesPerSecond : 0,
      status: "transferring",
      isServer: e.is_server ?? false,
    };
    const transfers = ix >= 0
      ? s.transfers.map((t, i) => i === ix ? entry : t)
      : [...s.transfers, entry];
    api.setState({ transfers });
  });

  const unDone = await listen("tftp-transfer-done", (event: any) => {
    const e = event.payload as any;
    if (e.session_id !== sessionId) return;
    const tid = e.transfer_id;
    if (!tid) return;
    const s = api.getState();
    const ix = findMatch(s, tid);
    if (ix < 0) return; // 无匹配条目则忽略
    const transfers = s.transfers.map((t, i) =>
      i === ix
        ? { ...t, status: e.success ? "completed" as const : "failed" as const, error: e.error, checksum: e.checksum, avgBytesPerSecond: e.avg_bytes_per_second ?? 0 }
        : t
    );
    api.setState({ transfers });
  });

  const unStatus = await listen("tftp-server-status", (event: any) => {
    const e = event.payload as any;
    if (e.session_id !== sessionId) return;
    api.setState({ serverRunning: e.running });
  });

  const unConnected = await listen("session-connected", (event: any) => {
    const e = event.payload as any;
    if (e.session_id !== sessionId) return;
    const s = api.getState();
    const p = e.params || {};
    api.setState({
      fileRoot: p.file_root || s.fileRoot,
      listenAddr: p.listen_ip ? `${p.listen_ip}:${p.listen_port ?? 69}` : s.listenAddr,
    });
  });

  // session-disconnected 由 hook 的全局监听按会话分发（keepAlive：
  // 不注销监听器，仅由 onSessionDisconnected 清空状态）
  return [unProgress, unDone, unStatus, unConnected];
}

// ═══════════════════════════════════════════════════════════════════
// 组件
// ═══════════════════════════════════════════════════════════════════

export default function TftpSessionView({ sessionId }: Props) {
  const { t } = useTranslation();

  const { state: snap, api } = usePluginSessionStore<CachedState>(sessionId, {
    createState,
    init: (api) => initListeners(sessionId, api),
    keepAlive: true,
    onSessionDisconnected: () => ({
      // 断连清空会话状态（参数/表单为本地 React 状态，经回写效果保留）
      transfers: [],
      serverRunning: false,
      fileRoot: "",
      listenAddr: "",
    }),
    getStatus: async (sid, api) => {
      try {
        const status = await invoke<any>("tftp_get_status", { sessionId: sid });
        const s = api.getState();
        return {
          fileRoot: status.file_root || "",
          listenAddr: (status.listen_addr != null && status.listen_port != null)
            ? `${status.listen_addr}:${status.listen_port}` : "",
          serverRunning: status.server_running ?? s.serverRunning,
        };
      } catch {
        return undefined;
      }
    },
  });

  const transfers = snap.transfers;
  const serverRunning = snap.serverRunning;
  const fileRoot = snap.fileRoot;
  const listenAddr = snap.listenAddr;
  const busy = transfers.some((t) => t.status === "transferring");

  const [pendingRequests, _setPendingRequests] = useState<PendingRequest[]>([]);
  const [params, setParams] = useState<TftpParams>(snap.params);
  const [clientForm, setClientForm] = useState<ClientFormState>(snap.clientForm);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout>>();

  // 本地专属状态回写缓存
  useEffect(() => {
    api.setState({ params, clientForm });
  }, [sessionId, params, clientForm, api]);

  useEffect(() => {
    return () => {
      if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current);
    };
  }, []);

  // 更新参数（防抖 500ms）
  const updateParam = useCallback(
    (newParams: typeof params) => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(async () => {
        try {
          await invoke("tftp_update_params", { sessionId, params: newParams });
        } catch (e) {
          console.error("TFTP 参数更新失败:", e);
        }
      }, 500);
    },
    [sessionId]
  );

  const handleParamChange = (key: string, value: any) => {
    if (typeof value === "number" && isNaN(value)) return;
    setParams((prev) => {
      const next = { ...prev, [key]: value };
      updateParam(next);
      return next;
    });
  };

  return (
    <div className={styles.container}>
      {/* 左列：传输参数 + 服务端 + 客户端 */}
      <div className={styles.leftColumn}>
        {/* 传输参数 —— 4×4 网格 */}
        <div className={`${styles.paramsCard} liquid-glass-card`}>
          <h3>{t("tftp.transferParams")}</h3>
          <div className={styles.paramGrid}>
            {/* 行1 */}
            <span className={styles.paramLabel}>{t("tftp.blksize")}</span>
            <select className="liquid-glass-input liquid-glass-select"
              value={params.blksize} onChange={(e) => handleParamChange("blksize", Number(e.target.value))}>
              <option value={512}>512</option>
              <option value={1024}>1024</option>
              <option value={1428}>1428</option>
              <option value={2048}>2048</option>
              <option value={4096}>4096</option>
              <option value={8192}>8192</option>
              <option value={16384}>16384</option>
              <option value={32768}>32768</option>
              <option value={65464}>65464</option>
            </select>
            <span className={styles.paramLabel}>{t("tftp.timeout")}</span>
            <input type="number" className="liquid-glass-input" min={1} max={255}
              value={params.timeout_secs} onChange={(e) => handleParamChange("timeout_secs", Number(e.target.value))} />
            {/* 行2 */}
            <span className={styles.paramLabel}>{t("tftp.windowsize")}</span>
            <select className="liquid-glass-input liquid-glass-select"
              value={params.windowsize} onChange={(e) => handleParamChange("windowsize", Number(e.target.value))}>
              <option value={1}>1</option>
              <option value={2}>2</option>
              <option value={4}>4</option>
              <option value={8}>8</option>
              <option value={16}>16</option>
            </select>
            <span className={styles.paramLabel}>{t("tftp.windowWait")}</span>
            <input type="number" className="liquid-glass-input" min={0}
              value={params.window_wait} onChange={(e) => handleParamChange("window_wait", Number(e.target.value))} />
            {/* 行3 */}
            <span className={styles.paramLabel}>{t("tftp.maxRetries")}</span>
            <input type="number" className="liquid-glass-input" min={1} max={255}
              value={params.max_retries} onChange={(e) => handleParamChange("max_retries", Number(e.target.value))} />
            <span className={styles.paramLabel}>{t("tftp.repeatCount")}</span>
            <input type="number" className="liquid-glass-input" min={1} max={4}
              value={params.repeat_count} onChange={(e) => handleParamChange("repeat_count", Number(e.target.value))} />
            {/* 行4 */}
            <span className={styles.paramLabel}>{t("tftp.rollover")}</span>
            <select className="liquid-glass-input liquid-glass-select"
              value={params.rollover} onChange={(e) => handleParamChange("rollover", e.target.value)}>
              <option value="Enforce0">{t("tftp.rolloverEnforce0")}</option>
              <option value="Enforce1">{t("tftp.rolloverEnforce1")}</option>
              <option value="None">{t("tftp.rolloverNone")}</option>
              <option value="DontCare">{t("tftp.rolloverDontCare")}</option>
            </select>
            <span className={styles.paramLabel}>{t("tftp.cleanOnError")}</span>
            <div className={styles.paramToggle}>
              <label className="liquid-glass-toggle">
                <input type="checkbox" checked={params.clean_on_error}
                  onChange={(e) => handleParamChange("clean_on_error", e.target.checked)} />
                <div />
              </label>
            </div>
          </div>
        </div>

        {/* 服务端 */}
        <TftpServerPanel
          sessionId={sessionId}
          serverRunning={serverRunning}
          fileRoot={fileRoot}
          listenAddr={listenAddr}
          pendingRequests={pendingRequests}
        />

        {/* 客户端 */}
        <TftpClientPanel sessionId={sessionId} params={params} form={clientForm} onFormChange={setClientForm} busy={busy} />
      </div>

      {/* 右列：传输列表 */}
      <div className={styles.rightColumn}>
        <TftpTransferList transfers={transfers} />
      </div>
    </div>
  );
}
