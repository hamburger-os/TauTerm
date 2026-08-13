import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useContextMenu } from "../../hooks/useContextMenu";
import ContextMenu from "../common/ContextMenu";
import Icon from "../common/Icon";
import type { ContextMenuItem } from "../common/ContextMenu";
import type { TabInfo } from "../../context/SessionContext";
import styles from "./SessionSidebar.module.css";

/** 树节点（扁平 TabInfo 渲染时推导） */
interface TreeNode {
  tab: TabInfo;
  children: TabInfo[];
}

interface SessionSidebarProps {
  onSelectSession?: (id: string) => void;
  onEditSession?: (id: string) => void;
  onSettingsClick?: () => void;
  onNewSession?: () => void;
}

/**
 * 左侧会话列表侧边栏（树形结构，支持 SSH 多连接）。
 *
 * - 根节点（parentId === null）：Serial 或 SSH 父会话
 * - 子节点（parentId 非空）：SSH 子 channel
 * - SSH 父会话在 connected 状态下可展开/折叠子项
 * - 选中父会话时自动路由到第一个子 channel
 */
export default function SessionSidebar({ onSelectSession, onEditSession, onSettingsClick, onNewSession }: SessionSidebarProps) {
  const { t } = useTranslation();
  const { state, switchTab, disconnect, deleteSession, connect, startSessionLog, stopSessionLog, loggingSessions, openChannel, closeChannel } = useSession();
  const [search, setSearch] = useState("");
  const { menu, openMenu, closeMenu } = useContextMenu();
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  // 构建树形结构（保持原有排序：connection_type → endpoint → name）
  const tree = useMemo<TreeNode[]>(() => {
    const roots = [...state.tabs.filter(t => !t.parentId)];
    roots.sort((a, b) => {
      const typeCmp = a.connection_type.localeCompare(b.connection_type);
      if (typeCmp !== 0) return typeCmp;
      const endpointCmp = a.endpoint.localeCompare(b.endpoint, undefined, { numeric: true });
      if (endpointCmp !== 0) return endpointCmp;
      return a.name.localeCompare(b.name);
    });
    return roots.map(root => ({
      tab: root,
      children: state.tabs.filter(t => t.parentId === root.id),
    }));
  }, [state.tabs]);

  // 最后一个子项删除后清理 expandedIds
  useEffect(() => {
    setExpandedIds(prev => {
      const next = new Set(prev);
      let changed = false;
      for (const id of prev) {
        const node = tree.find(n => n.tab.id === id);
        if (!node || node.children.length === 0) {
          next.delete(id);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [tree]);

  // 自动展开：检测新增的子连接 tab，展开其父节点
  // 覆盖首次 SSH 连接（channel-0 出现）、重连、右键菜单"新建终端"等场景
  const prevTabIdsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    const currentIds = new Set(state.tabs.map(t => t.id));
    const newTabs = state.tabs.filter(t => !prevTabIdsRef.current.has(t.id));

    for (const tab of newTabs) {
      if (tab.parentId) {
        setExpandedIds(prev => {
          if (prev.has(tab.parentId!)) return prev;
          return new Set(prev).add(tab.parentId!);
        });
      }
    }

    prevTabIdsRef.current = currentIds;
  }, [state.tabs]);

  // 按搜索过滤后的扁平列表（仅用于搜索匹配，树形结构渲染时过滤）
  const searchLower = search.toLowerCase();
  const filteredTree = useMemo(() => {
    if (!search) return tree;
    return tree.filter(node => {
      const parentMatch = node.tab.name.toLowerCase().includes(searchLower)
        || node.tab.endpoint.toLowerCase().includes(searchLower);
      const childMatch = node.children.some(c =>
        c.name.toLowerCase().includes(searchLower)
        || c.endpoint.toLowerCase().includes(searchLower)
      );
      return parentMatch || childMatch;
    });
  }, [tree, search]);

  // 展开/折叠切换
  const toggleExpand = useCallback((id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setExpandedIds(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // 双击父节点：如果未展开则自动展开
  const handleParentSelect = useCallback((node: TreeNode) => {
    // 先展开（SSH connected 会话有子项时）
    if (node.children.length > 0) {
      setExpandedIds(prev => new Set(prev).add(node.tab.id));
    }
    // switchTab 内部会自动路由到第一个子 channel
    switchTab(node.tab.id);
    onSelectSession?.(node.tab.id);
  }, [switchTab, onSelectSession]);

  const handleChildSelect = useCallback((child: TabInfo) => {
    switchTab(child.id);
    onSelectSession?.(child.id);
  }, [switchTab, onSelectSession]);

  const handleContextMenu = useCallback((e: React.MouseEvent, tab: TabInfo) => {
    e.preventDefault();
    e.stopPropagation();
    // 右键时先切换到该 tab（符合常规 UX）
    switchTab(tab.id);
    openMenu(e, tab);
  }, [switchTab, openMenu]);

  // ── 右键菜单项 ──

  const getMenuItems = useCallback((): ContextMenuItem[] => {
    if (!menu.session) return [];
    const { state: sessionState, parentId, pluginId } = menu.session;

    // 子 channel 菜单
    if (parentId) {
      return [
        { id: "close_channel", label: t("contextMenu.closeChannel") || "Disconnect", icon: "stop" },
      ];
    }

    // ── 父级 SSH / TFTP / Serial 会话 ──
    if (sessionState === "connected" || sessionState === "transferring") {
      const isLogging = loggingSessions.has(menu.session.id);
      const isSsh = pluginId === "ssh";
      const isTftp = pluginId === "tftp";
      const isIperf = pluginId === "iperf";
      const items: ContextMenuItem[] = [];
      if (isSsh) {
        items.push({ id: "connect", label: t("contextMenu.connect") || "Connect", icon: "plus" });
      }
      items.push(
        { id: "disconnect", label: t("contextMenu.disconnect") || "Disconnect All", icon: "stop" },
        { id: "configure", label: t("contextMenu.configure") || "Configure", icon: "settings" },
      );
      // TFTP/iperf 无终端数据流，不需要日志/实时监控功能
      if (!isTftp && !isIperf) {
        items.push(
          { id: "toggle_log", label: isLogging ? (t("contextMenu.stopLogging") || "Stop Logging") : (t("contextMenu.startLogging") || "Start Logging"), icon: "log" },
        );
      }
      items.push(
        { id: "delete", label: t("contextMenu.delete") || "Delete", icon: "trash", danger: true },
      );
      return items;
    }
    // 已断开会话（所有类型）
    const items: ContextMenuItem[] = [
      { id: "connect", label: t("contextMenu.connect") || "Connect", icon: "play" },
      { id: "configure", label: t("contextMenu.configure") || "Configure", icon: "settings" },
    ];
    if (menu.session.pluginId === "ssh") {
      // SSH 断开会话不显示"新建终端"（需先连接）
    }
    items.push({ id: "delete", label: t("contextMenu.delete") || "Delete", icon: "trash", danger: true });
    return items;
  }, [menu.session, t, loggingSessions]);

  const handleMenuSelect = useCallback(async (itemId: string) => {
    const sessionId = menu.session?.id || "";

    switch (itemId) {
      case "connect": {
        const tab = state.tabs.find(t => t.id === sessionId);
        if (tab?.pluginId === "ssh" && (tab?.state === "connected" || tab?.state === "transferring")) {
          // SSH 已连接 → 打开新通道
          await openChannel(sessionId);
          // 自动展开父节点
          setExpandedIds(prev => new Set(prev).add(sessionId));
        } else if (tab?.state === "disconnected" && tab.params) {
          // 已断开会话 → 重新连接
          try {
            await connect({
              endpoint: tab.endpoint,
              params: tab.params as Record<string, unknown>,
              name: tab.name,
              pluginId: tab.pluginId,
              transferEnabled: tab.transferEnabled,
              transferProtocol: tab.transferProtocol,
              sendBarEnabled: tab.sendBarEnabled,
              journaldEnabled: tab.journaldEnabled,
              sessionId,
            });
          } catch (_e) { /* ignored */ }
        }
        break;
      }
      case "configure":
        onEditSession?.(sessionId);
        break;
      case "disconnect":
        disconnect(sessionId);
        break;
      case "toggle_log": {
        if (loggingSessions.has(sessionId)) {
          stopSessionLog(sessionId);
        } else {
          startSessionLog(sessionId);
        }
        break;
      }
      case "delete":
        if (window.confirm(t("session.deleteConfirm") || "Delete this session?")) {
          deleteSession(sessionId);
        }
        break;
      case "close_channel": {
        const parentId = menu.session?.parentId;
        if (parentId) {
          await closeChannel(sessionId, parentId);
        }
        break;
      }
    }
  }, [menu.session, state.tabs, t, connect, disconnect, deleteSession, openChannel, closeChannel, onEditSession, loggingSessions, startSessionLog, stopSessionLog]);

  return (
    <div className={`${styles.sidebar} liquid-glass`}>
      {/* 顶部：标题 + 新建按钮 */}
      <div className={styles.header}>
        <span className={styles.title}>{t("session.sessions")}</span>
        <button
          className={`${styles.addBtn} liquid-glass-button`}
          onClick={() => onNewSession?.()}
          title={t("session.newSession") + " (Ctrl+Shift+N)"}
        >
          <Icon name="plus" size="md" color="var(--text-primary)" />
        </button>
      </div>

      <input
        className={`${styles.search} liquid-glass-input`}
        type="text"
        placeholder={t("search.placeholder") || "Search sessions..."}
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />

      {/* 中部：会话列表（树形结构） */}
      <div className={styles.list}>
        {filteredTree.length === 0 ? (
          <div className={styles.empty}>
            {search ? t("search.noResults") : t("session.noSessions")}
          </div>
        ) : (
          filteredTree.map(node => {
            const isExpanded = expandedIds.has(node.tab.id);
            const hasChildren = node.children.length > 0;
            const isSsh = node.tab.pluginId === "ssh";
            const isConnected = node.tab.state === "connected" || node.tab.state === "transferring";
            const canExpand = isSsh && isConnected && hasChildren;

            return (
              <div key={node.tab.id} className={styles.treeGroup}>
                {/* 父节点 */}
                <motion.div
                  className={`${styles.item} ${state.activeTabId === node.tab.id ? styles.active : ""}`}
                  whileHover={{ scale: 1.02 }}
                  whileTap={{ scale: 0.98 }}
                  onClick={() => handleParentSelect(node)}
                  onContextMenu={(e) => handleContextMenu(e, node.tab)}
                >
                  <div className={styles.itemLeft}>
                    {/* 展开/折叠箭头 */}
                    {canExpand ? (
                      <span
                        className={`${styles.expandArrow} ${isExpanded ? styles.open : ""}`}
                        onClick={(e) => toggleExpand(node.tab.id, e)}
                      >
                        ▶
                      </span>
                    ) : (
                      <span className={styles.noChildren} />
                    )}
                    <Icon
                      name={
                        node.tab.state === "connected" ? "status-connected" :
                        node.tab.state === "connecting" || node.tab.state === "transferring" ? "status-connecting" :
                        "status-idle"
                      }
                      size={10}
                    />
                    <div>
                      <div className={styles.itemName}>{node.tab.name}</div>
                      <div className={styles.itemEndpoint}>{node.tab.endpoint}</div>
                    </div>
                  </div>
                  {state.activeTabId === node.tab.id && (
                    <motion.div
                      className={styles.activeBar}
                      layoutId="activeBar"
                      transition={{ type: "spring", stiffness: 500, damping: 30 }}
                    />
                  )}
                </motion.div>

                {/* 子节点（展开时显示） */}
                {isExpanded && hasChildren && (
                  <div className={styles.children}>
                    {node.children.map(child => (
                      <motion.div
                        key={child.id}
                        className={`${styles.childItem} ${state.activeTabId === child.id ? styles.active : ""}`}
                        whileHover={{ scale: 1.02 }}
                        whileTap={{ scale: 0.98 }}
                        onClick={() => handleChildSelect(child)}
                        onContextMenu={(e) => handleContextMenu(e, child)}
                      >
                        <div className={styles.itemLeft}>
                          <Icon
                            name={
                              child.state === "connected" ? "status-connected" :
                              child.state === "connecting" ? "status-connecting" :
                              "status-idle"
                            }
                            size={8}
                          />
                          <div>
                            <div className={styles.itemName}>{child.name}</div>
                            <div className={styles.itemEndpoint}>{child.endpoint}</div>
                          </div>
                        </div>
                        {state.activeTabId === child.id && (
                          <motion.div
                            className={styles.activeBar}
                            layoutId="activeBar"
                            transition={{ type: "spring", stiffness: 500, damping: 30 }}
                          />
                        )}
                      </motion.div>
                    ))}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>

      {/* 底部：设置按钮 */}
      <div className={styles.bottomSection}>
        <button
          className={`${styles.settingsBtn} liquid-glass-button`}
          onClick={onSettingsClick}
          title={t("sidebar.settings")}
        >
          <Icon name="settings" size="sm" className={styles.settingsIcon} />
          <span className={styles.settingsLabel}>{t("sidebar.settings")}</span>
        </button>
      </div>

      {/* 右键上下文菜单 */}
      <ContextMenu
        state={menu}
        items={getMenuItems()}
        onSelect={(itemId) => handleMenuSelect(itemId)}
        onClose={closeMenu}
      />
    </div>
  );
}
