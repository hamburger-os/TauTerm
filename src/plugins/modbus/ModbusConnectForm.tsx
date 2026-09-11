import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ConnectFormProps } from "../../core/plugin-registry";
import styles from "./Modbus.module.css";

type Endpoint = { name: string; description: string; connection_type: string };
const num = (value: unknown, fallback: number) => typeof value === "number" && Number.isFinite(value) ? value : fallback;
const str = (value: unknown, fallback = "") => typeof value === "string" ? value : fallback;

export default function ModbusConnectForm({ params, onChange }: ConnectFormProps) {
  const [ports, setPorts] = useState<Endpoint[]>([]);
  const mode = str(params.mode, "tcp") as "rtu" | "ascii" | "tcp";
  const role = str(params.role, "client") as "client" | "server";
  const serial = useMemo(() => ({
    baud_rate: 115200,
    data_bits: 8,
    parity: "none",
    stop_bits: "1",
    flow_control: "none",
    read_timeout_ms: 20,
    ...(typeof params.serial === "object" && params.serial ? params.serial as Record<string, unknown> : {}),
  }), [params.serial]);
  const tcp = useMemo(() => ({
    connect_timeout_ms: 5000,
    read_timeout_ms: 20,
    nodelay: true,
    ...(typeof params.tcp === "object" && params.tcp ? params.tcp as Record<string, unknown> : {}),
  }), [params.tcp]);

  useEffect(() => {
    void invoke<Endpoint[]>("enumerate_endpoints", { connectionType: "serial" })
      .then(setPorts)
      .catch(() => setPorts([]));
  }, []);

  const patch = (next: Record<string, unknown>) => onChange({
    mode: "tcp",
    role: "client",
    serial_port: "",
    serial,
    host: "127.0.0.1",
    port: 502,
    tcp,
    unit_id: 1,
    response_timeout_ms: 1000,
    read_retries: 1,
    retry_writes: false,
    server_max_clients: 16,
    server_fault: { no_response: false, delay_ms: 0, exception_code: null },
    ...params,
    ...next,
  });
  const patchSerial = (next: Record<string, unknown>) => patch({ serial: { ...serial, ...next } });
  const patchTcp = (next: Record<string, unknown>) => patch({ tcp: { ...tcp, ...next } });

  return (
    <div className={styles.connectRoot}>
      <div className={styles.twoColumns}>
        <Field label="传输模式">
          <select className="liquid-glass-input liquid-glass-select" value={mode} onChange={e => patch({ mode: e.target.value })}>
            <option value="rtu">Modbus RTU</option><option value="ascii">Modbus ASCII</option><option value="tcp">Modbus TCP</option>
          </select>
        </Field>
        <Field label="角色">
          <select className="liquid-glass-input liquid-glass-select" value={role} onChange={e => patch({ role: e.target.value })}>
            <option value="client">Client / Master</option><option value="server">Server / Slave Simulator</option>
          </select>
        </Field>
      </div>

      {mode === "tcp" ? (
        <div className={styles.twoColumns}>
          <Field label={role === "client" ? "远端主机" : "监听地址"}>
            <input className="liquid-glass-input" value={str(params.host, "127.0.0.1")} onChange={e => patch({ host: e.target.value })} placeholder={role === "client" ? "192.168.1.10" : "0.0.0.0"} />
          </Field>
          <Field label="TCP 端口">
            <input className="liquid-glass-input" type="number" min={1} max={65535} value={num(params.port, 502)} onChange={e => patch({ port: Number(e.target.value) })} />
          </Field>
        </div>
      ) : (
        <>
          <Field label="串口">
            <select className="liquid-glass-input liquid-glass-select" value={str(params.serial_port)} onChange={e => patch({ serial_port: e.target.value })}>
              <option value="">选择串口…</option>
              {ports.map(port => <option key={port.name} value={port.name}>{port.description || port.name}</option>)}
            </select>
          </Field>
          <div className={styles.serialGrid}>
            <Field label="波特率"><input className="liquid-glass-input" type="number" min={1} value={num(serial.baud_rate, 115200)} onChange={e => patchSerial({ baud_rate: Number(e.target.value) })} /></Field>
            <Field label="数据位"><select className="liquid-glass-input liquid-glass-select" value={num(serial.data_bits, 8)} onChange={e => patchSerial({ data_bits: Number(e.target.value) })}>{[5,6,7,8].map(v => <option key={v}>{v}</option>)}</select></Field>
            <Field label="校验"><select className="liquid-glass-input liquid-glass-select" value={str(serial.parity, "none")} onChange={e => patchSerial({ parity: e.target.value })}><option value="none">None</option><option value="even">Even</option><option value="odd">Odd</option></select></Field>
            <Field label="停止位"><select className="liquid-glass-input liquid-glass-select" value={str(serial.stop_bits, "1")} onChange={e => patchSerial({ stop_bits: e.target.value })}><option value="1">1</option><option value="2">2</option></select></Field>
          </div>
        </>
      )}

      <div className={styles.twoColumns}>
        <Field label="Unit ID">
          <input className="liquid-glass-input" type="number" min={0} max={mode === "tcp" ? 255 : 247} value={num(params.unit_id, 1)} onChange={e => patch({ unit_id: Number(e.target.value) })} />
          {mode !== "tcp" && num(params.unit_id, 1) === 0 && <span className={styles.hint}>地址 0 为广播：仅允许写入且不会等待响应。</span>}
        </Field>
        {role === "client" ? <Field label="响应超时 (ms)"><input className="liquid-glass-input" type="number" min={1} max={120000} value={num(params.response_timeout_ms, 1000)} onChange={e => patch({ response_timeout_ms: Number(e.target.value) })} /></Field>
          : mode === "tcp" && <Field label="最大客户端"><input className="liquid-glass-input" type="number" min={0} max={256} value={num(params.server_max_clients, 16)} onChange={e => patch({ server_max_clients: Number(e.target.value) })} /></Field>}
      </div>

      <details className={`${styles.details} liquid-glass-card`}>
        <summary>高级</summary>
        <div className={styles.detailsBody}>
          {role === "client" ? <>
            <Field label="读取重试次数"><input className="liquid-glass-input" type="number" min={0} max={10} value={num(params.read_retries, 1)} onChange={e => patch({ read_retries: Number(e.target.value) })} /></Field>
            <label className="liquid-glass-toggle"><input type="checkbox" checked={params.retry_writes === true} onChange={e => patch({ retry_writes: e.target.checked })}/><div/><span>允许写请求超时后重试（可能重复写入）</span></label>
          </> : <p className={styles.hint}>Server 故障注入在会话的“高级”页动态配置。</p>}
          {mode === "tcp" && role === "client" && <Field label="连接超时 (ms)"><input className="liquid-glass-input" type="number" min={1} max={120000} value={num(tcp.connect_timeout_ms, 5000)} onChange={e => patchTcp({ connect_timeout_ms: Number(e.target.value) })}/></Field>}
        </div>
      </details>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className={styles.field}><span className={styles.label}>{label}</span>{children}</label>;
}
