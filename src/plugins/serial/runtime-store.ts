import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PluginRuntimeStore } from "../../core/plugin-registry";

export type SerialVirtualEndpointState = "ready" | "backpressured";

export interface SerialVirtualEndpoint {
  external_path: string;
  state: SerialVirtualEndpointState;
  reason?: string;
  queued_bytes?: number;
  backlog_limit_bytes?: number;
  stalled_for_ms?: number;
}

export interface SerialRuntimeSnapshot {
  endpoints: readonly SerialVirtualEndpoint[];
  error?: string;
  errorKind?: string;
}

interface SerialVirtualPortHealthEvent {
  session_id: string;
  external_path: string;
  state: SerialVirtualEndpointState;
  reason?: string;
  queued_bytes?: number;
  backlog_limit_bytes?: number;
  stalled_for_ms?: number;
}

const EMPTY: SerialRuntimeSnapshot = Object.freeze({
  endpoints: Object.freeze([]) as readonly SerialVirtualEndpoint[],
});

const sessions = new Map<string, SerialRuntimeSnapshot>();
const listeners = new Set<() => void>();
let revision = 0;
let listenerReady: Promise<void> | null = null;
let unlisteners: UnlistenFn[] = [];

function current(sessionId: string): SerialRuntimeSnapshot {
  return sessions.get(sessionId) ?? EMPTY;
}

function publish(sessionId: string, snapshot: SerialRuntimeSnapshot): void {
  sessions.set(sessionId, snapshot);
  revision += 1;
  listeners.forEach(listener => listener());
}

function applyEndpointHealth(event: SerialVirtualPortHealthEvent): void {
  const snapshot = current(event.session_id);
  const endpoints = snapshot.endpoints.map(endpoint =>
    endpoint.external_path === event.external_path
      ? {
          external_path: endpoint.external_path,
          state: event.state,
          reason: event.state === "backpressured" ? event.reason : undefined,
          queued_bytes: event.state === "backpressured" ? event.queued_bytes : undefined,
          backlog_limit_bytes:
            event.state === "backpressured" ? event.backlog_limit_bytes : undefined,
          stalled_for_ms: event.state === "backpressured" ? event.stalled_for_ms : undefined,
        }
      : endpoint,
  );
  if (!endpoints.some(endpoint => endpoint.external_path === event.external_path)) return;
  publish(event.session_id, { ...snapshot, endpoints });
}

function ensureListeners(): Promise<void> {
  if (listenerReady) return listenerReady;
  listenerReady = (async () => {
    const registered: UnlistenFn[] = [];
    registered.push(await listen<{ session_id: string; endpoints: SerialVirtualEndpoint[] }>(
      "virtual-port-created",
      event => publish(event.payload.session_id, {
        endpoints: event.payload.endpoints,
        error: undefined,
        errorKind: undefined,
      }),
    ));
    registered.push(await listen<SerialVirtualPortHealthEvent>(
      "virtual-port-health",
      event => applyEndpointHealth(event.payload),
    ));
    registered.push(await listen<{ session_id: string; kind?: string; reason: string }>(
      "virtual-port-failed",
      event => publish(event.payload.session_id, {
        endpoints: [],
        error: event.payload.reason,
        errorKind: event.payload.kind,
      }),
    ));
    registered.push(await listen("virtual-port-driver-ready", () => {
      let changed = false;
      for (const [sessionId, snapshot] of sessions) {
        if (!snapshot.error) continue;
        sessions.set(sessionId, { ...snapshot, error: undefined, errorKind: undefined });
        changed = true;
      }
      if (changed) {
        revision += 1;
        listeners.forEach(listener => listener());
      }
    }));
    registered.push(await listen<{ session_id: string }>("session-disconnected", event => {
      if (sessions.has(event.payload.session_id)) publish(event.payload.session_id, EMPTY);
    }));
    unlisteners = registered;
  })().catch(error => {
    console.error("[serial/runtime-store] 事件监听注册失败:", error);
    unlisteners.forEach(unlisten => unlisten());
    unlisteners = [];
    listenerReady = null;
  });
  return listenerReady;
}

export const serialRuntimeStore: PluginRuntimeStore = {
  subscribe(listener) {
    listeners.add(listener);
    void ensureListeners();
    return () => listeners.delete(listener);
  },
  getSnapshot(sessionId) {
    void ensureListeners();
    return current(sessionId);
  },
  revision: () => revision,
  release(sessionId) {
    if (!sessions.delete(sessionId)) return;
    revision += 1;
    listeners.forEach(listener => listener());
  },
};
