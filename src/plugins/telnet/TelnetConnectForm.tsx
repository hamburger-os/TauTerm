import { useTranslation } from "react-i18next";
import type { ConnectFormProps, SessionConnectOptions } from "../../core/plugin-registry";
import { CHARSETS, DEFAULT_ENCODING } from "../../utils/charsets";
import styles from "../../components/common/ConnectionForm.module.css";

export const DEFAULT_TELNET_PARAMS: Record<string, unknown> = {
  host: "",
  port: 23,
  encoding: DEFAULT_ENCODING,
  send_bar_enabled: true,
};

export function normalizeTelnetParams(params: Record<string, unknown>): Record<string, unknown> {
  return { ...DEFAULT_TELNET_PARAMS, ...params };
}

export function isTelnetConnectionConfigValid(params: Record<string, unknown>): boolean {
  const p = normalizeTelnetParams(params);
  const port = Number(p.port);
  return typeof p.host === "string" && p.host.trim().length > 0
    && Number.isInteger(port) && port >= 1 && port <= 65535
    && typeof p.encoding === "string" && p.encoding.length > 0;
}

export default function TelnetConnectForm({
  params,
  onChange,
  disabled = false,
  sessionOptions,
  onSessionOptionsChange,
}: ConnectFormProps) {
  const { t } = useTranslation();
  const p = normalizeTelnetParams(params);
  const options: SessionConnectOptions = sessionOptions ?? { transferEnabled: false, sendBarEnabled: true };
  const update = (patch: Record<string, unknown>) => onChange({ ...p, ...patch });

  return (
    <div className={styles.stack}>
      <div className={styles.field}>
        <label className={styles.label}>{t("telnet.host")}</label>
        <input className={`${styles.control} liquid-glass-input`} value={String(p.host ?? "")} onChange={event => update({ host: event.target.value })} placeholder={t("telnet.hostPlaceholder")} disabled={disabled} />
      </div>
      <div className={styles.grid2}>
        <div className={styles.field}>
          <label className={styles.label}>{t("telnet.port")}</label>
          <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={65535} value={Number(p.port ?? 23)} onChange={event => update({ port: Number(event.target.value) })} disabled={disabled} />
        </div>
        <div className={styles.field}>
          <label className={styles.label}>{t("serial.encoding")}</label>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.encoding ?? DEFAULT_ENCODING)} onChange={event => update({ encoding: event.target.value })} disabled={disabled}>
            {CHARSETS.map(charset => <option key={charset.id} value={charset.id}>{charset.label}</option>)}
          </select>
        </div>
      </div>
      <label className={`liquid-glass-toggle ${styles.toggle}`}>
        <input type="checkbox" checked={options.sendBarEnabled} onChange={event => {
          const sendBarEnabled = event.target.checked;
          onSessionOptionsChange?.({ ...options, sendBarEnabled, transferEnabled: false });
          update({ send_bar_enabled: sendBarEnabled });
        }} disabled={disabled} />
        <div />
        <span>{t("telnet.enableSendBar")}</span>
      </label>
    </div>
  );
}
