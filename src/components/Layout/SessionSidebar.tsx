import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { useSession, type NetworkPeerEntry } from "../../context/SessionContext";
import { useContextMenu } from "../../hooks/useContextMenu";
import ContextMenu from "../common/ContextMenu";
import Icon from "../common/Icon";
import type { ContextMenuItem } from "../common/ContextMenu";
import type { TabInfo } from "../../context/SessionContext";
import styles from "./SessionSidebar.module.css";

/** 树节点（扁平 TabInfo 渲染时推导；网络对端为 peerChildren，非标签页） */
interface TreeNode {
  tab: TabInfo;
  children: TabInfo[];
  peerChildren: NetworkPeerEntry[];
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
  const { state, switchTab, disconnect, deleteSession, connect, startSessionLog, stopSessionLog, loggingSessions, openChannel, closeChannel, selectNetworkPeer, disconnectNetworkPeer, clearNetworkPeer } = useSession();
  const [search, setSearch] = useState("");
  const { menu, openMenu, openPeerMenu, closeMenu } = useContextMenu();
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  // 构建树形结构（排序：connection_type → [网络会话: 传输层→角色] → endpoint → name）
  const tree = useMemo<TreeNode[]>(() => {
    // 网络调试会话按 params 显式分组（transport/role），避免 endpoint 字符串偶然顺序
    const groupKey = (tab: TabInfo): string => {
      if (tab.pluginId === "network") {
        const p = (tab.params ?? {}) as Record<string, unknown>;
        const transport = (p.transport as string | undefined) ?? "tcp";
        const role = (p.role as string | undefined) ?? "client";
        return `network/${transport}/${role}`;
      }
      return tab.connection_type;
    };
    // client 是单连接会话（无对端树）；仅 server 展示对端子节点
    const isNetworkClient = (tab: TabInfo): boolean => {
      if (tab.pluginId !== "network") return false;
      const p = (tab.params ?? {}) as Record<string, unknown>;
      return ((p.role as string | undefined) ?? "client") === "client";
    };
    const roots = [...state.tabs.filter(t => !t.parentId)];
    roots.sort((a, b) => {
      const ga = groupKey(a);
      const gb = groupKey(b);
      if (ga !== gb) return ga < gb ? -1 : 1;
      const endpointCmp = a.endpoint.localeCompare(b.endpoint, undefined, { numeric: true });
      if (endpointCmp !== 0) return endpointCmp;
      return a.name.localeCompare(b.name);
    });
    return roots.map(root => ({
      tab: root,
      children: state.tabs.filter(t => t.parentId === root.id),
      peerChildren: isNetworkClient(root) ? [] : (state.networkPeers[root.id] ?? []),
    }));
  }, [state.tabs, state.networkPeers]);

  // 最后一个子项删除后清理 expandedIds
  useEffect(() => {
    setExpandedIds(prev => {
      const next = new Set(prev);
      let changed = false;
      for (const id of prev) {
        const node = tree.find(n => n.tab.id === id);
        if (!node || (node.children.length === 0 && node.peerChildren.length === 0)) {
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

  // 自动展开：网络调试对端加入 → 展开所属容器节点（仅 server 容器，client 无对端树）
  const prevPeerIdsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    const currentIds = new Set<string>();
    for (const peers of Object.values(state.networkPeers)) {
      for (const p of peers) currentIds.add(p.peerId);
    }
    for (const [cid, peers] of Object.entries(state.networkPeers)) {
      // client 容器不渲染对端子节点：跳过，避免展开空容器
      const tab = state.tabs.find(t => t.id === cid);
      const isNetClient = tab?.pluginId === "network"
        && ((tab.params as Record<string, unknown> | undefined)?.role ?? "client") === "client";
      if (isNetClient) continue;
      const hasNew = peers.some(p => !prevPeerIdsRef.current.has(p.peerId));
      if (hasNew) {
        setExpandedIds(prev => {
          if (prev.has(cid)) return prev;
          return new Set(prev).add(cid);
        });
      }
    }
    prevPeerIdsRef.current = currentIds;
  }, [state.networkPeers, state.tabs]);

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
      const peerMatch = node.peerChildren.some(p =>
        p.name.toLowerCase().includes(searchLower)
        || p.addr.toLowerCase().includes(searchLower)
      );
      return parentMatch || childMatch || peerMatch;
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
    // 先展开（SSH connected 会话有子项 / 网络容器有对端时）
    if (node.children.length > 0 || node.peerChildren.length > 0) {
      setExpandedIds(prev => new Set(prev).add(node.tab.id));
    }
    // switchTab 内部会自动路由到第一个子 channel
    switchTab(node.tab.id);
    // 网络 server 容器：参照 SSH 主会话，点击容器自动选中第一个已连接对端；
    // 无对端时取消选择（UDP 网格回到"全部"时间线 / TCP 空状态）。
    // client 是单会话（无对端树），点击即切换会话，不应取消选择唯一对端。
    if (node.tab.pluginId === "network") {
      const netParams = (node.tab.params ?? {}) as Record<string, unknown>;
      if (((netParams.role as string | undefined) ?? "client") === "server") {
        const firstPeer = node.peerChildren.find(p => p.state === "connected");
        selectNetworkPeer(node.tab.id, firstPeer ? firstPeer.peerId : null);
      }
    }
    onSelectSession?.(node.tab.id);
  }, [switchTab, onSelectSession, selectNetworkPeer]);

  const handleChildSelect = useCallback((child: TabInfo) => {
    switchTab(child.id);
    onSelectSession?.(child.id);
  }, [switchTab, onSelectSession]);

  /** 网络对端：路由到容器 tab + 选中该对端（详情区/发送栏目标跟随） */
  const handlePeerChildSelect = useCallback((container: TabInfo, peerId: string) => {
    switchTab(container.id);
    selectNetworkPeer(container.id, peerId);
    onSelectSession?.(container.id);
  }, [switchTab, selectNetworkPeer, onSelectSession]);

  const handlePeerContextMenu = useCallback((e: React.MouseEvent, container: TabInfo, peerId: string) => {
    openPeerMenu(e, container, container.id, peerId);
  }, [openPeerMenu]);

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

    // 网络调试对端树节点
    if (menu.peer) {
      const peers = state.networkPeers[menu.peer.containerId] ?? [];
      const peer = peers.find(p => p.peerId === menu.peer!.peerId);
      if (!peer) return [];
      if (peer.state === "connected") {
        return [
          { id: "disconnect_peer", label: t("network.disconnect") || "Disconnect Peer", icon: "stop" },
        ];
      }
      return [
        { id: "clear_peer", label: t("network.clearClosed") || "Remove Peer", icon: "trash", danger: true },
      ];
    }

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
    items.push({ id: "delete", label: t("contextMenu.delete") || "Delete", icon: "trash", danger: true });
    return items;
  }, [menu.session, menu.peer, state.networkPeers, t, loggingSessions]);

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
      case "disconnect_peer": {
        const mp = menu.peer;
        if (mp) {
          await disconnectNetworkPeer(mp.containerId, mp.peerId);
        }
        break;
      }
      case "clear_peer": {
        const mp = menu.peer;
        if (mp) {
          await clearNetworkPeer(mp.containerId, mp.peerId);
        }
        break;
      }
    }
  }, [menu.session, menu.peer, state.tabs, t, connect, disconnect, deleteSession, openChannel, closeChannel, onEditSession, loggingSessions, startSessionLog, stopSessionLog, disconnectNetworkPeer, clearNetworkPeer]);

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
            const hasChildren = node.children.length > 0 || node.peerChildren.length > 0;
            const isSsh = node.tab.pluginId === "ssh";
            const isNetwork = node.tab.pluginId === "network";
            const isConnected = node.tab.state === "connected" || node.tab.state === "transferring";
            const canExpand = isConnected && hasChildren && (isSsh || isNetwork);
            // 网络 client 是单会话：本端地址并入端点行（连接后本机 ip:port，与服务端
            // 对端条目对应），保持与其余会话卡片一致的单行高度。
            // TCP client 本端地址来自对端条目；UDP client 无对端，来自 networkLocalAddrs。
            const netParams = (node.tab.params ?? {}) as Record<string, unknown>;
            const isNetClient = isNetwork && ((netParams.role ?? "client") as string) === "client";
            const clientLocalAddr = isNetClient
              ? ((netParams.transport as string) === "udp"
                  ? state.networkLocalAddrs[node.tab.id]
                  : state.networkPeers[node.tab.id]?.[0]?.localAddr)
              : undefined;

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
                        node.tab.state === "connecting" || node.tab.state === "transferring" ? "status-connecting" :
                        "status-idle"
                      }
                      size={10}
                    />
                    <div>
                      <div className={styles.itemName}>{node.tab.name}</div>
                      <div
                        className={styles.itemEndpoint}
                        title={clientLocalAddr ? `${node.tab.endpoint} · ${clientLocalAddr}` : node.tab.endpoint}
                      >
                        {clientLocalAddr ? `${node.tab.endpoint} · ${clientLocalAddr}` : node.tab.endpoint}
                      </div>
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
                    {/* SSH 子 channel（标签页） */}
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
                    {/* 网络调试对端（非标签页树节点） */}
                    {node.peerChildren.map(p => (
                      <motion.div
                        key={p.peerId}
                        className={`${styles.childItem} ${state.selectedNetworkPeer[node.tab.id] === p.peerId ? styles.active : ""}`}
                        whileHover={{ scale: 1.02 }}
                        whileTap={{ scale: 0.98 }}
                        onClick={() => handlePeerChildSelect(node.tab, p.peerId)}
                        onContextMenu={(e) => handlePeerContextMenu(e, node.tab, p.peerId)}
                      >
                        <div className={styles.itemLeft}>
                          <Icon
                            name={p.state === "connected" ? "status-connected" : "status-disconnected"}
                            size={8}
                          />
                          <div>
                            <div className={styles.itemName}>{p.name}</div>
                            <div className={styles.itemEndpoint}>{p.addr}</div>
                          </div>
                        </div>
                        {state.selectedNetworkPeer[node.tab.id] === p.peerId && (
                          <motion.div
                            className={styles.activeBar}
                            layoutId={`peerBar-${node.tab.id}`}
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
