import { useState, useEffect, useMemo, useRef, type ReactNode, Fragment } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { useSession } from "../../context/SessionContext";
import { useCom0comStatus } from "../../hooks/useCom0comStatus";
import { pluginRegistry, type StatusBarContext, type StatusBarItem } from "../../core/plugin-registry";
import { charsetLabel } from "../../utils/charsets";
import { formatBytes, formatUptime, formatPortParams, formatRate } from "../../utils/format";
import type { UpdatePhase } from "../../types/updater";
import Icon from "../common/Icon";
import styles from "./StatusBar.module.css";

/** 左区段优先级（数值越大越靠左，与 VS Code StatusBar 的 priority 语义一致） */
const PRI = {
  indicator: 1000,
  serialParams: 880,
  signalLines: 870,
  typeBadge: 860,
  uptime: 700,
  dataMode: 600,
  encoding: 500,
  stats: 400,
  vport: 300,
  log: 200,
} as const;

type LeftSegment = { key: string; priority: number; node: ReactNode } | null;

interface StatusBarProps {
  updatePhase: UpdatePhase;
  latestVersion?: string;
  downloadedBytes?: number;
  totalBytes?: number;
  onVersionClick: () => void;
}

export default function StatusBar({
  updatePhase,
  latestVersion,
  downloadedBytes,
  totalBytes,
  onVersionClick,
}: StatusBarProps) {
  const { t } = useTranslation();
  const { state, loggingSessions, logStatuses } = useSession();
  const activeTab = state.tabs.find(tab => tab.id === state.activeTabId);
  const activePlugin = activeTab ? pluginRegistry.get(activeTab.pluginId) : undefined;

  const [appVersion, setAppVersion] = useState("");
  useEffect(() => {
    getVersion().then(v => setAppVersion(`v${v}`)).catch(() => setAppVersion(""));
  }, []);

  const {
    driverMissing,
    driverInstalling,
    cleaningPorts,
    orphanCount,
    handleRetryVPort,
    handleCleanupVPorts,
  } = useCom0comStatus();

  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";
  const isSerial = activeTab?.pluginId === "serial";
  const isSsh = activeTab?.pluginId === "ssh";
  const params = activeTab?.params as Record<string, unknown> | undefined;
  const supportsStreamStatus = activePlugin?.manifest.content_type === "terminal" || activePlugin?.manifest.send_bar === true;
  const dataMode = params?.data_mode === "hex"
    ? t("serial.dataModeHex")
    : params?.data_mode === "dual"
      ? t("serial.dataModeDual")
      : params?.data_mode === "text"
        ? t("serial.dataModeText")
        : null;
  const encoding = typeof params?.encoding === "string" ? charsetLabel(params.encoding) : null;

  const [uptime, setUptime] = useState(0);
  useEffect(() => {
    if (!activeTab || !isConnected || !activeTab.connectedAt) {
      setUptime(0);
      return;
    }
    const tick = () => {
      setUptime(Math.floor((Date.now() - activeTab.connectedAt!) / 1000));
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [activeTab?.connectedAt, isConnected, activeTab?.id]);

  const [rate, setRate] = useState({ tx: 0, rx: 0 });
  const statsRef = useRef({ tx: 0, rx: 0 });
  const lastSampleRef = useRef<{ tx: number; rx: number; ts: number } | null>(null);
  const windowRef = useRef<Array<{ tx: number; rx: number }>>([]);
  statsRef.current = { tx: activeTab?.stats.txBytes ?? 0, rx: activeTab?.stats.rxBytes ?? 0 };

  useEffect(() => {
    if (!activeTab || !isConnected || !supportsStreamStatus) {
      lastSampleRef.current = null;
      windowRef.current = [];
      setRate({ tx: 0, rx: 0 });
      return;
    }
    const tick = () => {
      const now = Date.now();
      const current = statsRef.current;
      const last = lastSampleRef.current;
      if (last) {
        const dt = (now - last.ts) / 1000;
        if (dt > 0.5) {
          const tx = Math.max(0, (current.tx - last.tx) / dt);
          const rx = Math.max(0, (current.rx - last.rx) / dt);
          const window = windowRef.current;
          window.push({ tx, rx });
          if (window.length > 3) window.shift();
          setRate({
            tx: window.reduce((sum, item) => sum + item.tx, 0) / window.length,
            rx: window.reduce((sum, item) => sum + item.rx, 0) / window.length,
          });
        }
      }
      lastSampleRef.current = { tx: current.tx, rx: current.rx, ts: now };
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [activeTab?.id, isConnected, supportsStreamStatus]);

  const pluginItems: StatusBarItem[] = activePlugin?.statusBarItems ?? [];
  const statusBarContext: StatusBarContext = {
    sessionId: activeTab?.id ?? "",
    activeTab: activeTab ?? null,
  };
  const leftPluginSegments: LeftSegment[] = pluginItems
    .filter(item => item.align !== "right" && (item.when ? item.when(statusBarContext) : true))
    .map(item => ({
      key: item.id,
      priority: item.priority,
      node: <div className={styles.pluginItem}>{item.render(statusBarContext)}</div>,
    }));
  const rightPluginItems = pluginItems.filter(
    item => item.align === "right" && (item.when ? item.when(statusBarContext) : true)
  );

  const leftSegments: LeftSegment[] = [
    {
      key: "indicator",
      priority: PRI.indicator,
      node: (
        <div className={styles.indicator}>
          <span className={`${styles.dot} ${
            activeTab?.state === "connected"
              ? styles.connected
              : activeTab?.state === "transferring"
                ? styles.transferring
                : ""
          }`} />
          <span className={styles.text}>
            {isConnected
              ? (isSsh
                  ? `${params?.username ?? ""}@${activeTab?.endpoint}:${params?.port ?? 22}`
                  : activeTab?.endpoint)
              : t("statusBar.disconnected")}
          </span>
        </div>
      ),
    },
    isConnected && isSerial && params
      ? {
          key: "serialParams",
          priority: PRI.serialParams,
          node: <div className={styles.segment}><span className={styles.paramText}>{formatPortParams(params)}</span></div>,
        }
      : null,
    isConnected && isSerial
      ? {
          key: "signalLines",
          priority: PRI.signalLines,
          node: (
            <div className={styles.segment}>
              <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="DTR — 等待后端 API">DTR --</span>
              <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="RTS — 等待后端 API">RTS --</span>
              <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="CTS — 等待后端 API">CTS --</span>
              <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="DSR — 等待后端 API">DSR --</span>
            </div>
          ),
        }
      : null,
    isConnected && isSerial
      ? {
          key: "typeSerial",
          priority: PRI.typeBadge,
          node: <div className={styles.segment}><span className={styles.typeBadge}>{t("statusBar.typeSerial")}</span></div>,
        }
      : null,
    isConnected && isSsh
      ? {
          key: "typeSsh",
          priority: PRI.typeBadge,
          node: (
            <div className={styles.segment}>
              <span className={styles.typeBadge}>{t("statusBar.typeSsh")}</span>
              <span className={styles.sshAuthBadge}>
                {params?.auth_method === "key" ? t("statusBar.authKey") : t("statusBar.authPassword")}
              </span>
              {params?.file_service_enabled === true && (
                <span className={styles.sshFsBadge}>
                  {String(params.file_service_protocol ?? "sftp").toUpperCase()}
                </span>
              )}
            </div>
          ),
        }
      : null,
    isConnected && uptime > 0
      ? {
          key: "uptime",
          priority: PRI.uptime,
          node: <div className={styles.segment}><span className={styles.uptimeText}><Icon name="stopwatch" size="sm" /> {formatUptime(uptime)}</span></div>,
        }
      : null,
    isConnected && supportsStreamStatus && dataMode
      ? {
          key: "dataMode",
          priority: PRI.dataMode,
          node: <div className={styles.segment}><span className={styles.modeBadge}>{dataMode}</span></div>,
        }
      : null,
    isConnected && supportsStreamStatus && encoding
      ? {
          key: "encoding",
          priority: PRI.encoding,
          node: (
            <div className={styles.segment}>
              <span className={styles.paramText}>{encoding}</span>
            </div>
          ),
        }
      : null,
    activeTab && isConnected && supportsStreamStatus
      ? {
          key: "stats",
          priority: PRI.stats,
          node: (
            <div className={styles.stats}>
              <span className={styles.statItem} title="TX"><Icon name="arrow-up" size="xs" /> {formatBytes(activeTab.stats.txBytes)} · {formatRate(rate.tx)}</span>
              <span className={styles.statItem} title="RX"><Icon name="arrow-down" size="xs" /> {formatBytes(activeTab.stats.rxBytes)} · {formatRate(rate.rx)}</span>
            </div>
          ),
        }
      : null,
    activeTab && isConnected && isSerial && activeTab.virtualVirtualEndpoints && activeTab.virtualVirtualEndpoints.length > 0
      ? {
          key: "vport",
          priority: PRI.vport,
          node: (
            <div className={styles.segment}>
              <span className={styles.paramText}>
                VPort: {activeTab.virtualVirtualEndpoints
                  .map(endpoint => endpoint.external_path)
                  .filter(Boolean)
                  .join(", ")}
              </span>
            </div>
          ),
        }
      : null,
    activeTab && isConnected && isSerial && activeTab.virtualPortError
      ? {
          key: "vportError",
          priority: PRI.vport,
          node: (
            <div className={styles.segment} title={activeTab.virtualPortError}>
              <span className={`${styles.paramText} ${styles.vportWarning}`}>
                <Icon name="warning" size="xs" />
                {activeTab.virtualPortErrorKind === "files_missing"
                  ? t("serial.virtualPort.filesMissing")
                  : activeTab.virtualPortErrorKind === "permission"
                    ? t("serial.virtualPort.permissionRequired")
                    : activeTab.virtualPortErrorKind === "create_failed"
                      ? t("serial.virtualPort.createFailed")
                      : t("serial.virtualPort.notInstalled")}
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
          ),
        }
      : null,
    isSerial && driverMissing && !(activeTab && isConnected && activeTab.virtualPortError)
      ? {
          key: "driverMissing",
          priority: PRI.vport,
          node: (
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
          ),
        }
      : null,
    isSerial && orphanCount > 0
      ? {
          key: "orphans",
          priority: PRI.vport,
          node: (
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
          ),
        }
      : null,
    loggingSessions.size > 0
      ? {
          key: "log",
          priority: PRI.log,
          node: (
            <div className={styles.segment}>
              <span className={styles.logDot} />
              <span className={styles.logText}>
                {Array.from(loggingSessions).map(sessionId => {
                  const status = logStatuses.get(sessionId);
                  if (!status) return null;
                  return (
                    <span key={sessionId} className={styles.logFileInfo}>
                      {status.fileName} ({formatBytes(status.bytesWritten)})
                    </span>
                  );
                })}
              </span>
            </div>
          ),
        }
      : null,
    ...leftPluginSegments,
  ];

  const downloadPercent = useMemo(() => {
    if (updatePhase !== "downloading") return null;
    if (!totalBytes || totalBytes === 0) return null;
    const bytes = downloadedBytes ?? 0;
    return Math.min(Math.round((bytes / totalBytes) * 100), 99);
  }, [updatePhase, downloadedBytes, totalBytes]);

  const versionStyles = useMemo(() => {
    const base: React.CSSProperties = {};
    if (updatePhase === "available" || updatePhase === "ready") {
      base.color = "var(--color-success)";
      base.cursor = "pointer";
    }
    return base;
  }, [updatePhase]);

  const sortedLeftSegments = leftSegments
    .filter((segment): segment is NonNullable<LeftSegment> => segment !== null)
    .sort((a, b) => b.priority - a.priority);

  return (
    <div className={`${styles.bar} liquid-glass`}>
      <div className={styles.left}>
        {sortedLeftSegments.map(segment => (
          <Fragment key={segment.key}>{segment.node}</Fragment>
        ))}
      </div>

      <div className={styles.right}>
        {rightPluginItems
          .sort((a, b) => b.priority - a.priority)
          .map(item => (
            <div key={item.id} className={styles.pluginItem}>{item.render(statusBarContext)}</div>
          ))}
        {updatePhase === "downloading" && downloadPercent !== null && (
          <span className={styles.downloadProgress}>
            <Icon name="arrow-down" size="xs" />{" "}{downloadPercent}%
          </span>
        )}
        {appVersion && (
          <span
            className={`${styles.version} ${
              updatePhase === "available"
                ? styles.versionHasUpdate
                : updatePhase === "ready"
                  ? styles.versionReady
                  : updatePhase === "checking"
                    ? styles.versionChecking
                    : ""
            }`}
            style={versionStyles}
            onClick={onVersionClick}
            title={
              updatePhase === "available"
                ? t("statusBar.updateAvailable", { version: latestVersion ?? "" })
                : updatePhase === "ready"
                  ? t("updater.installAndRelaunch")
                  : updatePhase === "downloading"
                    ? t("updater.downloading")
                    : t("settings.about")
            }
          >
            {appVersion}
            {(updatePhase === "available" || updatePhase === "ready") && (
              <span className="liquid-glass-dot dot-success" />
            )}
          </span>
        )}
      </div>
    </div>
  );
}
