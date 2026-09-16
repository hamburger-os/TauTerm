import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import Icon from "../../components/common/Icon";
import RttTerminalView from "./RttTerminalView";
import {
  base64ToBytes,
  bytesToBase64,
  formatBytes,
  type RttChannelInfo,
  type RttChunk,
  type RttEvent,
  type RttHistoryResponse,
  type RttSnapshot,
  type RttViewMode,
} from "./model";
import styles from "./RttSessionView.module.css";

const CLIENT_HISTORY_BYTES = 512 * 1024;
const TEXT_VIEW_BYTES = 256 * 1024;
const HEX_VIEW_BYTES = 64 * 1024;

function causeMessage(cause: unknown): string {
  if (cause && typeof cause === "object" && "message" in cause) return String((cause as { message: unknown }).message);
  return String(cause);
}

function trimChunks(chunks: RttChunk[]): RttChunk[] {
  let bytes = chunks.reduce((total, chunk) => total + base64ToBytes(chunk.data_b64).length, 0);
  let start = 0;
  while (bytes > CLIENT_HISTORY_BYTES && start < chunks.length) {
    bytes -= base64ToBytes(chunks[start].data_b64).length;
    start += 1;
  }
  return start === 0 ? chunks : chunks.slice(start);
}

function mergeChunks(current: RttChunk[], incoming: RttChunk[]): RttChunk[] {
  const bySequence = new Map<number, RttChunk>();
  for (const chunk of current) bySequence.set(chunk.sequence, chunk);
  for (const chunk of incoming) bySequence.set(chunk.sequence, chunk);
  return trimChunks([...bySequence.values()].sort((a, b) => a.sequence - b.sequence));
}

function flattenBytes(chunks: RttChunk[], limit: number): Uint8Array {
  const arrays = chunks.map(chunk => base64ToBytes(chunk.data_b64));
  const total = arrays.reduce((sum, bytes) => sum + bytes.length, 0);
  const wanted = Math.min(total, limit);
  const output = new Uint8Array(wanted);
  let skip = total - wanted;
  let offset = 0;
  for (const bytes of arrays) {
    if (skip >= bytes.length) {
      skip -= bytes.length;
      continue;
    }
    const view = bytes.subarray(skip);
    output.set(view, offset);
    offset += view.length;
    skip = 0;
  }
  return output;
}

function formatHex(bytes: Uint8Array): string {
  const lines: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const row = bytes.subarray(offset, offset + 16);
    const hex = Array.from(row, value => value.toString(16).padStart(2, "0")).join(" ").padEnd(47, " ");
    const ascii = Array.from(row, value => value >= 32 && value < 127 ? String.fromCharCode(value) : ".").join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex}  ${ascii}`);
  }
  return lines.join("\n");
}

export default function RttSessionView({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const connected = tab?.state === "connected" || tab?.state === "transferring";
  const [snapshot, setSnapshot] = useState<RttSnapshot | null>(null);
  const [selectedChannel, setSelectedChannel] = useState<number | null>(null);
  const [revision, setRevision] = useState(0);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const historiesRef = useRef(new Map<number, RttChunk[]>());
  const loadedRef = useRef(new Set<number>());
  const modesRef = useRef(new Map<number, RttViewMode>());
  const writeChainRef = useRef<Promise<unknown>>(Promise.resolve());

  const appendChunks = useCallback((channelIndex: number, chunks: RttChunk[]) => {
    if (chunks.length === 0) return;
    const current = historiesRef.current.get(channelIndex) ?? [];
    historiesRef.current.set(channelIndex, mergeChunks(current, chunks));
    setRevision(value => value + 1);
  }, []);

  const refreshSnapshot = useCallback(async () => {
    if (!connected) return;
    try {
      const next = await invoke<RttSnapshot>("rtt_snapshot", { sessionId });
      setSnapshot(next);
      setError(next.last_error?.message ?? null);
    } catch (cause) {
      setError(causeMessage(cause));
    }
  }, [connected, sessionId]);

  useEffect(() => {
    historiesRef.current.clear();
    loadedRef.current.clear();
    modesRef.current.clear();
    setSnapshot(null);
    setSelectedChannel(null);
    setRevision(value => value + 1);
  }, [sessionId]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<RttEvent>("rtt-event", event => {
      const payload = event.payload;
      if (payload.session_id !== sessionId) return;
      if (payload.kind === "snapshot") {
        setSnapshot(payload.snapshot);
        setError(payload.snapshot.last_error?.message ?? null);
      } else {
        appendChunks(payload.channel_index, [payload]);
      }
    }).then(dispose => { unlisten = dispose; });
    return () => unlisten?.();
  }, [appendChunks, sessionId]);

  useEffect(() => {
    void refreshSnapshot();
    if (!connected) return;
    const timer = window.setInterval(() => void refreshSnapshot(), 1000);
    return () => window.clearInterval(timer);
  }, [connected, refreshSnapshot]);

  useEffect(() => {
    const channels = snapshot?.channels ?? [];
    if (channels.length === 0) {
      setSelectedChannel(null);
      return;
    }
    if (selectedChannel === null || !channels.some(channel => channel.index === selectedChannel)) {
      setSelectedChannel(channels[0].index);
    }
  }, [selectedChannel, snapshot?.channels]);

  useEffect(() => {
    if (!connected || selectedChannel === null || loadedRef.current.has(selectedChannel)) return;
    loadedRef.current.add(selectedChannel);
    void invoke<RttHistoryResponse>("rtt_history", {
      sessionId,
      channelIndex: selectedChannel,
      afterSequence: null,
    }).then(history => appendChunks(selectedChannel, history.chunks)).catch(cause => {
      loadedRef.current.delete(selectedChannel);
      setError(causeMessage(cause));
    });
  }, [appendChunks, connected, selectedChannel, sessionId]);

  const channels = snapshot?.channels ?? [];
  const channel = channels.find(item => item.index === selectedChannel) ?? null;
  void revision;
  const chunks = selectedChannel === null ? [] : (historiesRef.current.get(selectedChannel) ?? []);
  const mode = selectedChannel === null
    ? "terminal"
    : (modesRef.current.get(selectedChannel) ?? (selectedChannel === 0 ? "terminal" : "text"));
  const text = useMemo(() => new TextDecoder().decode(flattenBytes(chunks, TEXT_VIEW_BYTES)), [chunks]);
  const hex = useMemo(() => formatHex(flattenBytes(chunks, HEX_VIEW_BYTES)), [chunks]);

  const setMode = (next: RttViewMode) => {
    if (selectedChannel === null) return;
    modesRef.current.set(selectedChannel, next);
    setRevision(value => value + 1);
  };

  const sendBytes = useCallback((channelIndex: number, bytes: Uint8Array) => {
    if (!connected || bytes.length === 0) return;
    writeChainRef.current = writeChainRef.current
      .catch(() => undefined)
      .then(() => invoke("rtt_write", {
        sessionId,
        channelIndex,
        dataB64: bytesToBase64(bytes),
      }))
      .catch(cause => setError(causeMessage(cause)));
  }, [connected, sessionId]);

  const sendInput = () => {
    if (selectedChannel === null || !channel?.down || !input) return;
    sendBytes(selectedChannel, new TextEncoder().encode(input));
    setInput("");
  };

  const refreshChannels = async () => {
    if (!connected) return;
    try {
      await invoke<RttChannelInfo[]>("rtt_refresh_channels", { sessionId });
      await refreshSnapshot();
    } catch (cause) {
      setError(causeMessage(cause));
    }
  };

  return (
    <div className={styles.root} data-testid="tauterm-rtt-session-view">
      <header className={styles.toolbar}>
        <div className={styles.identity}>
          <strong>RTT</strong>
          <span>{snapshot?.backend?.probe ?? "—"}</span>
          {snapshot?.backend?.target && <span>{snapshot.backend.target}</span>}
          {snapshot?.backend?.control_block_address != null && <span>CB 0x{snapshot.backend.control_block_address.toString(16)}</span>}
        </div>
        <div className={styles.stats}>
          <span>{connected ? t("rtt.connected") : t("rtt.disconnected")}</span>
          <span>RX {formatBytes(snapshot?.rx_bytes ?? 0)}</span>
          <span>TX {formatBytes(snapshot?.tx_bytes ?? 0)}</span>
          <button type="button" className="liquid-glass-button" disabled={!connected} onClick={() => void refreshChannels()} title={t("rtt.refreshChannels")}>
            <Icon name="refresh" size="sm" />
          </button>
        </div>
      </header>

      {error && <div className={styles.errorBanner}><strong>{t("rtt.runtimeError")}</strong><span>{error}</span></div>}
      {(snapshot?.dropped_history_chunks ?? 0) > 0 && <div className={styles.warningBanner}>{t("rtt.historyDropped", { count: snapshot?.dropped_history_chunks })}</div>}

      <div className={styles.body}>
        <aside className={styles.channels}>
          <div className={styles.sectionTitle}>{t("rtt.channels")}</div>
          {channels.length === 0 && <div className={styles.empty}>{t("rtt.noChannels")}</div>}
          {channels.map(item => (
            <button
              key={item.index}
              type="button"
              className={`${styles.channelButton} ${selectedChannel === item.index ? styles.channelActive : ""}`}
              onClick={() => setSelectedChannel(item.index)}
            >
              <span className={styles.channelName}>{item.name || t("rtt.channel", { index: item.index })}</span>
              <span className={styles.direction}>{item.up ? "↑" : ""}{item.down ? "↓" : ""}</span>
            </button>
          ))}
        </aside>

        <main className={styles.viewer}>
          {channel ? (
            <>
              <div className={styles.viewerHeader}>
                <div>
                  <strong>{channel.name || t("rtt.channel", { index: channel.index })}</strong>
                  <span className={styles.directionDetail}>{channel.up ? "Up" : ""}{channel.up && channel.down ? " / " : ""}{channel.down ? "Down" : ""}</span>
                </div>
                <div className={styles.modeTabs}>
                  {(["terminal", "text", "hex"] as RttViewMode[]).map(item => (
                    <button key={item} type="button" className={`liquid-glass-button ${mode === item ? "liquid-theme-selected" : ""}`} onClick={() => setMode(item)}>
                      {t(`rtt.${item}`)}
                    </button>
                  ))}
                </div>
              </div>

              <div className={styles.viewerContent}>
                {!channel.up ? <div className={styles.empty}>{t("rtt.noUp")}</div> : mode === "terminal" ? (
                  <RttTerminalView chunks={chunks} connected={connected && Boolean(channel.down)} onData={data => sendBytes(channel.index, data)} />
                ) : mode === "text" ? (
                  <pre className={styles.textView}>{text}</pre>
                ) : (
                  <pre className={styles.hexView}>{hex}</pre>
                )}
              </div>

              <div className={styles.inputBar}>
                <input
                  className="liquid-glass-input"
                  value={input}
                  disabled={!connected || !channel.down}
                  placeholder={channel.down ? t("rtt.sendPlaceholder", { index: channel.index }) : t("rtt.noDown")}
                  onChange={event => setInput(event.target.value)}
                  onKeyDown={event => {
                    if (event.key === "Enter" && !event.shiftKey) {
                      event.preventDefault();
                      sendInput();
                    }
                  }}
                />
                <button type="button" className="liquid-glass-button" disabled={!connected || !channel.down || !input} onClick={sendInput}>{t("rtt.send")}</button>
              </div>
            </>
          ) : <div className={styles.emptyCenter}>{t("rtt.noChannels")}</div>}
        </main>
      </div>
    </div>
  );
}
