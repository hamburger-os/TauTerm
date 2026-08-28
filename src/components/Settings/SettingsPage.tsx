import { useState, useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import Icon from "../common/Icon";
import AppearanceSettings from "./panels/AppearanceSettings";
import LanguageSettings from "./panels/LanguageSettings";
import LoggingSettings from "./panels/LoggingSettings";
import ShortcutSettings from "./panels/ShortcutSettings";
import SecuritySettings from "./panels/SecuritySettings";
import AboutSettings from "./panels/AboutSettings";
import type { UpdateInfo, CheckFrequency } from "../../types/updater";
import styles from "./SettingsPage.module.css";

interface SettingsPageProps {
  isOpen: boolean;
  onClose: () => void;
  /** 从外部指定打开的初始分类（如 "about"），为 null 时保持默认 "appearance" */
  initialCategory?: string | null;
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

type Category = "appearance" | "language" | "logging" | "security" | "shortcuts" | "about";

const CATEGORIES: { id: Category; icon: import("../common/Icon").IconName; labelKey: string }[] = [
  { id: "appearance", icon: "palette" as const, labelKey: "settings.appearance" },
  { id: "language", icon: "globe" as const, labelKey: "settings.language" },
  { id: "logging", icon: "log" as const, labelKey: "settings.logging" },
  { id: "security", icon: "lock" as const, labelKey: "settings.security" },
  { id: "shortcuts", icon: "keyboard" as const, labelKey: "settings.shortcuts" },
  { id: "about", icon: "info" as const, labelKey: "settings.about" },
];

/**
 * 设置页面 — 全屏覆盖层
 *
 * 布局：左侧分类导航 + 右侧配置内容区。
 * 关闭方式：Esc / 点击遮罩 / 关闭按钮。
 */
export default function SettingsPage({
  isOpen,
  onClose,
  initialCategory,
  updateInfo,
  checkFrequency,
  onCheckUpdate,
  onDownloadUpdate,
  onInstallUpdate,
  onCheckFrequencyChange,
}: SettingsPageProps) {
  const { t } = useTranslation();
  const [activeCategory, setActiveCategory] = useState<Category>("appearance");

  // 外部指定初始分类时（如 StatusBar 点击版本号 → "about"）。
  // 运行时校验：未知分类（如已移除的 "general"/"encoding"）回退默认，
  // 避免 switch 无匹配分支渲染空白面板。
  useEffect(() => {
    if (isOpen && initialCategory) {
      const valid = CATEGORIES.some(c => c.id === initialCategory);
      setActiveCategory(valid ? initialCategory as Category : "appearance");
    }
  }, [isOpen, initialCategory]);

  // Esc 关闭
  useEffect(() => {
    if (!isOpen) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [isOpen, onClose]);

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  }, [onClose]);

  const panelContent = useMemo(() => {
    switch (activeCategory) {
      case "appearance": return <AppearanceSettings />;
      case "language": return <LanguageSettings />;
      case "logging": return <LoggingSettings />;
      case "security": return <SecuritySettings />;
      case "shortcuts": return <ShortcutSettings />;
      case "about": return (
        <AboutSettings
          updateInfo={updateInfo}
          checkFrequency={checkFrequency}
          onCheckUpdate={onCheckUpdate}
          onDownloadUpdate={onDownloadUpdate}
          onInstallUpdate={onInstallUpdate}
          onCheckFrequencyChange={onCheckFrequencyChange}
        />
      );
    }
  }, [activeCategory, updateInfo, checkFrequency, onCheckUpdate, onDownloadUpdate, onInstallUpdate, onCheckFrequencyChange, t]);

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2, ease: [0.4, 0, 0.2, 1] }}
          onClick={handleOverlayClick}
        >
          <motion.div
            initial={{ scale: 0.95, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.95, opacity: 0 }}
            transition={{ duration: 0.15, delay: 0.05, ease: [0.4, 0, 0.2, 1] }}
          >
            <div className={`${styles.container} liquid-glass`}>
            {/* 标题栏 */}
            <div className={styles.header}>
              <span className={styles.headerTitle}>{t("settings.title")}</span>
              <button className={`${styles.closeBtn} liquid-glass-ghost-button`} onClick={onClose}><Icon name="close" size="md" /></button>
            </div>

            <div className={styles.body}>
              {/* 左侧导航 */}
              <nav className={styles.nav}>
                {CATEGORIES.map(cat => (
                  <button
                    key={cat.id}
                    className={`${styles.navBtn} liquid-glass-button ${activeCategory === cat.id ? "active" : ""}`}
                    onClick={() => setActiveCategory(cat.id)}
                  >
                    <Icon name={cat.icon} size="md" />
                    <span>{t(cat.labelKey)}</span>
                  </button>
                ))}
              </nav>

              {/* 右侧内容 */}
              <div className={styles.content}>
                <div className={styles.contentInner}>
                  {panelContent}
                </div>
              </div>
            </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
