import { useRef, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import Icon from "../common/Icon";
import { useJournaldViewer } from "./hooks/useJournaldViewer";
import type { JournalEntry } from "./types";
import { LOG_LEVELS, priorityToLevelClass, formatTimestamp, formatTimestampTime, priorityLabel } from "./types";
import styles from "./JournaldViewerPanel.module.css";

interface JournaldViewerPanelProps {
  sessionId: string;
  isConnected: boolean;
}

export default function JournaldViewerPanel({
  sessionId,
  isConnected,
}: JournaldViewerPanelProps) {
  const { t } = useTranslation();
  const jvd = useJournaldViewer(sessionId, isConnected);
  const logListRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  // ── 自动滚动（实时模式）──
  useEffect(() => {
    if (jvd.subTab === "realtime" && autoScrollRef.current && logListRef.current) {
      logListRef.current.scrollTop = 0;
    }
  }, [jvd.entries, jvd.subTab]);

  const handleScroll = useCallback(() => {
    if (!logListRef.current) return;
    const el = logListRef.current;
    // 靠近顶部时启用自动滚动
    autoScrollRef.current = el.scrollTop < 40;
  }, []);

  // ── 历史查询加载更多 ──
  const handleLoadMore = useCallback(() => {
    jvd.queryHistory(true);
  }, [jvd]);

  // ── 过滤栏 ──
  const renderFilterBar = () => (
    <div className={`${styles.filterBar} liquid-glass-card`}>
      <div className={styles.filterRow}>
        <select
          className={`${styles.filterSelect} liquid-glass-input liquid-glass-select`}
          value={jvd.filter.level ?? ""}
          onChange={(e) =>
            jvd.setFilter({
              level: (e.target.value || null) as typeof jvd.filter.level,
            })
          }
        >
          <option value="">{t("journald.filterLevelAll")}</option>
          {LOG_LEVELS.map((l) => (
            <option key={l.value} value={l.value}>
              {t(`journald.level${l.value.charAt(0).toUpperCase() + l.value.slice(1)}`)}
            </option>
          ))}
        </select>
        <input
          className={`${styles.filterInput} liquid-glass-input`}
          type="text"
          placeholder={t("journald.filterKeyword") ?? "Keyword"}
          value={jvd.filter.keyword ?? ""}
          onChange={(e) => jvd.setFilter({ keyword: e.target.value || undefined })}
        />
      </div>
      <div className={styles.filterRow}>
        <input
          className={`${styles.filterInput} liquid-glass-input`}
          type="text"
          placeholder={t("journald.filterUnit") ?? "Service Unit"}
          value={jvd.filter.unit ?? ""}
          onChange={(e) => jvd.setFilter({ unit: e.target.value || undefined })}
        />
        <label className={`liquid-glass-toggle ${styles.filterCheckbox}`}>
          <input
            type="checkbox"
            checked={jvd.filter.kernelOnly ?? false}
            onChange={(e) => jvd.setFilter({ kernelOnly: e.target.checked })}
          />
          <div />
          <span className={styles.filterCheckboxLabel}>
            {t("journald.filterKernel")}
          </span>
        </label>
      </div>
      {jvd.subTab === "history" && (
        <div className={styles.filterRow}>
          <label className={styles.filterDateLabel}>
            <span className={styles.filterDateLabelText}>
              {t("journald.filterSince")}
            </span>
            <input
              className={`${styles.filterInput} liquid-glass-input`}
              type="datetime-local"
              value={jvd.filter.since ?? ""}
              onChange={(e) => jvd.setFilter({ since: e.target.value || null })}
            />
          </label>
          <label className={styles.filterDateLabel}>
            <span className={styles.filterDateLabelText}>
              {t("journald.filterUntil")}
            </span>
            <input
              className={`${styles.filterInput} liquid-glass-input`}
              type="datetime-local"
              value={jvd.filter.until ?? ""}
              onChange={(e) => jvd.setFilter({ until: e.target.value || null })}
            />
          </label>
        </div>
      )}
    </div>
  );

  // ── 工具栏 ──
  const renderToolbar = () => (
    <div className={styles.toolbar}>
      <div className={styles.modeToggle}>
        <button
          className={`${styles.modeBtn} liquid-glass-button ${
            jvd.displayMode === "compact" ? "active" : ""
          }`}
          onClick={() => jvd.setDisplayMode("compact")}
        >
          {t("journald.displayCompact")}
        </button>
        <button
          className={`${styles.modeBtn} liquid-glass-button ${
            jvd.displayMode === "full" ? "active" : ""
          }`}
          onClick={() => jvd.setDisplayMode("full")}
        >
          {t("journald.displayFull")}
        </button>
      </div>
      <div className={styles.actionArea}>
        {jvd.subTab === "realtime" ? (
          <>
            <span
              className={`liquid-glass-dot ${jvd.isStreaming ? "dot-success" : ""}`}
            />
            <button
              className={`${styles.actionBtn} liquid-glass-button`}
              onClick={() => jvd.toggleStreaming()}
            >
              <Icon name={jvd.isStreaming ? "stop" : "play"} size="sm" />
              {jvd.isStreaming
                ? t("journald.stopTracking")
                : t("journald.startTracking")}
            </button>
            {jvd.totalLoaded > 0 && (
              <span className={`${styles.countBadge} liquid-glass-mini-card`}>
                {jvd.totalLoaded}
              </span>
            )}
          </>
        ) : (
          <button
            className={`${styles.actionBtn} liquid-glass-button`}
            onClick={() => jvd.runHistoryQuery()}
            disabled={jvd.loading}
          >
            <Icon name="search" size="sm" />
            {jvd.loading ? t("journald.loading") : t("journald.query")}
          </button>
        )}
      </div>
    </div>
  );

  // ── 单条日志（紧凑模式）──
  const renderCompactEntry = (entry: JournalEntry, index: number) => (
    <div
      key={entry.__CURSOR ?? `${entry.__REALTIME_TIMESTAMP ?? '0'}-${index}`}
      className={`${styles.logEntry} ${styles.logEntryCompact} liquid-glass-mini-card`}
      onClick={() => {
        // 点击展开：切换到完整模式并滚动到该条目
        jvd.setDisplayMode("full");
      }}
    >
      <div
        className={`${styles.logLevel} ${priorityToLevelClass(entry.PRIORITY)}`}
        title={priorityLabel(entry.PRIORITY)}
      />
      <span className={styles.logTimestamp}>
        {entry.__REALTIME_TIMESTAMP
          ? formatTimestampTime(entry.__REALTIME_TIMESTAMP)
          : ""}
      </span>
      <span className={styles.logUnit}>
        {entry.SYSLOG_IDENTIFIER ??
          entry._SYSTEMD_UNIT?.split(".")[0] ??
          t("journald.unknownService")}
      </span>
      <span className={styles.logMessageCompact}>
        {entry.MESSAGE ?? ""}
      </span>
    </div>
  );

  // ── 单条日志（完整模式）──
  const renderFullEntry = (entry: JournalEntry, index: number) => (
    <div
      key={entry.__CURSOR ?? `${entry.__REALTIME_TIMESTAMP ?? '0'}-${index}`}
      className={`${styles.logEntryFull} liquid-glass-mini-card`}
    >
      <div className={styles.logEntryFullHeader}>
        <div
          className={`${styles.logLevel} ${priorityToLevelClass(entry.PRIORITY)}`}
          title={priorityLabel(entry.PRIORITY)}
        />
        <span className={styles.logTimestamp}>
          {formatTimestamp(entry.__REALTIME_TIMESTAMP)}
        </span>
        <span className={styles.logUnit}>
          {entry.SYSLOG_IDENTIFIER ??
            entry._SYSTEMD_UNIT ??
            t("journald.unknownService")}
        </span>
        <span className={styles.countBadge}>{priorityLabel(entry.PRIORITY)}</span>
      </div>
      <div className={styles.logMessageFull}>{entry.MESSAGE ?? ""}</div>
      {(entry._HOSTNAME || entry._BOOT_ID || entry.__CURSOR) && (
        <div className={styles.logExtra}>
          {entry._HOSTNAME && (
            <span className={`${styles.logExtraField} liquid-glass-mini-card`}>
              {t("journald.hostname")}: {entry._HOSTNAME}
            </span>
          )}
          {entry._BOOT_ID && (
            <span className={`${styles.logExtraField} liquid-glass-mini-card`}>
              {t("journald.bootId")}: {entry._BOOT_ID.slice(0, 8)}...
            </span>
          )}
          {entry.__CURSOR && (
            <span className={`${styles.logExtraField} liquid-glass-mini-card`}>
              {t("journald.cursor")}: {entry.__CURSOR.slice(0, 16)}...
            </span>
          )}
        </div>
      )}
    </div>
  );

  // ── 日志列表 ──
  const renderLogList = () => (
    <div className={styles.logList} ref={logListRef} onScroll={handleScroll}>
      <div className={styles.logListInner}>
        {jvd.entries.map((entry, i) =>
          jvd.displayMode === "compact"
            ? renderCompactEntry(entry, i)
            : renderFullEntry(entry, i),
        )}

        {/* 历史查询加载更多 */}
        {jvd.subTab === "history" && jvd.hasMore && (
          <button
            className={`${styles.loadMoreBtn} liquid-glass-button`}
            onClick={handleLoadMore}
            disabled={jvd.loading}
          >
            {jvd.loading ? t("journald.loading") : t("journald.loadMore")}
          </button>
        )}

        {/* 加载中 */}
        {jvd.loading && (
          <div className={styles.loadingContainer}>
            <span className={styles.loadingText}>{t("journald.loading")}</span>
          </div>
        )}

        {/* 空状态 */}
        {!jvd.loading && jvd.entries.length === 0 && !jvd.error && (
          <div className={styles.emptyState}>
            <span className={styles.emptyText}>{t("journald.noEntries")}</span>
          </div>
        )}
      </div>
    </div>
  );

  // ── 错误横幅 ──
  const renderError = () => {
    if (!jvd.error) return null;
    const isNotAvailable = jvd.error.includes("不可用") || jvd.error.includes("not available");
    return (
      <div className={`${styles.errorBanner} liquid-glass-mini-card`}>
        <span className="liquid-glass-dot dot-error" />
        <span>
          {isNotAvailable ? t("journald.notAvailable") : jvd.error}
        </span>
        {!isNotAvailable && (
          <button
            className={`${styles.errorRetryBtn} liquid-glass-button`}
            onClick={() => {
              jvd.clearError();
              if (jvd.subTab === "realtime") {
                jvd.toggleStreaming();
              } else {
                jvd.runHistoryQuery();
              }
            }}
          >
            Retry
          </button>
        )}
      </div>
    );
  };

  return (
    <RightSidebarPanel title={t("journald.title") ?? "Journald Viewer"} defaultExpanded={true}>
      <div className={styles.panel}>
        {/* 子标签页 */}
        <div className={styles.subTabs}>
          <button
            className={`${styles.subTab} liquid-glass-button ${
              jvd.subTab === "realtime" ? "active" : ""
            }`}
            onClick={() => jvd.setSubTab("realtime")}
          >
            {t("journald.realtime")}
          </button>
          <button
            className={`${styles.subTab} liquid-glass-button ${
              jvd.subTab === "history" ? "active" : ""
            }`}
            onClick={() => jvd.setSubTab("history")}
          >
            {t("journald.history")}
          </button>
        </div>

        {/* 错误横幅 */}
        {renderError()}

        {/* 过滤栏 */}
        {renderFilterBar()}

        {/* 工具栏 */}
        {renderToolbar()}

        {/* 日志列表 */}
        {renderLogList()}
      </div>
    </RightSidebarPanel>
  );
}
