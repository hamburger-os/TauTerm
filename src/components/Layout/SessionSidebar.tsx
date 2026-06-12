import { useState } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import styles from "./SessionSidebar.module.css";

interface SessionSidebarProps {
  onSelectSession?: (id: string) => void;
}

/**
 * 左侧会话列表侧边栏
 *
 * 显示所有活跃会话，支持搜索过滤。
 */
export default function SessionSidebar({ onSelectSession }: SessionSidebarProps) {
  const { t } = useTranslation();
  const { state, switchTab } = useSession();
  const [search, setSearch] = useState("");

  const filtered = state.tabs.filter(tab =>
    !search || tab.name.toLowerCase().includes(search.toLowerCase()) || tab.endpoint.toLowerCase().includes(search.toLowerCase())
  );

  const handleSelect = (id: string) => {
    switchTab(id);
    onSelectSession?.(id);
  };

  return (
    <div className={styles.sidebar}>
      <div className={styles.header}>
        <span className={styles.title}>{t("session.sessions")}</span>
        <span className={styles.count}>{state.tabs.length}/10</span>
      </div>

      <input
        className={styles.search}
        type="text"
        placeholder={t("search.placeholder") || "Search sessions..."}
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />

      <div className={styles.list}>
        {filtered.length === 0 ? (
          <div className={styles.empty}>
            {search ? t("search.noResults") : t("session.noSessions")}
          </div>
        ) : (
          filtered.map(tab => (
            <motion.div
              key={tab.id}
              className={`${styles.item} ${state.activeTabId === tab.id ? styles.active : ""}`}
              whileHover={{ scale: 1.02, backgroundColor: "rgba(255,255,255,0.06)" }}
              whileTap={{ scale: 0.98 }}
              onClick={() => handleSelect(tab.id)}
            >
              <div className={styles.itemLeft}>
                <span className={`${styles.statusDot} ${tab.state === "connected" ? styles.connected : ""}`} />
                <div>
                  <div className={styles.itemName}>{tab.name}</div>
                  <div className={styles.itemEndpoint}>{tab.endpoint}</div>
                </div>
              </div>
              {state.activeTabId === tab.id && (
                <motion.div
                  className={styles.activeBar}
                  layoutId="activeBar"
                  transition={{ type: "spring", stiffness: 500, damping: 30 }}
                />
              )}
            </motion.div>
          ))
        )}
      </div>
    </div>
  );
}
