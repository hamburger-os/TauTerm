from pathlib import Path
import re

P = Path('src/context/SessionContext.tsx')
s = P.read_text(encoding='utf-8')


def lit(old, new, label):
    global s
    n = s.count(old)
    if n != 1:
        raise SystemExit(f'{label}: expected 1 match, got {n}')
    s = s.replace(old, new, 1)


def sub(pattern, replacement, label, flags=0):
    global s
    s2, n = re.subn(pattern, replacement, s, count=1, flags=flags)
    if n != 1:
        raise SystemExit(f'{label}: expected 1 match, got {n}')
    s = s2

# Public/core types are protocol-agnostic.
sub(r'\n/\*\* 前端只接收用户可访问的虚拟端点.*?\n\}\n', '\n', 'remove VirtualPortEndpoint', re.S)
sub(r'\n  /\*\* Telnet: 本地回显状态.*?\n  journaldEnabled\?: boolean;', '', 'remove TabInfo private fields', re.S)
s = s.replace('\n  journaldEnabled?: boolean;', '')
sub(r'\n/\*\* 网络调试会话的对端条目.*?\n\}\n', '\n', 'remove NetworkPeerEntry', re.S)
sub(r'\n  peerSessions: Record<string, boolean>;.*?\n  networkBroadcast: Record<string, boolean>;', '', 'remove SessionState network fields', re.S)

# Action vocabulary contains only generic session lifecycle/configuration.
s = s.replace('\n  | { type: "UPDATE_TAB_ECHO"; id: string; localEcho: boolean }', '')
sub(r'  \| \{ type: "UPDATE_TAB_CONFIG";[^\n]+\}', '  | { type: "UPDATE_TAB_CONFIG"; id: string; endpoint: string; params: Record<string, unknown>; name: string; transferEnabled?: boolean; transferProtocol?: string; sendBarEnabled?: boolean; pluginId?: string; connectedAt?: number | null }', 'generic UPDATE_TAB_CONFIG')
sub(r'\n  \| \{ type: "UPDATE_TAB_VPORTS".*?\n  \| \{ type: "SET_NETWORK_BROADCAST"; containerId: string; on: boolean \}', '', 'remove private actions', re.S)
sub(r'\n  peerSessions: \{\},.*?\n  networkBroadcast: \{\},', '', 'remove initial private state', re.S)
s = s.replace('\n    case "UPDATE_TAB_ECHO":\n      return { ...state, tabs: state.tabs.map(t => t.id === action.id ? { ...t, localEcho: action.localEcho } : t) };', '')
sub(r'\n          fileServiceEnabled: action\.fileServiceEnabled.*?\n          journaldEnabled: action\.journaldEnabled.*?,', '', 'remove private config reducer fields', re.S)
sub(r'\n    case "UPDATE_TAB_VPORTS":.*?\n    case "CLEAR_VPORT_ERROR":\n      return \{.*?\};', '', 'remove vport reducer', re.S)
sub(r'\n    case "SET_PEER_CONNECTED":.*?\n    case "SET_NETWORK_BROADCAST": return \{.*?\};', '', 'remove network reducer', re.S)

# Context API and provider refs.
lit('reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string, journaldEnabled?: boolean) => Promise<void>;', 'reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string) => Promise<void>;', 'generic reconfigure API')
sub(r'\n  selectNetworkPeer:.*?\n  setNetworkBroadcast: \(containerId: string, on: boolean\) => void;', '', 'remove network context API', re.S)
sub(r'\n  const peerSessionsRef = .*?\n  const networkManualSentSubscribersRef = .*?;', '', 'remove network refs', re.S)
s = s.replace('\n  const stateRef = useRef(state);\n  stateRef.current = state;', '')
s = s.replace('\n  const pendingEchoRef = useRef<Map<string, boolean>>(new Map());', '')

# Generic connection request no longer duplicates SSH settings already present in params.
lit('const { endpoint, params, name, pluginId, transferEnabled, transferProtocol, sendBarEnabled, journaldEnabled, sessionId, initialElevated } = opts;', 'const { endpoint, params, name, pluginId, transferEnabled, transferProtocol, sendBarEnabled, sessionId, initialElevated } = opts;', 'connect destructuring')
s = s.replace('\n          journaldEnabled: journaldEnabled ?? false,', '')
s = s.replace('\n      journaldEnabled: tab.journaldEnabled,', '')
sub(r'\n          fileServiceEnabled: \(params\.file_service_enabled as boolean\) \?\? false,.*?\n          journaldEnabled: \(params\.journald_enabled as boolean\) \?\? false,', '', 'offline private tab fields', re.S)
lit('    dispatch({ type: "REMOVE_TAB", id: sessionId });\n    releaseSessionStore(sessionId);', '    dispatch({ type: "REMOVE_TAB", id: sessionId });\n    tab && pluginRegistry.get(tab.pluginId)?.runtimeStore?.release?.(sessionId);\n    releaseSessionStore(sessionId);', 'plugin runtime release')

# Data plane write is generic; plugin send contribution may target plugin-owned runtime IDs.
sub(r'''  const sendData = useCallback\(async \(sessionId: string, data: string \| Uint8Array\) => \{.*?\n  \}, \[\]\);''', '''  const writeData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const isText = typeof data === "string";
    const bytes = isText ? new TextEncoder().encode(data) : data;
    const written = await invoke<number[]>("write_data", { sessionId, data: Array.from(bytes), transcode: isText });
    const writtenBytes = new Uint8Array(written);
    sentDataCallbackRef.current?.(sessionId, writtenBytes);
    sentDataSubscribersRef.current.forEach(callback => callback(sessionId, writtenBytes));
  }, []);

  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state === "disconnected") return;
    try {
      await writeData(sessionId, data);
    } catch (error) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${error}` });
    }
  }, [writeData]);''', 'generic sendData', re.S)

# Reconfigure: plugin-private switches remain inside params.
s = s.replace('\n    pluginId?: string,\n    journaldEnabled?: boolean,', '\n    pluginId?: string,')
sub(r'\n      journaldEnabled: journaldEnabled.*?\n      fileServiceProtocol: .*?,', '', 'remove reconfigure private dispatch', re.S)
s = s.replace('\n            journaldEnabled: (persistedParams?.journald_enabled as boolean) ?? tab?.journaldEnabled ?? false,', '')
sub(r'\n          fileServiceEnabled: \(s\.params\?\.file_service_enabled as boolean\) \?\? false,.*?\n          journaldEnabled: \(s\.params\?\.journald_enabled as boolean\) \?\? false,', '', 'remove loaded private tab fields', re.S)

# Network runtime operations disappear from SessionContext. sendToTarget delegates generically.
sub(r'''  const isSessionConnected = useCallback\(\(sessionId: string\): boolean => \{.*?\n  \}, \[\]\);''', '''  const isSessionConnected = useCallback((sessionId: string): boolean => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    return tab?.state === "connected" || tab?.state === "transferring";
  }, []);''', 'generic connected predicate', re.S)
sub(r'''  const selectNetworkPeer = useCallback.*?\n  const sendToTarget = useCallback\(async \(containerId: string, data: string \| Uint8Array\) => \{.*?\n  \}, \[sendData\]\);''', '''  const sendToTarget = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state === "disconnected") return;
    const plugin = pluginRegistry.get(tab.pluginId);
    try {
      if (plugin?.sendData) {
        await plugin.sendData({
          sessionId,
          params: tab.params ?? {},
          data,
          sendDefault: writeData,
        });
      } else {
        await writeData(sessionId, data);
      }
    } catch (error) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${error}` });
    }
  }, [writeData]);''', 'plugin send contribution', re.S)

# Event handling: SessionContext subscribes only to protocol-agnostic lifecycle/data events.
sub(r'''\n      const u1b = await listen<\{ session_id: string; local_echo: boolean \}>\("telnet-echo-state".*?\n      unlisteners\.push\(u1b\);\n''', '\n', 'remove telnet event listener', re.S)
sub(r'''      const u2 = await listen<\{.*?\}>\("session-connected", event => \{.*?\n      unlisteners\.push\(u2\);''', '''      const u2 = await listen<{
        session_id: string;
        endpoint: string;
        connection_type: string;
        plugin_id: string;
        name: string;
        params: Record<string, unknown>;
        connected_at?: number | null;
        transfer_enabled?: boolean;
        transfer_protocol?: string;
        send_bar_enabled?: boolean;
        parent_id?: string | null;
        channel_index?: number;
        elevated?: boolean;
        is_container?: boolean;
      }>("session-connected", event => {
        const sid = event.payload.session_id;
        const eventPluginId = event.payload.plugin_id;
        const eventParams = pluginRegistry.get(eventPluginId)?.normalizeConnectionParams?.(event.payload.params) ?? event.payload.params;
        const eventSendBarEnabled = pluginRegistry.resolveSendBarEnabled(eventPluginId, event.payload.send_bar_enabled);
        const parentId = event.payload.parent_id ?? null;
        const existingTab = tabsRef.current.find(tab => tab.id === sid);
        if (existingTab) {
          dispatch({ type: "SET_TAB_STATE", id: sid, state: "connected" });
          dispatch({
            type: "UPDATE_TAB_CONFIG",
            id: sid,
            endpoint: event.payload.endpoint,
            params: eventParams,
            name: existingTab.name,
            transferEnabled: event.payload.transfer_enabled,
            transferProtocol: event.payload.transfer_protocol,
            sendBarEnabled: eventSendBarEnabled,
            pluginId: eventPluginId,
            connectedAt: event.payload.connected_at ?? Date.now(),
          });
        } else if (parentId) {
          dispatch({ type: "SET_TAB_STATE", id: parentId, state: "connected" });
          dispatch({
            type: "ADD_TAB",
            tab: {
              id: sid,
              name: event.payload.name,
              connection_type: event.payload.connection_type,
              endpoint: event.payload.endpoint,
              state: "connected",
              pluginId: eventPluginId,
              params: eventParams,
              stats: { txBytes: 0, rxBytes: 0 },
              connectedAt: event.payload.connected_at ?? Date.now(),
              transferEnabled: event.payload.transfer_enabled ?? false,
              transferProtocol: event.payload.transfer_protocol,
              sendBarEnabled: eventSendBarEnabled,
              parentId,
              channelIndex: event.payload.channel_index,
              elevated: event.payload.elevated ?? false,
            },
          });
        } else {
          dispatch({
            type: "ADD_TAB",
            tab: {
              id: sid,
              name: event.payload.name || `${pluginRegistry.get(eventPluginId)?.manifest.name || eventPluginId.toUpperCase()} @ ${event.payload.endpoint}`,
              connection_type: event.payload.connection_type,
              endpoint: event.payload.endpoint,
              state: "connected",
              pluginId: eventPluginId,
              params: eventParams,
              stats: { txBytes: 0, rxBytes: 0 },
              connectedAt: event.payload.connected_at ?? Date.now(),
              transferEnabled: event.payload.transfer_enabled ?? true,
              transferProtocol: event.payload.transfer_protocol,
              sendBarEnabled: eventSendBarEnabled,
              isContainer: event.payload.is_container ?? false,
            },
          });
        }
      });
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);''', 'generic session-connected listener', re.S)
sub(r'''\n      const u2b = await listen.*?\n      unlisteners\.push\(u2d\);\n''', '\n', 'remove virtual-port listeners', re.S)
sub(r'''\n      const u2f = await listen.*?\n      unlisteners\.push\(u2g\);\n''', '\n', 'remove network peer listeners', re.S)
sub(r'''      const u3 = await listen<\{ session_id: string; reason\?: string; disconnect_info\?: DisconnectInfo \}>\("session-disconnected", event => \{.*?\n      \}\);''', '''      const u3 = await listen<{ session_id: string; reason?: string; disconnect_info?: DisconnectInfo }>("session-disconnected", event => {
        const sid = event.payload.session_id;
        dispatch({ type: "SET_TAB_DISCONNECTED", id: sid, info: event.payload.disconnect_info });
        if (!event.payload.disconnect_info?.retain_terminal) dispatch({ type: "REMOVE_ALL_CHILDREN", parentId: sid });
        disconnectCallbackRef.current?.(sid, event.payload.reason);
        setLoggingSessions(prev => {
          if (!prev.has(sid)) return prev;
          const next = new Set(prev);
          next.delete(sid);
          invoke("stop_session_log", { sessionId: sid }).catch(() => {});
          return next;
        });
        setLogStatuses(prev => {
          const next = new Map(prev);
          next.delete(sid);
          return next;
        });
      });''', 'generic disconnect listener', re.S)
sub(r'''      const u9 = await listen<\{ tab_id: string; tx_bytes: number; rx_bytes: number; connected_at\?: number \| null \}>\("session-stats", event => \{.*?\n      \}\);''', '''      const u9 = await listen<{ tab_id: string; tx_bytes: number; rx_bytes: number; connected_at?: number | null }>("session-stats", event => {
        dispatch({ type: "UPDATE_TAB_STATS", id: event.payload.tab_id, stats: { txBytes: event.payload.tx_bytes, rxBytes: event.payload.rx_bytes }, connectedAt: event.payload.connected_at });
      });''', 'generic stats listener', re.S)

# Provider exports only common Session API.
sub(r'''\n      selectNetworkPeer,\n      getNetworkPeers,\n      disconnectNetworkPeer,\n      clearNetworkPeer,\n      mergeNetworkPeers,\n      setNetworkManualTarget,\n      registerNetworkUdpSource,\n      subscribeNetworkManualSent,\n      setNetworkBroadcast,''', '', 'remove private provider exports')

P.write_text(s, encoding='utf-8')
print('SessionContext protocol-private runtime state removed')
