/**
 * TFTP 客户端面板
 *
 * GET/PUT 操作：远端地址、文件选择、传输触发
 */
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import type { TftpParams } from "./TftpSessionView";
import type { ClientFormState } from "./TftpSessionView";
import styles from "./TftpSessionView.module.css";

interface Props {
  sessionId: string;
  params: TftpParams;
  form: ClientFormState;
  onFormChange: (f: ClientFormState) => void;
  busy: boolean;
}

export default function TftpClientPanel({ sessionId, params, form, onFormChange, busy }: Props) {
  const { t } = useTranslation();

  const { remoteIp, remotePort, remoteFile, localPath } = form;
  const [selfBusy, setSelfBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isBusy = busy || selfBusy;

  const setRemoteIp = (v: string) => onFormChange({ ...form, remoteIp: v });
  const setRemotePort = (v: number) => onFormChange({ ...form, remotePort: v });
  const setRemoteFile = (v: string) => onFormChange({ ...form, remoteFile: v });
  const setLocalPath = (v: string) => onFormChange({ ...form, localPath: v });

  const pickFile = async () => {
    const selected = await open({ multiple: false });
    if (selected) {
      const path = Array.isArray(selected) ? selected[0] : selected;
      setLocalPath(path);
      // 仅设置本地路径，不自动填充远端文件名（两者通常不同）
    }
  };

  const handleGet = async () => {
    if (!remoteIp || !remoteFile || !localPath) return;
    setSelfBusy(true);
    setError(null);
    console.log("[TFTP] invoking tftp_client_get:", { sessionId, remoteIp, remotePort, remoteFile, localPath });
    try {
      const transferId = await invoke<string>("tftp_client_get", {
        sessionId,
        remoteIp,
        remotePort,
        remoteFilename: remoteFile,
        localPath,
        params,
      });
      console.log("[TFTP] GET 已启动, transfer_id:", transferId);
    } catch (e) {
      const msg = String(e);
      console.error("TFTP GET 失败:", e);
      setError(msg);
    } finally {
      setSelfBusy(false);
    }
  };

  const handlePut = async () => {
    if (!remoteIp || !remoteFile || !localPath) return;
    setSelfBusy(true);
    setError(null);
    console.log("[TFTP] invoking tftp_client_put:", { sessionId, remoteIp, remotePort, remoteFile, localPath });
    try {
      const transferId = await invoke<string>("tftp_client_put", {
        sessionId,
        remoteIp,
        remotePort,
        remoteFilename: remoteFile,
        localPath,
        params,
      });
      console.log("[TFTP] PUT 已启动, transfer_id:", transferId);
    } catch (e) {
      const msg = String(e);
      console.error("TFTP PUT 失败:", e);
      setError(msg);
    } finally {
      setSelfBusy(false);
    }
  };

  return (
    <div className={`${styles.panel} liquid-glass-card`}>
      <h3>{t("tftp.client")}</h3>
      <div className={styles.row2}>
        <div className={styles.field}>
          <label>{t("tftp.remoteIp")}</label>
          <input
            type="text"
            placeholder="192.168.1.1"
            value={remoteIp}
            onChange={(e) => setRemoteIp(e.target.value)}
            className="liquid-glass-input"
          />
        </div>
        <div className={styles.field}>
          <label>{t("tftp.remotePort")}</label>
          <input
            type="number"
            min={1}
            max={65535}
            value={remotePort}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (!isNaN(n) && n >= 1 && n <= 65535) setRemotePort(n);
            }}
            className="liquid-glass-input"
          />
        </div>
      </div>
      <div className={styles.field}>
        <label>{t("tftp.localPath")}</label>
        <div className={styles.fileRow}>
          <input
            type="text"
            placeholder={t("tftp.localPath") + "..."}
            value={localPath}
            onChange={(e) => setLocalPath(e.target.value)}
            className="liquid-glass-input"
          />
          <button onClick={pickFile} className={`liquid-glass-button ${styles.browseBtn}`} title={t("tftp.browse")}>
            <Icon name="folder" size="sm" />
          </button>
        </div>
      </div>
      <div className={styles.field}>
        <label>{t("tftp.remoteFile")}</label>
        <input
          type="text"
          placeholder="filename.bin"
          value={remoteFile}
          onChange={(e) => setRemoteFile(e.target.value)}
          className="liquid-glass-input"
        />
      </div>
      <div className={styles.actions}>
        <button
          onClick={handleGet}
          disabled={isBusy || !remoteIp.trim() || !remoteFile.trim() || !localPath.trim()}
          className="liquid-primary-button"
        >
          {t("tftp.get")}
        </button>
        <button
          onClick={handlePut}
          disabled={isBusy || !remoteIp.trim() || !remoteFile.trim() || !localPath.trim()}
          className="liquid-primary-button"
        >
          {t("tftp.put")}
        </button>
      </div>
      {error && (
        <div className={styles.tfErrorBox}>{error}</div>
      )}
    </div>
  );
}