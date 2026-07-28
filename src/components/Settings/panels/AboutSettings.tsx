import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import Icon from "../../common/Icon";
import OptionButton from "../../common/OptionButton";
import type { UpdateInfo, CheckFrequency } from "../../../types/updater";
import styles from "../SettingsPage.module.css";

interface AboutSettingsProps {
  /** 更新器状态 */
  updateInfo: UpdateInfo;
  /** 当前检查频率 */
  checkFrequency: CheckFrequency;
  /** 手动检查更新 */
  onCheckUpdate: () => void;
  /** 下载更新 */
  onDownloadUpdate: () => void;
  /** 安装并重启 */
  onInstallUpdate: () => void;
  /** 修改检查频率 */
  onCheckFrequencyChange: (freq: CheckFrequency) => void;
}

export default function AboutSettings({
  updateInfo,
  checkFrequency,
  onCheckUpdate,
  onDownloadUpdate,
  onInstallUpdate,
  onCheckFrequencyChange,
}: AboutSettingsProps) {
  const { t } = useTranslation();
  const [appVersion, setAppVersion] = useState("");

  // 从 Tauri 动态读取版本号
  useEffect(() => {
    getVersion().then(v => setAppVersion(`v${v}`)).catch(() => setAppVersion(""));
  }, []);

  const downloadPct =
    updateInfo.phase === "downloading" &&
    updateInfo.totalBytes &&
    updateInfo.totalBytes > 0
      ? Math.min(
          Math.round(
            ((updateInfo.downloadedBytes ?? 0) / updateInfo.totalBytes) * 100
          ),
          100
        )
      : 0;

  return (
    <div className={styles.aboutSection}>
      <h3 className={styles.panelTitle}>TauTerm</h3>

      {/* 版本 + 描述 + 更新操作区 */}
      <div className={styles.updateSection}>
        {appVersion && <p className={styles.aboutVersion}>{appVersion}</p>}

        {/* 版本对比 */}
        {updateInfo.phase === "available" && updateInfo.latestVersion && (
          <div className={`${styles.updateVersionCompare} liquid-glass-status-card`}>
            <span className={styles.updateCurrentLabel}>
              {t("updater.currentVersion")}
            </span>
            <span className={styles.updateCurrentVersion}>{appVersion}</span>
            <span className={styles.updateArrow}>
              <Icon name="chevron-up" size="sm" />
            </span>
            <span className={styles.updateLatestVersion}>
              {updateInfo.latestVersion}
            </span>
          </div>
        )}

        <p className={styles.aboutDesc}>{t("app.description")}</p>

        {/* 更新按钮 */}
        {updateInfo.phase === "idle" && (
          <>
            <button className={`${styles.actionBtn} liquid-glass-button`} onClick={onCheckUpdate}>
              {t("updater.checkForUpdates")}
            </button>
            {updateInfo.resultMessage && (
              <p className={styles.updateResultMsg}>
                {updateInfo.resultMessage}
              </p>
            )}
          </>
        )}
        {updateInfo.phase === "checking" && (
          <button
            className={`${styles.actionBtn} liquid-glass-button`}
            disabled
          >
            <span className={styles.updateSpinner} />
            {t("updater.checking")}
          </button>
        )}
        {updateInfo.phase === "available" && (
          <button
            className={`${styles.actionBtn} liquid-primary-button`}
            onClick={onDownloadUpdate}
          >
            {t("updater.downloadUpdate", {
              version: updateInfo.latestVersion ?? "",
            })}
          </button>
        )}
        {updateInfo.phase === "downloading" && (
          <div className={styles.updateProgressWrap}>
            <button
              className={`${styles.actionBtn} liquid-glass-button`}
              disabled
            >
              {t("updater.downloading")}
            </button>
            <div className={styles.updateProgressBar}>
              <div
                className={styles.updateProgressFill}
                style={{ width: `${downloadPct}%` }}
              />
            </div>
            <span className={styles.updateProgressText}>{downloadPct}%</span>
          </div>
        )}
        {updateInfo.phase === "ready" && (
          <button
            className={`${styles.actionBtn} liquid-primary-button`}
            onClick={onInstallUpdate}
          >
            {t("updater.installAndRelaunch")}
          </button>
        )}
        {updateInfo.phase === "error" && (
          <div className={styles.updateError}>
            <p className={styles.updateErrorText}>
              {updateInfo.error
                ? t("updater.checkFailed", { error: updateInfo.error })
                : t("updater.checkFailed", { error: "" })}
            </p>
            <button className={`${styles.actionBtn} liquid-glass-button`} onClick={onCheckUpdate}>
              {t("updater.retry")}
            </button>
          </div>
        )}

        {/* Release Notes */}
        {updateInfo.releaseNotes &&
          (updateInfo.phase === "available" ||
            updateInfo.phase === "downloading" ||
            updateInfo.phase === "ready") && (
            <details className={styles.updateReleaseNotes}>
              <summary className={styles.updateReleaseNotesSummary}>
                {t("updater.releaseNotes")}
              </summary>
              <pre className={styles.updateReleaseNotesBody}>
                {updateInfo.releaseNotes}
              </pre>
            </details>
          )}
      </div>

      {/* 检查频率设置 */}
      <h4 className={styles.categoryTitle}>{t("updater.checkFrequency")}</h4>
      <div className={styles.optionList}>
          {(
            [
              ["always", t("updater.frequencyAlways")],
              ["daily", t("updater.frequencyDaily")],
              ["weekly", t("updater.frequencyWeekly")],
              ["never", t("updater.frequencyNever")],
            ] as const
          ).map(([val, label]) => (
            <OptionButton
              key={val}
              selected={checkFrequency === val}
              onClick={() => onCheckFrequencyChange(val)}
            >
              {label}
            </OptionButton>
          ))}
        </div>

      <h4 className={styles.categoryTitle}>{t("settings.buildInfo")}</h4>
      <div className={styles.aboutRow}>
          <span className={styles.aboutValue}>Tauri + React + xterm.js</span>
        </div>
    </div>
  );
}
