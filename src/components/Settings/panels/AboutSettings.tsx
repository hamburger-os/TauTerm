import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../common/Icon";
import OptionButton from "../../common/OptionButton";
import ConfirmDialog from "../../common/ConfirmDialog";
import { formatBytes } from "../../../utils/format";
import type { UpdateInfo, CheckFrequency, UpdaterPolicy } from "../../../types/updater";
import styles from "../SettingsPage.module.css";

interface AboutSettingsProps {
  /** 更新器状态 */
  updateInfo: UpdateInfo;
  updaterPolicy: UpdaterPolicy | null;
  /** 当前检查频率 */
  checkFrequency: CheckFrequency;
  /** 手动检查更新 */
  onCheckUpdate: () => void;
  /** 下载更新 */
  onDownloadUpdate: () => void;
  /** 安装已下载的更新 */
  onInstallUpdate: () => void;
  onRestartUpdate: () => void;
  onRetryUpdate: () => void;
  /** 修改检查频率 */
  onCheckFrequencyChange: (freq: CheckFrequency) => void;
}

export default function AboutSettings({
  updateInfo,
  updaterPolicy,
  checkFrequency,
  onCheckUpdate,
  onDownloadUpdate,
  onInstallUpdate,
  onRestartUpdate,
  onRetryUpdate,
  onCheckFrequencyChange,
}: AboutSettingsProps) {
  const { t } = useTranslation();
  const [appVersion, setAppVersion] = useState("");
  const [confirmation, setConfirmation] = useState<"install" | "restart" | null>(null);
  const [diagnosticsMessage, setDiagnosticsMessage] = useState("");

  // 从 Tauri 动态读取版本号
  useEffect(() => {
    getVersion().then(v => setAppVersion(`v${v}`)).catch(() => setAppVersion(""));
  }, []);

  const exportDiagnostics = async () => {
    setDiagnosticsMessage("");
    try {
      const saved = await invoke<boolean>("export_diagnostics");
      if (saved) {
        setDiagnosticsMessage(t("diagnostics.exportSuccess"));
      }
    } catch (error) {
      setDiagnosticsMessage(t("diagnostics.exportFailed", { error: String(error) }));
    }
  };

  const release = "release" in updateInfo ? updateInfo.release : undefined;
  const progress = updateInfo.phase === "downloading" ? updateInfo : null;
  const downloadPct = progress?.totalBytes
    ? Math.min(Math.round(progress.downloadedBytes / progress.totalBytes * 100), 99)
    : null;
  const canInstall = updaterPolicy?.mode === "installed";
  const queryOnly = updaterPolicy?.mode === "queryOnly";
  const canCheck = updateInfo.phase === "idle" || updateInfo.phase === "latest"
    || (updateInfo.phase === "available" && queryOnly);

  const retry = () => {
    if (updateInfo.phase === "error" && (updateInfo.recovery === "install" || updateInfo.recovery === "relaunch")) {
      setConfirmation(updateInfo.recovery === "install" ? "install" : "restart");
    } else onRetryUpdate();
  };

  return (
    <div className={styles.aboutSection}>
      <h3 className={styles.panelTitle}>TauTerm</h3>
      <div className={styles.updateSection} data-testid="updater-state">
        {appVersion && <p className={styles.aboutVersion}>{appVersion}</p>}
        {queryOnly && <p data-testid="updater-query-only" className={styles.updateResultMsg}>{t("updater.queryOnly")}</p>}
        {release && (
          <div className={`${styles.updateVersionCompare} liquid-glass-status-card`}>
            <span className={styles.updateCurrentLabel}>{t("updater.currentVersion")}</span>
            <span className={styles.updateCurrentVersion}>{appVersion}</span>
            <span className={styles.updateArrow}><Icon name="arrow-right" size="sm" /></span>
            <span className={styles.updateLatestVersion}>{release.version}</span>
          </div>
        )}
        {canCheck && (
          <button data-testid="updater-check" className={`${styles.actionBtn} liquid-glass-button`} onClick={onCheckUpdate}>
            {t("updater.checkForUpdates")}
          </button>
        )}
        {updateInfo.phase === "latest" && <p role="status" className={styles.updateResultMsg}>{t("updater.alreadyLatest")}</p>}
        {updateInfo.phase === "unavailable" && <p role="status" className={styles.updateResultMsg}>{t("updater.unavailable")}</p>}
        {(updateInfo.phase === "initializing" || updateInfo.phase === "checking" || updateInfo.phase === "installing" || updateInfo.phase === "restarting") && (
          <button className={`${styles.actionBtn} liquid-glass-button`} disabled aria-busy="true">
            <span className={styles.updateSpinner} />
            {t(`updater.${updateInfo.phase}`)}
          </button>
        )}
        {updateInfo.phase === "available" && canInstall && (
          <button className={`${styles.actionBtn} liquid-primary-button`} onClick={onDownloadUpdate}>
            {t("updater.downloadUpdate", { version: updateInfo.release.version })}
          </button>
        )}
        {progress && (
          <div className={styles.updateProgressWrap}>
            <button className={`${styles.actionBtn} liquid-glass-button`} disabled aria-busy="true">
              {t("updater.downloading")}
            </button>
            <div role="progressbar" aria-label={t("updater.downloading")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={downloadPct ?? undefined}>
              {downloadPct !== null && <div className={styles.updateProgressBar}><div className={styles.updateProgressFill} style={{ width: `${downloadPct}%` }} /></div>}
              <span className={styles.updateProgressText}>
                {downloadPct !== null ? `${downloadPct}%` : t("updater.downloadedBytes", { bytes: formatBytes(progress.downloadedBytes) })}
              </span>
            </div>
          </div>
        )}
        {updateInfo.phase === "downloaded" && (
          <>
            <p role="status" className={styles.updateResultMsg}>{t("updater.downloadComplete")}</p>
            <button className={`${styles.actionBtn} liquid-primary-button`} onClick={() => setConfirmation("install")}>
              {t("updater.installUpdate")}
            </button>
          </>
        )}
        {updateInfo.phase === "restartRequired" && (
          <button className={`${styles.actionBtn} liquid-primary-button`} onClick={() => setConfirmation("restart")}>
            {t("updater.restartNow")}
          </button>
        )}
        {updateInfo.phase === "error" && (
          <div className={styles.updateError}>
            <p role="alert" className={styles.updateErrorText}>
              {t(`updaterError.${updateInfo.kind}`, { defaultValue: t("updaterError.unknown") })}
            </p>
            <button className={`${styles.actionBtn} liquid-glass-button`} onClick={retry}>{t("updater.retry")}</button>
          </div>
        )}
        {release?.notes && (
          <details className={styles.updateReleaseNotes}>
            <summary className={styles.updateReleaseNotesSummary}>{t("updater.releaseNotes")}</summary>
            <pre className={styles.updateReleaseNotesBody}>{release.notes}</pre>
          </details>
        )}
      </div>

      {canInstall && <>
        <h4 className={styles.categoryTitle}>{t("updater.checkFrequency")}</h4>
        <div className={styles.optionList}>
          {([
            ["always", t("updater.frequencyAlways")],
            ["daily", t("updater.frequencyDaily")],
            ["weekly", t("updater.frequencyWeekly")],
            ["never", t("updater.frequencyNever")],
          ] as const).map(([val, label]) => (
            <OptionButton key={val} selected={checkFrequency === val} onClick={() => onCheckFrequencyChange(val)}>{label}</OptionButton>
          ))}
        </div>
      </>}
      <ConfirmDialog
        open={confirmation !== null}
        title={t(confirmation === "restart" ? "updater.restartNow" : "updater.installUpdate")}
        message={t("updater.installConfirmation")}
        onCancel={() => setConfirmation(null)}
        onConfirm={() => {
          const action = confirmation;
          setConfirmation(null);
          if (action === "install") onInstallUpdate();
          if (action === "restart") onRestartUpdate();
        }}
      />

      <h4 className={styles.categoryTitle}>{t("diagnostics.title")}</h4>
      <div className={styles.updateSection}>
        <p className={styles.updateResultMsg}>{t("diagnostics.description")}</p>
        <button className={`${styles.actionBtn} liquid-glass-button`} onClick={exportDiagnostics}>
          {t("diagnostics.export")}
        </button>
        {diagnosticsMessage && <p className={styles.updateResultMsg}>{diagnosticsMessage}</p>}
      </div>

      <h4 className={styles.categoryTitle}>{t("settings.buildInfo")}</h4>
      <div className={styles.aboutRow}>
          <span className={styles.aboutValue}>{t("settings.buildStack")}</span>
        </div>
    </div>
  );
}
