import { useState, useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import Icon from "../common/Icon";
import GeneralSettings from "./panels/GeneralSettings";
import AppearanceSettings from "./panels/AppearanceSettings";
import LanguageSettings from "./panels/LanguageSettings";
import EncodingSettings from "./panels/EncodingSettings";
import LoggingSettings from "./panels/LoggingSettings";
import ShortcutSettings from "./panels/ShortcutSettings";
import AboutSettings from "./panels/AboutSettings";
import type { UpdateInfo, CheckFrequency } from "../../types/updater";
import styles from "./SettingsPage.module.css";

interface SettingsPageProps {
  isOpen: boolean;
  onClose: () => void;
  /** 从外部指定打开的初始分类（如 "about"），为 null 时保持默认 "general" */
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

type Category = "general" | "appearance" | "language" | "encoding" | "logging" | "shortcuts" | "about";

const CATEGORIES: { id: Category; icon: import("../common/Icon").IconName; labelKey: string }[] = [
  { id: "general", icon: "settings" as const, labelKey: "settings.general" },
  { id: "appearance", icon: "palette" as const, labelKey: "settings.appearance" },
  { id: "language", icon: "globe" as const, labelKey: "settings.language" },
  { id: "encoding", icon: "font" as const, labelKey: "settings.encoding" },
  { id: "logging", icon: "log" as const, labelKey: "settings.logging" },
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
  const [activeCategory, setActiveCategory] = useState<Category>("general");

  // 外部指定初始分类时（如 StatusBar 点击版本号 → "about"）
  useEffect(() => {
    if (isOpen && initialCategory) {
      setActiveCategory(initialCategory as Category);
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
      case "general": return <GeneralSettings />;
      case "appearance": return <AppearanceSettings />;
      case "language": return <LanguageSettings />;
      case "encoding": return <EncodingSettings />;
      case "logging": return <LoggingSettings />;
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
              <button className={styles.closeBtn} onClick={onClose}><Icon name="close" size="md" /></button>
            </div>

            <div className={styles.body}>
              {/* 左侧导航 */}
              <nav className={styles.nav}>
                {CATEGORIES.map(cat => (
                  <button
                    key={cat.id}
                    className={`${styles.navItem} ${activeCategory === cat.id ? styles.navItemActive : ""}`}
                    onClick={() => setActiveCategory(cat.id)}
                  >
                    <Icon name={cat.icon} size="md" className={styles.navIcon} />
                    <span className={styles.navLabel}>{t(cat.labelKey)}</span>
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
