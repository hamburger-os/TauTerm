import { createContext, useContext, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { pluginRegistry } from "../core/plugin-registry";
import { releaseSessionStore } from "../hooks/usePluginSessionStore";
import i18n from "../i18n";
import { AsyncRequestScope } from "../core/session-lifecycle";

// ── Types ───────────────────────────────────────────

import {
  initialSessionState, sessionReducer,
  type SessionAction, type SessionState, type TabInfo, type SavedSessionConfig,
  type ConnectionStatus, type DisconnectInfo, type ConnectOptions, type ConnectionTypeInfo, type EndpointInfo,
} from "../core/session-state";
export type { TabInfo, SessionStats, ConnectionStatus, DisconnectInfo, ConnectOptions, ConnectionTypeInfo, EndpointInfo } from "../core/session-state";

function decodeBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const len = binary.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

interface SessionContextValue {
  state: SessionState;
  fetchConnectionTypes: () => Promise<void>;
  refreshEndpoints: (pluginId?: string, force?: boolean) => Promise<void>;
  connect: (opts: ConnectOptions) => Promise<string | null>;
  reconnectSession: (sessionId: string, initialElevated?: boolean) => Promise<string | null>;
  createOfflineSession: (endpoint: string, params: Record<string, unknown>, name: string | undefined, pluginId: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean) => Promise<string | null>;
  disconnect: (sessionId: string) => Promise<boolean>;
  deleteSession: (sessionId: string, skipDisconnect?: boolean) => Promise<void>;
  sendData: (sessionId: string, data: string | Uint8Array) => Promise<void>;
  switchTab: (sessionId: string | null) => Promise<void>;
  renameTab: (sessionId: string, name: string) => Promise<void>;
  reconfigureSession: (sessionId: string, endpoint: string, params: Record<string, unknown>, name?: string, transferEnabled?: boolean, transferProtocol?: string, sendBarEnabled?: boolean, pluginId?: string) => Promise<void>;
  openChannel: (parentSessionId: string, elevated?: boolean) => Promise<string | null>;
  closeChannel: (channelId: string, parentId: string) => Promise<void>;
  onSessionData: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  onDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => void;
  subscribeDataSent: (callback: (sessionId: string, data: Uint8Array) => void) => () => void;
  isSessionConnected: (sessionId: string) => boolean;
  isSessionRuntimeCurrent: (sessionId: string, runtimeId: string) => boolean;
  getSessionRuntimeId: (sessionId: string) => string | null;
  sendToTarget: (containerId: string, data: string | Uint8Array) => Promise<void>;
  updateSessionStats: (sessionId: string, txBytes: number, rxBytes: number, rxPackets?: number, txPackets?: number) => void;
  onSessionDisconnect: (callback: (sessionId: string, reason?: string) => void) => void;
  clearError: () => void;
  reportError: (message: string) => void;
  startSessionLog: (sessionId: string) => Promise<string>;
  stopSessionLog: (sessionId: string) => Promise<void>;
  loggingSessions: Set<string>;
  logStatuses: Map<string, { fileName: string; bytesWritten: number }>;
}

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState(initialSessionState);
  const stateRef = useRef(state);
  const dispatch = useCallback((action: SessionAction) => {
    const next = sessionReducer(stateRef.current, action);
    stateRef.current = next;
    setState(next);
  }, []);
  const operationScopes = useRef(new Map<string, AsyncRequestScope>());
  const pendingOperations = useRef(new Set<string>());
  const scopeFor = (sessionId: string) => {
    let scope = operationScopes.current.get(sessionId);
    if (!scope) { scope = new AsyncRequestScope(); operationScopes.current.set(sessionId, scope); }
    return scope;
  };
  const dataCallbackRef = useRef<((sessionId: string, data: Uint8Array) => void) | null>(null);
  const sentDataCallbackRef = useRef<((sessionId: string, data: Uint8Array) => void) | null>(null);
  const sentDataSubscribersRef = useRef<Set<(sessionId: string, data: Uint8Array) => void>>(new Set());
  const disconnectCallbackRef = useRef<((sessionId: string, reason?: string) => void) | null>(null);
  const tabsRef = { get current() { return stateRef.current.tabs; } };
  const lastActiveChildRef = useRef<Map<string, string>>(new Map());
  const endpointRefreshesRef = useRef<Map<string, Promise<EndpointInfo[]>>>(new Map());
  const endpointRefreshCompletedAtRef = useRef<Map<string, number>>(new Map());
  const catalogReadRef = useRef<Promise<SavedSessionConfig[]> | null>(null);

  useEffect(() => {
    const activeId = state.activeTabId;
    if (!activeId) return;
    const activeTab = state.tabs.find(tab => tab.id === activeId);
    if (activeTab?.parentId) lastActiveChildRef.current.set(activeTab.parentId, activeTab.id);
  }, [state.activeTabId, state.tabs]);

  const [loggingSessions, setLoggingSessions] = useState<Set<string>>(new Set());
  const [logStatuses, setLogStatuses] = useState<Map<string, { fileName: string; bytesWritten: number }>>(new Map());

  const startSessionLog = useCallback(async (sessionId: string): Promise<string> => {
    try {
      await invoke<string>("start_session_log", { sessionId });
      setLoggingSessions(prev => new Set(prev).add(sessionId));
      const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> = await invoke("get_log_status");
      setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }])));
      return sessionId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `启动日志失败: ${e}` });
      throw e;
    }
  }, []);

  const stopSessionLog = useCallback(async (sessionId: string) => {
    try {
      await invoke("stop_session_log", { sessionId });
      setLoggingSessions(prev => {
        const next = new Set(prev);
        next.delete(sessionId);
        return next;
      });
      setLogStatuses(prev => {
        const next = new Map(prev);
        next.delete(sessionId);
        return next;
      });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `停止日志失败: ${e}` });
    }
  }, []);

  const fetchConnectionTypes = useCallback(async () => {
    try {
      const types = await invoke<ConnectionTypeInfo[]>("get_connection_types");
      dispatch({ type: "SET_CONNECTION_TYPES", types });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `${e}` });
    }
  }, []);

  const refreshEndpoints = useCallback(async (pluginId?: string, force = false) => {
    const discoverableIds = pluginRegistry.getByCapability("endpoint_discovery").map(plugin => plugin.manifest.id);
    const pluginIds = pluginId ? (discoverableIds.includes(pluginId) ? [pluginId] : []) : discoverableIds;
    await Promise.all(pluginIds.map(async currentPluginId => {
      const lastCompleted = endpointRefreshCompletedAtRef.current.get(currentPluginId) ?? 0;
      if (!force && Date.now() - lastCompleted < 5000) return;
      let pending = endpointRefreshesRef.current.get(currentPluginId);
      if (!pending) {
        pending = invoke<EndpointInfo[]>("enumerate_endpoints", { pluginId: currentPluginId });
        endpointRefreshesRef.current.set(currentPluginId, pending);
      }
      try {
        const endpoints = await pending;
        endpointRefreshCompletedAtRef.current.set(currentPluginId, Date.now());
        dispatch({ type: "REPLACE_ENDPOINTS_FOR_PLUGIN", pluginId: currentPluginId, endpoints });
      } catch (e) {
        console.warn(`Endpoint discovery failed for ${currentPluginId}:`, e);
      } finally {
        if (endpointRefreshesRef.current.get(currentPluginId) === pending) endpointRefreshesRef.current.delete(currentPluginId);
      }
    }));
  }, []);

  const connect = useCallback(async (opts: ConnectOptions) => {
    const { endpoint, params, name, pluginId, transferEnabled, transferProtocol, sendBarEnabled, sessionId, initialElevated } = opts;
    const plugin = pluginRegistry.get(pluginId);
    const effectiveParams = plugin?.normalizeConnectionParams?.(params) ?? params;
    const sessionOptions = pluginRegistry.resolveSessionOptions(pluginId, {
      transferEnabled,
      transferProtocol,
      sendBarEnabled,
    });
    if (sessionId && (pendingOperations.current.has(sessionId)
      || tabsRef.current.find(tab => tab.id === sessionId)?.state !== "disconnected")) return null;
    const isCurrent = sessionId ? scopeFor(sessionId).begin() : () => true;
    if (sessionId) pendingOperations.current.add(sessionId);
    dispatch({ type: "SET_ERROR", error: null });
    if (sessionId) dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connecting" });
    try {
      return await invoke<string>("connect_session", {
        request: {
          endpoint,
          params: effectiveParams,
          name,
          pluginId,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol ?? null,
          sendBarEnabled: sessionOptions.sendBarEnabled,
          sessionId: sessionId || null,
          initialElevated: initialElevated ?? false,
        },
      });
    } catch (e) {
      if (isCurrent() && (!sessionId || tabsRef.current.find(tab => tab.id === sessionId)?.state === "connecting")) {
        const error = plugin?.formatSessionError?.(e, "connect") ?? String(e);
        dispatch({ type: "SET_ERROR", error });
        if (sessionId) dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnected" });
      }
      return null;
    } finally {
      if (sessionId && isCurrent()) pendingOperations.current.delete(sessionId);
    }
  }, []);

  const reconnectSession = useCallback(async (sessionId: string, initialElevated = false): Promise<string | null> => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state !== "disconnected" || !tab.params) return null;

    const params = tab.params as Record<string, unknown>;
    const guard = pluginRegistry.get(tab.pluginId)?.reconnectGuard;
    if (guard) {
      const result = await guard({ sessionId: tab.id, endpoint: tab.endpoint, params });
      if (!result.ok) {
        dispatch({ type: "SET_ERROR", error: result.message });
        return null;
      }
    }

    return connect({
      endpoint: tab.endpoint,
      params,
      name: tab.name,
      pluginId: tab.pluginId,
      transferEnabled: initialElevated ? false : tab.transferEnabled,
      transferProtocol: tab.transferProtocol,
      sendBarEnabled: tab.sendBarEnabled,
      sessionId: tab.id,
      initialElevated,
    });
  }, [connect]);

  const createOfflineSession = useCallback(async (
    endpoint: string,
    params: Record<string, unknown>,
    name: string | undefined,
    pluginId: string,
    transferEnabled?: boolean,
    transferProtocol?: string,
    sendBarEnabled?: boolean,
  ) => {
    dispatch({ type: "SET_ERROR", error: null });
    try {
      const plugin = pluginRegistry.get(pluginId);
      const normalizedParams = plugin?.normalizeConnectionParams?.(params) ?? params;
      const sessionOptions = pluginRegistry.resolveSessionOptions(pluginId, {
        transferEnabled,
        transferProtocol,
        sendBarEnabled,
      });
      const requestedName = name?.trim();
      const generatedName = await pluginRegistry.resolveSessionDefaultName(
        pluginId,
        normalizedParams,
        endpoint,
      );
      const effectiveName = requestedName || generatedName;
      const sessionId = await invoke<string>("save_session_config", {
        request: {
          endpoint,
          params: normalizedParams,
          name: effectiveName,
          pluginId,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol ?? null,
          sendBarEnabled: sessionOptions.sendBarEnabled,
        },
      });
      const persistedParams = plugin?.persistedConnectionParams?.(normalizedParams, sessionId) ?? normalizedParams;
      dispatch({
        type: "ADD_TAB",
        tab: {
          id: sessionId,
          runtimeId: null,
          name: effectiveName,
          connection_type: pluginId,
          endpoint,
          state: "disconnected",
          pluginId,
          params: persistedParams,
          stats: { txBytes: 0, rxBytes: 0 },
          connectedAt: null,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol,
          sendBarEnabled: sessionOptions.sendBarEnabled,
        },
      });
      return sessionId;
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `创建会话失败: ${e}` });
      return null;
    }
  }, []);

  const disconnect = useCallback(async (sessionId: string): Promise<boolean> => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (tab?.state === "disconnected") return true;
    if (!tab?.runtimeId || tab.state !== "connected" || pendingOperations.current.has(sessionId)) return false;
    const isCurrent = scopeFor(sessionId).begin();
    pendingOperations.current.add(sessionId);
    dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "disconnecting" });
    try {
      const ack = await invoke<{ session_id: string; runtime_id: string }>("disconnect_session", { sessionId, runtimeId: tab.runtimeId });
      if (!isCurrent() || tabsRef.current.find(item => item.id === sessionId)?.runtimeId !== ack.runtime_id) return false;
      dispatch({ type: "SESSION_DISCONNECTED", id: sessionId, runtimeId: ack.runtime_id });
      return true;
    } catch (error) {
      // Reconcile from the authoritative runtime snapshot, never infer connected from failure.
      try {
        const snapshot = await invoke<Array<{ id: string; runtimeId: string; state: string }>>("get_tabs");
        const pending = tabsRef.current.find(item => item.id === sessionId);
        if (isCurrent() && pending?.runtimeId === tab.runtimeId && pending.state === "disconnecting") {
          const current = snapshot.find(item => item.id === sessionId);
          if (!current) dispatch({ type: "SESSION_DISCONNECTED", id: sessionId, runtimeId: tab.runtimeId });
          else if (current.runtimeId === tab.runtimeId && current.state === "connected") dispatch({ type: "SET_TAB_STATE", id: sessionId, state: "connected" });
        }
      } catch { /* Preserve the pending state when runtime authority cannot be reached. */ }
      if (isCurrent()) dispatch({ type: "SET_ERROR", error: `断开失败: ${error}` });
      return false;
    } finally {
      if (isCurrent()) pendingOperations.current.delete(sessionId);
    }
  }, []);

  const deleteSession = useCallback(async (sessionId: string, skipDisconnect = false) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!skipDisconnect && tab?.state !== "disconnected" && !(await disconnect(sessionId))) return;
    if (pendingOperations.current.has(sessionId)) return;
    const isCurrent = scopeFor(sessionId).begin();
    pendingOperations.current.add(sessionId);
    try {
      await invoke("delete_session_config", { sessionId });
    } catch (e) {
      dispatch({
        type: "SET_ERROR",
        error: i18n.t("session.deletePersistFailed", {
          defaultValue: "Failed to delete the saved session: {{error}}. The session remains in the list so you can retry.",
          error: String(e),
        }),
      });
      if (isCurrent()) pendingOperations.current.delete(sessionId);
      return;
    }
    if (!isCurrent()) return;
    pendingOperations.current.delete(sessionId);
    operationScopes.current.get(sessionId)?.invalidate();
    operationScopes.current.delete(sessionId);
    dispatch({ type: "REMOVE_TAB", id: sessionId });
    tab && pluginRegistry.get(tab.pluginId)?.runtimeStore?.release?.(sessionId);
    releaseSessionStore(sessionId);
  }, [disconnect]);

  const writeData = useCallback(async (sessionId: string, data: string | Uint8Array, owner: TabInfo) => {
    const identity = owner.runtimeId;
    if (!identity) throw new Error("Session runtime is not connected");
    const isText = typeof data === "string";
    const bytes = isText ? new TextEncoder().encode(data) : data;
    const written = await invoke<number[]>("write_data", { sessionId, runtimeId: identity, data: Array.from(bytes), transcode: isText });
    const writtenBytes = new Uint8Array(written);
    if (tabsRef.current.find(tab => tab.id === owner.id)?.runtimeId !== identity) return writtenBytes;
    sentDataCallbackRef.current?.(sessionId, writtenBytes);
    sentDataSubscribersRef.current.forEach(callback => callback(sessionId, writtenBytes));
    return writtenBytes;
  }, []);

  const sendData = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab || tab.state !== "connected") return;
    try {
      await writeData(sessionId, data, tab);
    } catch (error) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${error}` });
      throw error;
    }
  }, [writeData]);

  const switchTab = useCallback(async (sessionId: string | null) => {
    if (sessionId === null) {
      dispatch({ type: "SET_ACTIVE", id: null });
      return;
    }
    const tabs = tabsRef.current;
    const targetTab = tabs.find(tab => tab.id === sessionId);
    let resolvedSessionId = sessionId;
    if (targetTab?.parentId) {
      lastActiveChildRef.current.set(targetTab.parentId, targetTab.id);
    } else if (targetTab) {
      const supportsMultiple = pluginRegistry.get(targetTab.pluginId)?.manifest.capabilities.includes("multi_session") ?? false;
      if (supportsMultiple) {
        const children = tabs.filter(tab => tab.parentId === targetTab.id);
        if (children.length > 0) {
          const rememberedId = lastActiveChildRef.current.get(targetTab.id);
          const remembered = rememberedId ? children.find(child => child.id === rememberedId) : undefined;
          resolvedSessionId = remembered?.id ?? children[0].id;
          lastActiveChildRef.current.set(targetTab.id, resolvedSessionId);
        }
      }
    }
    dispatch({ type: "SET_ACTIVE", id: resolvedSessionId });
    try {
      await invoke("switch_active_session", { sessionId: resolvedSessionId });
    } catch (_e) {
      // Restored/offline Session may not exist in the backend runtime yet.
    }
  }, []);

  const renameTab = useCallback(async (sessionId: string, name: string) => {
    const previousName = tabsRef.current.find(tab => tab.id === sessionId)?.name;
    dispatch({ type: "RENAME_TAB", id: sessionId, name });
    try {
      await invoke("rename_session", { sessionId, newName: name });
    } catch (e) {
      if (previousName !== undefined) dispatch({ type: "RENAME_TAB", id: sessionId, name: previousName });
      dispatch({
        type: "SET_ERROR",
        error: i18n.t("session.renamePersistFailed", {
          defaultValue: "Failed to rename the saved session: {{error}}. The previous name was restored.",
          error: String(e),
        }),
      });
    }
  }, []);

  const reconfigureSession = useCallback(async (
    sessionId: string,
    endpoint: string,
    params: Record<string, unknown>,
    name?: string,
    transferEnabled?: boolean,
    transferProtocol?: string,
    sendBarEnabled?: boolean,
    pluginId?: string,
  ) => {
    const tab = tabsRef.current.find(t => t.id === sessionId);
    const wasConnected = tab?.state === "connected";
    if (wasConnected && !(await disconnect(sessionId))) return;
    if (pendingOperations.current.has(sessionId)) return;
    const isCurrent = scopeFor(sessionId).begin();

    const effectivePluginId = pluginId || tab?.pluginId;
    if (!effectivePluginId) {
      dispatch({ type: "SET_ERROR", error: "无法确定会话的协议类型 (pluginId)" });
      return;
    }
    const effectiveName = name?.trim() || tab?.name;
    if (!effectiveName) {
      dispatch({ type: "SET_ERROR", error: "无法确定会话名称" });
      return;
    }
    const plugin = pluginRegistry.get(effectivePluginId);
    const normalizedParams = plugin?.normalizeConnectionParams?.(params) ?? params;
    const sessionOptions = pluginRegistry.resolveSessionOptions(effectivePluginId, {
      transferEnabled: transferEnabled ?? tab?.transferEnabled,
      transferProtocol: transferProtocol ?? tab?.transferProtocol,
      sendBarEnabled: sendBarEnabled ?? tab?.sendBarEnabled,
    });
    pendingOperations.current.add(sessionId);
    try {
      await invoke("save_session_config", {
        request: {
          endpoint,
          params: normalizedParams,
          name: effectiveName,
          pluginId: effectivePluginId,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol ?? null,
          sendBarEnabled: sessionOptions.sendBarEnabled,
          sessionId,
        },
      });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `保存配置失败: ${e}` });
      return;
    } finally {
      if (isCurrent()) pendingOperations.current.delete(sessionId);
    }

    if (!isCurrent()) return;
    const persistedParams = plugin?.persistedConnectionParams?.(normalizedParams, sessionId) ?? normalizedParams;
    dispatch({
      type: "UPDATE_TAB_CONFIG",
      id: sessionId,
      endpoint,
      params: persistedParams,
      name: effectiveName,
      transferEnabled: sessionOptions.transferEnabled,
      transferProtocol: sessionOptions.transferProtocol,
      sendBarEnabled: sessionOptions.sendBarEnabled,
      pluginId: effectivePluginId,
    });

    if (wasConnected) {
      await connect({ endpoint, params: persistedParams, name: effectiveName, pluginId: effectivePluginId,
        transferEnabled: sessionOptions.transferEnabled, transferProtocol: sessionOptions.transferProtocol,
        sendBarEnabled: sessionOptions.sendBarEnabled, sessionId });
    }
  }, [connect, disconnect]);

  const openChannel = useCallback(async (parentSessionId: string, elevated = false): Promise<string | null> => {
    try {
      const runtimeId = tabsRef.current.find(tab => tab.id === parentSessionId)?.runtimeId;
      if (!runtimeId) return null;
      return await invoke<string>("open_channel", { sessionId: parentSessionId, runtimeId, elevated });
    } catch (e) {
      const parent = tabsRef.current.find(tab => tab.id === parentSessionId);
      const plugin = parent ? pluginRegistry.get(parent.pluginId) : undefined;
      const error = plugin?.formatSessionError?.(e, "open_channel") ?? String(e);
      dispatch({ type: "SET_ERROR", error });
      return null;
    }
  }, []);

  const closeChannel = useCallback(async (channelId: string, parentId: string) => {
    const runtimeId = tabsRef.current.find(tab => tab.id === parentId)?.runtimeId;
    if (!runtimeId) return;
    const resetCounter = !tabsRef.current.some(tab => tab.parentId === parentId && tab.id !== channelId);
    try {
      await invoke("close_channel", { sessionId: channelId, runtimeId, parentId, resetCounter });
      if (tabsRef.current.find(tab => tab.id === parentId)?.runtimeId === runtimeId) dispatch({ type: "REMOVE_CHILD", id: channelId, parentId });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `关闭终端失败: ${e}` });
    }
  }, []);

  const clearError = useCallback(() => dispatch({ type: "SET_ERROR", error: null }), []);
  const reportError = useCallback((message: string) => dispatch({ type: "SET_ERROR", error: message }), []);

  const loadSavedSessions = useCallback(async () => {
    try {
      // StrictMode may replay startup effects. Share the one catalog read across those calls.
      const saved = await (catalogReadRef.current ??= invoke<SavedSessionConfig[]>("load_sessions"));
      const tabs: TabInfo[] = saved.map(s => {
        const pluginId = s.plugin_id;
        const params = pluginRegistry.get(pluginId)?.normalizeConnectionParams?.(s.params) ?? s.params;
        const sessionOptions = pluginRegistry.resolveSessionOptions(pluginId, {
          transferEnabled: s.transfer_enabled,
          transferProtocol: s.transfer_protocol,
          sendBarEnabled: s.send_bar_enabled,
        });
        return {
          id: s.id,
          runtimeId: null,
          name: s.name,
          connection_type: s.connection_type,
          endpoint: s.endpoint,
          state: "disconnected" as ConnectionStatus,
          pluginId,
          params,
          stats: { txBytes: 0, rxBytes: 0 },
          connectedAt: null,
          transferEnabled: sessionOptions.transferEnabled,
          transferProtocol: sessionOptions.transferProtocol,
          sendBarEnabled: sessionOptions.sendBarEnabled,
        };
      });
      dispatch({ type: "CATALOG_LOADED", tabs });
    } catch (e) {
      dispatch({
        type: "CATALOG_FAILED",
        error: i18n.t("session.loadPersistFailed", {
          defaultValue: "Failed to load saved sessions: {{error}}. The on-disk library was left untouched.",
          error: String(e),
        }),
      });
    }
  }, []);

  const onSessionData = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => { dataCallbackRef.current = callback; }, []);
  const onDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => { sentDataCallbackRef.current = callback; }, []);
  const subscribeDataSent = useCallback((callback: (sessionId: string, data: Uint8Array) => void) => {
    sentDataSubscribersRef.current.add(callback);
    return () => { sentDataSubscribersRef.current.delete(callback); };
  }, []);

  const isSessionConnected = useCallback((sessionId: string): boolean => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    return tab?.state === "connected";
  }, []);
  const getSessionRuntimeId = useCallback((sessionId: string): string | null => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    return tab?.state === "connected" ? tab.runtimeId : null;
  }, []);
  const isSessionRuntimeCurrent = useCallback((sessionId: string, runtimeId: string): boolean => {
    return getSessionRuntimeId(sessionId) === runtimeId;
  }, [getSessionRuntimeId]);

  const sendToTarget = useCallback(async (sessionId: string, data: string | Uint8Array) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab?.runtimeId || tab.state !== "connected") return;
    const plugin = pluginRegistry.get(tab.pluginId);
    try {
      if (plugin?.sendData) {
        await plugin.sendData({
          sessionId,
          runtimeId: tab.runtimeId,
          params: tab.params ?? {},
          data,
          sendDefault: (targetId, payload) => writeData(targetId, payload, tab),
        });
      } else {
        await writeData(sessionId, data, tab);
      }
    } catch (error) {
      dispatch({ type: "SET_ERROR", error: `发送失败: ${error}` });
      throw error;
    }
  }, [writeData]);

  const updateSessionStats = useCallback((sessionId: string, txBytes: number, rxBytes: number, rxPackets?: number, txPackets?: number) => {
    const tab = tabsRef.current.find(item => item.id === sessionId);
    if (!tab?.runtimeId || tab.state !== "connected") return;
    dispatch({
      type: "UPDATE_TAB_STATS",
      id: sessionId,
      runtimeId: tab.runtimeId,
      stats: { txBytes, rxBytes, ...(rxPackets !== undefined ? { rxPackets } : {}), ...(txPackets !== undefined ? { txPackets } : {}) },
      connectedAt: undefined,
    });
  }, []);
  const onSessionDisconnect = useCallback((callback: (sessionId: string, reason?: string) => void) => { disconnectCallbackRef.current = callback; }, []);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      const u1 = await listen<{ session_id: string; runtime_id: string; data_b64: string }>("session-data", event => {
        const tab = tabsRef.current.find(tab => tab.id === event.payload.session_id);
        // A batch queued before EOF may arrive after the same runtime's disconnect event.
        if (!tab || tab.runtimeId !== event.payload.runtime_id) return;
        const data = decodeBase64(event.payload.data_b64);
        dataCallbackRef.current?.(event.payload.session_id, data);
      });
      if (cancelled) { u1(); return; }
      unlisteners.push(u1);

      const u2 = await listen<{
        session_id: string;
        runtime_id: string;
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
        const parentId = event.payload.parent_id ?? null;
        const existingTab = tabsRef.current.find(tab => tab.id === sid);
        const eventOptions = pluginRegistry.resolveSessionOptions(eventPluginId, {
          transferEnabled: event.payload.transfer_enabled ?? existingTab?.transferEnabled,
          transferProtocol: event.payload.transfer_protocol ?? existingTab?.transferProtocol,
          sendBarEnabled: event.payload.send_bar_enabled ?? existingTab?.sendBarEnabled,
        });
        dispatch({
          type: "SESSION_CONNECTED",
          tab: {
            id: sid, runtimeId: event.payload.runtime_id,
            name: event.payload.name || existingTab?.name || pluginRegistry.get(eventPluginId)?.manifest.name || eventPluginId,
            connection_type: event.payload.connection_type,
            endpoint: event.payload.endpoint, state: "connected", pluginId: eventPluginId,
            params: eventParams, stats: { txBytes: 0, rxBytes: 0 },
            connectedAt: event.payload.connected_at ?? Date.now(),
            transferEnabled: eventOptions.transferEnabled, transferProtocol: eventOptions.transferProtocol,
            sendBarEnabled: eventOptions.sendBarEnabled, parentId,
            channelIndex: event.payload.channel_index, elevated: event.payload.elevated,
            isContainer: event.payload.is_container,
          },
        });
      });
      if (cancelled) { u2(); return; }
      unlisteners.push(u2);

      const u2e = await listen<{ channel_id: string; parent_id: string; runtime_id: string; disconnect_info?: DisconnectInfo }>("channel-closed", event => {
        if (tabsRef.current.find(tab => tab.id === event.payload.channel_id)?.runtimeId !== event.payload.runtime_id) return;
        if (event.payload.disconnect_info?.retain_terminal) dispatch({ type: "SET_TAB_DISCONNECTED", id: event.payload.channel_id, info: event.payload.disconnect_info });
        else dispatch({ type: "REMOVE_CHILD", id: event.payload.channel_id, parentId: event.payload.parent_id });
      });
      if (cancelled) { u2e(); return; }
      unlisteners.push(u2e);

      const u3 = await listen<{ session_id: string; runtime_id: string; reason?: string; disconnect_info?: DisconnectInfo }>("session-disconnected", event => {
        const sid = event.payload.session_id;
        if (tabsRef.current.find(tab => tab.id === sid)?.runtimeId !== event.payload.runtime_id) return;
        dispatch({ type: "SESSION_DISCONNECTED", id: sid, runtimeId: event.payload.runtime_id, info: event.payload.disconnect_info });
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
      });
      if (cancelled) { u3(); return; }
      unlisteners.push(u3);

      const u7 = await listen<{ session_id: string }>("session-switched", event => {
        dispatch({ type: "SET_ACTIVE", id: event.payload.session_id });
      });
      if (cancelled) { u7(); return; }
      unlisteners.push(u7);

      const u8 = await listen<{ session_id: string; name: string }>("session-renamed", event => {
        dispatch({ type: "RENAME_TAB", id: event.payload.session_id, name: event.payload.name });
      });
      if (cancelled) { u8(); return; }
      unlisteners.push(u8);

      const u9 = await listen<{ tab_id: string; runtime_id: string; tx_bytes: number; rx_bytes: number; connected_at?: number | null }>("session-stats", event => {
        dispatch({ type: "UPDATE_TAB_STATS", id: event.payload.tab_id, runtimeId: event.payload.runtime_id, stats: { txBytes: event.payload.tx_bytes, rxBytes: event.payload.rx_bytes }, connectedAt: event.payload.connected_at });
      });
      if (cancelled) { u9(); return; }
      unlisteners.push(u9);
    })().catch(e => console.error("SessionContext: 事件监听器注册失败:", e));

    return () => {
      cancelled = true;
      unlisteners.forEach(unlisten => unlisten());
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    void listen<{ session_id: string; runtime_id: string; dropped_chunks: number }>("session-display-overflow", event => {
      if (disposed) return;
      const tab = tabsRef.current.find(item => item.id === event.payload.session_id);
      if (tab?.runtimeId !== event.payload.runtime_id || tab.state !== "connected") return;
      dispatch({
        type: "SET_ERROR",
        error: i18n.t("session.displayOverflow", {
          defaultValue: "Display overflow in {{session}} — {{count}} chunks were dropped from the presentation path.",
          session: tab?.name ?? event.payload.session_id,
          count: event.payload.dropped_chunks,
        }),
      });
    }).then(fn => {
      if (disposed) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const hasActiveLogs = loggingSessions.size > 0;
  useEffect(() => {
    if (!hasActiveLogs) return;
    const interval = setInterval(async () => {
      try {
        const statuses: Array<{ session_id: string; file_name: string; bytes_written: number }> = await invoke("get_log_status");
        setLogStatuses(new Map(statuses.map(s => [s.session_id, { fileName: s.file_name, bytesWritten: s.bytes_written }])));
        setLoggingSessions(new Set(statuses.map(s => s.session_id)));
      } catch (_e) {
        // best-effort status refresh
      }
    }, 5000);
    return () => clearInterval(interval);
  }, [hasActiveLogs]);

  useEffect(() => {
    fetchConnectionTypes();
    loadSavedSessions();
  }, [fetchConnectionTypes, loadSavedSessions]);

  return (
    <SessionContext.Provider value={{
      state,
      fetchConnectionTypes,
      refreshEndpoints,
      connect,
      reconnectSession,
      createOfflineSession,
      disconnect,
      deleteSession,
      sendData,
      switchTab,
      renameTab,
      reconfigureSession,
      openChannel,
      closeChannel,
      onSessionData,
      onDataSent,
      subscribeDataSent,
      isSessionConnected,
      isSessionRuntimeCurrent,
      getSessionRuntimeId,
      sendToTarget,
      updateSessionStats,
      onSessionDisconnect,
      clearError,
      reportError,
      startSessionLog,
      stopSessionLog,
      loggingSessions,
      logStatuses,
    }}>
      {children}
    </SessionContext.Provider>
  );
}

export function useSession() {
  const ctx = useContext(SessionContext);
  if (!ctx) throw new Error("useSession must be used within SessionProvider");
  return ctx;
}
