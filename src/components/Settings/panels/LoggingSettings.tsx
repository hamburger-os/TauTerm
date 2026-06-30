import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../common/Icon";
import styles from "../SettingsPage.module.css";

const STORAGE_KEYS = {
  systemEnabled: "tauterm-log-system-enabled",
  systemLevel: "tauterm-log-system-level",
  enabled: "tauterm-log-enabled",
  fileMaxSize: "tauterm-log-file-max-size",
  bufferSize: "tauterm-log-buffer-size",
  flushInterval: "tauterm-log-flush-interval",
  retentionDays: "tauterm-log-retention-days",
};

const LOG_LEVELS = [
  { value: "error", labelKey: "logging.levelError" },
  { value: "warn", labelKey: "logging.levelWarn" },
  { value: "info", labelKey: "logging.levelInfo" },
  { value: "debug", labelKey: "logging.levelDebug" },
];

function getStored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

export default function LoggingSettings() {
  const { t } = useTranslation();

  // System log state
  const [systemEnabled, setSystemEnabled] = useState(() => getStored(STORAGE_KEYS.systemEnabled, true));
  const [systemLevel, setSystemLevel] = useState(() => getStored(STORAGE_KEYS.systemLevel, "info"));

  // Session data log state
  const [enabled, setEnabled] = useState(() => getStored(STORAGE_KEYS.enabled, true));
  const [fileMaxSize, setFileMaxSize] = useState(() => getStored(STORAGE_KEYS.fileMaxSize, 10));
  const [bufferSize, setBufferSize] = useState(() => getStored(STORAGE_KEYS.bufferSize, 4096));
  const [flushInterval, setFlushInterval] = useState(() => getStored(STORAGE_KEYS.flushInterval, 500));
  const [retentionDays, setRetentionDays] = useState(() => getStored(STORAGE_KEYS.retentionDays, 7));
  const [logDir, setLogDir] = useState("");

  // ── Mount: load config from Rust as source of truth ──
  // localStorage 可能被清除或过期，首次加载时从 Rust 获取当前配置
  // 仅在 localStorage 无缓存值时使用 Rust 配置（用户修改优先）
  useEffect(() => {
    invoke<{
      enabled: boolean;
      log_dir: string;
      file_max_size: number;
      buffer_size: number;
      flush_interval_ms: number;
      retention_days: number;
    }>("get_log_config").then(config => {
      if (config.log_dir) setLogDir(config.log_dir);
      // 仅在 localStorage 无缓存时使用 Rust 配置填充（保留用户修改）
      if (localStorage.getItem(STORAGE_KEYS.enabled) === null) {
        setEnabled(config.enabled);
        persist(STORAGE_KEYS.enabled, config.enabled);
      }
      if (localStorage.getItem(STORAGE_KEYS.fileMaxSize) === null) {
        const sizeMB = Math.round(config.file_max_size / (1024 * 1024));
        setFileMaxSize(sizeMB);
        persist(STORAGE_KEYS.fileMaxSize, sizeMB);
      }
      if (localStorage.getItem(STORAGE_KEYS.bufferSize) === null) {
        setBufferSize(config.buffer_size);
        persist(STORAGE_KEYS.bufferSize, config.buffer_size);
      }
      if (localStorage.getItem(STORAGE_KEYS.flushInterval) === null) {
        setFlushInterval(config.flush_interval_ms);
        persist(STORAGE_KEYS.flushInterval, config.flush_interval_ms);
      }
      if (localStorage.getItem(STORAGE_KEYS.retentionDays) === null) {
        setRetentionDays(config.retention_days);
        persist(STORAGE_KEYS.retentionDays, config.retention_days);
      }
    }).catch(() => {
      // Rust 配置加载失败，回退到 get_log_dir + localStorage 默认值
      invoke<string>("get_log_dir").then(dir => {
        if (dir) setLogDir(dir);
      }).catch(() => {});
    });
  }, []);

  // Sync system log config to Rust immediately on change
  useEffect(() => {
    invoke("set_system_log_config", { enabled: systemEnabled, level: systemLevel }).catch(() => {});
  }, [systemEnabled, systemLevel]);

  // Sync session log config to Rust immediately on change
  useEffect(() => {
    invoke("update_log_config", {
      config: {
        enabled,
        file_max_size: fileMaxSize * 1024 * 1024,
        buffer_size: bufferSize,
        flush_interval_ms: flushInterval,
        retention_days: retentionDays,
      },
    }).catch(() => {});
  }, [enabled, fileMaxSize, bufferSize, flushInterval, retentionDays]);

  const handleOpenLogDir = useCallback(() => {
    invoke("open_log_dir").catch(() => {});
  }, []);

  const handleClearLogs = useCallback(() => {
    if (!window.confirm(t("logging.clearConfirm") || "Delete all log files? This cannot be undone.")) return;
    invoke("clear_all_logs").then(() => {
      alert(t("logging.cleared") || "All log files cleared.");
    }).catch((e) => {
      alert(`${t("logging.clearError") || "Failed to clear logs"}: ${e}`);
    });
  }, [t]);

  const persist = useCallback((key: string, value: unknown) => {
    localStorage.setItem(key, JSON.stringify(value));
  }, []);

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.logging")}</h3>

      {/* ═══ System Log ═══ */}
      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.systemLog") || "System Log"}</span>
        <p className={styles.settingDesc}>
          {t("logging.systemLogDesc") || "Automatically records app events (connection, disconnection, errors, warnings). File: TauTerm_YYYYMMDD.log"}
        </p>
      </div>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.systemLogStatus") || "Status"}</span>
        <div className={styles.optionList}>
          <button
            className={`${styles.optionItem} ${systemEnabled ? styles.optionItemActive : ""}`}
            onClick={() => { setSystemEnabled(true); persist(STORAGE_KEYS.systemEnabled, true); }}
          >
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("common.ok")}
          </button>
          <button
            className={`${styles.optionItem} ${!systemEnabled ? styles.optionItemActive : ""}`}
            onClick={() => { setSystemEnabled(false); persist(STORAGE_KEYS.systemEnabled, false); }}
          >
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("common.cancel")}
          </button>
        </div>
      </div>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.systemLogLevel") || "Minimum Level"}</span>
        <div className={styles.optionList}>
          {LOG_LEVELS.map(lv => (
            <button
              key={lv.value}
              className={`${styles.optionItem} ${systemLevel === lv.value ? styles.optionItemActive : ""}`}
              onClick={() => { setSystemLevel(lv.value); persist(STORAGE_KEYS.systemLevel, lv.value); }}
            >
              <Icon name="check-plain" size="sm" className={styles.optionIcon} />
              {t(lv.labelKey)}
            </button>
          ))}
        </div>
      </div>

      {/* ═══ Session Data Log ═══ */}
      <div className={styles.settingGroup} style={{ marginTop: "8px" }}>
        <span className={styles.settingLabel}>{t("logging.sessionLog") || "Session Data Log"}</span>
        <p className={styles.settingDesc}>
          {t("logging.sessionLogDesc") || "Right-click a session → 'Start Logging' to record all TX/RX data to file."}
        </p>
      </div>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.enableLogging") || "Enable Session Logging"}</span>
        <div className={styles.optionList}>
          <button
            className={`${styles.optionItem} ${enabled ? styles.optionItemActive : ""}`}
            onClick={() => { setEnabled(true); persist(STORAGE_KEYS.enabled, true); }}
          >
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("common.ok")}
          </button>
          <button
            className={`${styles.optionItem} ${!enabled ? styles.optionItemActive : ""}`}
            onClick={() => { setEnabled(false); persist(STORAGE_KEYS.enabled, false); }}
          >
            <Icon name="check-plain" size="sm" className={styles.optionIcon} />
            {t("common.cancel")}
          </button>
        </div>
      </div>

      {/* ═══ 通用配置（软件自身日志 + 会话数据日志共用） ═══ */}
      <div className={styles.settingGroup} style={{ marginTop: "8px" }}>
        <span className={styles.settingLabel}>{t("logging.commonConfig") || "Common Settings"}</span>
        <p className={styles.settingDesc}>
          {t("logging.commonConfigDesc") || "These settings apply to both system log and session data log."}
        </p>
      </div>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.maxFileSize") || "Max File Size"}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={1}
            max={100}
            step={1}
            value={fileMaxSize}
            onChange={(e) => { setFileMaxSize(Number(e.target.value)); persist(STORAGE_KEYS.fileMaxSize, Number(e.target.value)); }}
          />
          <span className={styles.fontSliderValue}>{fileMaxSize} MB</span>
        </div>
      </div>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.bufferSize") || "Buffer Size"}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={1}
            max={64}
            step={1}
            value={Math.round(bufferSize / 1024)}
            onChange={(e) => {
              const kb = Number(e.target.value);
              setBufferSize(kb * 1024);
              persist(STORAGE_KEYS.bufferSize, kb * 1024);
            }}
          />
          <span className={styles.fontSliderValue}>{Math.round(bufferSize / 1024)} KB</span>
        </div>
      </div>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.flushInterval") || "Flush Interval"}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={100}
            max={2000}
            step={100}
            value={flushInterval}
            onChange={(e) => { setFlushInterval(Number(e.target.value)); persist(STORAGE_KEYS.flushInterval, Number(e.target.value)); }}
          />
          <span className={styles.fontSliderValue}>{flushInterval} ms</span>
        </div>
      </div>

      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.retentionDays") || "Keep Logs For"}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={1}
            max={90}
            step={1}
            value={retentionDays}
            onChange={(e) => { setRetentionDays(Number(e.target.value)); persist(STORAGE_KEYS.retentionDays, Number(e.target.value)); }}
          />
          <span className={styles.fontSliderValue}>{retentionDays} {t("logging.days") || "days"}</span>
        </div>
      </div>

      {/* Log Directory */}
      <div className={styles.settingGroup}>
        <span className={styles.settingLabel}>{t("logging.logDirectory") || "Log Directory"}</span>
        {logDir ? (
          <>
            <p className={styles.settingDesc} style={{ fontFamily: "var(--font-mono)", fontSize: "10px", wordBreak: "break-all" }}>
              {logDir}
            </p>
            <div style={{ display: "flex", gap: "8px", marginTop: "6px", flexWrap: "wrap" }}>
              <button
                className={`${styles.optionItem}`}
                onClick={handleOpenLogDir}
                style={{ gap: "6px" }}
              >
                <Icon name="folder" size="sm" />
                {t("logging.openLogDir") || "Open Log Directory"}
              </button>
              <button
                className={`${styles.optionItem}`}
                onClick={handleClearLogs}
                style={{ gap: "6px" }}
              >
                <Icon name="trash" size="sm" />
                {t("logging.clearAllLogs") || "Clear All Logs"}
              </button>
            </div>
          </>
        ) : (
          <p className={styles.settingDesc}>{t("logging.loading") || "Loading..."}</p>
        )}
      </div>
    </div>
  );
}
