import { useCallback, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { usePluginRuntime } from "../../core/usePluginRuntime";
import GlassButton from "../../components/common/GlassButton";
import Icon from "../../components/common/Icon";
import RttTerminalView from "./RttTerminalView";
import SystemViewTraceView from "./SystemViewTraceView";
import {
  base64ToBytes,
  formatBytes,
  hasRttChannelIssues,
  isUsableRttDirection,
  resolveRttViewMode,
  rttChannelIssues,
  systemViewObserver,
  type RttChannelInfo,
  type RttChunk,
  type RttViewMode,
} from "./model";
import {
  attachSystemView,
  clearSystemView,
  controlSystemView,
  detachSystemView,
  ensureRttHistory,
  ensureSystemViewHistory,
  refreshRttRuntime,
  rttChannelChunks,
  selectRttChannel,
  sendRttTerminalData,
  setRttViewMode,
  type RttRuntimeSnapshot,
} from "./runtime-store";
import styles from "./RttSessionView.module.css";

const HEX_VIEW_BYTES = 64 * 1024;
const LOG_VIEW_MAX_CHUNKS = 2_000;

function formatHex(chunks: readonly RttChunk[]): string {
  const rows: string[] = [];
  let remaining = HEX_VIEW_BYTES;
  const visible: Array<{ chunk: RttChunk; bytes: Uint8Array; offset: number }> = [];

  for (let index = chunks.length - 1; index >= 0 && remaining > 0; index -= 1) {
    const chunk = chunks[index];
    const bytes = base64ToBytes(chunk.data_b64);
    if (bytes.length <= remaining) {
      visible.unshift({ chunk, bytes, offset: chunk.channel_offset });
      remaining -= bytes.length;
    } else {
      const keep = bytes.subarray(bytes.length - remaining);
      visible.unshift({
        chunk,
        bytes: keep,
        offset: chunk.channel_offset + bytes.length - keep.length,
      });
      remaining = 0;
    }
  }

  for (const item of visible) {
    for (let offset = 0; offset < item.bytes.length; offset += 16) {
      const row = item.bytes.subarray(offset, offset + 16);
      const hex = Array.from(row, value => value.toString(16).padStart(2, "0")).join(" ").padEnd(47, " ");
      const ascii = Array.from(row, value => value >= 32 && value < 127 ? String.fromCharCode(value) : ".").join("");
      rows.push(`${(item.offset + offset).toString(16).padStart(8, "0")}  ${hex}  ${ascii}`);
    }
  }
  return rows.join("\n");
}

function formatChunkTime(timestampMs: number): string {
  return new Date(timestampMs).toLocaleTimeString([], {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    fractionalSecondDigits: 3,
  });
}

function decodeLogChunks(chunks: readonly RttChunk[]): Array<{ chunk: RttChunk; text: string }> {
  const decoder = new TextDecoder();
  return chunks.map((chunk, index) => ({
    chunk,
    text: decoder.decode(base64ToBytes(chunk.data_b64), { stream: index + 1 < chunks.length }),
  }));
}

export default function RttSessionView({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const connected = tab?.state === "connected" || tab?.state === "transferring";
  const runtimeReadable = connected || tab?.disconnectInfo?.retain_terminal === true;
  const runtime = usePluginRuntime<RttRuntimeSnapshot>("rtt", sessionId);
  const snapshot = runtime.snapshot;
  const selectedChannel = runtime.selectedChannel;
  const channels = snapshot?.channels ?? [];
  const channel = channels.find(item => item.index === selectedChannel) ?? null;
  const channelIssues = channel ? rttChannelIssues(channel) : [];
  const usableUp = isUsableRttDirection(channel?.up);
  const usableDown = isUsableRttDirection(channel?.down);
  const chunks = rttChannelChunks(runtime, selectedChannel);
  const observer = selectedChannel == null ? null : systemViewObserver(snapshot, selectedChannel);
  const systemViewState = selectedChannel == null ? undefined : runtime.systemview[selectedChannel];
  const configuredMode = selectedChannel == null ? undefined : runtime.viewModes[selectedChannel];
  const mode = resolveRttViewMode(snapshot, selectedChannel, configuredMode);

  useEffect(() => {
    if (!runtimeReadable) return;
    void refreshRttRuntime(sessionId);
  }, [runtimeReadable, sessionId, tab?.connectedAt]);

  useEffect(() => {
    if (!runtimeReadable || selectedChannel == null) return;
    void ensureRttHistory(sessionId, selectedChannel);
  }, [runtimeReadable, selectedChannel, sessionId, snapshot?.generation]);

  useEffect(() => {
    if (!runtimeReadable || selectedChannel == null || !observer) return;
    void ensureSystemViewHistory(sessionId, selectedChannel);
  }, [observer, runtimeReadable, selectedChannel, sessionId, snapshot?.generation]);

  const hex = useMemo(() => mode === "hex" || mode === "raw" ? formatHex(chunks) : "", [chunks, mode]);
  const logRows = useMemo(
    () => mode === "log" ? decodeLogChunks(chunks.slice(-LOG_VIEW_MAX_CHUNKS)) : [],
    [chunks, mode],
  );

  const refreshChannels = useCallback(async () => {
    if (!connected || !snapshot?.backend?.capabilities.enumerate_channels) return;
    await invoke<RttChannelInfo[]>("rtt_refresh_channels", { sessionId });
    await refreshRttRuntime(sessionId);
  }, [connected, sessionId, snapshot?.backend?.capabilities.enumerate_channels]);

  return (
    <div className={styles.root} data-testid="tauterm-rtt-session-view">
      <header className={styles.toolbar}>
        <div className={styles.identity}>
          <strong>RTT</strong>
          <span>{snapshot?.backend?.probe ?? "—"}</span>
          {snapshot?.backend?.target && <span>{snapshot.backend.target}</span>}
          {snapshot?.backend?.control_block_address != null && (
            <span>CB {snapshot.backend.control_block_address}</span>
          )}
        </div>
        <div className={styles.stats}>
          <span>{connected ? t("rtt.connected") : t("rtt.disconnected")}</span>
          <span>RX {formatBytes(snapshot?.rx_bytes ?? 0)}</span>
          <span>TX {formatBytes(snapshot?.tx_bytes ?? 0)}</span>
          {(snapshot?.evicted_history_chunks ?? 0) > 0 && (
            <span
              title={t("rtt.historyRollingHint", {
                chunks: snapshot?.evicted_history_chunks ?? 0,
                bytes: formatBytes(snapshot?.evicted_history_bytes ?? 0),
              })}
            >
              {t("rtt.historyRolling")}
            </span>
          )}
          {snapshot?.backend?.capabilities.enumerate_channels && (
            <GlassButton
              size="sm"
              iconOnly
              disabled={!connected}
              onClick={() => void refreshChannels()}
              title={t("rtt.refreshChannels")}
              aria-label={t("rtt.refreshChannels")}
            >
              <Icon name="refresh" size="md" />
            </GlassButton>
          )}
        </div>
      </header>

      {runtime.error && (
        <div className={styles.errorBanner}>
          <strong>{t("rtt.runtimeError")}</strong>
          <span>{runtime.error}</span>
        </div>
      )}
      {(snapshot?.dropped_automation_chunks ?? 0) > 0 && (
        <div className={styles.warningBanner}>
          {t("rtt.automationDropped", { count: snapshot?.dropped_automation_chunks })}
        </div>
      )}
      {(snapshot?.dropped_presentation_chunks ?? 0) > 0 && (
        <div className={styles.warningBanner}>
          {t("rtt.presentationDropped", { count: snapshot?.dropped_presentation_chunks })}
        </div>
      )}
      {(snapshot?.runtime_pressure_events ?? 0) > 0 && (
        <div className={styles.warningBanner}>
          {t("rtt.runtimePressure", { count: snapshot?.runtime_pressure_events })}
        </div>
      )}

      <div className={styles.body}>
        <aside className={styles.channels}>
          <div className={styles.sectionTitle}>{t("rtt.channels")}</div>
          {channels.length === 0 && <div className={styles.empty}>{t("rtt.noChannels")}</div>}
          {channels.map(item => (
            <button
              key={item.index}
              type="button"
              className={`${styles.channelButton} ${selectedChannel === item.index ? styles.channelActive : ""}`}
              onClick={() => {
                void selectRttChannel(sessionId, item.index);
                void ensureRttHistory(sessionId, item.index);
              }}
            >
              <span className={styles.channelName}>
                {item.name || t("rtt.channel", { index: item.index })}
              </span>
              <span className={styles.direction}>
                {isUsableRttDirection(item.up) ? "↑" : ""}
                {isUsableRttDirection(item.down) ? "↓" : ""}
                {hasRttChannelIssues(item) ? <span className={styles.channelWarning}>⚠</span> : null}
              </span>
            </button>
          ))}
        </aside>

        <main className={styles.viewer}>
          {channel ? (
            <>
              <div className={styles.viewerHeader}>
                <div>
                  <strong>{channel.name || t("rtt.channel", { index: channel.index })}</strong>
                  <span className={styles.directionDetail}>
                    {usableUp ? "Up" : ""}
                    {usableUp && usableDown ? " / " : ""}
                    {usableDown ? "Down" : ""}
                    {channelIssues.length > 0 ? ` · ${t("rtt.channelDegraded")}` : ""}
                  </span>
                </div>
                <div className={styles.viewerActions}>
                  <div className={`${styles.modeTabs} liquid-selector-strip`}>
                    {(observer
                      ? (["trace", "events", "raw"] as RttViewMode[])
                      : (["terminal", "log", "hex"] as RttViewMode[])
                    ).map(item => (
                      <button
                        key={item}
                        type="button"
                        className={`liquid-glass-button liquid-selector-button ${mode === item ? "active" : ""}`}
                        aria-pressed={mode === item}
                        onClick={() => setRttViewMode(sessionId, channel.index, item)}
                      >
                        {t(`rtt.${item}`)}
                      </button>
                    ))}
                  </div>
                  {!observer && usableUp && (
                    <GlassButton
                      size="sm"
                      title={t("rtt.enableTraceHint")}
                      onClick={() => {
                        void attachSystemView(sessionId, channel.index).then(attached => {
                          if (attached) setRttViewMode(sessionId, channel.index, "trace");
                        });
                      }}
                    >
                      {t("rtt.enableTrace")}
                    </GlassButton>
                  )}
                  {observer && (
                    <GlassButton
                      size="sm"
                      title={t("rtt.disableTraceHint")}
                      onClick={() => void detachSystemView(sessionId, channel.index)}
                    >
                      {t("rtt.disableTrace")}
                    </GlassButton>
                  )}
                </div>
              </div>

              <div className={styles.viewerContent}>
                {!usableUp ? (
                  channelIssues.length > 0 ? (
                    <div className={styles.issuePanel}>
                      <strong>{t("rtt.channelUnavailable")}</strong>
                      {channelIssues.map(issue => (
                        <div key={issue} className={styles.issueRow}>{issue}</div>
                      ))}
                    </div>
                  ) : (
                    <div className={styles.empty}>{t("rtt.noUp")}</div>
                  )
                ) : observer && (mode === "trace" || mode === "events") ? (
                  <SystemViewTraceView
                    state={systemViewState}
                    mode={mode}
                    controlAvailable={observer.control_channel_index != null}
                    onStart={() => void controlSystemView(sessionId, channel.index, "start")}
                    onStop={() => void controlSystemView(sessionId, channel.index, "stop")}
                    onRefresh={() => void controlSystemView(sessionId, channel.index, "refresh")}
                    onClear={() => void clearSystemView(sessionId, channel.index)}
                  />
                ) : mode === "terminal" ? (
                  <RttTerminalView
                    key={`${snapshot?.generation ?? 0}:${channel.index}`}
                    chunks={chunks}
                    connected={connected && usableDown}
                    onData={data => void sendRttTerminalData(sessionId, channel.index, data)}
                  />
                ) : mode === "log" ? (
                  <div className={styles.logView}>
                    {logRows.length === 0 ? (
                      <div className={styles.empty}>{t("rtt.noData")}</div>
                    ) : logRows.map(({ chunk, text }) => (
                      <div key={`${chunk.generation}:${chunk.sequence}`} className={styles.logRow}>
                        <span className={styles.logMeta}>
                          {formatChunkTime(chunk.timestamp_ms)} · #{chunk.sequence} · +0x{chunk.channel_offset.toString(16)}
                        </span>
                        <span className={styles.logPayload}>{text}</span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <pre className={styles.hexView}>{hex}</pre>
                )}
              </div>
            </>
          ) : (
            <div className={styles.emptyCenter}>{t("rtt.noChannels")}</div>
          )}
        </main>
      </div>
    </div>
  );
}
