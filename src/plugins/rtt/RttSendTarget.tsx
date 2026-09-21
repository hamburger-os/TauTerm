import { useTranslation } from "react-i18next";
import { usePluginRuntime } from "../../core/usePluginRuntime";
import {
  isRttDownChannelClaimed,
  isUsableRttDirection,
  systemViewObserver,
} from "./model";
import {
  selectRttAutomationSource,
  selectRttSendChannel,
  type RttRuntimeSnapshot,
} from "./runtime-store";
import styles from "../../components/SendBar/TargetBar.module.css";

export default function RttSendTarget({ sessionId, disabled = false }: { sessionId: string; disabled?: boolean }) {
  const { t } = useTranslation();
  const runtime = usePluginRuntime<RttRuntimeSnapshot>("rtt", sessionId);
  const channels = runtime.snapshot?.channels ?? [];
  const readable = channels.filter(
    channel => isUsableRttDirection(channel.up)
      && !systemViewObserver(runtime.snapshot, channel.index),
  );
  const writable = channels.filter(
    channel => isUsableRttDirection(channel.down)
      && !isRttDownChannelClaimed(runtime.snapshot, channel.index),
  );

  if (writable.length === 0) return null;

  const runtimeSend = runtime.snapshot?.send_channel ?? null;
  const sendChannel = runtimeSend != null
    && writable.some(channel => channel.index === runtimeSend)
    ? runtimeSend
    : writable[0].index;

  const runtimeSource = runtime.snapshot?.automation_source_channel ?? null;
  const receiveChannel = runtimeSource != null
    && readable.some(channel => channel.index === runtimeSource)
    ? runtimeSource
    : readable[0]?.index ?? null;

  return (
    <div className={`${styles.bar} liquid-glass-panel`}>
      {readable.length > 1 && receiveChannel != null && (
        <>
          <span className={styles.label}>{t("rtt.automationReceiveSource")}</span>
          <select
            className={`${styles.select} liquid-glass-input liquid-glass-select`}
            value={receiveChannel}
            onChange={(event) => {
              void selectRttAutomationSource(sessionId, Number(event.target.value));
            }}
            title={t("rtt.automationReceiveSourceHint")}
            disabled={disabled}
          >
            {readable.map(channel => (
              <option key={channel.index} value={channel.index}>
                {channel.name || t("rtt.channel", { index: channel.index })} · Up {channel.index}
              </option>
            ))}
          </select>
        </>
      )}
      <span className={styles.label}>{t("rtt.sendTarget")}</span>
      <select
        className={`${styles.select} liquid-glass-input liquid-glass-select`}
        value={sendChannel}
        onChange={(event) => {
          void selectRttSendChannel(sessionId, Number(event.target.value));
        }}
        title={t("rtt.sendTarget")}
        disabled={disabled}
      >
        {writable.map(channel => (
          <option key={channel.index} value={channel.index}>
            {channel.name || t("rtt.channel", { index: channel.index })} · Down {channel.index}
          </option>
        ))}
      </select>
    </div>
  );
}
