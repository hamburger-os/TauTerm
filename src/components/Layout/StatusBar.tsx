import { useState, useEffect, useMemo, useRef, type ReactNode, Fragment } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { useSession } from "../../context/SessionContext";
import { useCom0comStatus } from "../../hooks/useCom0comStatus";
import { pluginRegistry, type StatusBarContext, type StatusBarItem } from "../../core/plugin-registry";
import { charsetLabel, DEFAULT_ENCODING } from "../../utils/charsets";
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

/** 左区段：可空项表示不可见，统一排序后渲染 */
type LeftSegment = { key: string; priority: number; node: ReactNode } | null;

interface StatusBarProps {
  /** 更新阶段 */
  updatePhase: UpdatePhase;
  /** 最新版本号 */
  latestVersion?: string;
  /** 已下载字节数 */
  downloadedBytes?: number;
  /** 总字节数 */
  totalBytes?: number;
  /** 点击版本号区域回调（跳转到 About 页） */
  onVersionClick: () => void;
}

/**
 * 底部状态栏（多协议）
 *
 * 协议无关的基础段（连接状态、运行时间、数据模式、编码、TX/RX、日志）保留在本组件，
 * 协议专属段（串口参数/信号线、SSH 认证、网络 role/对端计数/报文计数）通过
 * `statusBarItems` 声明式注册，两者按 priority 统一排序，核心不感知具体协议。
 */
export default function StatusBar({
  updatePhase,
  latestVersion,
  downloadedBytes,
  totalBytes,
  onVersionClick,
}: StatusBarProps) {
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

  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";
  const isSerial = activeTab?.pluginId === "serial";
  const isSsh = activeTab?.pluginId === "ssh";
  const params = activeTab?.params as Record<string, unknown> | undefined;

  // 数据模式（使用 i18n 键确保语言切换时正确显示）
  const dataMode = params?.data_mode === "hex" ? t("serial.dataModeHex") : params?.data_mode === "dual" ? t("serial.dataModeDual") : t("serial.dataModeText");

  // 运行时间计时器
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

  // 实时速率（3s 滑动平均）：1s 采样 stats 的 delta，稳定不抖动
  const [rate, setRate] = useState({ tx: 0, rx: 0 });
  const statsRef = useRef({ tx: 0, rx: 0 });
  const lastSampleRef = useRef<{ tx: number; rx: number; ts: number } | null>(null);
  const windowRef = useRef<Array<{ tx: number; rx: number }>>([]);
  statsRef.current = { tx: activeTab?.stats.txBytes ?? 0, rx: activeTab?.stats.rxBytes ?? 0 };

  useEffect(() => {
    if (!activeTab || !isConnected) {
      lastSampleRef.current = null;
      windowRef.current = [];
      setRate({ tx: 0, rx: 0 });
      return;
    }
    const tick = () => {
      const now = Date.now();
      const cur = statsRef.current;
      const last = lastSampleRef.current;
      if (last) {
        const dt = (now - last.ts) / 1000;
        if (dt > 0.5) {
          const instTx = Math.max(0, (cur.tx - last.tx) / dt);
          const instRx = Math.max(0, (cur.rx - last.rx) / dt);
          const w = windowRef.current;
          w.push({ tx: instTx, rx: instRx });
          if (w.length > 3) w.shift();
          const tx = w.reduce((s, x) => s + x.tx, 0) / w.length;
          const rx = w.reduce((s, x) => s + x.rx, 0) / w.length;
          setRate({ tx, rx });
        }
      }
      lastSampleRef.current = { tx: cur.tx, rx: cur.rx, ts: now };
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [activeTab?.id, isConnected]);

  // 活跃插件的状态栏项（声明式描述符）
  const pluginItems: StatusBarItem[] = activeTab
    ? pluginRegistry.get(activeTab.pluginId)?.statusBarItems ?? []
    : [];
  const statusBarContext: StatusBarContext = { sessionId: activeTab?.id ?? "", activeTab: activeTab ?? null };

  const leftPluginSegments: LeftSegment[] = pluginItems
    .filter(item => item.align !== "right" && (item.when ? item.when(statusBarContext) : true))
    .map(item => ({
      key: item.id,
      priority: item.priority,
      node: <div className={styles.pluginItem}>{item.render(statusBarContext)}</div>,
    }));

  const rightPluginItems = pluginItems.filter(item => item.align === "right" && (item.when ? item.when(statusBarContext) : true));

  // 协议无关基础段 + 左对齐插件段，统一按 priority 降序（左→右）
  const leftSegments: LeftSegment[] = [
    // 连接状态
    {
      key: "indicator",
      priority: PRI.indicator,
      node: (
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
      ),
    },

    // 串口参数（仅串口会话显示）
    isConnected && isSerial && params
      ? {
          key: "serialParams",
          priority: PRI.serialParams,
          node: <div className={styles.segment}><span className={styles.paramText}>{formatPortParams(params)}</span></div>,
        }
      : null,

    // 硬件信号线（仅串口会话，当前显示未知，等待后端 API 接入真实信号状态）
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

    // 会话类型标签
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

    // 运行时间
    isConnected && uptime > 0
      ? {
          key: "uptime",
          priority: PRI.uptime,
          node: <div className={styles.segment}><span className={styles.uptimeText}><Icon name="stopwatch" size="sm" /> {formatUptime(uptime)}</span></div>,
        }
      : null,

    // 数据模式
    isConnected && params
      ? {
          key: "dataMode",
          priority: PRI.dataMode,
          node: <div className={styles.segment}><span className={styles.modeBadge}>{dataMode}</span></div>,
        }
      : null,

    // 字符编码（纯显示，连接后不可变）
    isConnected && params
      ? {
          key: "encoding",
          priority: PRI.encoding,
          node: (
            <div className={styles.segment}>
              <span className={styles.paramText}>
                {charsetLabel(typeof params.encoding === "string" ? params.encoding : DEFAULT_ENCODING)}
              </span>
            </div>
          ),
        }
      : null,

    // TX/RX 吞吐量 + 实时速率
    activeTab && isConnected
      ? {
          key: "stats",
          priority: PRI.stats,
          node: (
            <div className={styles.stats}>
              <span className={styles.statItem} title="TX"><Icon name="chevron-up" size="xs" /> {formatBytes(activeTab.stats.txBytes)} · {formatRate(rate.tx)}</span>
              <span className={styles.statItem} title="RX"><Icon name="chevron-down" size="xs" /> {formatBytes(activeTab.stats.rxBytes)} · {formatRate(rate.rx)}</span>
            </div>
          ),
        }
      : null,

    // 虚拟串口指示器
    activeTab && isConnected && isSerial && activeTab.virtualVirtualEndpoints && activeTab.virtualVirtualEndpoints.length > 0
      ? {
          key: "vport",
          priority: PRI.vport,
          node: (
            <div className={styles.segment}>
              <span className={styles.paramText}>
                VPort: {activeTab.virtualVirtualEndpoints.map(p => `${p.bridge_path}↔${p.external_path}`).join(", ")}
              </span>
            </div>
          ),
        }
      : null,

    // 虚拟串口失败警告
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

    // 全局驱动未安装警告（非会话级，持续显示）
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

    // 手动清理残留端口按钮（仅在检测到残留端口对时显示）
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

    // 日志状态指示器
    loggingSessions.size > 0
      ? {
          key: "log",
          priority: PRI.log,
          node: (
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
          ),
        }
      : null,

    // 左对齐插件段（网络 role 徽标 / 对端计数 / UDP 报文计数）
    ...leftPluginSegments,
  ];

  // 更新下载进度百分比
  const downloadPercent = useMemo(() => {
    if (updatePhase !== "downloading") return null;
    if (!totalBytes || totalBytes === 0) return null;
    const bytes = downloadedBytes ?? 0;
    return Math.min(Math.round((bytes / totalBytes) * 100), 99);
  }, [updatePhase, downloadedBytes, totalBytes]);

  // 版本号样式（下载速度暂不实现，仅展示百分比）
  const versionStyles = useMemo(() => {
    const base: React.CSSProperties = {};
    if (updatePhase === "available" || updatePhase === "ready") {
      base.color = "var(--color-success)";
      base.cursor = "pointer";
    }
    return base;
  }, [updatePhase]);

  const sortedLeftSegments = leftSegments
    .filter((s): s is NonNullable<LeftSegment> => s !== null)
    .sort((a, b) => b.priority - a.priority);

  return (
    <div className={`${styles.bar} liquid-glass`}>
      <div className={styles.left}>
        {sortedLeftSegments.map(seg => (
          <Fragment key={seg.key}>{seg.node}</Fragment>
        ))}
      </div>

      <div className={styles.right}>
        {/* 右对齐插件段（当前无插件注册，保留扩展点） */}
        {rightPluginItems
          .sort((a, b) => b.priority - a.priority)
          .map(item => (
            <div key={item.id} className={styles.pluginItem}>{item.render(statusBarContext)}</div>
          ))}
        {/* 下载进度 */}
        {updatePhase === "downloading" && downloadPercent !== null && (
          <span className={styles.downloadProgress}>
            <Icon name="chevron-down" size="xs" />
            {" "}{downloadPercent}%
          </span>
        )}
        {/* 版本号（可点击，有更新时变色+闪烁） */}
        {appVersion && (
          <span
            className={`${styles.version} ${
              updatePhase === "available" ? styles.versionHasUpdate :
              updatePhase === "ready" ? styles.versionReady :
              updatePhase === "checking" ? styles.versionChecking :
              ""
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
