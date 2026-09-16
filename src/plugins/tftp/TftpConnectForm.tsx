import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import Icon from "../../components/common/Icon";
import type { ConnectFormProps } from "../../core/plugin-registry";
import styles from "../../components/common/ConnectionForm.module.css";

export const DEFAULT_TFTP_PARAMS: Record<string, unknown> = {
  listen_ip: "0.0.0.0",
  listen_port: 69,
  file_root: "",
  write_enabled: false,
  overwrite: false,
  single_port: false,
};

export function normalizeTftpParams(params: Record<string, unknown>): Record<string, unknown> {
  const normalized = { ...DEFAULT_TFTP_PARAMS, ...params };
  if (normalized.write_enabled !== true) normalized.overwrite = false;
  return normalized;
}

export function isTftpConnectionConfigValid(params: Record<string, unknown>): boolean {
  const p = normalizeTftpParams(params);
  const port = Number(p.listen_port);
  return typeof p.listen_ip === "string" && p.listen_ip.trim().length > 0
    && Number.isInteger(port) && port >= 1 && port <= 65535
    && typeof p.file_root === "string" && p.file_root.trim().length > 0;
}

export default function TftpConnectForm({ params, onChange, disabled = false }: ConnectFormProps) {
  const { t } = useTranslation();
  const p = normalizeTftpParams(params);
  const update = (patch: Record<string, unknown>) => onChange({ ...p, ...patch });
  const bind = String(p.listen_ip ?? "").trim().toLowerCase();
  const exposureRisk = p.write_enabled === true
    && p.overwrite === true
    && bind !== "localhost"
    && bind !== "::1"
    && !bind.startsWith("127.");

  return (
    <div className={styles.stack}>
      <div className={styles.grid2}>
        <div className={styles.field}>
          <label className={styles.label}>{t("tftp.listenIp")}</label>
          <input className={`${styles.control} liquid-glass-input`} value={String(p.listen_ip ?? "")} onChange={event => update({ listen_ip: event.target.value })} placeholder="0.0.0.0" disabled={disabled} />
        </div>
        <div className={styles.field}>
          <label className={styles.label}>{t("tftp.listenPort")}</label>
          <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={65535} value={Number(p.listen_port ?? 69)} onChange={event => update({ listen_port: Number(event.target.value) })} disabled={disabled} />
        </div>
      </div>
      <div className={styles.field}>
        <label className={styles.label}>{t("tftp.fileRoot")}</label>
        <div className={styles.row}>
          <input className={`${styles.control} liquid-glass-input`} value={String(p.file_root ?? "")} onChange={event => update({ file_root: event.target.value })} placeholder={"C:\\tftp-root\\"} disabled={disabled} />
          <button type="button" className={`${styles.iconButton} liquid-glass-button`} onClick={async () => {
            const selected = await open({ directory: true, multiple: false });
            if (typeof selected === "string") update({ file_root: selected });
          }} title={t("tftp.selectDir")} aria-label={t("tftp.selectDir")} disabled={disabled}>
            <Icon name="folder" size="md" />
          </button>
        </div>
      </div>
      <Toggle checked={p.write_enabled === true} disabled={disabled} onChange={checked => update({ write_enabled: checked, ...(checked ? {} : { overwrite: false }) })} label={t("tftp.writeEnabled")} />
      <Toggle checked={p.write_enabled === true && p.overwrite === true} disabled={disabled || p.write_enabled !== true} onChange={checked => update({ overwrite: checked })} label={t("tftp.overwrite")} />
      {exposureRisk && (
        <p className={styles.warning} role="status"><Icon name="warning" size="sm" />{t("tftp.exposureWarning")}</p>
      )}
      <Toggle checked={p.single_port === true} disabled={disabled} onChange={checked => update({ single_port: checked })} label={t("tftp.singlePort")} />
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
