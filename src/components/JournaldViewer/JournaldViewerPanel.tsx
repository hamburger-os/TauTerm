import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import Icon from "../common/Icon";
import { useJournaldViewer } from "./hooks/useJournaldViewer";
import type { JournalEntry } from "./types";
import {
  LOG_LEVELS,
  formatTimestamp,
  formatTimestampTime,
  priorityLabel,
  priorityToLevelClass,
} from "./types";
import styles from "./JournaldViewerPanel.module.css";

interface JournaldViewerPanelProps {
  sessionId: string;
  isConnected: boolean;
}

const COMPACT_ROW_HEIGHT = 22;
const VIRTUAL_OVERSCAN = 8;

export default function JournaldViewerPanel({
  sessionId,
  isConnected,
}: JournaldViewerPanelProps) {
  const { t } = useTranslation();
  const jvd = useJournaldViewer(sessionId, isConnected);
  const logListRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);
  const expandIndexRef = useRef<number | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(500);

  useEffect(() => {
    const element = logListRef.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setViewportHeight(entry.contentRect.height);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (
      jvd.subTab === "realtime" &&
      autoScrollRef.current &&
      logListRef.current
    ) {
      logListRef.current.scrollTop = 0;
      setScrollTop(0);
    }
  }, [jvd.displayMode, jvd.entries, jvd.subTab]);

  useEffect(() => {
    if (jvd.displayMode !== "full" || expandIndexRef.current === null) return;
    const index = expandIndexRef.current;
    expandIndexRef.current = null;
    requestAnimationFrame(() => {
      const target = logListRef.current?.querySelector<HTMLElement>(
        `[data-journal-index="${index}"]`,
      );
      target?.scrollIntoView({ block: "nearest" });
    });
  }, [jvd.displayMode]);

  const handleScroll = useCallback(() => {
    const element = logListRef.current;
    if (!element) return;
    autoScrollRef.current = element.scrollTop < 40;
    setScrollTop(element.scrollTop);
  }, []);

  const compactWindow = useMemo(() => {
    if (jvd.displayMode !== "compact") {
      return { start: 0, end: jvd.entries.length };
    }
    const visibleRows = Math.ceil(viewportHeight / COMPACT_ROW_HEIGHT);
    const start = Math.max(
      0,
      Math.floor(scrollTop / COMPACT_ROW_HEIGHT) - VIRTUAL_OVERSCAN,
    );
    const end = Math.min(
      jvd.entries.length,
      start + visibleRows + VIRTUAL_OVERSCAN * 2,
    );
    return { start, end };
  }, [jvd.displayMode, jvd.entries.length, scrollTop, viewportHeight]);

  const handleLoadMore = useCallback(() => {
    void jvd.queryHistory(true);
  }, [jvd.queryHistory]);

  const levelClass = (entry: JournalEntry) =>
    styles[priorityToLevelClass(entry.priority)];

  const renderFilterBar = () => (
    <div className={`${styles.filterBar} liquid-glass-card`}>
      <div className={styles.filterRow}>
        <select
          className={`${styles.filterSelect} liquid-glass-input liquid-glass-select`}
          value={jvd.filter.level ?? ""}
          onChange={(event) =>
            jvd.setFilter({
              level: (event.target.value || null) as typeof jvd.filter.level,
            })
          }
        >
          <option value="">{t("journald.filterLevelAll")}</option>
          {LOG_LEVELS.map((level) => (
            <option key={level.value} value={level.value}>
              {t(
                `journald.level${level.value.charAt(0).toUpperCase()}${level.value.slice(1)}`,
              )}
            </option>
          ))}
        </select>
        <select
          className={`${styles.searchModeSelect} liquid-glass-input liquid-glass-select`}
          value={jvd.filter.searchMode ?? "literal"}
          onChange={(event) =>
            jvd.setFilter({
              searchMode: event.target.value as "literal" | "regex",
            })
          }
          title={t("journald.filterKeyword") as string}
        >
          <option value="literal">{t("serial.dataModeText")}</option>
          <option value="regex">{t("sendBar.matchRegex")}</option>
        </select>
        <input
          className={`${styles.filterInput} liquid-glass-input`}
          type="text"
          placeholder={t("journald.filterKeyword") ?? "Keyword"}
          value={jvd.filter.keyword ?? ""}
          onChange={(event) =>
            jvd.setFilter({ keyword: event.target.value || undefined })
          }
        />
      </div>
      <div className={styles.filterRow}>
        <input
          className={`${styles.filterInput} liquid-glass-input`}
          type="text"
          placeholder={t("journald.filterUnit") ?? "Service Unit"}
          value={jvd.filter.unit ?? ""}
          onChange={(event) =>
            jvd.setFilter({ unit: event.target.value || undefined })
          }
        />
        <label className={`liquid-glass-toggle ${styles.filterCheckbox}`}>
          <input
            type="checkbox"
            checked={jvd.filter.kernelOnly ?? false}
            onChange={(event) =>
              jvd.setFilter({ kernelOnly: event.target.checked })
            }
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
              onChange={(event) =>
                jvd.setFilter({ since: event.target.value || null })
              }
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
              onChange={(event) =>
                jvd.setFilter({ until: event.target.value || null })
              }
            />
          </label>
        </div>
      )}
    </div>
  );

  const renderToolbar = () => (
    <div className={styles.toolbar}>
      <div className={styles.toolbarLeft}>
        <button
          className={`${styles.modeToggleBtn} liquid-glass-button`}
          onClick={() =>
            jvd.setDisplayMode(jvd.displayMode === "compact" ? "full" : "compact")
          }
          title={
            jvd.displayMode === "compact"
              ? (t("journald.displayFull") as string)
              : (t("journald.displayCompact") as string)
          }
        >
          {jvd.displayMode === "compact"
            ? t("journald.displayCompact")
            : t("journald.displayFull")}
        </button>

        {jvd.subTab === "history" &&
          (jvd.exporting ? (
            <div className={styles.exportProgress}>
              <span className="liquid-glass-dot dot-success" />
              <span>
                {t("journald.exportProgress", { loaded: jvd.exportLoaded })}
              </span>
              <button
                className={`${styles.cancelExportBtn} liquid-glass-button`}
                onClick={() => void jvd.cancelExport()}
              >
                {t("journald.exportCancel")}
              </button>
            </div>
          ) : (
            <button
              className={`${styles.exportBtn} liquid-glass-button`}
              onClick={() => void jvd.startExport()}
            >
              <Icon name="download" size="sm" />
              <span>{t("journald.exportAll")}</span>
            </button>
          ))}
      </div>

      <div className={styles.actionArea}>
        {jvd.subTab === "realtime" ? (
          <>
            <span
              className={`liquid-glass-dot ${jvd.isStreaming ? "dot-success" : ""}`}
            />
            <button
              className={`${styles.actionBtn} liquid-glass-button`}
              onClick={() => void jvd.toggleStreaming()}
              disabled={jvd.loading}
            >
              <Icon name={jvd.isStreaming ? "stop" : "play"} size="sm" />
              {jvd.isStreaming
                ? t("journald.stopTracking")
                : t("journald.startTracking")}
            </button>
          </>
        ) : (
          <button
            className={`${styles.actionBtn} liquid-glass-button`}
            onClick={() => void jvd.runHistoryQuery()}
            disabled={jvd.loading}
          >
            <Icon name="search" size="sm" />
            {jvd.loading ? t("journald.loading") : t("journald.query")}
          </button>
        )}
        {jvd.totalLoaded > 0 && (
          <span className={`${styles.countBadge} liquid-glass-mini-card`}>
            {jvd.totalLoaded}
          </span>
        )}
      </div>
    </div>
  );

  const renderCompactEntry = (entry: JournalEntry, index: number) => (
    <div
      key={entry.cursor ?? `${entry.realtimeTimestamp ?? "0"}-${index}`}
      className={`${styles.logEntry} ${styles.logEntryCompact} liquid-glass-mini-card`}
      data-journal-index={index}
      onClick={() => {
        expandIndexRef.current = index;
        jvd.setDisplayMode("full");
      }}
    >
      <div
        className={`${styles.logLevel} ${levelClass(entry)}`}
        title={priorityLabel(entry.priority)}
      />
      <span className={styles.logTimestamp}>
        {formatTimestampTime(entry.realtimeTimestamp)}
      </span>
      <span className={styles.logUnit}>
        {entry.identifier ?? entry.unit?.split(".")[0] ?? t("journald.unknownService")}
      </span>
      <span className={styles.logMessageCompact}>{entry.message ?? ""}</span>
    </div>
  );

  const renderFullEntry = (entry: JournalEntry, index: number) => (
    <div
      key={entry.cursor ?? `${entry.realtimeTimestamp ?? "0"}-${index}`}
      className={`${styles.logEntryFull} liquid-glass-mini-card`}
      data-journal-index={index}
    >
      <div className={styles.logEntryFullHeader}>
        <div
          className={`${styles.logLevel} ${levelClass(entry)}`}
          title={priorityLabel(entry.priority)}
        />
        <span className={styles.logTimestamp}>
          {formatTimestamp(entry.realtimeTimestamp)}
        </span>
        <span className={styles.logUnit}>
          {entry.identifier ?? entry.unit ?? t("journald.unknownService")}
        </span>
        <span className={styles.countBadge}>{priorityLabel(entry.priority)}</span>
      </div>
      <div className={styles.logMessageFull}>{entry.message ?? ""}</div>
      {(entry.hostname || entry.bootId || entry.cursor) && (
        <div className={styles.logExtra}>
          {entry.hostname && (
            <span className={`${styles.logExtraField} liquid-glass-mini-card`}>
              {t("journald.hostname")}: {entry.hostname}
            </span>
          )}
          {entry.bootId && (
            <span className={`${styles.logExtraField} liquid-glass-mini-card`}>
              {t("journald.bootId")}: {entry.bootId.slice(0, 8)}...
            </span>
          )}
          {entry.cursor && (
            <span className={`${styles.logExtraField} liquid-glass-mini-card`}>
              {t("journald.cursor")}: {entry.cursor.slice(0, 16)}...
            </span>
          )}
        </div>
      )}
    </div>
  );

  const renderEntries = () => {
    if (jvd.displayMode === "full") {
      return (
        <div className={styles.logListInner}>
          {jvd.entries.map(renderFullEntry)}
        </div>
      );
    }

    const visible = jvd.entries.slice(compactWindow.start, compactWindow.end);
    return (
      <div
        className={styles.virtualList}
        style={{ height: jvd.entries.length * COMPACT_ROW_HEIGHT }}
      >
        {visible.map((entry, offset) => {
          const index = compactWindow.start + offset;
          return (
            <div
              key={entry.cursor ?? `${entry.realtimeTimestamp ?? "0"}-${index}`}
              className={styles.virtualRow}
              style={{ transform: `translateY(${index * COMPACT_ROW_HEIGHT}px)` }}
            >
              {renderCompactEntry(entry, index)}
            </div>
          );
        })}
      </div>
    );
  };

  const renderLogList = () => (
    <div className={styles.logList} ref={logListRef} onScroll={handleScroll}>
      {renderEntries()}
      {jvd.subTab === "history" && jvd.hasMore && (
        <button
          className={`${styles.loadMoreBtn} liquid-glass-button`}
          onClick={handleLoadMore}
          disabled={jvd.loading}
        >
          {jvd.loading ? t("journald.loading") : t("journald.loadMore")}
        </button>
      )}
      {jvd.loading && jvd.entries.length === 0 && (
        <div className={styles.loadingContainer}>
          <span className={styles.loadingText}>{t("journald.loading")}</span>
        </div>
      )}
      {!jvd.loading && jvd.entries.length === 0 && !jvd.error && (
        <div className={styles.emptyState}>
          <span className={styles.emptyText}>{t("journald.noEntries")}</span>
        </div>
      )}
    </div>
  );

  const renderError = () => {
    if (!jvd.error) return null;
    const unavailable = jvd.error.code === "command_unavailable";
    return (
      <div className={`${styles.errorBanner} liquid-glass-mini-card`}>
        <span className="liquid-glass-dot dot-error" />
        <span>{unavailable ? t("journald.notAvailable") : jvd.error.message}</span>
        {!unavailable && (
          <button
            className={`${styles.errorRetryBtn} liquid-glass-button`}
            onClick={() => {
              jvd.clearError();
              if (jvd.subTab === "realtime") {
                void jvd.toggleStreaming();
              } else {
                void jvd.runHistoryQuery();
              }
            }}
          >
            {t("common.retry")}
          </button>
        )}
      </div>
    );
  };

  return (
    <RightSidebarPanel
      title={t("journald.title") ?? "Journald Viewer"}
      defaultExpanded={true}
    >
      <div className={styles.panel}>
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
        {renderError()}
        {renderFilterBar()}
        {renderToolbar()}
        {renderLogList()}
      </div>
    </RightSidebarPanel>
  );
}
