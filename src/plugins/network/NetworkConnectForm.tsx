import { useTranslation } from "react-i18next";
import type { ConnectFormProps } from "../../core/plugin-registry";
import { CHARSETS, DEFAULT_ENCODING } from "../../utils/charsets";
import styles from "../../components/common/ConnectionForm.module.css";

export const DEFAULT_NETWORK_PARAMS: Record<string, unknown> = {
  transport: "tcp",
  role: "client",
  remote_host: "",
  remote_port: 8080,
  local_host: "0.0.0.0",
  local_port: 8080,
  max_clients: 16,
  connect_timeout_ms: 5000,
  nodelay: true,
  broadcast: false,
  multicast_group: "",
  ttl: 64,
  multicast_interface: "0.0.0.0",
  self_receive: true,
  data_mode: "text",
  encoding: DEFAULT_ENCODING,
};

export function normalizeNetworkParams(params: Record<string, unknown>): Record<string, unknown> {
  const normalized = { ...DEFAULT_NETWORK_PARAMS, ...params };
  if (normalized.transport === "udp") delete normalized.data_mode;
  return normalized;
}

export function isNetworkConnectionConfigValid(params: Record<string, unknown>): boolean {
  const p = normalizeNetworkParams(params);
  const transport = p.transport;
  const role = p.role;
  const remotePort = Number(p.remote_port);
  const localPort = Number(p.local_port);
  if (transport !== "tcp" && transport !== "udp") return false;
  if (role !== "client" && role !== "server") return false;
  if (role === "client") {
    if (typeof p.remote_host !== "string" || p.remote_host.trim().length === 0) return false;
    if (!Number.isInteger(remotePort) || remotePort < 1 || remotePort > 65535) return false;
  } else if (!Number.isInteger(localPort) || localPort < 1 || localPort > 65535) {
    return false;
  }
  return typeof p.encoding === "string" && p.encoding.length > 0
    && Number.isInteger(Number(p.max_clients)) && Number(p.max_clients) >= 1
    && Number.isFinite(Number(p.connect_timeout_ms)) && Number(p.connect_timeout_ms) >= 100;
}

export default function NetworkConnectForm({ params, onChange, disabled = false }: ConnectFormProps) {
  const { t } = useTranslation();
  const p = normalizeNetworkParams(params);
  const update = (patch: Record<string, unknown>) => onChange({ ...p, ...patch });
  const transport = p.transport === "udp" ? "udp" : "tcp";
  const role = p.role === "server" ? "server" : "client";
  const multicastGroup = String(p.multicast_group ?? "");
  const multicastInvalid = multicastGroup.trim().length > 0
    && !/^2(2[4-9]|3\d)(\.\d{1,3}){3}$/.test(multicastGroup.trim());

  return (
    <div className={styles.stack}>
      <div className={styles.grid2}>
        <div className={styles.field}>
          <label className={styles.label}>{t("network.transport")}</label>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={transport} onChange={event => update({ transport: event.target.value, ...(event.target.value === "udp" ? { data_mode: undefined } : { data_mode: p.data_mode ?? "text" }) })} disabled={disabled}>
            <option value="tcp">{t("network.transportTcp")}</option>
            <option value="udp">{t("network.transportUdp")}</option>
          </select>
        </div>
        <div className={styles.field}>
          <label className={styles.label}>{t("network.role")}</label>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={role} onChange={event => update({ role: event.target.value })} disabled={disabled}>
            <option value="client">{t("network.roleClient")}</option>
            <option value="server">{t("network.roleServer")}</option>
          </select>
        </div>
      </div>

      {role === "client" ? (
        <div className={styles.grid2}>
          <div className={styles.field}>
            <label className={styles.label}>{t("network.remoteHost")}</label>
            <input className={`${styles.control} liquid-glass-input`} value={String(p.remote_host ?? "")} onChange={event => update({ remote_host: event.target.value })} placeholder={t("network.remoteHostPlaceholder")} disabled={disabled} />
          </div>
          <div className={styles.field}>
            <label className={styles.label}>{t("network.remotePort")}</label>
            <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={65535} value={Number(p.remote_port ?? 8080)} onChange={event => update({ remote_port: Number(event.target.value) })} disabled={disabled} />
          </div>
        </div>
      ) : (
        <div className={styles.grid2}>
          <div className={styles.field}>
            <label className={styles.label}>{t("network.localHost")}</label>
            <input className={`${styles.control} liquid-glass-input`} value={String(p.local_host ?? "0.0.0.0")} onChange={event => update({ local_host: event.target.value })} disabled={disabled} />
          </div>
          <div className={styles.field}>
            <label className={styles.label}>{t("network.localPort")}</label>
            <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={65535} value={Number(p.local_port ?? 8080)} onChange={event => update({ local_port: Number(event.target.value) })} disabled={disabled} />
          </div>
        </div>
      )}

      {transport === "tcp" && role === "server" && (
        <div className={styles.field}>
          <label className={styles.label}>{t("network.maxClients")}</label>
          <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={1024} value={Number(p.max_clients ?? 16)} onChange={event => update({ max_clients: Number(event.target.value) })} disabled={disabled} />
        </div>
      )}
      {transport === "tcp" && role === "client" && (
        <div className={styles.field}>
          <label className={styles.label}>{t("network.connectTimeoutMs")}</label>
          <input className={`${styles.control} liquid-glass-input`} type="number" min={100} value={Number(p.connect_timeout_ms ?? 5000)} onChange={event => update({ connect_timeout_ms: Number(event.target.value) })} disabled={disabled} />
        </div>
      )}
      {transport === "tcp" && (
        <Toggle checked={p.nodelay !== false} disabled={disabled} onChange={checked => update({ nodelay: checked })} label={t("network.nodelay", { defaultValue: "TCP No Delay" })} />
      )}

      {transport === "udp" && role === "server" && (
        <>
          <Toggle checked={p.broadcast === true} disabled={disabled} onChange={checked => update({ broadcast: checked })} label={t("network.broadcast")} />
          <div className={styles.field}>
            <label className={styles.label}>{t("network.multicastGroup")}</label>
            <input className={`${styles.control} liquid-glass-input`} value={multicastGroup} onChange={event => update({ multicast_group: event.target.value })} placeholder={t("network.multicastGroupPlaceholder")} disabled={disabled} />
            {multicastInvalid && <p className={styles.hint}>{t("network.multicastIpv4Only")}</p>}
          </div>
          {multicastGroup.trim() && (
            <>
              <div className={styles.grid2}>
                <div className={styles.field}>
                  <label className={styles.label}>{t("network.ttl")}</label>
                  <input className={`${styles.control} liquid-glass-input`} type="number" min={1} max={255} value={Number(p.ttl ?? 64)} onChange={event => update({ ttl: Number(event.target.value) })} disabled={disabled} />
                </div>
                <div className={styles.field}>
                  <label className={styles.label}>{t("network.multicastInterface")}</label>
                  <input className={`${styles.control} liquid-glass-input`} value={String(p.multicast_interface ?? "0.0.0.0")} onChange={event => update({ multicast_interface: event.target.value })} disabled={disabled} />
                </div>
              </div>
              <Toggle checked={p.self_receive !== false} disabled={disabled} onChange={checked => update({ self_receive: checked })} label={t("network.selfReceive")} />
            </>
          )}
        </>
      )}

      {transport === "tcp" && (
        <div className={styles.field}>
          <label className={styles.label}>{t("serial.dataMode")}</label>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.data_mode ?? "text")} onChange={event => update({ data_mode: event.target.value })} disabled={disabled}>
            <option value="text">{t("serial.dataModeText")}</option>
            <option value="hex">{t("serial.dataModeHex")}</option>
            <option value="dual">{t("serial.dataModeDual")}</option>
          </select>
        </div>
      )}
      <div className={styles.field}>
        <label className={styles.label}>{t("serial.encoding")}</label>
        <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.encoding ?? DEFAULT_ENCODING)} onChange={event => update({ encoding: event.target.value })} disabled={disabled}>
          {CHARSETS.map(charset => <option key={charset.id} value={charset.id}>{charset.label}</option>)}
        </select>
      </div>
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
