import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PluginRuntimeStore, PluginSendContext } from "../../core/plugin-registry";
import {
  bytesToBase64,
  type RttChunk,
  type RttEvent,
  type RttHistoryResponse,
  type RttSnapshot,
  type RttViewMode,
} from "./model";

const CLIENT_HISTORY_BYTES_PER_CHANNEL = 512 * 1024;
const CLIENT_HISTORY_BYTES_PER_SESSION = 2 * 1024 * 1024;

export interface RttRuntimeSnapshot {
  snapshot: RttSnapshot | null;
  selectedChannel: number | null;
  viewModes: Readonly<Record<number, RttViewMode>>;
  buffers: Readonly<Record<number, readonly RttChunk[]>>;
  error: string | null;
}

const EMPTY: RttRuntimeSnapshot = Object.freeze({
  snapshot: null,
  selectedChannel: null,
  viewModes: Object.freeze({}),
  buffers: Object.freeze({}),
  error: null,
});

const sessions = new Map<string, RttRuntimeSnapshot>();
const loadedChannels = new Map<string, Set<number>>();
const listeners = new Set<() => void>();
let revision = 0;
let listenerReady: Promise<void> | null = null;
let unlisteners: UnlistenFn[] = [];

function current(sessionId: string): RttRuntimeSnapshot {
  return sessions.get(sessionId) ?? EMPTY;
}

function publish(sessionId: string, next: RttRuntimeSnapshot): void {
  sessions.set(sessionId, next);
  revision += 1;
  listeners.forEach(listener => listener());
}

function chooseViewChannel(snapshot: RttSnapshot, previous: number | null): number | null {
  if (previous != null && snapshot.channels.some(channel => channel.index === previous)) {
    return previous;
  }
  return snapshot.channels.find(channel => channel.up)?.index
    ?? snapshot.channels[0]?.index
    ?? null;
}

function applySnapshot(sessionId: string, snapshot: RttSnapshot): void {
  const prev = current(sessionId);
  const generationChanged = prev.snapshot == null || prev.snapshot.generation !== snapshot.generation;
  if (generationChanged) loadedChannels.delete(sessionId);

  const selectedChannel = generationChanged
    ? (snapshot.automation_source_channel ?? chooseViewChannel(snapshot, null))
    : chooseViewChannel(snapshot, prev.selectedChannel);
  publish(sessionId, {
    snapshot,
    selectedChannel,
    viewModes: prev.viewModes,
    buffers: generationChanged ? Object.freeze({}) : prev.buffers,
    error: snapshot.last_error?.message ?? null,
  });
}

function base64ByteLength(value: string): number {
  if (!value) return 0;
  const padding = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor(value.length * 3 / 4) - padding);
}

function trimChunks(chunks: RttChunk[]): RttChunk[] {
  let total = chunks.reduce((sum, chunk) => sum + base64ByteLength(chunk.data_b64), 0);
  let start = 0;
  while (total > CLIENT_HISTORY_BYTES_PER_CHANNEL && start < chunks.length) {
    total -= base64ByteLength(chunks[start].data_b64);
    start += 1;
  }
  return start === 0 ? chunks : chunks.slice(start);
}

function trimBuffers(
  buffers: Record<number, readonly RttChunk[]>,
  preferredChannel: number | null,
): Record<number, readonly RttChunk[]> {
  const next: Record<number, readonly RttChunk[]> = { ...buffers };
  const starts = new Map<number, number>();
  let total = Object.values(next).reduce(
    (sum, chunks) => sum + chunks.reduce((chunkSum, chunk) => chunkSum + base64ByteLength(chunk.data_b64), 0),
    0,
  );

  while (total > CLIENT_HISTORY_BYTES_PER_SESSION) {
    let oldestChannel: number | null = null;
    let oldestSequence = Number.POSITIVE_INFINITY;
    const candidates = Object.entries(next).filter(([rawChannel, chunks]) => {
      const channel = Number(rawChannel);
      const first = chunks[starts.get(channel) ?? 0];
      return first && channel !== preferredChannel;
    });
    const pool = candidates.length > 0 ? candidates : Object.entries(next);
    for (const [rawChannel, chunks] of pool) {
      const channel = Number(rawChannel);
      const first = chunks[starts.get(channel) ?? 0];
      if (first && first.sequence < oldestSequence) {
        oldestSequence = first.sequence;
        oldestChannel = channel;
      }
    }
    if (oldestChannel == null) break;

    const chunks = next[oldestChannel] ?? [];
    const start = starts.get(oldestChannel) ?? 0;
    const first = chunks[start];
    if (!first) break;
    total -= base64ByteLength(first.data_b64);
    starts.set(oldestChannel, start + 1);
  }

  for (const [channel, start] of starts) {
    if (start > 0) next[channel] = (next[channel] ?? []).slice(start);
  }
  return next;
}

function mergeHistory(currentChunks: readonly RttChunk[], history: readonly RttChunk[]): RttChunk[] {
  if (currentChunks.length === 0) return trimChunks([...history]);
  if (history.length === 0) return [...currentChunks];
  const bySequence = new Map<number, RttChunk>();
  for (const chunk of history) bySequence.set(chunk.sequence, chunk);
  for (const chunk of currentChunks) bySequence.set(chunk.sequence, chunk);
  return trimChunks([...bySequence.values()].sort((a, b) => a.sequence - b.sequence));
}

function appendBatch(sessionId: string, generation: number, chunks: readonly RttChunk[]): void {
  if (chunks.length === 0) return;
  const prev = current(sessionId);
  if (!prev.snapshot || prev.snapshot.generation !== generation) return;

  const nextBuffers: Record<number, readonly RttChunk[]> = { ...prev.buffers };
  const grouped = new Map<number, RttChunk[]>();
  for (const chunk of chunks) {
    if (chunk.generation !== generation) continue;
    const list = grouped.get(chunk.channel_index) ?? [];
    list.push(chunk);
    grouped.set(chunk.channel_index, list);
  }
  for (const [channelIndex, incoming] of grouped) {
    const existing = nextBuffers[channelIndex] ?? [];
    const lastSequence = existing[existing.length - 1]?.sequence ?? 0;
    const appendable = incoming.filter(chunk => chunk.sequence > lastSequence);
    if (appendable.length === 0) continue;
    nextBuffers[channelIndex] = trimChunks([...existing, ...appendable]);
  }
  publish(sessionId, {
    ...prev,
    buffers: Object.freeze(trimBuffers(nextBuffers, prev.selectedChannel)),
  });
}

function ensureListeners(): Promise<void> {
  if (listenerReady) return listenerReady;
  listenerReady = (async () => {
    const registered: UnlistenFn[] = [];
    registered.push(await listen<RttEvent>("rtt-event", event => {
      const payload = event.payload;
      if (payload.kind === "snapshot") {
        applySnapshot(payload.session_id, payload.snapshot);
      } else {
        appendBatch(payload.session_id, payload.generation, payload.chunks);
      }
    }));
    registered.push(await listen<{ session_id: string }>("session-disconnected", event => {
      const prev = sessions.get(event.payload.session_id);
      if (!prev) return;
      publish(event.payload.session_id, {
        ...prev,
        error: prev.snapshot?.last_error?.message ?? prev.error,
      });
    }));
    unlisteners = registered;
  })().catch(error => {
    console.error("[rtt/runtime-store] 事件监听注册失败:", error);
    unlisteners.forEach(unlisten => unlisten());
    unlisteners = [];
    listenerReady = null;
  });
  return listenerReady;
}

export const rttRuntimeStore: PluginRuntimeStore = {
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
    loadedChannels.delete(sessionId);
    if (sessions.delete(sessionId)) {
      revision += 1;
      listeners.forEach(listener => listener());
    }
  },
};

export async function refreshRttRuntime(sessionId: string): Promise<void> {
  await ensureListeners();
  try {
    const snapshot = await invoke<RttSnapshot>("rtt_snapshot", { sessionId });
    applySnapshot(sessionId, snapshot);
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
  }
}

export async function ensureRttHistory(sessionId: string, channelIndex: number): Promise<void> {
  await ensureListeners();
  const loaded = loadedChannels.get(sessionId) ?? new Set<number>();
  if (loaded.has(channelIndex)) return;
  loaded.add(channelIndex);
  loadedChannels.set(sessionId, loaded);

  try {
    const history = await invoke<RttHistoryResponse>("rtt_history", {
      sessionId,
      channelIndex,
      afterSequence: null,
    });
    const prev = current(sessionId);
    if (!prev.snapshot || prev.snapshot.generation !== history.generation) {
      loaded.delete(channelIndex);
      return;
    }
    const nextBuffers: Record<number, readonly RttChunk[]> = { ...prev.buffers };
    nextBuffers[channelIndex] = mergeHistory(nextBuffers[channelIndex] ?? [], history.chunks);
    publish(sessionId, {
      ...prev,
      buffers: Object.freeze(trimBuffers(nextBuffers, prev.selectedChannel)),
    });
  } catch (cause) {
    loaded.delete(channelIndex);
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
  }
}

export async function selectRttChannel(sessionId: string, channelIndex: number): Promise<void> {
  const prev = current(sessionId);
  const expectedGeneration = prev.snapshot?.generation ?? null;
  const channel = prev.snapshot?.channels.find(item => item.index === channelIndex);
  if (!channel?.up) {
    if (prev.selectedChannel !== channelIndex) {
      publish(sessionId, { ...prev, selectedChannel: channelIndex });
    }
    return;
  }

  try {
    await invoke("rtt_set_automation_source_channel", { sessionId, channelIndex });
    const latest = current(sessionId);
    if (latest.snapshot?.generation !== expectedGeneration) {
      await refreshRttRuntime(sessionId);
      return;
    }
    publish(sessionId, { ...latest, selectedChannel: channelIndex, error: null });
    await refreshRttRuntime(sessionId);
  } catch (cause) {
    const latest = current(sessionId);
    publish(sessionId, { ...latest, error: String(cause) });
  }
}

export function setRttViewMode(sessionId: string, channelIndex: number, mode: RttViewMode): void {
  const prev = current(sessionId);
  publish(sessionId, {
    ...prev,
    viewModes: Object.freeze({ ...prev.viewModes, [channelIndex]: mode }),
  });
}

export async function selectRttSendChannel(sessionId: string, channelIndex: number): Promise<void> {
  try {
    await invoke("rtt_set_send_channel", { sessionId, channelIndex });
    await refreshRttRuntime(sessionId);
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
  }
}

async function ensureSendChannel(sessionId: string): Promise<number> {
  let state = current(sessionId);
  if (!state.snapshot) {
    await refreshRttRuntime(sessionId);
    state = current(sessionId);
  }
  const channelIndex = state.snapshot?.send_channel ?? null;
  if (channelIndex == null) throw new Error("当前 RTT 会话没有可写 Down Channel");
  return channelIndex;
}

export async function sendRttBytes(
  sessionId: string,
  channelIndex: number,
  bytes: Uint8Array,
): Promise<void> {
  if (bytes.length === 0) return;
  await invoke<number>("rtt_write", {
    sessionId,
    channelIndex,
    dataB64: bytesToBase64(bytes),
  });
}

export async function sendRttTerminalData(
  sessionId: string,
  channelIndex: number,
  bytes: Uint8Array,
): Promise<void> {
  await sendRttBytes(sessionId, channelIndex, bytes);
}

export async function sendRttData(context: PluginSendContext): Promise<void> {
  const channelIndex = await ensureSendChannel(context.sessionId);
  const bytes = typeof context.data === "string"
    ? new TextEncoder().encode(context.data)
    : context.data;
  await sendRttBytes(context.sessionId, channelIndex, bytes);
}

export function rttChannelChunks(
  runtime: RttRuntimeSnapshot,
  channelIndex: number | null,
): readonly RttChunk[] {
  return channelIndex == null ? [] : (runtime.buffers[channelIndex] ?? []);
}
