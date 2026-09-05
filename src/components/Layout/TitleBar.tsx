import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Icon from "../common/Icon";
import styles from "./TitleBar.module.css";

interface TitleBarProps {
  isMaximized: boolean;
}

/** 同步检测当前平台是否需要自定义窗口控制。 */
export function needsCustomTitleBar(): boolean {
  // 优先使用 User-Agent Client Hints API。
  if ("userAgentData" in navigator && (navigator as any).userAgentData?.platform) {
    return (navigator as any).userAgentData.platform !== "macOS";
  }
  // 降级：使用 navigator.platform。
  if (navigator.platform?.startsWith("Mac")) return false;
  // 最后兜底：使用 userAgent。
  return !/Mac/i.test(navigator.userAgent);
}

/**
 * 自定义窗口控制按钮（最小化 / 最大化 / 关闭）
 *
 * 仅在需要自绘窗口按钮的平台上渲染；系统已提供原生窗口控件时直接返回 null。
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
        <Icon name="window-close" size="sm" />
      </button>
    </div>
  );
}
