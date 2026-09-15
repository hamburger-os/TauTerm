from pathlib import Path
import re


def read(path):
    return Path(path).read_text(encoding="utf-8")


def write(path, text):
    Path(path).write_text(text, encoding="utf-8")


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 literal match, got {count}")
    return text.replace(old, new, 1)


def sub_once(text, pattern, replacement, label, flags=0):
    text2, count = re.subn(pattern, replacement, text, count=1, flags=flags)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 regex match, got {count}")
    return text2

# SplitView: no protocol presentation state and no Network-only disconnected branch.
p = "src/components/Layout/SplitView.tsx"
s = read(p)
s = sub_once(s, r'import \{\n  getPaneDisplayLabel,\n  type SessionPresentationLabels,\n  type SessionPresentationNetworkState,\n\} from "\./sessionPresentation";', 'import { getPaneDisplayLabel } from "./sessionPresentation";', "SplitView presentation import")
s = replace_once(s, '''function shouldShowDisconnectedPlaceholder(tab: TabInfo, contentType: string): boolean {
  if (contentType === "terminal") return !terminalHasRuntime(tab);
  // Network Debug 的断开态以前由自定义 renderer 自己画空状态，导致图标框/字号与其它 Pane 漂移。
  // 断开时直接复用 SplitView 的统一 PaneEmptyState；连接/连接中仍交给 renderer。
  return tab.pluginId === "network" && tab.state === "disconnected";
}''', '''function shouldShowDisconnectedPlaceholder(tab: TabInfo, contentType: string): boolean {
  if (contentType === "terminal") return !terminalHasRuntime(tab);
  return tab.state === "disconnected";
}''', "SplitView disconnected policy")
s = sub_once(s, r'\n  const presentationLabels = useMemo<SessionPresentationLabels>\(\(\) => \(\{.*?\n  \}\), \[sessionState\.networkLocalAddrs, sessionState\.networkPeers\]\);\n', '\n', "SplitView legacy runtime presentation", re.S)
s = replace_once(s, 'getPaneDisplayLabel(tab, tabsById, presentationLabels, presentationNetworkState)', 'getPaneDisplayLabel(tab, tabsById)', "SplitView label call")
s = s.replace('peer: null,', 'extension: null,')
write(p, s)

# TerminalView: local echo is plugin runtime policy, not TabInfo state.
p = "src/components/Terminal/TerminalView.tsx"
s = read(p)
s = replace_once(s, 'import { PendingSessionData } from "../../core/pending-session-data";', 'import { PendingSessionData } from "../../core/pending-session-data";\nimport { pluginRegistry } from "../../core/plugin-registry";', "Terminal plugin import")
s = replace_once(s, '''      if (tab.localEcho) {
        const writeFn = writeRefs.current.get(sessionId);''', '''      const plugin = pluginRegistry.get(tab.pluginId);
      const runtimeSnapshot = plugin?.runtimeStore?.getSnapshot(sessionId);
      if (plugin?.terminalLocalEcho?.(runtimeSnapshot)) {
        const writeFn = writeRefs.current.get(sessionId);''', "Terminal local echo")
write(p, s)

# Network view: consume Network plugin runtime directly.
p = "src/components/Network/NetworkDebugSessionView.tsx"
s = read(p)
s = replace_once(s, 'import { useSession, type NetworkPeerEntry } from "../../context/SessionContext";', 'import { useSession } from "../../context/SessionContext";\nimport { usePluginRuntime } from "../../core/usePluginRuntime";\nimport {\n  getNetworkRuntime,\n  refreshNetworkPeers,\n  registerNetworkUdpSource,\n  selectNetworkPeer,\n  subscribeNetworkManualSent,\n  type NetworkPeerEntry,\n  type NetworkRuntimeSnapshot,\n} from "../../plugins/network/runtime-store";', "Network view imports")
s = replace_once(s, 'const { state: sessionState, selectNetworkPeer, getNetworkPeers, mergeNetworkPeers, registerNetworkUdpSource, subscribeDataSent, subscribeNetworkManualSent, updateSessionStats } = useSession();', 'const { state: sessionState, subscribeDataSent, updateSessionStats } = useSession();', "Network view session API")
s = replace_once(s, '''  // 对端数据源（SessionContext；ref 镜像供全局监听器读取最新列表）
  const peers = sessionState.networkPeers[sessionId] ?? [];
  const selectedPeerId = sessionState.selectedNetworkPeer[sessionId] ?? null;
  const peersRef = useRef<NetworkPeerEntry[]>([]);
  peersRef.current = peers;''', '''  // 对端运行态完全由 Network 插件拥有；公共 SessionContext 不解释 peer 语义。
  const runtime = usePluginRuntime<NetworkRuntimeSnapshot>("network", sessionId);
  const peers = [...runtime.peers];
  const selectedPeerId = runtime.selectedPeerId;
  const getNetworkPeers = useCallback((id: string): NetworkPeerEntry[] => [...getNetworkRuntime(id).peers], []);
  const peersRef = useRef<NetworkPeerEntry[]>([]);
  peersRef.current = peers;''', "Network view runtime source")
s = sub_once(s, r'''    getStatus: async \(sid, _api\) => \{\n      try \{\n        const list = await invoke<any\[]>\("list_network_peers", \{ sessionId: sid \}\);.*?\n      \}\n    \},''', '''    getStatus: async (sid, _api) => {
      await refreshNetworkPeers(sid);
      return undefined;
    },''', "Network view refresh status", re.S)
s = s.replace('SessionContext stateRef', 'Network runtime store')
s = s.replace('SessionContext；', 'Network runtime；')
s = s.replace('SessionContext.sendData', 'Network plugin send contribution')
s = s.replace('对端注册由 SessionContext 级联清理', '对端注册由 Network runtime store 级联清理')
write(p, s)

# SessionSidebar: generic extension tree contributions replace Network peer knowledge.
p = "src/components/Layout/SessionSidebar.tsx"
s = read(p)
s = replace_once(s, 'import { useSession, type NetworkPeerEntry } from "../../context/SessionContext";', 'import { useSession } from "../../context/SessionContext";', "Sidebar session import")
s = sub_once(s, r'import \{\n  getSessionSubtitle,\n  type SessionPresentationLabels,\n  type SessionPresentationNetworkState,\n\} from "\./sessionPresentation";', 'import { getSessionSubtitle } from "./sessionPresentation";', "Sidebar presentation import")
s = replace_once(s, 'import { pluginRegistry } from "../../core/plugin-registry";', 'import { pluginRegistry, type PluginSessionTreeChild } from "../../core/plugin-registry";\nimport { usePluginRuntimeRevision } from "../../core/usePluginRuntime";', "Sidebar plugin imports")
s = replace_once(s, '''interface TreeNode {
  tab: TabInfo;
  children: TabInfo[];
  peerChildren: NetworkPeerEntry[];
}''', '''interface TreeNode {
  tab: TabInfo;
  children: TabInfo[];
  extensionChildren: PluginSessionTreeChild[];
}''', "Sidebar tree node")
s = replace_once(s, 'const { state, switchTab, disconnect, deleteSession, reconnectSession, startSessionLog, stopSessionLog, loggingSessions, openChannel, closeChannel, selectNetworkPeer, disconnectNetworkPeer, clearNetworkPeer } = useSession();', 'const { state, switchTab, disconnect, deleteSession, reconnectSession, startSessionLog, stopSessionLog, loggingSessions, openChannel, closeChannel } = useSession();', "Sidebar session actions")
s = replace_once(s, 'const { menu, openMenu, openPeerMenu, closeMenu } = useContextMenu();', 'const { menu, openMenu, openExtensionMenu, closeMenu } = useContextMenu();', "Sidebar menu hook")
s = sub_once(s, r'\n  const presentationLabels = useMemo<SessionPresentationLabels>\(\(\) => \(\{.*?\n  \}\), \[state\.networkLocalAddrs, state\.networkPeers\]\);\n', '\n  const runtimeRevision = usePluginRuntimeRevision();\n', "Sidebar legacy presentation", re.S)
s = sub_once(s, r'''  // 构建树形结构.*?\n  \}, \[state\.tabs, state\.networkPeers\]\);''', '''  // 构建树形结构。插件可贡献非 Session 的后台实体，但公共层只消费通用节点合同。
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
  }, [state.tabs, runtimeRevision]);''', "Sidebar tree build", re.S)
s = s.replace('node.children.length === 0 && node.peerChildren.length === 0', 'node.children.length === 0 && node.extensionChildren.length === 0')
s = sub_once(s, r'''  // 自动展开：网络调试对端加入.*?\n  \}, \[state\.networkPeers, state\.tabs\]\);''', '''  // 插件后台实体新增时自动展开所属根会话。
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
  }, [tree]);''', "Sidebar extension auto expand", re.S)
s = replace_once(s, 'getSessionSubtitle(node.tab, presentationLabels, presentationNetworkState).toLowerCase()', 'getSessionSubtitle(node.tab).toLowerCase()', "Sidebar subtitle search")
s = replace_once(s, '''      const peerMatch = node.peerChildren.some(p =>
        p.name.toLowerCase().includes(searchLower)
        || p.addr.toLowerCase().includes(searchLower)
      );
      return parentMatch || childMatch || peerMatch;''', '''      const extensionMatch = node.extensionChildren.some(child =>
        child.name.toLowerCase().includes(searchLower)
        || child.subtitle.toLowerCase().includes(searchLower)
      );
      return parentMatch || childMatch || extensionMatch;''', "Sidebar extension search")
s = replace_once(s, '}, [tree, search, presentationLabels, presentationNetworkState]);', '}, [tree, search, searchLower]);', "Sidebar search deps")
s = sub_once(s, r'''  const handleParentSelect = useCallback\(\(node: TreeNode\) => \{.*?\n  \}, \[switchTab, onSelectSession, selectNetworkPeer\]\);''', '''  const handleParentSelect = useCallback((node: TreeNode) => {
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
  }, [switchTab, onSelectSession]);''', "Sidebar parent select", re.S)
s = sub_once(s, r'''  /\*\* 网络对端：.*?\n  \}, \[openPeerMenu\]\);''', '''  const handleExtensionChildSelect = useCallback((container: TabInfo, child: PluginSessionTreeChild) => {
    switchTab(container.id);
    child.onSelect?.();
    onSelectSession?.(container.id);
  }, [switchTab, onSelectSession]);

  const handleExtensionContextMenu = useCallback((e: React.MouseEvent, container: TabInfo, childId: string) => {
    openExtensionMenu(e, container, container.id, childId);
  }, [openExtensionMenu]);''', "Sidebar extension handlers", re.S)
s = sub_once(s, r'''    // 网络调试对端树节点\n    if \(menu\.peer\) \{.*?\n    \}\n\n    const \{ state: sessionState,''', '''    if (menu.extension) {
      const node = tree.find(item => item.tab.id === menu.extension!.parentSessionId);
      const child = node?.extensionChildren.find(item => item.id === menu.extension!.childId);
      return (child?.menuItems ?? []).map(item => ({
        id: item.id,
        label: item.label,
        icon: item.icon,
        danger: item.danger,
      }));
    }

    const { state: sessionState,''', "Sidebar extension menu items", re.S)
s = replace_once(s, '}, [menu.session, menu.peer, state.networkPeers, t, loggingSessions]);', '}, [menu.session, menu.extension, tree, t, loggingSessions]);', "Sidebar menu deps")
s = replace_once(s, '''  const handleMenuSelect = useCallback(async (itemId: string) => {
    const sessionId = menu.session?.id || "";

    switch (itemId) {''', '''  const handleMenuSelect = useCallback(async (itemId: string) => {
    if (menu.extension) {
      const node = tree.find(item => item.tab.id === menu.extension!.parentSessionId);
      const child = node?.extensionChildren.find(item => item.id === menu.extension!.childId);
      await child?.menuItems?.find(item => item.id === itemId)?.run();
      return;
    }
    const sessionId = menu.session?.id || "";

    switch (itemId) {''', "Sidebar menu extension dispatch")
s = sub_once(s, r'''      case "disconnect_peer": \{.*?\n      \}\n      case "clear_peer": \{.*?\n      \}\n''', '', "Sidebar old peer menu actions", re.S)
s = replace_once(s, '}, [menu.session, menu.peer, state.tabs, reconnectSession, disconnect, openChannel, closeChannel, selectPane, onEditSession, loggingSessions, startSessionLog, stopSessionLog, disconnectNetworkPeer, clearNetworkPeer]);', '}, [menu.session, menu.extension, tree, state.tabs, reconnectSession, disconnect, openChannel, closeChannel, selectPane, onEditSession, loggingSessions, startSessionLog, stopSessionLog]);', "Sidebar menu select deps")
s = replace_once(s, 'const hasChildren = node.children.length > 0 || node.peerChildren.length > 0;\n            const isNetwork = node.tab.pluginId === "network";', 'const hasChildren = node.children.length > 0 || node.extensionChildren.length > 0;', "Sidebar render children")
s = replace_once(s, 'const canExpand = hasChildren && (supportsMultiple || isNetwork);', 'const canExpand = (node.children.length > 0 && supportsMultiple) || node.extensionChildren.length > 0;', "Sidebar expand policy")
s = sub_once(s, r'''            const parentEndpoint = getSessionSubtitle\(\n              node\.tab,\n              presentationLabels,\n              presentationNetworkState,\n            \);''', '            const parentEndpoint = getSessionSubtitle(node.tab);', "Sidebar render subtitle")
s = sub_once(s, r'''                    /\* 网络调试对端（非标签页树节点） \*/\n                    \{node\.peerChildren\.map\(p => \(.*?\n                    \)\)\}''', '''                    {/* 插件后台实体（不是 Session tab） */}
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
                    ))}''', "Sidebar extension render", re.S)
write(p, s)

# App: SSH panel flags are derived from plugin params rather than duplicated TabInfo fields.
p = "src/App.tsx"
s = read(p)
s = s.replace('(activePlugin?.manifest.id === "ssh" && activeTab.fileServiceEnabled === true)', '(activePlugin?.manifest.id === "ssh" && activeTab.params?.file_service_enabled === true)')
s = s.replace('(activePlugin?.manifest.id === "ssh" && activeTab.journaldEnabled === true)', '(activePlugin?.manifest.id === "ssh" && activeTab.params?.journald_enabled === true)')
s = s.replace('tabPlugin?.manifest.id === "ssh" && tab.fileServiceEnabled === true', 'tabPlugin?.manifest.id === "ssh" && tab.params?.file_service_enabled === true')
s = s.replace('tabPlugin?.manifest.id === "ssh" && tab.journaldEnabled === true', 'tabPlugin?.manifest.id === "ssh" && tab.params?.journald_enabled === true')
write(p, s)

print("frontend consumer migration applied")
