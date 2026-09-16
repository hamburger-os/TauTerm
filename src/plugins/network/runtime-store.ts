import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PluginRuntimeStore, PluginSendContext } from "../../core/plugin-registry";

export interface NetworkPeerEntry {
  peerId: string;
  name: string;
  addr: string;
  localAddr?: string;
  state: "connected" | "disconnected";
  txBytes: number;
  rxBytes: number;
}

export interface NetworkRuntimeSnapshot {
  peers: readonly NetworkPeerEntry[];
  selectedPeerId: string | null;
  manualTarget: string;
  udpSources: readonly string[];
  localAddr: string;
  broadcast: boolean;
}

const EMPTY: NetworkRuntimeSnapshot = Object.freeze({
  peers: Object.freeze([]) as readonly NetworkPeerEntry[],
  selectedPeerId: null,
  manualTarget: "",
  udpSources: Object.freeze([]) as readonly string[],
  localAddr: "",
  broadcast: false,
});

const sessions = new Map<string, NetworkRuntimeSnapshot>();
const peerContainers = new Map<string, string>();
const listeners = new Set<() => void>();
const manualSentSubscribers = new Set<(
  sessionId: string,
  target: string,
  data: Uint8Array,
) => void>();
let revision = 0;
let listenerReady: Promise<void> | null = null;
let unlisteners: UnlistenFn[] = [];

function current(sessionId: string): NetworkRuntimeSnapshot {
  return sessions.get(sessionId) ?? EMPTY;
}

function publish(sessionId: string, next: NetworkRuntimeSnapshot): void {
  sessions.set(sessionId, next);
  revision += 1;
  listeners.forEach(listener => listener());
}

function patch(
  sessionId: string,
  update: Partial<NetworkRuntimeSnapshot> | ((prev: NetworkRuntimeSnapshot) => Partial<NetworkRuntimeSnapshot>),
): void {
  const prev = current(sessionId);
  const delta = typeof update === "function" ? update(prev) : update;
  publish(sessionId, { ...prev, ...delta });
}

function removePeerMappings(sessionId: string): void {
  for (const [peerId, containerId] of [...peerContainers]) {
    if (containerId === sessionId) peerContainers.delete(peerId);
  }
}

function ensureListeners(): Promise<void> {
  if (listenerReady) return listenerReady;
  listenerReady = (async () => {
    const registered: UnlistenFn[] = [];
    registered.push(await listen<{
      session_id: string;
      peer_id: string;
      peer_name: string;
      peer_addr: string;
      local_addr?: string;
    }>("netdbg-peer-joined", event => {
      const { session_id: sessionId, peer_id: peerId, peer_name: name, peer_addr: addr, local_addr: localAddr } = event.payload;
      peerContainers.set(peerId, sessionId);
      patch(sessionId, prev => {
        const existing = prev.peers.find(peer => peer.peerId === peerId);
        const peer: NetworkPeerEntry = {
          peerId,
          name,
          addr,
          localAddr,
          state: "connected",
          txBytes: existing?.txBytes ?? 0,
          rxBytes: existing?.rxBytes ?? 0,
        };
        return {
          peers: [...prev.peers.filter(item => item.peerId !== peerId), peer],
          selectedPeerId: prev.selectedPeerId ?? peerId,
        };
      });
    }));

    registered.push(await listen<{
      session_id: string;
      peer_id: string;
      tx_bytes?: number | null;
      rx_bytes?: number | null;
    }>("netdbg-peer-left", event => {
      const { session_id: sessionId, peer_id: peerId, tx_bytes: txBytes, rx_bytes: rxBytes } = event.payload;
      peerContainers.delete(peerId);
      patch(sessionId, prev => ({
        peers: prev.peers.map(peer => peer.peerId === peerId ? {
          ...peer,
          state: "disconnected" as const,
          txBytes: typeof txBytes === "number" ? txBytes : peer.txBytes,
          rxBytes: typeof rxBytes === "number" ? rxBytes : peer.rxBytes,
        } : peer),
        selectedPeerId: prev.selectedPeerId === peerId ? null : prev.selectedPeerId,
      }));
    }));

    registered.push(await listen<{
      tab_id: string;
      tx_bytes: number;
      rx_bytes: number;
    }>("session-stats", event => {
      const peerId = event.payload.tab_id;
      const sessionId = peerContainers.get(peerId);
      if (!sessionId) return;
      patch(sessionId, prev => ({
        peers: prev.peers.map(peer => peer.peerId === peerId ? {
          ...peer,
          txBytes: event.payload.tx_bytes,
          rxBytes: event.payload.rx_bytes,
        } : peer),
      }));
    }));

    registered.push(await listen<{
      session_id: string;
      plugin_id?: string;
      local_addr?: string | null;
    }>("session-connected", event => {
      if (event.payload.plugin_id !== "network") return;
      if (typeof event.payload.local_addr === "string") {
        patch(event.payload.session_id, { localAddr: event.payload.local_addr });
      }
      void refreshNetworkPeers(event.payload.session_id);
    }));

    registered.push(await listen<{ session_id: string }>("session-disconnected", event => {
      if (!sessions.has(event.payload.session_id)) return;
      removePeerMappings(event.payload.session_id);
      publish(event.payload.session_id, EMPTY);
    }));

    unlisteners = registered;
  })().catch(error => {
    console.error("[network/runtime-store] 事件监听注册失败:", error);
    unlisteners.forEach(unlisten => unlisten());
    unlisteners = [];
    listenerReady = null;
  });
  return listenerReady;
}

export const networkRuntimeStore: PluginRuntimeStore = {
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
    removePeerMappings(sessionId);
    if (sessions.delete(sessionId)) {
      revision += 1;
      listeners.forEach(listener => listener());
    }
  },
};

export function getNetworkRuntime(sessionId: string): NetworkRuntimeSnapshot {
  return current(sessionId);
}

export function selectNetworkPeer(sessionId: string, peerId: string | null): void {
  patch(sessionId, { selectedPeerId: peerId });
}

export function setNetworkManualTarget(sessionId: string, target: string): void {
  patch(sessionId, { manualTarget: target });
}

export function setNetworkBroadcast(sessionId: string, broadcast: boolean): void {
  patch(sessionId, { broadcast });
}

export function registerNetworkUdpSource(sessionId: string, addr: string): void {
  if (!addr) return;
  patch(sessionId, prev => ({
    udpSources: [addr, ...prev.udpSources.filter(item => item !== addr)].slice(0, 20),
  }));
}

export function mergeNetworkPeers(sessionId: string, entries: NetworkPeerEntry[]): void {
  for (const peer of entries) {
    if (peer.state === "connected") peerContainers.set(peer.peerId, sessionId);
  }
  patch(sessionId, prev => {
    const old = new Map(prev.peers.map(peer => [peer.peerId, peer]));
    const peers = entries.map(peer => ({
      ...peer,
      txBytes: peer.txBytes ?? old.get(peer.peerId)?.txBytes ?? 0,
      rxBytes: peer.rxBytes ?? old.get(peer.peerId)?.rxBytes ?? 0,
    }));
    const selectedPeerId = prev.selectedPeerId && peers.some(peer => peer.peerId === prev.selectedPeerId)
      ? prev.selectedPeerId
      : peers.find(peer => peer.state === "connected")?.peerId ?? null;
    return { peers, selectedPeerId };
  });
}

export async function refreshNetworkPeers(sessionId: string): Promise<void> {
  try {
    const list = await invoke<Array<{
      peer_id: string;
      name: string;
      addr: string;
      local_addr?: string;
      state: string;
      tx_bytes?: number;
      rx_bytes?: number;
    }>>("list_network_peers", { sessionId });
    mergeNetworkPeers(sessionId, list.map(peer => ({
      peerId: peer.peer_id,
      name: peer.name,
      addr: peer.addr,
      localAddr: peer.local_addr,
      state: peer.state === "connected" ? "connected" : "disconnected",
      txBytes: peer.tx_bytes ?? 0,
      rxBytes: peer.rx_bytes ?? 0,
    })));
  } catch {
    // Session 可能尚未连接；事件流会在连接后刷新。
  }
}

export async function disconnectNetworkPeer(sessionId: string, peerId: string): Promise<void> {
  try {
    await invoke("close_network_peer", { sessionId: peerId });
  } catch (error) {
    console.error("网络调试: 断开对端失败:", error);
  }
  peerContainers.delete(peerId);
  patch(sessionId, prev => ({
    peers: prev.peers.map(peer => peer.peerId === peerId
      ? { ...peer, state: "disconnected" as const }
      : peer),
    selectedPeerId: prev.selectedPeerId === peerId ? null : prev.selectedPeerId,
  }));
}

export async function clearNetworkPeer(sessionId: string, peerId: string): Promise<void> {
  try {
    await invoke("close_network_peer", { sessionId: peerId });
  } catch {
    // 已关闭对端允许直接清理前端历史条目。
  }
  peerContainers.delete(peerId);
  patch(sessionId, prev => ({
    peers: prev.peers.filter(peer => peer.peerId !== peerId),
    selectedPeerId: prev.selectedPeerId === peerId ? null : prev.selectedPeerId,
  }));
}

export function subscribeNetworkManualSent(
  callback: (sessionId: string, target: string, data: Uint8Array) => void,
): () => void {
  manualSentSubscribers.add(callback);
  return () => manualSentSubscribers.delete(callback);
}

export async function sendNetworkData(context: PluginSendContext): Promise<void> {
  const { sessionId, params, data, sendDefault } = context;
  const snapshot = current(sessionId);
  const transport = (params.transport as string | undefined) ?? "tcp";
  const role = (params.role as string | undefined) ?? "client";
  const isText = typeof data === "string";
  const bytes = isText ? new TextEncoder().encode(data) : data;
  const byteArray = Array.from(bytes);

  if (transport === "udp") {
    if (role === "server") {
      const target = snapshot.manualTarget.trim();
      if (!target) throw new Error("无可用发送目标");
      const written = await invoke<number[]>("network_udp_send_to", {
        sessionId,
        targetAddr: target,
        data: byteArray,
        transcode: isText,
      });
      const payload = new Uint8Array(written);
      manualSentSubscribers.forEach(callback => callback(sessionId, target, payload));
      return;
    }
    const written = await invoke<number[]>("network_udp_send", {
      sessionId,
      data: byteArray,
      transcode: isText,
    });
    const target = `${params.remote_host ?? "127.0.0.1"}:${params.remote_port ?? 0}`;
    const payload = new Uint8Array(written);
    manualSentSubscribers.forEach(callback => callback(sessionId, target, payload));
    return;
  }

  const peers = snapshot.peers.filter(peer => peer.state === "connected");
  if (snapshot.broadcast) {
    for (const peer of peers) await sendDefault(peer.peerId, data);
    return;
  }
  const selected = peers.find(peer => peer.peerId === snapshot.selectedPeerId);
  if (selected) {
    await sendDefault(selected.peerId, data);
    return;
  }
  if (peers.length === 1) {
    await sendDefault(peers[0].peerId, data);
    return;
  }
  throw new Error("无可用发送目标");
}
