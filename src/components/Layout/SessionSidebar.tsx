import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useContextMenu } from "../../hooks/useContextMenu";
import ContextMenu from "../common/ContextMenu";
import type { ContextMenuItem } from "../common/ContextMenu";
import styles from "./SessionSidebar.module.css";

interface SessionSidebarProps {
  onSelectSession?: (id: string) => void;
  onEditSession?: (id: string) => void;
  onSettingsClick?: () => void;
  onNewSession?: () => void;
}

/**
 * 左侧会话列表侧边栏
 *
 * 显示所有活跃和已断开会话，支持搜索过滤和右键上下文菜单。
 * 底部提供设置按钮。
 */
export default function SessionSidebar({ onSelectSession, onEditSession, onSettingsClick, onNewSession }: SessionSidebarProps) {
  const { t } = useTranslation();
  const { state, switchTab, disconnect, deleteSession, connect } = useSession();
  const [search, setSearch] = useState("");
  const { menu, openMenu, closeMenu } = useContextMenu();

  const filtered = state.tabs.filter(tab =>
    !search || tab.name.toLowerCase().includes(search.toLowerCase()) || tab.endpoint.toLowerCase().includes(search.toLowerCase())
  );

  const handleSelect = useCallback((id: string) => {
    switchTab(id);
    onSelectSession?.(id);
  }, [switchTab, onSelectSession]);

  const handleContextMenu = useCallback((e: React.MouseEvent, tab: typeof state.tabs[0]) => {
    openMenu(e, tab);
  }, [openMenu]);

  const getMenuItems = useCallback((): ContextMenuItem[] => {
    if (!menu.session) return [];
    const { state: sessionState } = menu.session;

    if (sessionState === "connected" || sessionState === "transferring") {
      return [
        { id: "disconnect", label: t("contextMenu.disconnect") || "Disconnect", icon: "⏹" },
        { id: "configure", label: t("contextMenu.configure") || "Configure", icon: "⚙" },
        { id: "delete", label: t("contextMenu.delete") || "Delete", icon: "🗑", danger: true },
      ];
    }
    return [
      { id: "connect", label: t("contextMenu.connect") || "Connect", icon: "▶" },
      { id: "configure", label: t("contextMenu.configure") || "Configure", icon: "⚙" },
      { id: "delete", label: t("contextMenu.delete") || "Delete", icon: "🗑", danger: true },
    ];
  }, [menu.session, t]);

  const handleMenuSelect = useCallback(async (itemId: string, sessionId: string) => {
    switch (itemId) {
      case "connect": {
        const tab = state.tabs.find(t => t.id === sessionId);
        if (tab?.state === "disconnected" && tab.params) {
          try {
            const sid = await connect(tab.endpoint, tab.params as Record<string, unknown>, tab.name, undefined, tab.transferEnabled, tab.transferProtocol);
            if (sid) {
              await switchTab(sid);
              await deleteSession(sessionId); // 删除旧的断开标签页
            }
          } catch (_e) {
            // 错误已在 SessionContext 中处理
          }
        }
        break;
      }
      case "configure":
        onEditSession?.(sessionId);
        break;
      case "disconnect":
        disconnect(sessionId);
        break;
      case "delete":
        if (window.confirm(t("session.deleteConfirm") || "Delete this session?")) {
          deleteSession(sessionId);
        }
        break;
    }
  }, [onEditSession, disconnect, deleteSession, connect, switchTab, state.tabs, t]);

  return (
    <div className={styles.sidebar}>
      {/* 顶部：标题 + 新建按钮 */}
      <div className={styles.header}>
        <span className={styles.title}>{t("session.sessions")}</span>
        <button
          className={styles.addBtn}
          onClick={() => onNewSession?.()}
          title={t("session.newSession") + " (Ctrl+N)"}
        >
          +
        </button>
      </div>

      <input
        className={styles.search}
        type="text"
        placeholder={t("search.placeholder") || "Search sessions..."}
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />

      {/* 中部：会话列表 (可滚动) */}
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
              onContextMenu={(e) => handleContextMenu(e, tab)}
            >
              <div className={styles.itemLeft}>
                <span className={`${styles.statusDot} ${
                  tab.state === "connected" ? styles.connected :
                  tab.state === "transferring" ? styles.transferring : ""
                }`} />
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

      {/* 底部：设置按钮 */}
      <div className={styles.bottomSection}>
        <button
          className={styles.settingsBtn}
          onClick={onSettingsClick}
          title={t("sidebar.settings")}
        >
          <span className={styles.settingsIcon}>⚙</span>
          <span className={styles.settingsLabel}>{t("sidebar.settings")}</span>
        </button>
      </div>

      {/* 右键上下文菜单 */}
      <ContextMenu
        state={menu}
        items={getMenuItems()}
        onSelect={handleMenuSelect}
        onClose={closeMenu}
      />
    </div>
  );
}
