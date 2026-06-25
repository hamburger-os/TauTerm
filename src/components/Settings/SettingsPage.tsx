import { useState, useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { motion, AnimatePresence } from "framer-motion";
import Icon from "../common/Icon";
import GeneralSettings from "./panels/GeneralSettings";
import AppearanceSettings from "./panels/AppearanceSettings";
import LanguageSettings from "./panels/LanguageSettings";
import EncodingSettings from "./panels/EncodingSettings";
import styles from "./SettingsPage.module.css";

interface SettingsPageProps {
  isOpen: boolean;
  onClose: () => void;
}

type Category = "general" | "appearance" | "language" | "encoding" | "about";

const CATEGORIES: { id: Category; icon: import("../common/Icon").IconName; labelKey: string }[] = [
  { id: "general", icon: "settings" as const, labelKey: "settings.general" },
  { id: "appearance", icon: "palette" as const, labelKey: "settings.appearance" },
  { id: "language", icon: "globe" as const, labelKey: "settings.language" },
  { id: "encoding", icon: "font" as const, labelKey: "settings.encoding" },
  { id: "about", icon: "info" as const, labelKey: "settings.about" },
];

/**
 * 设置页面 — 全屏覆盖层
 *
 * 布局：左侧分类导航 + 右侧配置内容区。
 * 关闭方式：Esc / 点击遮罩 / 关闭按钮。
 */
export default function SettingsPage({ isOpen, onClose }: SettingsPageProps) {
  const { t } = useTranslation();
  const [activeCategory, setActiveCategory] = useState<Category>("general");
  const [appVersion, setAppVersion] = useState("");

  // 从 tauri.conf.json 动态读取版本号
  useEffect(() => {
    getVersion().then(v => setAppVersion(`v${v}`)).catch(() => setAppVersion(""));
  }, []);

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
      case "about": return (
        <div className={styles.aboutSection}>
          <h3 className={styles.panelTitle}>TauTerm</h3>
          {appVersion && <p className={styles.aboutVersion}>{appVersion}</p>}
          <p className={styles.aboutDesc}>{t("app.description")}</p>
          <div className={styles.aboutInfo}>
            <div className={styles.aboutRow}>
              <span className={styles.aboutLabel}>{t("settings.buildInfo")}</span>
              <span className={styles.aboutValue}>Tauri + React + xterm.js</span>
            </div>
          </div>
        </div>
      );
    }
  }, [activeCategory, appVersion, t]);

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
          onClick={handleOverlayClick}
        >
          <motion.div
            className={`${styles.container} liquid-glass`}
            initial={{ scale: 0.95, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.95, opacity: 0 }}
            transition={{ duration: 0.2 }}
          >
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
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
