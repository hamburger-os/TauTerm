import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

/** 会话连接状态 */
export type ConnectionStatus = "disconnected" | "connecting" | "connected";

/** 连接类型信息 */
export interface ConnectionTypeInfo {
  id: string;
  label: string;
  available: boolean;
}

/** 端点信息 */
export interface EndpointInfo {
  name: string;
  description: string;
  connection_type: string;
}

/** 会话连接参数 */
export interface SessionParams {
  connection_type: string;
  endpoint: string;
  params: Record<string, unknown>;
}

/**
 * 串口参数（SessionParams.params 的子集）
 */
export interface SerialPortConfig {
  port_name: string;
  baud_rate: number;
  data_bits: number;
  parity: string;
  stop_bits: string;
  flow_control: string;
}

/**
 * useSerialPort Hook
 *
 * 通过会话抽象层管理终端连接状态。
 * 当前仅实现串口连接，未来支持 SSH/Telnet。
 */
export function useSerialPort() {
  const [status, setStatus] = useState<ConnectionStatus>("disconnected");
  const [endpoints, setEndpoints] = useState<EndpointInfo[]>([]);
  const [connectionTypes, setConnectionTypes] = useState<ConnectionTypeInfo[]>([]);
  const [connectedEndpoint, setConnectedEndpoint] = useState<string | null>(null);
  const [connectedType, setConnectedType] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastData, setLastData] = useState<Uint8Array | null>(null);

  const dataCallbackRef = useRef<((data: Uint8Array) => void) | null>(null);
  const disconnectCallbackRef = useRef<(() => void) | null>(null);

  /** 获取可用连接类型 */
  const fetchConnectionTypes = useCallback(async () => {
    try {
      const types = await invoke<ConnectionTypeInfo[]>("get_connection_types");
      setConnectionTypes(types);
    } catch (e) {
      setError(`获取连接类型失败: ${e}`);
    }
  }, []);

  /** 刷新端点列表（当前仅串口） */
  const refreshEndpoints = useCallback(async () => {
    try {
      const list = await invoke<EndpointInfo[]>("enumerate_endpoints");
      setEndpoints(list);
      setError(null);
    } catch (e) {
      setError(`端点扫描失败: ${e}`);
    }
  }, []);

  /** 连接到端点 */
  const connect = useCallback(async (endpoint: string, params: Record<string, unknown>) => {
    setStatus("connecting");
    setError(null);
    try {
      await invoke("connect_session", { endpoint, params });
      // 连接成功由 session-connected 事件处理
    } catch (e) {
      setStatus("disconnected");
      setError(`连接失败: ${e}`);
    }
  }, []);

  /** 断开连接 */
  const disconnect = useCallback(async () => {
    try {
      await invoke("disconnect_session");
    } catch (e) {
      setError(`断开失败: ${e}`);
    }
  }, []);

  /** 发送数据到当前会话 */
  const sendData = useCallback(async (data: string | Uint8Array) => {
    try {
      const bytes = typeof data === "string"
        ? new TextEncoder().encode(data)
        : data;
      await invoke("write_data", { data: Array.from(bytes) });
    } catch (e) {
      setError(`发送失败: ${e}`);
    }
  }, []);

  /** 注册数据接收回调 */
  const onData = useCallback((callback: (data: Uint8Array) => void) => {
    dataCallbackRef.current = callback;
  }, []);

  /** 注册断开回调 */
  const onDisconnect = useCallback((callback: () => void) => {
    disconnectCallbackRef.current = callback;
  }, []);

  // 监听 Tauri 会话事件
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    listen<number[]>("session-data", (event) => {
      const data = new Uint8Array(event.payload);
      setLastData(data);
      dataCallbackRef.current?.(data);
    }).then((u) => { unlisteners.push(u); });

    listen<{ endpoint: string; connection_type: string }>(
      "session-connected",
      (event) => {
        setStatus("connected");
        setConnectedEndpoint(event.payload.endpoint);
        setConnectedType(event.payload.connection_type);
        setError(null);
      }
    ).then((u) => { unlisteners.push(u); });

    listen("session-disconnected", () => {
      setStatus("disconnected");
      setConnectedEndpoint(null);
      setConnectedType(null);
      disconnectCallbackRef.current?.();
    }).then((u) => { unlisteners.push(u); });

    return () => {
      unlisteners.forEach((u) => u());
    };
  }, []);

  // 初始化
  useEffect(() => {
    fetchConnectionTypes();
    refreshEndpoints();
  }, [fetchConnectionTypes, refreshEndpoints]);

  return {
    status,
    endpoints,
    connectionTypes,
    connectedEndpoint,
    connectedType,
    error,
    lastData,
    refreshEndpoints,
    fetchConnectionTypes,
    connect,
    disconnect,
    sendData,
    onData,
    onDisconnect,
    clearError: () => setError(null),
  };
}
