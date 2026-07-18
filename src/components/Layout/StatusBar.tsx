import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { useSession } from "../../context/SessionContext";
import { useCom0comStatus } from "../../hooks/useCom0comStatus";
import { pluginRegistry } from "../../core/plugin-registry";
import { formatBytes, formatUptime, formatPortParams } from "../../utils/format";
import Icon from "../common/Icon";
import styles from "./StatusBar.module.css";

/**
 * 底部状态栏（多协议）
 *
 * 根据活跃会话的 pluginId（"serial" | "ssh"）条件渲染不同的状态信息：
 * - 串口：端口参数（波特率/数据位/校验位/流控）、硬件信号线、虚拟串口
 * - SSH：user@host:port、认证方式、文件服务状态
 * - 通用：连接状态、运行时间、数据模式、Tx/Rx 实时速率、日志状态
 */
export default function StatusBar() {
  const { t } = useTranslation();
  const { state, loggingSessions, logStatuses } = useSession();
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  // 应用版本（从 tauri.conf.json 动态读取）
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    getVersion().then(v => setAppVersion(`v${v}`)).catch(() => setAppVersion(""));
  }, []);

  // com0com 驱动全局状态（提取为独立 hook）
  const {
    driverMissing,
    driverInstalling,
    cleaningPorts,
    orphanCount,
    handleRetryVPort,
    handleCleanupVPorts,
  } = useCom0comStatus();

  // 运行时间计时器
  const [uptime, setUptime] = useState(0);

  useEffect(() => {
    if (!activeTab || (activeTab.state !== "connected" && activeTab.state !== "transferring") || !activeTab.connectedAt) {
      setUptime(0);
      return;
    }
    const tick = () => {
      setUptime(Math.floor((Date.now() - activeTab.connectedAt!) / 1000));
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [activeTab?.connectedAt, activeTab?.state, activeTab?.id]);

  // 获取活跃插件的状态栏项
  const pluginStatusItems = activeTab
    ? pluginRegistry.get(activeTab.pluginId)?.statusBarItems ?? []
    : [];

  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";
  const isSerial = activeTab?.pluginId === "serial";
  const isSsh = activeTab?.pluginId === "ssh";
  const params = activeTab?.params as Record<string, unknown> | undefined;

  // 数据模式（使用 i18n 键确保语言切换时正确显示）
  const dataMode = params?.data_mode === "hex" ? t("serial.dataModeHex") : params?.data_mode === "dual" ? t("serial.dataModeDual") : t("serial.dataModeText");

  return (
    <div className={`${styles.bar} liquid-glass`}>
      <div className={styles.left}>
        {/* 连接状态 */}
        <div className={styles.indicator}>
          <span className={`${styles.dot} ${
            activeTab?.state === "connected" ? styles.connected :
            activeTab?.state === "transferring" ? styles.transferring : ""
          }`} />
          <span className={styles.text}>
            {isConnected
              ? (isSsh
                  ? `${params?.username ?? ""}@${activeTab?.endpoint}:${params?.port ?? 22}`
                  : activeTab?.endpoint)
              : t("statusBar.disconnected")}
          </span>
        </div>

        {/* 串口参数（仅串口会话显示） */}
        {isConnected && isSerial && params && (
          <div className={styles.segment}>
            <span className={styles.paramText}>{formatPortParams(params)}</span>
          </div>
        )}

        {/* 硬件信号线（仅串口会话，当前显示未知，等待后端 API 接入真实信号状态） */}
        {isConnected && isSerial && (
          <div className={styles.segment}>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="DTR — 等待后端 API">DTR --</span>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="RTS — 等待后端 API">RTS --</span>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="CTS — 等待后端 API">CTS --</span>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="DSR — 等待后端 API">DSR --</span>
          </div>
        )}

        {/* 会话类型标签 */}
        {isConnected && isSerial && (
          <div className={styles.segment}>
            <span className={styles.typeBadge}>{t("statusBar.typeSerial")}</span>
          </div>
        )}
        {isConnected && isSsh && (
          <div className={styles.segment}>
            <span className={styles.typeBadge}>{t("statusBar.typeSsh")}</span>
            <span className={styles.sshAuthBadge}>
              {params?.auth_method === "key"
                ? t("statusBar.authKey")
                : t("statusBar.authPassword")}
            </span>
            {params?.file_service_enabled === true && (
              <span className={styles.sshFsBadge}>
                {String(params.file_service_protocol ?? "sftp").toUpperCase()}
              </span>
            )}
          </div>
        )}

        {/* 运行时间 */}
        {isConnected && uptime > 0 && (
          <div className={styles.segment}>
            <span className={styles.uptimeText}><Icon name="stopwatch" size="sm" /> {formatUptime(uptime)}</span>
          </div>
        )}

        {/* 数据模式 */}
        {isConnected && params && (
          <div className={styles.segment}>
            <span className={styles.modeBadge}>{dataMode}</span>
          </div>
        )}

        {/* TX/RX 吞吐量 */}
        {activeTab && isConnected && (
          <div className={styles.stats}>
            <span className={styles.statItem} title="TX"><Icon name="chevron-up" size="xs" /> {formatBytes(activeTab.stats.txBytes)}</span>
            <span className={styles.statItem} title="RX"><Icon name="chevron-down" size="xs" /> {formatBytes(activeTab.stats.rxBytes)}</span>
          </div>
        )}

        {/* 虚拟串口指示器 */}
        {activeTab && isConnected && isSerial && activeTab.virtualPortPairs && activeTab.virtualPortPairs.length > 0 && (
          <div className={styles.segment}>
            <span className={styles.paramText}>
              VPort: {activeTab.virtualPortPairs.map(p => `${p.port_a}↔${p.port_b}`).join(", ")}
            </span>
          </div>
        )}

        {/* 虚拟串口失败警告 */}
        {activeTab && isConnected && isSerial && activeTab.virtualPortError && (
          <div className={styles.segment}>
            <span className={`${styles.paramText} ${styles.vportWarning}`}>
              <Icon name="warning" size="xs" /> {activeTab.virtualPortError}
            </span>
            <span
              className={`${styles.paramText} ${styles.vportAction}`}
              style={{ opacity: driverInstalling ? 0.5 : 1 }}
              onClick={() => !driverInstalling && handleRetryVPort()}
              title={t("serial.virtualPort.retryHint")}
            >
              [{driverInstalling ? t("serial.virtualPort.installing") : t("serial.virtualPort.retry")}]
            </span>
          </div>
        )}

        {/* 全局驱动未安装警告（非会话级，持续显示） */}
        {isSerial && driverMissing && !(activeTab && isConnected && activeTab.virtualPortError) && (
          <div className={styles.segment} title={t("serial.virtualPort.retryHint")}>
            <span className={`${styles.paramText} ${styles.vportWarning}`}>
              <Icon name="warning" size="xs" /> {t("serial.virtualPort.notInstalled")}
            </span>
            <span
              className={`${styles.paramText} ${styles.vportAction}`}
              style={{ opacity: driverInstalling ? 0.5 : 1 }}
              onClick={() => !driverInstalling && handleRetryVPort()}
              title={t("serial.virtualPort.retryHint")}
            >
              [{driverInstalling ? t("serial.virtualPort.installing") : t("serial.virtualPort.retry")}]
            </span>
          </div>
        )}

        {/* 手动清理残留端口按钮（仅在检测到残留端口对时显示） */}
        {isSerial && orphanCount > 0 && (
          <div className={styles.segment} title={t("serial.virtualPort.cleanupHint")}>
            <span className={`${styles.paramText} ${styles.vportWarning}`}>
              <Icon name="warning" size="xs" /> VPort {orphanCount} {t("serial.virtualPort.orphansDetected")}
            </span>
            <span
              className={`${styles.paramText} ${styles.vportAction}`}
              style={{ opacity: cleaningPorts ? 0.5 : 1 }}
              onClick={() => !cleaningPorts && handleCleanupVPorts()}
              title={t("serial.virtualPort.cleanupHint")}
            >
              [{cleaningPorts ? (t("serial.virtualPort.cleaning") || "正在清理...") : (t("serial.virtualPort.cleanup") || "清理")}]
            </span>
          </div>
        )}

        {/* 日志状态指示器 */}
        {loggingSessions.size > 0 && (
          <div className={styles.segment}>
            <span className={styles.logDot} />
            <span className={styles.logText}>
              {Array.from(loggingSessions).map(sid => {
                const status = logStatuses.get(sid);
                if (!status) return null;
                return (
                  <span key={sid} className={styles.logFileInfo}>
                    {status.fileName} ({formatBytes(status.bytesWritten)})
                  </span>
                );
              })}
            </span>
          </div>
        )}

        {/* 插件状态项 */}
        {pluginStatusItems.map(item => (
          <div key={item.id} className={styles.pluginItem}>
            {item.render({ sessionId: activeTab?.id ?? "" })}
          </div>
        ))}
      </div>

      <div className={styles.right}>
        {appVersion && <span className={styles.version}>{appVersion}</span>}
      </div>
    </div>
  );
}
