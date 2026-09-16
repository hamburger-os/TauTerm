/**
 * iperf 服务端面板
 *
 * 服务端生命周期跟随会话（对齐 TFTP）：连接自动启动、断开自动停止，
 * 监听参数（IP/端口/版本）在连接对话框中配置——本面板仅展示状态。
 */
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import IperfCommandPreview from "./IperfCommandPreview";
import { buildIperfCommand } from "./iperf-utils";
import type { IperfVersionStr } from "./iperf-events";
import styles from "./IperfSessionView.module.css";

interface Props {
  version: IperfVersionStr;
  serverRunning: boolean;
  listenAddr: string;
  listenIp: string;
  listenPort: number;
  serverError: string | null;
}

export default function IperfServerPanel({
  version,
  serverRunning,
  listenAddr,
  listenIp,
  listenPort,
  serverError,
}: Props) {
  const { t } = useTranslation();

  const command = buildIperfCommand({
    version,
    role: "server",
    targetHost: "",
    listenPort,
    protocol: "tcp",
    durationSecs: 10,
    port: listenPort,
    parallelStreams: 1,
    reportIntervalSecs: 1,
    bandwidthBps: null,
    bidirectional: false,
    tradeoff: false,
    windowSize: null,
    reverse: false,
    bidir: false,
    omitSecs: 0,
  });

  return (
    <div className={`${styles.panel} liquid-glass-card`}>
      <div className={styles.panelHeader}>
        <h3>{t("iperf.server")}</h3>
        <span
          className={serverRunning ? styles.statusRunning : styles.statusStopped}
        >
          {serverRunning ? (
            <><Icon name="status-connected" size="sm" /> {t("iperf.serverRunning")}</>
          ) : (
            <><Icon name="status-idle" size="sm" /> {t("iperf.serverStopped")}</>
          )}
        </span>
      </div>
      {/* 启动失败错误（如端口被占用）——可见，不静默 */}
      {serverError && <div className={styles.statusFailed}>{serverError}</div>}

      {/* 监听地址（连接对话框配置，会话内只读） */}
      <div className={styles.row2}>
        <div className={styles.field}>
          <label>{t("iperf.listenAddr")}</label>
          <span className={styles.listenAddrValue}>
            {listenAddr || `${listenIp}:${listenPort}`}
          </span>
        </div>
      </div>

      {/* 当前角色命令预览（提示性） */}
      <IperfCommandPreview command={command} />
    </div>
  );
}
