import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../common/Icon";
import GlassButton from "../../common/GlassButton";
import OptionButton from "../../common/OptionButton";
import styles from "../SettingsPage.module.css";

const LOG_LEVELS = [
  { value: "error", labelKey: "logging.levelError" },
  { value: "warn", labelKey: "logging.levelWarn" },
  { value: "info", labelKey: "logging.levelInfo" },
  { value: "debug", labelKey: "logging.levelDebug" },
];

interface LogHealth {
  dropped_session_entries: number;
  dropped_system_entries: number;
}

export default function LoggingSettings() {
  const { t } = useTranslation();

  const [systemEnabled, setSystemEnabled] = useState(true);
  const [systemLevel, setSystemLevel] = useState("info");
  const [enabled, setEnabled] = useState(true);
  const [fileMaxSize, setFileMaxSize] = useState(10);
  const [bufferSize, setBufferSize] = useState(4096);
  const [flushInterval, setFlushInterval] = useState(500);
  const [retentionDays, setRetentionDays] = useState(7);
  const [logDir, setLogDir] = useState("");
  const [hydrated, setHydrated] = useState(false);
  const [configError, setConfigError] = useState<string | null>(null);
  const skipSystemPersistRef = useRef(false);
  const skipSessionPersistRef = useRef(false);
  const [health, setHealth] = useState<LogHealth>({
    dropped_session_entries: 0,
    dropped_system_entries: 0,
  });

  useEffect(() => {
    let cancelled = false;
    invoke<{
      system_enabled: boolean;
      system_level: string;
      session_enabled: boolean;
      log_dir: string;
      file_max_size: number;
      buffer_size: number;
      flush_interval_ms: number;
      retention_days: number;
    }>("get_log_config")
      .then(config => {
        if (cancelled) return;
        setSystemEnabled(config.system_enabled);
        setSystemLevel(config.system_level);
        setEnabled(config.session_enabled);
        setLogDir(config.log_dir);
        setFileMaxSize(Math.max(1, Math.round(config.file_max_size / (1024 * 1024))));
        setBufferSize(config.buffer_size);
        setFlushInterval(config.flush_interval_ms);
        setRetentionDays(config.retention_days);
        setHydrated(true);
      })
      .catch(error => {
        if (!cancelled) {
          setConfigError(String(error));
          setHydrated(true);
        }
      });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (!hydrated) return;
    if (skipSystemPersistRef.current) {
      skipSystemPersistRef.current = false;
      return;
    }

    void invoke("set_system_log_config", { enabled: systemEnabled, level: systemLevel })
      .then(() => setConfigError(null))
      .catch(async error => {
        setConfigError(String(error));
        try {
          const config = await invoke<{
            system_enabled: boolean;
            system_level: string;
          }>("get_log_config");
          skipSystemPersistRef.current = true;
          setSystemEnabled(config.system_enabled);
          setSystemLevel(config.system_level);
        } catch {
          // Keep the explicit persistence error visible; do not invent a local success state.
        }
      });
  }, [hydrated, systemEnabled, systemLevel]);

  useEffect(() => {
    if (!hydrated) return;
    if (skipSessionPersistRef.current) {
      skipSessionPersistRef.current = false;
      return;
    }

    void invoke("update_log_config", {
      config: {
        session_enabled: enabled,
        file_max_size: fileMaxSize * 1024 * 1024,
        buffer_size: bufferSize,
        flush_interval_ms: flushInterval,
        retention_days: retentionDays,
      },
    })
      .then(() => setConfigError(null))
      .catch(async error => {
        setConfigError(String(error));
        try {
          const config = await invoke<{
            session_enabled: boolean;
            file_max_size: number;
            buffer_size: number;
            flush_interval_ms: number;
            retention_days: number;
          }>("get_log_config");
          skipSessionPersistRef.current = true;
          setEnabled(config.session_enabled);
          setFileMaxSize(Math.max(1, Math.round(config.file_max_size / (1024 * 1024))));
          setBufferSize(config.buffer_size);
          setFlushInterval(config.flush_interval_ms);
          setRetentionDays(config.retention_days);
        } catch {
          // Keep the explicit persistence error visible; do not invent a local success state.
        }
      });
  }, [hydrated, enabled, fileMaxSize, bufferSize, flushInterval, retentionDays]);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const next = await invoke<LogHealth>("get_log_health");
        if (!cancelled) setHealth(next);
      } catch {
        // Health telemetry is diagnostic; settings remain usable if the query fails.
      }
    };
    void refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

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

  return (
    <div>
      <h3 className={styles.panelTitle}>{t("settings.logging")}</h3>
      {configError && (
        <p className={styles.settingDesc} role="alert">
          {t("logging.configSaveError", {
            defaultValue: "Logging settings were not saved: {{error}}. The UI was restored to the backend state.",
            error: configError,
          })}
        </p>
      )}

      {/* ═══ System Log ═══ */}
      <h4 className={styles.categoryTitle}>{t("logging.systemLog") || "System Log"}</h4>
      <div className={styles.settingGroup}>
        <p className={styles.settingDesc}>
          {t("logging.systemLogDesc") || "Automatically records app events (connection, disconnection, errors, warnings). File: TauTerm_YYYYMMDD.log"}
        </p>

        <span className={styles.settingLabel}>{t("logging.systemLogStatus") || "Status"}</span>
        <div className={styles.optionList}>
          <OptionButton selected={systemEnabled} onClick={() => setSystemEnabled(true)}>
            {t("common.ok")}
          </OptionButton>
          <OptionButton selected={!systemEnabled} onClick={() => setSystemEnabled(false)}>
            {t("common.cancel")}
          </OptionButton>
        </div>

        <span className={styles.settingLabel}>{t("logging.systemLogLevel") || "Minimum Level"}</span>
        <div className={styles.optionList}>
          {LOG_LEVELS.map(lv => (
            <OptionButton
              key={lv.value}
              selected={systemLevel === lv.value}
              onClick={() => setSystemLevel(lv.value)}
            >
              {t(lv.labelKey)}
            </OptionButton>
          ))}
        </div>
      </div>

      {/* ═══ Session Data Log ═══ */}
      <h4 className={styles.categoryTitle}>{t("logging.sessionLog") || "Session Data Log"}</h4>
      <div className={styles.settingGroup}>
        <p className={styles.settingDesc}>
          {t("logging.sessionLogDesc") || "Right-click a session → 'Start Logging' to record all TX/RX data to file."}
        </p>

        {(health.dropped_session_entries > 0 || health.dropped_system_entries > 0) && (
          <p className={styles.settingDesc} role="status">
            {t("logging.lossWarning", {
              defaultValue: "Logging overflow detected — session: {{session}}, system: {{system}}. The affected log stream may be incomplete.",
              session: health.dropped_session_entries,
              system: health.dropped_system_entries,
            })}
          </p>
        )}

        <span className={styles.settingLabel}>{t("logging.enableLogging") || "Enable Session Logging"}</span>
        <div className={styles.optionList}>
          <OptionButton selected={enabled} onClick={() => setEnabled(true)}>
            {t("common.ok")}
          </OptionButton>
          <OptionButton selected={!enabled} onClick={() => setEnabled(false)}>
            {t("common.cancel")}
          </OptionButton>
        </div>
      </div>

      {/* ═══ Common Settings ═══ */}
      <h4 className={styles.categoryTitle}>{t("logging.commonConfig") || "Common Settings"}</h4>
      <div className={styles.settingGroup}>
        <p className={styles.settingDesc}>
          {t("logging.commonConfigDesc") || "These settings apply to both system log and session data log."}
        </p>

        <span className={styles.settingLabel}>{t("logging.maxFileSize") || "Max File Size"}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={1}
            max={100}
            step={1}
            value={fileMaxSize}
            onChange={(e) => setFileMaxSize(Number(e.target.value))}
          />
          <span className={styles.fontSliderValue}>{fileMaxSize} MB</span>
        </div>

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
            }}
          />
          <span className={styles.fontSliderValue}>{Math.round(bufferSize / 1024)} KB</span>
        </div>

        <span className={styles.settingLabel}>{t("logging.flushInterval") || "Flush Interval"}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={100}
            max={2000}
            step={100}
            value={flushInterval}
            onChange={(e) => setFlushInterval(Number(e.target.value))}
          />
          <span className={styles.fontSliderValue}>{flushInterval} ms</span>
        </div>

        <span className={styles.settingLabel}>{t("logging.retentionDays") || "Keep Logs For"}</span>
        <div className={styles.fontSlider}>
          <input
            type="range"
            className={styles.fontSliderInput}
            min={1}
            max={90}
            step={1}
            value={retentionDays}
            onChange={(e) => setRetentionDays(Number(e.target.value))}
          />
          <span className={styles.fontSliderValue}>{retentionDays} {t("logging.days") || "days"}</span>
        </div>
      </div>

      {/* Log Directory */}
      <h4 className={styles.categoryTitle}>{t("logging.logDirectory") || "Log Directory"}</h4>
      <div className={styles.settingGroup}>
        {logDir ? (
          <>
            <p className={styles.settingDesc} style={{ fontFamily: "var(--font-mono)", fontSize: "10px", wordBreak: "break-all" }}>
              {logDir}
            </p>
            <div style={{ display: "flex", gap: "8px", marginTop: "6px", flexWrap: "wrap" }}>
              <GlassButton variant="secondary" size="sm" onClick={handleOpenLogDir}>
                <Icon name="folder" size="sm" />
                {t("logging.openLogDir") || "Open Log Directory"}
              </GlassButton>
              <GlassButton variant="danger" size="sm" onClick={handleClearLogs}>
                <Icon name="trash" size="sm" />
                {t("logging.clearAllLogs") || "Clear All Logs"}
              </GlassButton>
            </div>
          </>
        ) : (
          <p className={styles.settingDesc}>{t("logging.loading") || "Loading..."}</p>
        )}
      </div>
    </div>
  );
}
