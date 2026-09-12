import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../components/common/Icon";
import type { ConnectFormProps } from "../../core/plugin-registry";
import {
  defaultModbusSessionParams,
  normalizeModbusSessionParams,
  type ModbusMode,
  type ModbusRole,
} from "./model";
import styles from "./Modbus.module.css";

type Endpoint = { name: string; description: string; connection_type: string };

const defaultTcpHost = (role: ModbusRole) => role === "server" ? "0.0.0.0" : "127.0.0.1";
const isRoleDefaultTcpHost = (host: string) => !host || host === "127.0.0.1" || host === "0.0.0.0";

export default function ModbusConnectForm({ params, onChange }: ConnectFormProps) {
  const { t } = useTranslation();
  const [ports, setPorts] = useState<Endpoint[]>([]);
  const normalized = useMemo(() => normalizeModbusSessionParams(params), [params]);
  const { mode, role, serial, tcp } = normalized;

  useEffect(() => {
    if (Object.keys(params).length !== 0) return;
    onChange(defaultModbusSessionParams());
  }, [onChange, params]);

  useEffect(() => {
    if (mode === "tcp") {
      setPorts([]);
      return;
    }
    void invoke<Endpoint[]>("enumerate_endpoints", { connectionType: "serial" })
      .then(setPorts)
      .catch(() => setPorts([]));
  }, [mode]);

  const patch = (next: Record<string, unknown>) => onChange({ ...normalized, ...next });
  const patchSerial = (next: Record<string, unknown>) => patch({ serial: { ...serial, ...next } });
  const patchTcp = (next: Record<string, unknown>) => patch({ tcp: { ...tcp, ...next } });

  const changeMode = (nextMode: ModbusMode) => {
    const next: Record<string, unknown> = { mode: nextMode };
    if (nextMode === "tcp") {
      const host = normalized.host.trim();
      if (isRoleDefaultTcpHost(host)) next.host = defaultTcpHost(role);
    } else if (role === "server" && normalized.unit_id === 0) {
      next.unit_id = 1;
    }
    patch(next);
  };

  const changeRole = (nextRole: ModbusRole) => {
    const next: Record<string, unknown> = { role: nextRole };
    if (mode === "tcp") {
      const host = normalized.host.trim();
      if (isRoleDefaultTcpHost(host)) next.host = defaultTcpHost(nextRole);
    } else if (nextRole === "server" && normalized.unit_id === 0) {
      next.unit_id = 1;
    }
    patch(next);
  };

  const unitIdMin = mode !== "tcp" && role === "server" ? 1 : 0;

  return (
    <div className={styles.connectRoot} data-testid="tauterm-modbus-connect-form">
      <div className={styles.connectGroup}>
        <div className={styles.connectGroupTitle}>{t("modbus.connectSessionMode")}</div>
        <div className={styles.twoColumns}>
          <Field label={t("modbus.connectTransportMode")}>
            <select
              className="liquid-glass-input liquid-glass-select"
              value={mode}
              onChange={event => changeMode(event.target.value as ModbusMode)}
              data-testid="tauterm-modbus-mode"
            >
              <option value="rtu">Modbus RTU</option>
              <option value="ascii">Modbus ASCII</option>
              <option value="tcp">Modbus TCP</option>
            </select>
          </Field>
          <Field label={t("modbus.connectRole")}>
            <select
              className="liquid-glass-input liquid-glass-select"
              value={role}
              onChange={event => changeRole(event.target.value as ModbusRole)}
              data-testid="tauterm-modbus-role"
            >
              <option value="client">{mode === "tcp" ? t("modbus.connectClient") : t("modbus.connectMaster")}</option>
              <option value="server">{mode === "tcp" ? t("modbus.connectServer") : t("modbus.connectSlave")}</option>
            </select>
          </Field>
        </div>
      </div>

      <div className={styles.connectGroup}>
        <div className={styles.connectGroupTitle}>{mode === "tcp" ? t("modbus.connectNetworkParams") : t("modbus.connectSerialParams")}</div>
        {mode === "tcp" ? (
          <div className={styles.twoColumns}>
            <Field label={role === "client" ? t("modbus.connectRemoteHost") : t("modbus.connectListenAddress")}>
              <input
                className="liquid-glass-input"
                value={normalized.host}
                onChange={event => patch({ host: event.target.value })}
                placeholder={role === "client" ? "192.168.1.10" : "0.0.0.0"}
                data-testid="tauterm-modbus-host"
              />
            </Field>
            <Field label={role === "client" ? t("modbus.connectTcpPort") : t("modbus.connectListenPort")}>
              <input className="liquid-glass-input" type="number" min={1} max={65535} value={normalized.port} onChange={event => patch({ port: Number(event.target.value) })} />
            </Field>
          </div>
        ) : (
          <>
            <Field label={t("modbus.connectSerialPort")}>
              <select className="liquid-glass-input liquid-glass-select" value={normalized.serial_port} onChange={event => patch({ serial_port: event.target.value })} data-testid="tauterm-modbus-serial-port">
                <option value="">{t("modbus.connectSelectSerial")}</option>
                {ports.map(port => {
                  const description = port.description?.trim();
                  const label = description && description !== port.name ? `${port.name} — ${description}` : port.name;
                  return <option key={port.name} value={port.name}>{label}</option>;
                })}
              </select>
            </Field>
            <div className={styles.serialGrid}>
              <Field label={t("modbus.connectBaudRate")}><input className="liquid-glass-input" type="number" min={1} value={serial.baud_rate} onChange={event => patchSerial({ baud_rate: Number(event.target.value) })} /></Field>
              <Field label={t("modbus.connectDataBits")}><select className="liquid-glass-input liquid-glass-select" value={serial.data_bits} onChange={event => patchSerial({ data_bits: Number(event.target.value) })}>{[5, 6, 7, 8].map(value => <option key={value} value={value}>{value}</option>)}</select></Field>
              <Field label={t("modbus.connectParity")}><select className="liquid-glass-input liquid-glass-select" value={serial.parity} onChange={event => patchSerial({ parity: event.target.value })}><option value="none">None</option><option value="even">Even</option><option value="odd">Odd</option></select></Field>
              <Field label={t("modbus.connectStopBits")}><select className="liquid-glass-input liquid-glass-select" value={serial.stop_bits} onChange={event => patchSerial({ stop_bits: event.target.value })}><option value="1">1</option><option value="2">2</option></select></Field>
            </div>
          </>
        )}
      </div>

      <div className={styles.connectGroup}>
        <div className={styles.connectGroupTitle}>{t("modbus.connectProtocolParams")}</div>
        <div className={styles.twoColumns}>
          <Field label={role === "client" ? t("modbus.connectDefaultUnit") : t("modbus.connectUnit")}>
            <input className="liquid-glass-input" type="number" min={unitIdMin} max={mode === "tcp" ? 255 : 247} value={normalized.unit_id} onChange={event => patch({ unit_id: Number(event.target.value) })} />
            {role === "client" && <span className={styles.hint}>{t("modbus.connectDefaultUnitHint")}</span>}
            {mode !== "tcp" && role === "client" && normalized.unit_id === 0 && <span className={styles.hint}>{t("modbus.connectBroadcastHint")}</span>}
          </Field>
          {role === "client" ? (
            <Field label={t("modbus.connectResponseTimeout")}><input className="liquid-glass-input" type="number" min={1} max={120000} value={normalized.response_timeout_ms} onChange={event => patch({ response_timeout_ms: Number(event.target.value) })} /></Field>
          ) : mode === "tcp" ? (
            <Field label={t("modbus.connectMaxClients")}>
              <input className="liquid-glass-input" type="number" min={0} max={256} value={normalized.server_max_clients} onChange={event => patch({ server_max_clients: Number(event.target.value) })} />
              <span className={styles.hint}>{t("modbus.connectMaxClientsHint")}</span>
            </Field>
          ) : null}
        </div>
      </div>

      {role === "client" && (
        <details className={`${styles.details} liquid-glass-card`}>
          <summary className={styles.detailsSummary}><Icon name="chevron-right" size="xs" className={styles.detailsChevron} />{t("modbus.connectAdvanced")}</summary>
          <div className={styles.detailsBody}>
            <Field label={t("modbus.connectReadRetries")}><input className="liquid-glass-input" type="number" min={0} max={10} value={normalized.read_retries} onChange={event => patch({ read_retries: Number(event.target.value) })} /></Field>
            <label className="liquid-glass-toggle"><input type="checkbox" checked={normalized.retry_writes} onChange={event => patch({ retry_writes: event.target.checked })} /><div /><span>{t("modbus.connectRetryWrites")}</span></label>
            {mode === "tcp" && <Field label={t("modbus.connectTimeout")}><input className="liquid-glass-input" type="number" min={1} max={120000} value={tcp.connect_timeout_ms} onChange={event => patchTcp({ connect_timeout_ms: Number(event.target.value) })} /></Field>}
          </div>
        </details>
      )}

      {role === "server" && <span className={styles.connectFootnote}>{t("modbus.connectServerHint")}</span>}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className={styles.field}><span className={styles.label}>{label}</span>{children}</label>;
}
