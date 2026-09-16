/**
 * TFTP 服务端面板
 *
 * 连接即自动启动服务端；启停完全由左侧栏会话右键菜单管理。
 * 面板仅展示运行状态和待审批请求。
 */
import Icon from "../common/Icon";
import type { PendingRequest } from "./TftpSessionView";
import styles from "./TftpSessionView.module.css";
import { useTranslation } from "react-i18next";

interface Props {
  sessionId: string;
  serverRunning: boolean;
  fileRoot: string;
  listenAddr: string;
  pendingRequests: PendingRequest[];
}

export default function TftpServerPanel({
  sessionId: _sessionId,
  serverRunning,
  fileRoot,
  listenAddr,
  pendingRequests,
}: Props) {
  const { t } = useTranslation();

  return (
    <div className={`${styles.panel} liquid-glass-card`}>
      <h3>{t("tftp.server")}</h3>
      <div className={styles.status}>
        {t("tftp.status")}{" "}
        <span
          className={serverRunning ? styles.running : styles.stopped}
        >
          {serverRunning ? (
            <><Icon name="status-connected" size="sm" /> {t("tftp.serverRunning")}</>
          ) : (
            <><Icon name="status-idle" size="sm" /> {t("tftp.serverStopped")}</>
          )}
        </span>
      </div>
      <div className={styles.configItem}>
        <span className={styles.configLabel}>{t("tftp.listenAddr")}</span>
        <span className={styles.configValue}>{listenAddr || "—"}</span>
      </div>
      <div className={styles.configItem}>
        <span className={styles.configLabel}>{t("tftp.fileRoot")}</span>
        <span className={styles.configValue}>{fileRoot || "—"}</span>
      </div>
      {pendingRequests.length > 0 && (
        <div className={styles.requests}>
          <h4>{t("tftp.requests")}</h4>
          {pendingRequests.map((req) => (
            <div key={req.id} className={styles.requestItem}>
              <span>
                {req.remote_addr} → {req.is_write ? "PUT" : "GET"} {req.filename}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}