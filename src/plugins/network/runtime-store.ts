import { AsyncRequestScope, RuntimeEventCursor } from "../../core/session-lifecycle";
import { StreamFramer } from "../../utils/streamDisplay";
import { appendFrame, appendPacket, bytesToHex, createNetworkPresentation, decodeText, nowTime, type NetworkPresentation } from "./presentation";
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

export interface NetworkRuntimeSnapshot extends NetworkPresentation {
  peers: readonly NetworkPeerEntry[];
  selectedPeerId: string | null;
  manualTarget: string;
  udpSources: readonly string[];
  localAddr: string;
  broadcast: boolean;
}

const EMPTY: NetworkRuntimeSnapshot = Object.freeze({
  ...createNetworkPresentation(),
  peers: Object.freeze([]) as readonly NetworkPeerEntry[],
  selectedPeerId: null,
  manualTarget: "",
  udpSources: Object.freeze([]) as readonly string[],
  localAddr: "",
  broadcast: false,
});

const sessions = new Map<string, NetworkRuntimeSnapshot>();
const connections = new Map<string, { events: RuntimeEventCursor; queries: AsyncRequestScope }>();
function connection(sessionId: string) {
  let value = connections.get(sessionId);
  if (!value) { value = { events: new RuntimeEventCursor(), queries: new AsyncRequestScope() }; connections.set(sessionId, value); }
  return value;
}
export function isNetworkRuntimeCurrent(sessionId: string, runtimeId: string): boolean {
  return connections.get(sessionId)?.events.matches(runtimeId) ?? false;
}
function observeConnected(sessionId: string, runtimeId: string): boolean {
  const value = connections.get(sessionId);
  if (!value) return false;
  if (value.events.accept(runtimeId, true)) {
    value.queries.invalidate();
    clearReceivers(sessionId);
    removePeerMappings(sessionId);
    publish(sessionId, EMPTY);
  }
  return value.events.matches(runtimeId);
}
const peerContainers = new Map<string, string>();
const listeners = new Set<() => void>();
const receivers = new Map<string, Map<string, { framer: StreamFramer; decoder: TextDecoder }>>();

function clearReceivers(sessionId: string): void {
  for (const receiver of receivers.get(sessionId)?.values() ?? []) receiver.framer.dispose();
  receivers.delete(sessionId);
}

function recordPacket(sessionId: string, direction: "RX" | "TX", target: string, data: Uint8Array): void {
  patch(sessionId, prev => appendPacket(prev, {
    time: nowTime(), direction, peerLabel: target, length: data.length,
    hex: bytesToHex(data), text: decodeText(data, prev.encoding),
  }));
}
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
    unlisteners = registered;
    registered.push(await listen<{
      session_id: string;
      runtime_id: string;
      peer_id: string;
      peer_name: string;
      peer_addr: string;
      local_addr?: string;
    }>("netdbg-peer-joined", event => {
      const { session_id: sessionId, peer_id: peerId, peer_name: name, peer_addr: addr, local_addr: localAddr } = event.payload;
      if (!observeConnected(sessionId, event.payload.runtime_id)) return;
      connection(sessionId).queries.invalidate();
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
      runtime_id: string;
      peer_id: string;
      tx_bytes?: number | null;
      rx_bytes?: number | null;
    }>("netdbg-peer-left", event => {
      const { session_id: sessionId, peer_id: peerId, tx_bytes: txBytes, rx_bytes: rxBytes } = event.payload;
      if (!isNetworkRuntimeCurrent(sessionId, event.payload.runtime_id)) return;
      connection(sessionId).queries.invalidate();
      // Keep the route with peer history: DataBatcher may deliver its final bytes after left.
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
      runtime_id: string;
      tx_bytes: number;
      rx_bytes: number;
    }>("session-stats", event => {
      const peerId = event.payload.tab_id;
      const sessionId = peerContainers.get(peerId);
      if (!sessionId || !isNetworkRuntimeCurrent(sessionId, event.payload.runtime_id)) return;
      connection(sessionId).queries.invalidate();
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
      runtime_id: string;
      plugin_id?: string;
      local_addr?: string | null;
      params?: Record<string, unknown>;
    }>("session-connected", event => {
      if (event.payload.plugin_id !== "network" || !observeConnected(event.payload.session_id, event.payload.runtime_id)) return;
      const params = event.payload.params ?? {};
      patch(event.payload.session_id, {
        transport: params.transport === "udp" ? "udp" : "tcp",
        role: params.role === "server" ? "server" : "client",
        encoding: typeof params.encoding === "string" ? params.encoding : "utf-8",
      });
      if (typeof event.payload.local_addr === "string") {
        patch(event.payload.session_id, { localAddr: event.payload.local_addr });
      }
      void refreshNetworkPeers(event.payload.session_id);
    }));

    registered.push(await listen<{ session_id: string; runtime_id: string }>("session-disconnected", event => {
      const binding = connections.get(event.payload.session_id);
      if (!binding?.events.accept(event.payload.runtime_id, false)) return;
      binding.queries.invalidate();
      clearReceivers(event.payload.session_id);
      removePeerMappings(event.payload.session_id);
      publish(event.payload.session_id, EMPTY);
    }));

    registered.push(await listen<{ session_id: string; runtime_id: string; data_b64: string; source_addr?: string }>("session-data", event => {
      const payload = event.payload;
      const sessionId = peerContainers.get(payload.session_id) ?? payload.session_id;
      if (!isNetworkRuntimeCurrent(sessionId, payload.runtime_id)) return;
      const state = current(sessionId);
      const data = Uint8Array.from(atob(payload.data_b64), byte => byte.charCodeAt(0));
      if (!data.length) return;
      if (state.transport === "udp") {
        const source = payload.source_addr ?? payload.session_id;
        registerNetworkUdpSource(sessionId, source);
        recordPacket(sessionId, "RX", source, data);
        return;
      }
      if (!peerContainers.has(payload.session_id)) return;
      let channels = receivers.get(sessionId);
      if (!channels) { channels = new Map(); receivers.set(sessionId, channels); }
      let receiver = channels.get(payload.session_id);
      if (!receiver) {
        let decoder: TextDecoder;
        try { decoder = new TextDecoder(state.encoding); } catch { decoder = new TextDecoder("utf-8"); }
        receiver = { framer: new StreamFramer(50), decoder };
        channels.set(payload.session_id, receiver);
      }
      const captured = receiver;
      const binding = connections.get(sessionId);
      captured.framer.push(data, frame => {
        if (connections.get(sessionId) !== binding || !isNetworkRuntimeCurrent(sessionId, payload.runtime_id)) return;
        patch(sessionId, prev => appendFrame(prev, payload.session_id, "RX", frame, prev.encoding, captured.decoder.decode(frame, { stream: true })));
      });
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
    clearReceivers(sessionId);
    connections.get(sessionId)?.queries.invalidate();
    connections.delete(sessionId);
    removePeerMappings(sessionId);
    if (sessions.delete(sessionId)) {
      revision += 1;
      listeners.forEach(listener => listener());
    }
  },
};

export async function prepareNetworkRuntime(sessionId: string): Promise<void> {
  connection(sessionId);
  await ensureListeners();
  if (!listenerReady) throw new Error("Network event listeners are unavailable");
}

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
    peerContainers.set(peer.peerId, sessionId);
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
  const binding = connections.get(sessionId);
  if (!binding) return;
  const currentRequest = binding.queries.begin();
  await ensureListeners();
  if (connections.get(sessionId) !== binding || !currentRequest()) return;
  try {
    const snapshot = await invoke<{ runtime_id: string; peers: Array<{
      peer_id: string; name: string; addr: string; local_addr?: string;
      state: string; tx_bytes?: number; rx_bytes?: number;
    }> } | null>("list_network_peers", { sessionId });
    if (connections.get(sessionId) !== binding || !currentRequest()) return;
    if (!snapshot) return;
    if (!observeConnected(sessionId, snapshot.runtime_id)) return;
    mergeNetworkPeers(sessionId, snapshot.peers.map(peer => ({
      peerId: peer.peer_id, name: peer.name, addr: peer.addr, localAddr: peer.local_addr,
      state: peer.state === "connected" ? "connected" : "disconnected",
      txBytes: peer.tx_bytes ?? 0, rxBytes: peer.rx_bytes ?? 0,
    })));
  } catch {
    // A reconnect/close may retire this connection while its snapshot is in flight.
  }
}

export async function disconnectNetworkPeer(sessionId: string, peerId: string): Promise<void> {
  try {
    await invoke("close_network_peer", { sessionId: peerId });
  } catch (error) {
    console.error("网络调试: 断开对端失败:", error);
  }
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
  receivers.get(sessionId)?.get(peerId)?.framer.dispose();
  receivers.get(sessionId)?.delete(peerId);
  patch(sessionId, prev => {
    const { [peerId]: _removed, ...frames } = prev.frames;
    return { frames, peers: prev.peers.filter(peer => peer.peerId !== peerId),
      selectedPeerId: prev.selectedPeerId === peerId ? null : prev.selectedPeerId };
  });
}

export async function sendNetworkData(context: PluginSendContext): Promise<void> {
  const { sessionId, params, data, sendDefault } = context;
  const snapshot = current(sessionId);
  const transport = (params.transport as string | undefined) ?? "tcp";
  const role = (params.role as string | undefined) ?? "client";
  const isText = typeof data === "string";
  const bytes = isText ? new TextEncoder().encode(data) : data;
  const byteArray = Array.from(bytes);
  const binding = connections.get(sessionId);
  const runtimeId = context.runtimeId;
  const isCurrent = () => connections.get(sessionId) === binding && isNetworkRuntimeCurrent(sessionId, runtimeId);
  if (!isCurrent()) throw new Error("Network session runtime is not connected");

  if (transport === "udp") {
    if (role === "server") {
      const target = snapshot.manualTarget.trim();
      if (!target) throw new Error("无可用发送目标");
      const written = await invoke<number[]>("network_udp_send_to", {
        sessionId,
        runtimeId,
        targetAddr: target,
        data: byteArray,
        transcode: isText,
      });
      const payload = new Uint8Array(written);
      if (isCurrent()) recordPacket(sessionId, "TX", target, payload);
      return;
    }
    const written = await invoke<number[]>("network_udp_send", {
      sessionId,
      runtimeId,
      data: byteArray,
      transcode: isText,
    });
    const target = `${params.remote_host ?? "127.0.0.1"}:${params.remote_port ?? 0}`;
    const payload = new Uint8Array(written);
    if (isCurrent()) recordPacket(sessionId, "TX", target, payload);
    return;
  }

  const peers = snapshot.peers.filter(peer => peer.state === "connected");
  const send = async (peerId: string) => {
    const written = await sendDefault(peerId, data);
    if (isCurrent()) patch(sessionId, prev => appendFrame(prev, peerId, "TX", written, prev.encoding));
  };
  if (snapshot.broadcast) {
    for (const peer of peers) await send(peer.peerId);
    return;
  }
  const selected = peers.find(peer => peer.peerId === snapshot.selectedPeerId);
  if (selected) {
    await send(selected.peerId);
    return;
  }
  if (peers.length === 1) {
    await send(peers[0].peerId);
    return;
  }
  throw new Error("无可用发送目标");
}
