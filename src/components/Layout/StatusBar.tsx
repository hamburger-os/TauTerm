import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import { formatBytes, formatUptime, formatPortParams } from "../../utils/format";
import Icon from "../common/Icon";
import styles from "./StatusBar.module.css";

/**
 * 底部状态栏（增强版）
 *
 * 显示连接状态、端口参数、硬件信号线（当前显示未知状态，
 * 等待后端添加信号线读取 API 后接入真实数据）、运行时间、
 * 数据模式、Tx/Rx 实时速率、插件状态项。
 */
export default function StatusBar() {
  const { t } = useTranslation();
  const { state } = useSession();
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  // 应用版本（从 tauri.conf.json 动态读取）
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    getVersion().then(v => setAppVersion(`v${v}`)).catch(() => setAppVersion(""));
  }, []);

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
  const params = activeTab?.params as Record<string, unknown> | undefined;

  // 数据模式
  const dataMode = params?.data_mode === "hex" ? "HEX" : "Text";

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
            {isConnected ? activeTab?.endpoint : t("serial.disconnected")}
          </span>
        </div>

        {/* 串口参数 */}
        {isConnected && params && (
          <div className={styles.segment}>
            <span className={styles.paramText}>{formatPortParams(params)}</span>
          </div>
        )}

        {/* 硬件信号线（当前显示未知，等待后端 API 接入真实信号状态） */}
        {isConnected && (
          <div className={styles.segment}>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="DTR — 等待后端 API">DTR --</span>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="RTS — 等待后端 API">RTS --</span>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="CTS — 等待后端 API">CTS --</span>
            <span className={`${styles.signalDot} ${styles.signalUnknown}`} title="DSR — 等待后端 API">DSR --</span>
          </div>
        )}

        {/* 运行时间 */}
        {isConnected && uptime > 0 && (
          <div className={styles.segment}>
            <span className={styles.uptimeText}><Icon name="stopwatch" size="xs" /> {formatUptime(uptime)}</span>
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
