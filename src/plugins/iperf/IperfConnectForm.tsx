import { useTranslation } from "react-i18next";
import type { ConnectFormProps } from "../../core/plugin-registry";
import styles from "../../components/common/ConnectionForm.module.css";

export const DEFAULT_IPERF_PARAMS: Record<string, unknown> = {
  version: "iperf2",
  listen_ip: "0.0.0.0",
  listen_port: 5001,
};

export function normalizeIperfParams(params: Record<string, unknown>): Record<string, unknown> {
  return { ...DEFAULT_IPERF_PARAMS, ...params };
}

export function isIperfConnectionConfigValid(params: Record<string, unknown>): boolean {
  const p = normalizeIperfParams(params);
  const port = Number(p.listen_port);
  return (p.version === "iperf2" || p.version === "iperf3")
    && typeof p.listen_ip === "string" && p.listen_ip.trim().length > 0
    && Number.isInteger(port) && port >= 1 && port <= 65535;
}

export default function IperfConnectForm({ params, onChange, disabled = false }: ConnectFormProps) {
  const { t } = useTranslation();
  const p = normalizeIperfParams(params);
  const update = (patch: Record<string, unknown>) => onChange({ ...p, ...patch });
  const version = p.version === "iperf3" ? "iperf3" : "iperf2";

  return (
    <div className={styles.stack}>
      <div className={styles.field}>
        <label className={styles.label}>{t("iperf.version")}</label>
        <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={version} onChange={event => {
          const next = event.target.value === "iperf3" ? "iperf3" : "iperf2";
          const currentPort = Number(p.listen_port);
          update({
            version: next,
            ...(currentPort === 5001 || currentPort === 5201
              ? { listen_port: next === "iperf3" ? 5201 : 5001 }
              : {}),
          });
        }} disabled={disabled}>
          <option value="iperf2">iperf2</option>
          <option value="iperf3">iperf3</option>
        </select>
      </div>
      <div className={styles.grid2}>
        <div className={styles.field}>
          <label className={styles.label}>{t("iperf.listenIp")}</label>
          <input className={`${styles.control} liquid-glass-input`} value={String(p.listen_ip ?? "")} onChange={event => update({ listen_ip: event.target.value })} placeholder="0.0.0.0" disabled={disabled} />
        </div>
        <div className={styles.field}>
          <label className={styles.label}>{t("iperf.listenPort")}</label>
          <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={65535} value={Number(p.listen_port ?? (version === "iperf3" ? 5201 : 5001))} onChange={event => {
            if (event.target.value === "") return;
            const port = Number(event.target.value);
            if (Number.isInteger(port) && port >= 1 && port <= 65535) update({ listen_port: port });
          }} disabled={disabled} />
        </div>
      </div>
    </div>
  );
}
