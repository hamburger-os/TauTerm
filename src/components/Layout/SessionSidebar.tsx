import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import { useSplitLayout } from "../../context/SplitLayoutContext";
import { useContextMenu } from "../../hooks/useContextMenu";
import ConfirmDialog from "../common/ConfirmDialog";
import ContextMenu from "../common/ContextMenu";
import Icon from "../common/Icon";
import PaneMiniMap from "./PaneMiniMap";
import { getSessionSubtitle } from "./sessionPresentation";
import type { ContextMenuItem } from "../common/ContextMenu";
import type { TabInfo } from "../../context/SessionContext";
import { pluginRegistry, type PluginSessionTreeChild } from "../../core/plugin-registry";
import { usePluginRuntimeRevision } from "../../core/usePluginRuntime";
import styles from "./SessionSidebar.module.css";

/** 树节点（扁平 TabInfo 渲染时推导；网络对端为 peerChildren，非标签页） */
interface TreeNode {
  tab: TabInfo;
  children: TabInfo[];
  extensionChildren: PluginSessionTreeChild[];
}

interface SessionSidebarProps {
  onSelectSession?: (id: string) => void;
  onEditSession?: (id: string) => void;
  onSettingsClick?: () => void;
  onNewSession?: () => void;
}

/**
 * 左侧会话列表侧边栏（树形结构，支持协议能力驱动的多终端）。
 *
 * - 根节点（parentId === null）：普通会话或多终端父配置
 * - 子节点（parentId 非空）：SSH / Local Shell 终端实例
 * - 支持 multi_session 的父会话可展开/折叠子项
 * - 选中多终端父会话时由 SessionContext 路由到上一次活动的子 channel
 * - 分屏模式下在已显示 Session 右侧绘制当前 Split Tree mini-map，标出所在 Pane
 */
export default function SessionSidebar({ onSelectSession, onEditSession, onSettingsClick, onNewSession }: SessionSidebarProps) {
  const { t } = useTranslation();
  const { state, switchTab, disconnect, deleteSession, reconnectSession, startSessionLog, stopSessionLog, loggingSessions, openChannel, closeChannel } = useSession();
  const { state: splitLayout, sessionToPane, paneCount, selectPane } = useSplitLayout();
  const [search, setSearch] = useState("");
  const { menu, openMenu, openExtensionMenu, closeMenu } = useContextMenu();
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [pendingDeleteSessionId, setPendingDeleteSessionId] = useState<string | null>(null);
  /** 右键菜单打开前的 Pane。新建子终端时用它作为落点，避免右键导航抢走空 Pane。 */
  const contextMenuOriginPaneRef = useRef(splitLayout.selectedPaneId);
  const runtimeRevision = usePluginRuntimeRevision();

  // 构建树形结构。插件可贡献非 Session 的后台实体，但公共层只消费通用节点合同。
  const tree = useMemo<TreeNode[]>(() => {
    const groupKey = (tab: TabInfo): string => {
      const plugin = pluginRegistry.get(tab.pluginId);
      return plugin?.sessionTree?.groupKey?.(tab.params ?? {}, tab.connection_type)
        ?? tab.connection_type;
    };
    const roots = [...state.tabs.filter(tab => !tab.parentId)];
    roots.sort((a, b) => {
      const ga = groupKey(a);
      const gb = groupKey(b);
      if (ga !== gb) return ga < gb ? -1 : 1;
      const endpointCmp = a.endpoint.localeCompare(b.endpoint, undefined, { numeric: true });
      if (endpointCmp !== 0) return endpointCmp;
      return a.name.localeCompare(b.name);
    });
    return roots.map(root => {
      const plugin = pluginRegistry.get(root.pluginId);
      const runtime = plugin?.runtimeStore?.getSnapshot(root.id);
      return {
        tab: root,
        children: state.tabs.filter(tab => tab.parentId === root.id),
        extensionChildren: plugin?.sessionTree?.children(root.id, root.params ?? {}, runtime) ?? [],
      };
    });
  }, [state.tabs, runtimeRevision]);

  // 最后一个子项删除后清理 expandedIds
  useEffect(() => {
    setExpandedIds(prev => {
      const next = new Set(prev);
      let changed = false;
      for (const id of prev) {
        const node = tree.find(n => n.tab.id === id);
        if (!node || (node.children.length === 0 && node.extensionChildren.length === 0)) {
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

  // 插件后台实体新增时自动展开所属根会话。
  const prevExtensionIdsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    const currentIds = new Set<string>();
    for (const node of tree) {
      for (const child of node.extensionChildren) currentIds.add(`${node.tab.id}:${child.id}`);
    }
    for (const node of tree) {
      if (node.extensionChildren.some(child => !prevExtensionIdsRef.current.has(`${node.tab.id}:${child.id}`))) {
        setExpandedIds(prev => new Set(prev).add(node.tab.id));
      }
    }
    prevExtensionIdsRef.current = currentIds;
  }, [tree]);

  // 按搜索过滤后的扁平列表（仅用于搜索匹配，树形结构渲染时过滤）
  const searchLower = search.toLowerCase();
  const filteredTree = useMemo(() => {
    if (!search) return tree;
    return tree.filter(node => {
      const parentMatch = node.tab.name.toLowerCase().includes(searchLower)
        || getSessionSubtitle(node.tab).toLowerCase().includes(searchLower)
        || node.tab.endpoint.toLowerCase().includes(searchLower);
      const childMatch = node.children.some(c =>
        c.name.toLowerCase().includes(searchLower)
        || c.endpoint.toLowerCase().includes(searchLower)
      );
      const extensionMatch = node.extensionChildren.some(child =>
        child.name.toLowerCase().includes(searchLower)
        || child.subtitle.toLowerCase().includes(searchLower)
      );
      return parentMatch || childMatch || extensionMatch;
    });
  }, [tree, search, searchLower]);

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

  const handleParentSelect = useCallback((node: TreeNode) => {
    if (node.children.length > 0 || node.extensionChildren.length > 0) {
      setExpandedIds(prev => new Set(prev).add(node.tab.id));
    }
    switchTab(node.tab.id);
    const plugin = pluginRegistry.get(node.tab.pluginId);
    plugin?.sessionTree?.onParentSelect?.(
      node.tab.id,
      node.tab.params ?? {},
      plugin.runtimeStore?.getSnapshot(node.tab.id),
    );
    onSelectSession?.(node.tab.id);
  }, [switchTab, onSelectSession]);

  const handleChildSelect = useCallback((child: TabInfo) => {
    switchTab(child.id);
    onSelectSession?.(child.id);
  }, [switchTab, onSelectSession]);

  const handleExtensionChildSelect = useCallback((container: TabInfo, child: PluginSessionTreeChild) => {
    switchTab(container.id);
    child.onSelect?.();
    onSelectSession?.(container.id);
  }, [switchTab, onSelectSession]);

  const handleExtensionContextMenu = useCallback((e: React.MouseEvent, container: TabInfo, childId: string) => {
    openExtensionMenu(e, container, container.id, childId);
  }, [openExtensionMenu]);

  const handleContextMenu = useCallback((e: React.MouseEvent, tab: TabInfo) => {
    e.preventDefault();
    e.stopPropagation();
    // 保留原有交互：右键同时进入该会话上下文。
    // 先记住右键前的 Pane；若随后选择“新建终端”，新 Channel 仍应落到这个 Pane。
    contextMenuOriginPaneRef.current = splitLayout.selectedPaneId;
    switchTab(tab.id);
    openMenu(e, tab);
  }, [splitLayout.selectedPaneId, switchTab, openMenu]);

  // ── 右键菜单项 ──

  const getMenuItems = useCallback((): ContextMenuItem[] => {
    if (!menu.session) return [];

    if (menu.extension) {
      const node = tree.find(item => item.tab.id === menu.extension!.parentSessionId);
      const child = node?.extensionChildren.find(item => item.id === menu.extension!.childId);
      return (child?.menuItems ?? []).map(item => ({
        id: item.id,
        label: item.label,
        icon: item.icon,
        danger: item.danger,
      }));
    }

    const { state: sessionState, parentId, pluginId } = menu.session;
    const registration = pluginRegistry.get(pluginId);
    const capabilities = registration?.manifest.capabilities ?? [];
    const supportsMultiple = capabilities.includes("multi_session");
    const supportsElevation = capabilities.includes("elevated_session")
      && (registration?.canCreateElevatedSession?.(menu.session.params ?? {}) ?? true);

    // 子 channel 菜单
    if (parentId) {
      return [
        { id: "close_channel", label: t("contextMenu.closeChannel") || "Disconnect", icon: "stop" },
      ];
    }

    // ── 根会话菜单 ──
    if (sessionState === "connected" || sessionState === "transferring") {
      const isLogging = loggingSessions.has(menu.session.id);
      const supportsLogging = capabilities.includes("session_logging");
      const items: ContextMenuItem[] = [];
      if (supportsMultiple) {
        items.push({ id: "connect", label: t("contextMenu.connect") || "Connect", icon: "play" });
      }
      if (supportsElevation) {
        items.push({ id: "connect_elevated", label: t("contextMenu.connectAsAdministrator"), icon: "shield" });
      }
      items.push(
        { id: "disconnect", label: t("contextMenu.disconnect") || "Disconnect All", icon: "stop" },
        { id: "configure", label: t("contextMenu.configure") || "Configure", icon: "settings" },
      );
      if (supportsLogging) {
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
    if (supportsElevation) {
      items.splice(1, 0, { id: "connect_elevated", label: t("contextMenu.connectAsAdministrator"), icon: "shield" });
    }
    items.push({ id: "delete", label: t("contextMenu.delete") || "Delete", icon: "trash", danger: true });
    return items;
  }, [menu.session, menu.extension, tree, t, loggingSessions]);

  const handleMenuSelect = useCallback(async (itemId: string) => {
    if (menu.extension) {
      const node = tree.find(item => item.tab.id === menu.extension!.parentSessionId);
      const child = node?.extensionChildren.find(item => item.id === menu.extension!.childId);
      await child?.menuItems?.find(item => item.id === itemId)?.run();
      return;
    }
    const sessionId = menu.session?.id || "";

    switch (itemId) {
      case "connect": {
        const tab = state.tabs.find(t => t.id === sessionId);
        const supportsMultiple = tab
          ? pluginRegistry.get(tab.pluginId)?.manifest.capabilities.includes("multi_session")
          : false;
        if (supportsMultiple && (tab?.state === "connected" || tab?.state === "transferring")) {
          // 右键父卡片会按原交互导航到最近活动 Channel；真正新建前恢复右键前 Pane，
          // 让 session-connected 带来的新 Channel 被 SplitLayout 分配到用户原先选中的位置。
          selectPane(contextMenuOriginPaneRef.current);
          await openChannel(sessionId);
          // 自动展开父节点
          setExpandedIds(prev => new Set(prev).add(sessionId));
        } else if (tab?.state === "disconnected") {
          await reconnectSession(sessionId);
        }
        break;
      }
      case "connect_elevated": {
        const tab = state.tabs.find(t => t.id === sessionId);
        const registration = tab ? pluginRegistry.get(tab.pluginId) : undefined;
        const supportsElevation = Boolean(
          tab
          && registration?.manifest.capabilities.includes("elevated_session")
          && (registration.canCreateElevatedSession?.(tab.params ?? {}) ?? true),
        );
        if (!tab || !supportsElevation) break;
        if (tab.state === "connected" || tab.state === "transferring") {
          // 与普通“新建终端”一致：保留右键前 Pane 作为新管理员终端落点。
          selectPane(contextMenuOriginPaneRef.current);
          await openChannel(sessionId, true);
        } else if (tab.state === "disconnected") {
          await reconnectSession(sessionId, true);
        }
        setExpandedIds(prev => new Set(prev).add(sessionId));
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
        setPendingDeleteSessionId(sessionId);
        break;
      case "close_channel": {
        const parentId = menu.session?.parentId;
        if (parentId) {
          await closeChannel(sessionId, parentId);
        }
        break;
      }
    }
  }, [menu.session, menu.extension, tree, state.tabs, reconnectSession, disconnect, openChannel, closeChannel, selectPane, onEditSession, loggingSessions, startSessionLog, stopSessionLog]);

  const confirmSessionDelete = useCallback(() => {
    const sessionId = pendingDeleteSessionId;
    setPendingDeleteSessionId(null);
    if (sessionId) void deleteSession(sessionId);
  }, [deleteSession, pendingDeleteSessionId]);

  return (
    <div className={styles.sidebar}>
      {/* 顶部：标题 + 新建按钮 */}
      <div className={styles.header}>
        <span className={styles.title}>{t("session.sessions")}</span>
        <button
          data-testid="new-session-button"
          className={`${styles.addBtn} liquid-glass-button`}
          onClick={() => onNewSession?.()}
          title={t("session.newSession") + " (Ctrl+Shift+N)"}
        >
          <Icon name="plus" size="md" />
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
            const hasChildren = node.children.length > 0 || node.extensionChildren.length > 0;
            const supportsMultiple = pluginRegistry
              .get(node.tab.pluginId)?.manifest.capabilities.includes("multi_session") ?? false;
            const canExpand = (node.children.length > 0 && supportsMultiple) || node.extensionChildren.length > 0;
            const parentEndpoint = getSessionSubtitle(node.tab);
            const parentPaneId = sessionToPane[node.tab.id];

            return (
              <div key={node.tab.id}>
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
                        node.tab.state === "transferring" ? "status-transferring" :
                        node.tab.state === "connecting" ? "status-connecting" :
                        "status-idle"
                      }
                      size={10}
                    />
                    <div className={styles.itemText}>
                      <div className={styles.itemName} title={node.tab.name}>{node.tab.name}</div>
                      <div
                        className={styles.itemEndpoint}
                        title={parentEndpoint}
                      >
                        {parentEndpoint}
                      </div>
                    </div>
                  </div>
                  {paneCount > 1 && parentPaneId && (
                    <PaneMiniMap
                      layout={splitLayout.root}
                      paneId={parentPaneId}
                      selected={parentPaneId === splitLayout.selectedPaneId}
                      title={t("split.sessionLocation", { defaultValue: "已显示在分屏中" })}
                    />
                  )}
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
                    {/* SSH 子 channel（标签页） */}
                    {node.children.map(child => {
                      const childPaneId = sessionToPane[child.id];
                      return (
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
                          <div className={styles.itemText}>
                            <div className={styles.childNameRow}>
                              <div className={styles.itemName} title={child.name}>{child.name}</div>
                              {child.elevated && (
                                <Icon name="shield" size="xs" label={t("localShell.administrator")} />
                              )}
                            </div>
                            <div className={styles.itemEndpoint} title={child.endpoint}>{child.endpoint}</div>
                          </div>
                        </div>
                        {paneCount > 1 && childPaneId && (
                          <PaneMiniMap
                            layout={splitLayout.root}
                            paneId={childPaneId}
                            selected={childPaneId === splitLayout.selectedPaneId}
                            title={t("split.sessionLocation", { defaultValue: "已显示在分屏中" })}
                          />
                        )}
                        {state.activeTabId === child.id && (
                          <motion.div
                            className={styles.activeBar}
                            layoutId="activeBar"
                            transition={{ type: "spring", stiffness: 500, damping: 30 }}
                          />
                        )}
                      </motion.div>
                      );
                    })}
                    {/* 插件后台实体（不是 Session tab） */}
                    {node.extensionChildren.map(child => (
                      <motion.div
                        key={child.id}
                        className={`${styles.childItem} ${child.selected ? styles.active : ""}`}
                        whileHover={{ scale: 1.02 }}
                        whileTap={{ scale: 0.98 }}
                        onClick={() => handleExtensionChildSelect(node.tab, child)}
                        onContextMenu={(e) => handleExtensionContextMenu(e, node.tab, child.id)}
                      >
                        <div className={styles.itemLeft}>
                          <Icon
                            name={child.state === "connected" ? "status-connected" : child.state === "connecting" ? "status-connecting" : "status-disconnected"}
                            size={8}
                          />
                          <div className={styles.itemText}>
                            <div className={styles.itemName} title={child.name}>{child.name}</div>
                            <div className={styles.itemEndpoint} title={child.subtitle}>{child.subtitle}</div>
                          </div>
                        </div>
                        {child.selected && (
                          <motion.div
                            className={styles.activeBar}
                            layoutId={`extensionBar-${node.tab.id}`}
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

      <ConfirmDialog
        open={pendingDeleteSessionId !== null}
        title={t("fileManager.deleteConfirmTitle")}
        message={t("session.deleteConfirm")}
        intent="danger"
        size="compact"
        onConfirm={confirmSessionDelete}
        onCancel={() => setPendingDeleteSessionId(null)}
      />
    </div>
  );
}
