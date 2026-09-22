import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PluginRuntimeStore, PluginSendContext } from "../../core/plugin-registry";
import {
  bytesToBase64,
  type RttChunk,
  type RttEvent,
  isUsableRttDirection,
  type RttHistoryResponse,
  type RttSnapshot,
  type RttViewMode,
  type SystemViewEvent,
  type SystemViewHistoryResponse,
  type SystemViewRuntimeEvent,
  type SystemViewSnapshot,
} from "./model";

const CLIENT_HISTORY_BYTES_PER_CHANNEL = 512 * 1024;
const CLIENT_HISTORY_BYTES_PER_SESSION = 2 * 1024 * 1024;
const CLIENT_SYSTEMVIEW_EVENTS_PER_CHANNEL = 8_192;

export type SystemViewMetadataSyncPhase = "idle" | "syncing" | "incomplete" | "unavailable";

export interface SystemViewMetadataSyncState {
  phase: SystemViewMetadataSyncPhase;
  unresolved_tasks: number;
  attempts: number;
  max_attempts: number;
}

export interface SystemViewPresentationRecoveryState {
  recovered_through_drops: number;
  pending_drops: number;
}

export interface RttSystemViewChannelState {
  snapshot: SystemViewSnapshot | null;
  events: readonly SystemViewEvent[];
  loaded: boolean;
  error: string | null;
  metadataSync: SystemViewMetadataSyncState;
  presentationRecovery: SystemViewPresentationRecoveryState;
}

export interface RttRuntimeSnapshot {
  snapshot: RttSnapshot | null;
  selectedChannel: number | null;
  viewModes: Readonly<Record<number, RttViewMode>>;
  buffers: Readonly<Record<number, readonly RttChunk[]>>;
  systemview: Readonly<Record<number, RttSystemViewChannelState>>;
  error: string | null;
}

const EMPTY: RttRuntimeSnapshot = Object.freeze({
  snapshot: null,
  selectedChannel: null,
  viewModes: Object.freeze({}),
  buffers: Object.freeze({}),
  systemview: Object.freeze({}),
  error: null,
});

const EMPTY_METADATA_SYNC: SystemViewMetadataSyncState = Object.freeze({
  phase: "idle",
  unresolved_tasks: 0,
  attempts: 0,
  max_attempts: 0,
});

const EMPTY_PRESENTATION_RECOVERY: SystemViewPresentationRecoveryState = Object.freeze({
  recovered_through_drops: 0,
  pending_drops: 0,
});

function presentationRecoveryState(
  snapshot: SystemViewSnapshot,
  recoveredThroughDrops: number,
): SystemViewPresentationRecoveryState {
  const recovered = Math.min(
    recoveredThroughDrops,
    snapshot.presentation_dropped_events,
  );
  return {
    recovered_through_drops: recovered,
    pending_drops: Math.max(0, snapshot.presentation_dropped_events - recovered),
  };
}

const sessions = new Map<string, RttRuntimeSnapshot>();
const loadedChannels = new Map<string, Set<number>>();
const systemViewHistoryRecoveries = new Set<string>();
const systemViewHistoryRecoveryRequests = new Map<string, number>();
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
  return snapshot.channels.find(channel => isUsableRttDirection(channel.up))?.index
    ?? snapshot.channels[0]?.index
    ?? null;
}

function metadataSyncState(snapshot: SystemViewSnapshot): SystemViewMetadataSyncState {
  const unresolved = snapshot.tasks.filter(task => !task.name || task.priority == null).length;
  const attempts = snapshot.metadata_sync_attempts ?? 0;
  const maxAttempts = snapshot.metadata_sync_max_attempts ?? 0;
  if (unresolved === 0) {
    return {
      phase: "idle",
      unresolved_tasks: 0,
      attempts,
      max_attempts: maxAttempts,
    };
  }
  if (!snapshot.control_available) {
    return {
      phase: "unavailable",
      unresolved_tasks: unresolved,
      attempts,
      max_attempts: maxAttempts,
    };
  }
  return {
    phase: maxAttempts > 0 && attempts >= maxAttempts ? "incomplete" : "syncing",
    unresolved_tasks: unresolved,
    attempts,
    max_attempts: maxAttempts,
  };
}

function applySnapshot(sessionId: string, snapshot: RttSnapshot): void {
  const prev = current(sessionId);
  const generationChanged = prev.snapshot == null || prev.snapshot.generation !== snapshot.generation;
  if (generationChanged) loadedChannels.delete(sessionId);

  const selectedChannel = generationChanged
    ? chooseViewChannel(snapshot, null)
    : chooseViewChannel(snapshot, prev.selectedChannel);
  publish(sessionId, {
    snapshot,
    selectedChannel,
    viewModes: prev.viewModes,
    buffers: generationChanged ? Object.freeze({}) : prev.buffers,
    systemview: generationChanged ? Object.freeze({}) : prev.systemview,
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

function mergeSystemViewEvents(
  currentEvents: readonly SystemViewEvent[],
  incoming: readonly SystemViewEvent[],
): SystemViewEvent[] {
  const bySequence = new Map<number, SystemViewEvent>();
  for (const event of currentEvents) bySequence.set(event.sequence, event);
  for (const event of incoming) bySequence.set(event.sequence, event);
  const merged = [...bySequence.values()].sort((a, b) => a.sequence - b.sequence);
  return merged.length <= CLIENT_SYSTEMVIEW_EVENTS_PER_CHANNEL
    ? merged
    : merged.slice(merged.length - CLIENT_SYSTEMVIEW_EVENTS_PER_CHANNEL);
}

function systemViewRecoveryKey(sessionId: string, channelIndex: number, generation: number): string {
  return `${sessionId}:${channelIndex}:${generation}`;
}

function hasSystemViewSequenceGap(
  previous: readonly SystemViewEvent[],
  incoming: readonly SystemViewEvent[],
): boolean {
  if (previous.length === 0 || incoming.length === 0) return false;
  const previousLast = previous[previous.length - 1]?.sequence;
  const incomingFirst = incoming[0]?.sequence;
  return previousLast != null && incomingFirst != null && incomingFirst > previousLast + 1;
}

async function recoverSystemViewHistory(
  sessionId: string,
  channelIndex: number,
  generation: number,
  requestedThroughDrops: number,
): Promise<void> {
  const key = systemViewRecoveryKey(sessionId, channelIndex, generation);
  const queuedThroughDrops = systemViewHistoryRecoveryRequests.get(key) ?? 0;
  systemViewHistoryRecoveryRequests.set(
    key,
    Math.max(queuedThroughDrops, requestedThroughDrops),
  );
  if (systemViewHistoryRecoveries.has(key)) return;

  systemViewHistoryRecoveries.add(key);
  try {
    while (systemViewHistoryRecoveryRequests.has(key)) {
      // Capture the recovery watermark *before* requesting history. Drops that happen while
      // the command is in flight enqueue another pass instead of being falsely marked recovered.
      const recoverThroughDrops = systemViewHistoryRecoveryRequests.get(key) ?? 0;
      systemViewHistoryRecoveryRequests.delete(key);

      const history = await invoke<SystemViewHistoryResponse>("rtt_systemview_history", {
        sessionId,
        channelIndex,
        limit: CLIENT_SYSTEMVIEW_EVENTS_PER_CHANNEL,
      });
      if (history.generation !== generation) return;

      const prev = current(sessionId);
      const channel = prev.systemview[channelIndex];
      const snapshot = channel?.snapshot;
      if (!channel || !snapshot || snapshot.generation !== generation) return;

      const events = mergeSystemViewEvents(
        channel.events.filter(event => event.sequence > snapshot.cleared_through_sequence),
        history.events.filter(event => event.sequence > snapshot.cleared_through_sequence),
      );
      const recoveredThroughDrops = Math.max(
        channel.presentationRecovery.recovered_through_drops,
        recoverThroughDrops,
      );
      publish(sessionId, {
        ...prev,
        systemview: Object.freeze({
          ...prev.systemview,
          [channelIndex]: Object.freeze({
            ...channel,
            events: Object.freeze(events),
            loaded: true,
            error: null,
            presentationRecovery: Object.freeze(
              presentationRecoveryState(snapshot, recoveredThroughDrops),
            ),
          }),
        }),
      });
    }
  } catch (cause) {
    console.warn("[rtt/runtime-store] SystemView history recovery failed:", cause);
  } finally {
    systemViewHistoryRecoveries.delete(key);
    systemViewHistoryRecoveryRequests.delete(key);
  }
}

function applySystemViewEvent(payload: SystemViewRuntimeEvent): void {
  const prev = current(payload.session_id);
  if (!prev.snapshot || prev.snapshot.generation !== payload.generation) return;
  const previous = prev.systemview[payload.channel_index] ?? {
    snapshot: null,
    events: [],
    loaded: false,
    error: null,
    metadataSync: EMPTY_METADATA_SYNC,
    presentationRecovery: EMPTY_PRESENTATION_RECOVERY,
  };
  const visibleEvents = payload.kind === "batch"
    ? payload.events.filter(
      event => event.sequence > payload.snapshot.cleared_through_sequence,
    )
    : [];
  const retainedPrevious = previous.events.filter(
    event => event.sequence > payload.snapshot.cleared_through_sequence,
  );
  const sequenceGap = payload.kind === "batch"
    && hasSystemViewSequenceGap(retainedPrevious, visibleEvents);
  const presentationLossAdvanced = payload.snapshot.presentation_dropped_events
    > (previous.snapshot?.presentation_dropped_events ?? 0);
  const events = payload.kind === "batch"
    ? mergeSystemViewEvents(retainedPrevious, visibleEvents)
    : retainedPrevious;
  publish(payload.session_id, {
    ...prev,
    systemview: Object.freeze({
      ...prev.systemview,
      [payload.channel_index]: Object.freeze({
        snapshot: payload.snapshot,
        events: Object.freeze(events),
        loaded: previous.loaded,
        error: null,
        metadataSync: Object.freeze(
          metadataSyncState(payload.snapshot),
        ),
        presentationRecovery: Object.freeze(
          presentationRecoveryState(
            payload.snapshot,
            previous.presentationRecovery.recovered_through_drops,
          ),
        ),
      }),
    }),
  });
  if (sequenceGap || presentationLossAdvanced) {
    void recoverSystemViewHistory(
      payload.session_id,
      payload.channel_index,
      payload.generation,
      payload.snapshot.presentation_dropped_events,
    );
  }
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
    registered.push(await listen<SystemViewRuntimeEvent>("rtt-systemview-event", event => {
      applySystemViewEvent(event.payload);
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
    for (const key of systemViewHistoryRecoveries) {
      if (key.startsWith(`${sessionId}:`)) systemViewHistoryRecoveries.delete(key);
    }
    for (const key of systemViewHistoryRecoveryRequests.keys()) {
      if (key.startsWith(`${sessionId}:`)) systemViewHistoryRecoveryRequests.delete(key);
    }
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
  if (!prev.snapshot?.channels.some(channel => channel.index === channelIndex)) return;
  if (prev.selectedChannel !== channelIndex) {
    publish(sessionId, { ...prev, selectedChannel: channelIndex, error: null });
  }
}

export async function selectRttAutomationSource(
  sessionId: string,
  channelIndex: number,
): Promise<void> {
  try {
    await invoke("rtt_set_automation_source_channel", { sessionId, channelIndex });
    await refreshRttRuntime(sessionId);
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
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


export async function ensureSystemViewHistory(
  sessionId: string,
  channelIndex: number,
): Promise<void> {
  await ensureListeners();
  const before = current(sessionId);
  const existing = before.systemview[channelIndex];
  if (existing?.loaded) return;

  try {
    const [snapshot, history] = await Promise.all([
      invoke<SystemViewSnapshot>("rtt_systemview_snapshot", { sessionId, channelIndex }),
      invoke<SystemViewHistoryResponse>("rtt_systemview_history", {
        sessionId,
        channelIndex,
        limit: CLIENT_SYSTEMVIEW_EVENTS_PER_CHANNEL,
      }),
    ]);
    const prev = current(sessionId);
    if (!prev.snapshot || prev.snapshot.generation !== snapshot.generation) return;
    const previous = prev.systemview[channelIndex];
    publish(sessionId, {
      ...prev,
      systemview: Object.freeze({
        ...prev.systemview,
        [channelIndex]: Object.freeze({
          snapshot,
          events: Object.freeze(mergeSystemViewEvents(
            (previous?.events ?? []).filter(
              event => event.sequence > snapshot.cleared_through_sequence,
            ),
            history.events.filter(
              event => event.sequence > snapshot.cleared_through_sequence,
            ),
          )),
          loaded: true,
          error: null,
          metadataSync: Object.freeze(
            metadataSyncState(snapshot),
          ),
          presentationRecovery: Object.freeze(
            presentationRecoveryState(snapshot, snapshot.presentation_dropped_events),
          ),
        }),
      }),
    });
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, {
      ...prev,
      systemview: Object.freeze({
        ...prev.systemview,
        [channelIndex]: Object.freeze({
          snapshot: prev.systemview[channelIndex]?.snapshot ?? null,
          events: prev.systemview[channelIndex]?.events ?? Object.freeze([]),
          loaded: false,
          error: String(cause),
          metadataSync: prev.systemview[channelIndex]?.metadataSync ?? EMPTY_METADATA_SYNC,
          presentationRecovery: prev.systemview[channelIndex]?.presentationRecovery
            ?? EMPTY_PRESENTATION_RECOVERY,
        }),
      }),
    });
  }
}

export async function attachSystemView(
  sessionId: string,
  channelIndex: number,
): Promise<boolean> {
  try {
    await invoke("rtt_systemview_attach", { sessionId, channelIndex });
    await refreshRttRuntime(sessionId);
    await ensureSystemViewHistory(sessionId, channelIndex);
    return true;
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
    return false;
  }
}

export async function detachSystemView(
  sessionId: string,
  channelIndex: number,
): Promise<boolean> {
  try {
    await invoke("rtt_systemview_detach", { sessionId, channelIndex });
    await refreshRttRuntime(sessionId);
    for (const key of systemViewHistoryRecoveries) {
      if (key.startsWith(`${sessionId}:${channelIndex}:`)) systemViewHistoryRecoveries.delete(key);
    }
    for (const key of systemViewHistoryRecoveryRequests.keys()) {
      if (key.startsWith(`${sessionId}:${channelIndex}:`)) {
        systemViewHistoryRecoveryRequests.delete(key);
      }
    }
    const prev = current(sessionId);
    const { [channelIndex]: _detached, ...systemview } = prev.systemview;
    publish(sessionId, {
      ...prev,
      systemview: Object.freeze(systemview),
      error: null,
    });
    return true;
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
    return false;
  }
}

export async function controlSystemView(
  sessionId: string,
  channelIndex: number,
  control: "start" | "stop" | "refresh" | "refresh_tasks",
): Promise<void> {
  try {
    await invoke("rtt_systemview_control", { sessionId, channelIndex, control });
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
  }
}

export async function clearSystemView(
  sessionId: string,
  channelIndex: number,
): Promise<void> {
  try {
    await invoke("rtt_systemview_clear", { sessionId, channelIndex });
    const prev = current(sessionId);
    publish(sessionId, {
      ...prev,
      systemview: Object.freeze({
        ...prev.systemview,
        [channelIndex]: Object.freeze({
          snapshot: prev.systemview[channelIndex]?.snapshot ?? null,
          events: Object.freeze([]),
          loaded: false,
          error: null,
          metadataSync: prev.systemview[channelIndex]?.metadataSync ?? EMPTY_METADATA_SYNC,
          presentationRecovery: prev.systemview[channelIndex]?.presentationRecovery
            ?? EMPTY_PRESENTATION_RECOVERY,
        }),
      }),
    });
    await ensureSystemViewHistory(sessionId, channelIndex);
  } catch (cause) {
    const prev = current(sessionId);
    publish(sessionId, { ...prev, error: String(cause) });
  }
}
