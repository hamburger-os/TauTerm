import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Icon from "../common/Icon";
import styles from "./TitleBar.module.css";

interface TitleBarProps {
  isMaximized: boolean;
}

/** 同步检测当前平台是否为需要自定义窗口控制的平台（Windows / Linux） */
export function needsCustomTitleBar(): boolean {
  // User-Agent Client Hints API（Chromium 90+ / WebView2 90+）
  if ("userAgentData" in navigator && (navigator as any).userAgentData?.platform) {
    return (navigator as any).userAgentData.platform !== "macOS";
  }
  // 降级：navigator.platform（WebView2 始终返回 "Win32"）
  if (navigator.platform?.startsWith("Mac")) return false;
  // 最后兜底：userAgent（兼容旧版 WebView）
  return !/Mac/i.test(navigator.userAgent);
}

/**
 * 自定义窗口控制按钮（最小化 / 最大化 / 关闭）
 *
 * 在 Windows 和 Linux 上渲染——macOS 在 decorations:false 时仍保留原生红绿灯按钮，
 * 因此无需自定义控件，直接返回 null。
 */
export default function TitleBar({ isMaximized }: TitleBarProps) {
  const { t } = useTranslation();
  const needsControls = useMemo(() => needsCustomTitleBar(), []);

  const handleMinimize = useCallback(() => {
    getCurrentWindow().minimize();
  }, []);

  const handleToggleMaximize = useCallback(() => {
    getCurrentWindow().toggleMaximize();
  }, []);

  const handleClose = useCallback(() => {
    getCurrentWindow().close();
  }, []);

  if (!needsControls) return null;

  return (
    <div className={styles.controls}>
      <button
        className={styles.controlButton}
        onClick={handleMinimize}
        aria-label={t("titleBar.minimize")}
        title={t("titleBar.minimize")}
      >
        <Icon name="window-minimize" size="sm" />
      </button>
      <button
        className={styles.controlButton}
        onClick={handleToggleMaximize}
        aria-label={isMaximized ? t("titleBar.restore") : t("titleBar.maximize")}
        title={isMaximized ? t("titleBar.restore") : t("titleBar.maximize")}
      >
        <Icon name={isMaximized ? "window-restore" : "window-maximize"} size="sm" />
      </button>
      <button
        className={`${styles.controlButton} ${styles.closeButton}`}
        onClick={handleClose}
        aria-label={t("titleBar.close")}
        title={t("titleBar.close")}
      >
        <Icon name="close" size="sm" />
      </button>
    </div>
  );
}
