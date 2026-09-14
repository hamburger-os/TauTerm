import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ConnectFormProps, SessionConnectOptions } from "../../core/plugin-registry";
import { CHARSETS, DEFAULT_ENCODING } from "../../utils/charsets";
import Icon from "../../components/common/Icon";
import styles from "./SerialConnectForm.module.css";

export const DEFAULT_SERIAL_PARAMS: Record<string, unknown> = {
  baud_rate: 115200,
  data_bits: 8,
  parity: "none",
  stop_bits: "1",
  flow_control: "none",
  data_mode: "text",
  dual_frame_timeout_ms: 50,
  encoding: DEFAULT_ENCODING,
  virtual_port_enabled: false,
  virtual_port_count: 1,
};

const BAUD_RATES = [110, 300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const DATA_BITS = [5, 6, 7, 8];
const SERIAL_PARAM_KEYS = Object.keys(DEFAULT_SERIAL_PARAMS);

export function normalizeSerialParams(params: Record<string, unknown>): Record<string, unknown> {
  const normalized: Record<string, unknown> = {};
  for (const key of SERIAL_PARAM_KEYS) {
    if (params[key] !== undefined) normalized[key] = params[key];
  }
  return normalized;
}

export function isSerialConnectionConfigValid(params: Record<string, unknown>): boolean {
  const p = normalizeSerialParams(params);
  return typeof p.baud_rate === "number" && Number.isInteger(p.baud_rate) && p.baud_rate > 0
    && typeof p.data_bits === "number" && DATA_BITS.includes(p.data_bits)
    && ["none", "even", "odd"].includes(String(p.parity))
    && ["1", "2"].includes(String(p.stop_bits))
    && ["none", "rts_cts", "xon_xoff"].includes(String(p.flow_control))
    && ["text", "hex", "dual"].includes(String(p.data_mode))
    && typeof p.encoding === "string" && p.encoding.length > 0
    && typeof p.dual_frame_timeout_ms === "number"
    && Number.isFinite(p.dual_frame_timeout_ms)
    && p.dual_frame_timeout_ms >= 5
    && p.dual_frame_timeout_ms <= 500
    && typeof p.virtual_port_enabled === "boolean"
    && typeof p.virtual_port_count === "number"
    && Number.isInteger(p.virtual_port_count)
    && p.virtual_port_count >= 1
    && p.virtual_port_count <= 4;
}

export default function SerialConnectForm({
  params,
  onChange,
  endpoints = [],
  endpoint = "",
  onEndpointChange,
  onRefreshEndpoints,
  refreshingEndpoints = false,
  disabled = false,
  sessionOptions,
  onSessionOptionsChange,
}: ConnectFormProps) {
  const { t } = useTranslation();
  const p = normalizeSerialParams(params);
  const options: SessionConnectOptions = sessionOptions ?? {
    transferEnabled: true,
    transferProtocol: "ymodem",
    sendBarEnabled: true,
  };
  const update = (key: string, value: unknown) => onChange({ ...p, [key]: value });
  const updateOptions = (patch: Partial<SessionConnectOptions>) => {
    onSessionOptionsChange?.({ ...options, ...patch });
  };
  const mode = String(p.data_mode);
  const virtualPortEnabled = p.virtual_port_enabled === true;

  return (
    <div className={styles.stack}>
      <div className={styles.field}>
        <label className={styles.label}>{t("serial.port")}</label>
        <div className={styles.endpointRow}>
          <select
            className={`${styles.control} liquid-glass-input liquid-glass-select`}
            value={endpoint}
            onChange={event => onEndpointChange?.(event.target.value)}
            disabled={disabled}
          >
            {endpoints.length === 0 && <option value="">{t("serial.noPorts")}</option>}
            {endpoints.map(item => (
              <option key={item.name} value={item.name}>
                {item.name}{item.description !== item.name ? ` — ${item.description}` : ""}
              </option>
            ))}
          </select>
          <button
            type="button"
            className={`${styles.refreshButton} liquid-glass-button`}
            onClick={onRefreshEndpoints}
            title={t("serial.refresh")}
            aria-label={t("serial.refresh")}
            disabled={disabled || refreshingEndpoints}
          >
            <Icon name="refresh" size="md" />
          </button>
        </div>
      </div>

      <div className={styles.grid2}>
        <Field label={t("serial.baudRate")}>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={Number(p.baud_rate)} onChange={event => update("baud_rate", Number(event.target.value))} disabled={disabled}>
            {BAUD_RATES.map(value => <option key={value} value={value}>{value}</option>)}
          </select>
        </Field>
        <Field label={t("serial.dataBits")}>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={Number(p.data_bits)} onChange={event => update("data_bits", Number(event.target.value))} disabled={disabled}>
            {DATA_BITS.map(value => <option key={value} value={value}>{value}</option>)}
          </select>
        </Field>
      </div>

      <div className={styles.grid2}>
        <Field label={t("serial.parity")}>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.parity)} onChange={event => update("parity", event.target.value)} disabled={disabled}>
            <option value="none">None</option><option value="even">Even</option><option value="odd">Odd</option>
          </select>
        </Field>
        <Field label={t("serial.stopBits")}>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.stop_bits)} onChange={event => update("stop_bits", event.target.value)} disabled={disabled}>
            <option value="1">1</option><option value="2">2</option>
          </select>
        </Field>
      </div>

      <Field label={t("serial.flowControl")}>
        <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.flow_control)} onChange={event => update("flow_control", event.target.value)} disabled={disabled}>
          <option value="none">None</option><option value="rts_cts">RTS/CTS</option><option value="xon_xoff">XON/XOFF</option>
        </select>
      </Field>

      <Field label={t("serial.dataMode")}>
        <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={mode} onChange={event => update("data_mode", event.target.value)} disabled={disabled}>
          <option value="text">{t("serial.dataModeText")}</option>
          <option value="hex">{t("serial.dataModeHex")}</option>
          <option value="dual">{t("serial.dataModeDual")}</option>
        </select>
      </Field>

      <Field label={t("serial.encoding")}>
        <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={String(p.encoding)} onChange={event => update("encoding", event.target.value)} disabled={disabled}>
          {CHARSETS.map(charset => <option key={charset.id} value={charset.id}>{charset.label}</option>)}
        </select>
      </Field>

      {mode === "dual" && (
        <Field label={t("serial.dualFrameTimeout")}>
          <input className={`${styles.control} liquid-glass-input`} type="number" min={5} max={500} step={5} value={Number(p.dual_frame_timeout_ms)} onChange={event => update("dual_frame_timeout_ms", Number(event.target.value))} disabled={disabled} />
        </Field>
      )}

      <Toggle checked={options.transferEnabled} disabled={disabled} onChange={checked => updateOptions({ transferEnabled: checked })} label={t("serial.enableTransfer")} />
      {options.transferEnabled && (
        <Field label={t("serial.transferProtocol")}>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={options.transferProtocol ?? "ymodem"} onChange={event => updateOptions({ transferProtocol: event.target.value })} disabled={disabled}>
            <option value="ymodem">YModem</option><option value="xmodem">XModem</option><option value="zmodem">ZModem</option>
          </select>
        </Field>
      )}
      <Toggle checked={options.sendBarEnabled} disabled={disabled} onChange={checked => updateOptions({ sendBarEnabled: checked })} label={t("serial.enableSendBar") || "启用发送栏"} />
      <Toggle checked={virtualPortEnabled} disabled={disabled} onChange={checked => update("virtual_port_enabled", checked)} label={t("serial.enableVirtualPort") || "启用虚拟串口"} />
      {virtualPortEnabled && (
        <Field label={t("serial.virtualPortCount") || "设备数量"}>
          <select className={`${styles.control} liquid-glass-input liquid-glass-select`} value={Number(p.virtual_port_count)} onChange={event => update("virtual_port_count", Number(event.target.value))} disabled={disabled}>
            {[1, 2, 3, 4].map(value => <option key={value} value={value}>{value}</option>)}
          </select>
        </Field>
      )}
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <div className={styles.field}><label className={styles.label}>{label}</label>{children}</div>;
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
