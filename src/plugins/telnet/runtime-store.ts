import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PluginRuntimeStore } from "../../core/plugin-registry";

export interface TelnetRuntimeSnapshot {
  localEcho: boolean;
}

const EMPTY: TelnetRuntimeSnapshot = Object.freeze({ localEcho: false });
const sessions = new Map<string, TelnetRuntimeSnapshot>();
const listeners = new Set<() => void>();
let revision = 0;
let listenerReady: Promise<void> | null = null;
let unlisteners: UnlistenFn[] = [];

function publish(sessionId: string, snapshot: TelnetRuntimeSnapshot): void {
  sessions.set(sessionId, snapshot);
  revision += 1;
  listeners.forEach(listener => listener());
}

function ensureListeners(): Promise<void> {
  if (listenerReady) return listenerReady;
  listenerReady = (async () => {
    const registered: UnlistenFn[] = [];
    registered.push(await listen<{ session_id: string; local_echo: boolean }>("telnet-echo-state", event => {
      publish(event.payload.session_id, { localEcho: event.payload.local_echo });
    }));
    registered.push(await listen<{ session_id: string }>("session-disconnected", event => {
      if (sessions.has(event.payload.session_id)) publish(event.payload.session_id, EMPTY);
    }));
    unlisteners = registered;
  })().catch(error => {
    console.error("[telnet/runtime-store] 事件监听注册失败:", error);
    unlisteners.forEach(unlisten => unlisten());
    unlisteners = [];
    listenerReady = null;
  });
  return listenerReady;
}

export const telnetRuntimeStore: PluginRuntimeStore = {
  subscribe(listener) {
    listeners.add(listener);
    void ensureListeners();
    return () => listeners.delete(listener);
  },
  getSnapshot(sessionId) {
    void ensureListeners();
    return sessions.get(sessionId) ?? EMPTY;
  },
  revision: () => revision,
  release(sessionId) {
    if (!sessions.delete(sessionId)) return;
    revision += 1;
    listeners.forEach(listener => listener());
  },
};
