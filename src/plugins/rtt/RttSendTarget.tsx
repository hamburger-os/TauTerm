import { useTranslation } from "react-i18next";
import { usePluginRuntime } from "../../core/usePluginRuntime";
import { isRttDownChannelClaimed, isUsableRttDirection } from "./model";
import {
  selectRttSendChannel,
  type RttRuntimeSnapshot,
} from "./runtime-store";
import styles from "../../components/SendBar/TargetBar.module.css";

export default function RttSendTarget({ sessionId, disabled = false }: { sessionId: string; disabled?: boolean }) {
  const { t } = useTranslation();
  const runtime = usePluginRuntime<RttRuntimeSnapshot>("rtt", sessionId);
  const writable = (runtime.snapshot?.channels ?? []).filter(
    channel => isUsableRttDirection(channel.down)
      && !isRttDownChannelClaimed(runtime.snapshot, channel.index),
  );

  if (writable.length === 0) return null;

  const runtimeSelected = runtime.snapshot?.send_channel ?? null;
  const selected = runtimeSelected != null
    && writable.some(channel => channel.index === runtimeSelected)
    ? runtimeSelected
    : writable[0].index;

  return (
    <div className={`${styles.bar} liquid-glass-panel`}>
      <span className={styles.label}>{t("rtt.sendTarget")}</span>
      <select
        className={`${styles.select} liquid-glass-input liquid-glass-select`}
        value={selected}
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
