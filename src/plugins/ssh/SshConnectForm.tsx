import { useTranslation } from "react-i18next";
import type { ConnectFormProps, SessionConnectOptions } from "../../core/plugin-registry";
import { CHARSETS, DEFAULT_ENCODING } from "../../utils/charsets";
import styles from "../../components/common/ConnectionForm.module.css";

export const DEFAULT_SSH_PARAMS: Record<string, unknown> = {
  host: "",
  port: 22,
  username: "",
  auth_method: "password",
  data_mode: "text",
  encoding: DEFAULT_ENCODING,
  send_bar_enabled: false,
  transfer_enabled: false,
  file_service_enabled: true,
  file_service_protocol: "sftp",
  journald_enabled: false,
};

export function normalizeSshParams(params: Record<string, unknown>): Record<string, unknown> {
  return { ...DEFAULT_SSH_PARAMS, ...params, file_service_protocol: "sftp" };
}

export function isSshConnectionConfigValid(params: Record<string, unknown>): boolean {
  const p = normalizeSshParams(params);
  const port = Number(p.port);
  return typeof p.host === "string" && p.host.trim().length > 0
    && typeof p.username === "string" && p.username.trim().length > 0
    && Number.isInteger(port) && port >= 1 && port <= 65535
    && (p.auth_method === "password" || p.auth_method === "key")
    && typeof p.encoding === "string" && p.encoding.length > 0;
}

export default function SshConnectForm({
  params,
  onChange,
  disabled = false,
  sessionOptions,
  onSessionOptionsChange,
}: ConnectFormProps) {
  const { t } = useTranslation();
  const p = normalizeSshParams(params);
  const options: SessionConnectOptions = sessionOptions ?? {
    transferEnabled: false,
    sendBarEnabled: false,
  };
  const update = (patch: Record<string, unknown>) => onChange({ ...p, ...patch });
  const updateOptions = (patch: Partial<SessionConnectOptions>) => {
    const next = { ...options, ...patch };
    onSessionOptionsChange?.(next);
    update({
      send_bar_enabled: next.sendBarEnabled,
      transfer_enabled: next.transferEnabled,
    });
  };
  const authMethod = p.auth_method === "key" ? "key" : "password";

  return (
    <div className={styles.stack}>
      <div className={styles.field}>
        <label className={styles.label}>{t("ssh.host")}</label>
        <input className={`${styles.control} liquid-glass-input`} value={String(p.host ?? "")} onChange={event => update({ host: event.target.value })} placeholder={t("ssh.hostPlaceholder")} disabled={disabled} />
      </div>
      <div className={styles.grid2}>
        <div className={styles.field}>
          <label className={styles.label}>{t("ssh.port")}</label>
          <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={65535} value={Number(p.port ?? 22)} onChange={event => update({ port: Number(event.target.value) })} disabled={disabled} />
        </div>
        <div className={styles.field}>
          <label className={styles.label}>{t("ssh.username")}</label>
          <input className={`${styles.control} liquid-glass-input`} value={String(p.username ?? "")} onChange={event => update({ username: event.target.value })} placeholder={t("ssh.usernamePlaceholder")} disabled={disabled} />
        </div>
      </div>
      <div className={styles.field}>
        <label className={styles.label}>{t("serial.encoding")}</label>
        <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.encoding ?? DEFAULT_ENCODING)} onChange={event => update({ encoding: event.target.value })} disabled={disabled}>
          {CHARSETS.map(charset => <option key={charset.id} value={charset.id}>{charset.label}</option>)}
        </select>
      </div>
      <div className={styles.field}>
        <label className={styles.label}>{t("ssh.authMethod")}</label>
        <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={authMethod} onChange={event => update({ auth_method: event.target.value })} disabled={disabled}>
          <option value="password">{t("ssh.authPassword")}</option>
          <option value="key">{t("ssh.authKey")}</option>
        </select>
      </div>
      {authMethod === "password" ? (
        <div className={styles.field}>
          <label className={styles.label}>{t("ssh.password")}</label>
          <input className={`${styles.control} liquid-glass-input`} type="password" value={typeof p.password === "string" ? p.password : ""} onChange={event => update({ password: event.target.value })} placeholder={t("ssh.passwordPlaceholder")} disabled={disabled} autoComplete="new-password" />
        </div>
      ) : (
        <>
          <div className={styles.field}>
            <label className={styles.label}>{t("ssh.sshKey")}</label>
            <textarea className={`${styles.control} ${styles.textarea} liquid-glass-input`} value={typeof p.private_key === "string" ? p.private_key : ""} onChange={event => update({ private_key: event.target.value })} placeholder={t("ssh.keyPlaceholder")} disabled={disabled} />
          </div>
          <div className={styles.field}>
            <label className={styles.label}>{t("ssh.passphrase")}</label>
            <input className={`${styles.control} liquid-glass-input`} type="password" value={typeof p.passphrase === "string" ? p.passphrase : ""} onChange={event => update({ passphrase: event.target.value })} placeholder={t("ssh.passphrasePlaceholder")} disabled={disabled} autoComplete="new-password" />
          </div>
        </>
      )}
      <Toggle checked={options.sendBarEnabled} disabled={disabled} onChange={checked => updateOptions({ sendBarEnabled: checked })} label={t("ssh.enableSendBar")} />
      <Toggle checked={options.transferEnabled} disabled={disabled} onChange={checked => updateOptions({ transferEnabled: checked })} label={t("ssh.enableTransfer")} />
      <Toggle checked={p.file_service_enabled !== false} disabled={disabled} onChange={checked => update({ file_service_enabled: checked })} label={t("ssh.enableFileService")} />
      <Toggle checked={p.journald_enabled === true} disabled={disabled} onChange={checked => update({ journald_enabled: checked })} label={t("journald.enableJournald")} />
    </div>
  );
}

function Toggle({ checked, disabled, onChange, label }: { checked: boolean; disabled: boolean; onChange: (checked: boolean) => void; label: string }) {
  return (
    <label className={`liquid-glass-toggle ${styles.toggle}`}>
      <input type="checkbox" checked={checked} onChange={event => onChange(event.target.checked)} disabled={disabled} />
      <div />
      <span>{label}</span>
    </label>
  );
}
