import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { resolveProfile } from "../../profiles";
import styles from "./BottomInfoPanel.module.css";

/**
 * 底部信息面板（重构版）
 *
 * 左右分栏 T 型布局：
 * - 左栏（~36%）：会话身份信息（名称、类型、端口、状态）
 * - 右栏（~64%）：协议参数（上）+ 运行时统计（下）
 *
 * 协议无关：通过 Profile 注册表解析不同连接类型的展示数据。
 */

/** 格式化字节数（自动选择 B/KB/MB） */
function formatBytes(bytes: number): string {
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

/** 格式化秒数为 HH:MM:SS */
function formatUptime(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** 运行时统计区 */
function StatsSection({
  txBytes,
  rxBytes,
  connectedAt,
  state,
}: {
  txBytes: number;
  rxBytes: number;
  connectedAt: number | null;
  state: string;
}) {
  const { t } = useTranslation();
  const [uptime, setUptime] = useState(0);

  useEffect(() => {
    if (state !== "connected" || !connectedAt) {
      setUptime(0);
      return;
    }

    const tick = () => {
      setUptime(Math.floor((Date.now() - connectedAt) / 1000));
    };
    tick(); // immediate
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [connectedAt, state]);

  const hasStats = state === "connected" || state === "transferring";

  return (
    <div className={styles.statsSection}>
      <div className={styles.statRow}>
        <span className={styles.statIcon}>⬆</span>
        <span className={styles.statLabel}>{t("stats.txBytes")}</span>
        <span className={styles.statValue}>{hasStats ? formatBytes(txBytes) : "--"}</span>
      </div>
      <div className={styles.statRow}>
        <span className={styles.statIcon}>⬇</span>
        <span className={styles.statLabel}>{t("stats.rxBytes")}</span>
        <span className={styles.statValue}>{hasStats ? formatBytes(rxBytes) : "--"}</span>
      </div>
      <div className={styles.statRow}>
        <span className={styles.statIcon}>⏱</span>
        <span className={styles.statLabel}>{t("stats.uptime")}</span>
        <span className={styles.statValue}>{uptime > 0 ? formatUptime(uptime) : "--"}</span>
      </div>
    </div>
  );
}

export default function BottomInfoPanel() {
  const { t } = useTranslation();
  const { state } = useSession();

  const activeTab = state.tabs.find((t) => t.id === state.activeTabId);

  if (!activeTab) {
    return (
      <div className={styles.panel}>
        <div className={styles.emptyState}>
          <span className={styles.emptyIcon}>⚡</span>
          <span>{t("bottomPanel.noSession") || "No active session"}</span>
        </div>
      </div>
    );
  }

  const profile = resolveProfile(activeTab);

  return (
    <div className={styles.panel}>
      {/* 左栏：身份信息 */}
      <div className={styles.identityColumn}>
        {profile.identity.map((item, i) => (
          <div key={i} className={styles.infoItem}>
            <span className={styles.label}>
              {item.icon && <span className={styles.itemIcon}>{item.icon}</span>}
              {t(item.label)}
            </span>
            <span
              className={`${styles.value} ${item.monospace ? styles.mono : ""}`}
            >
              {t(item.value, { defaultValue: item.value })}
            </span>
          </div>
        ))}
      </div>

      {/* 分隔线 */}
      <div className={styles.divider} />

      {/* 右栏：技术详情 */}
      <div className={styles.detailsColumn}>
        {/* 协议参数区 */}
        <div className={styles.paramsSection}>
          {profile.parameters.map((item, i) => (
            <div key={i} className={styles.paramItem}>
              <span className={styles.paramLabel}>{t(item.label)}</span>
              <span className={`${styles.paramValue} ${item.monospace ? styles.mono : ""}`}>
                {t(item.value, { defaultValue: item.value })}
              </span>
            </div>
          ))}
        </div>

        {/* 运行时统计区 */}
        <StatsSection
          txBytes={activeTab.stats?.txBytes ?? 0}
          rxBytes={activeTab.stats?.rxBytes ?? 0}
          connectedAt={activeTab.connectedAt}
          state={activeTab.state}
        />
      </div>
    </div>
  );
}
