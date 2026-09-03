import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useSession } from "../../context/SessionContext";

type Page = "overview" | "publishers" | "subscribers" | "messages" | "traffic";
type LinkChoice = "a" | "b" | "both";

type TrdpEvent = {
  session_id?: string;
  event?: string;
  kind?: string;
  link?: string;
  com_id?: number;
  src_ip?: string;
  dest_ip?: string;
  src_port?: number;
  dest_port?: number;
  transport?: string;
  msg_type?: string;
  seq_count?: number;
  protocol_version?: number;
  etb_topo_count?: number;
  op_trn_topo_count?: number;
  data_len?: number;
  payload_hex?: string;
  raw_frame_hex?: string;
  timestamp_us?: number;
  result_code?: number;
  reply_status?: number;
  user_status?: number;
  num_replies?: number;
  error?: string;
};

type TrdpObject = {
  id: string;
  kind: "pd_publisher" | "pd_subscriber" | "pd_request" | "md_request" | "md_listener" | "md_notify";
  name: string;
  comId: number;
  link: LinkChoice;
  state: "stopped" | "running";
  destination: string;
  source: string;
  cycleUs: number;
  payloadHex: string;
  transport: "udp" | "tcp";
  etbTopoCount: number;
  opTrnTopoCount: number;
  redId: number;
};

type XmlElement = { name: string; data_type: string; array_size: number; unit?: string };
type XmlDataset = { id: number; name: string; elements: XmlElement[] };
type XmlTelegram = {
  name: string;
  com_id: number;
  dataset_id: number;
  cycle_us?: number;
  timeout_us?: number;
  sources: string[];
  destinations: string[];
  sdt_detected: boolean;
};
type XmlImport = {
  path: string;
  datasets: XmlDataset[];
  telegrams: XmlTelegram[];
  sdt_detected: boolean;
  warnings: string[];
};
type DecodedDataset = {
  dataset_id: number;
  dataset_name: string;
  consumed_bytes: number;
  payload_bytes: number;
  fields: Array<{ name: string; type: string; unit?: string; raw?: unknown; value?: unknown; error?: string }>;
};

const nav: Array<[Page, string]> = [
  ["overview", "概览 / Overview · TRDP Application Session"],
  ["publishers", "发布 / Publishers · PD Publisher"],
  ["subscribers", "订阅 / Subscribers · PD Subscriber / Request"],
  ["messages", "消息 / Messages · MD Request/Reply/Notify"],
  ["traffic", "流量 / Traffic · Packet Inspector"],
];

const tableStyle = { width: "100%", borderCollapse: "collapse" } as const;
const cellInputStyle = { width: "100%", minWidth: 80 } as const;

function hexPreview(value?: string) {
  if (!value) return "—";
  return value.length > 72 ? `${value.slice(0, 72)}…` : value;
}

function createObject(kind: TrdpObject["kind"], index: number): TrdpObject {
  return {
    id: crypto.randomUUID(),
    kind,
    name: `${kind} ${index}`,
    comId: 1000,
    link: "a",
    state: "stopped",
    destination: kind.startsWith("pd_") ? "239.255.1.1" : "10.0.0.2",
    source: "0.0.0.0",
    cycleUs: kind === "pd_subscriber" || kind === "pd_request" ? 300000 : 100000,
    payloadHex: kind === "pd_subscriber" ? "" : "00000000",
    transport: "udp",
    etbTopoCount: 0,
    opTrnTopoCount: 0,
    redId: 0,
  };
}

function displayValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (value === null || value === undefined) return "—";
  try { return JSON.stringify(value); } catch { return String(value); }
}

export default function TrdpSessionView({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const mode = (tab?.params?.mode as string | undefined) ?? "node";
  const storageKey = `tauterm:trdp:${sessionId}:objects`;
  const [page, setPage] = useState<Page>("overview");
  const [events, setEvents] = useState<TrdpEvent[]>([]);
  const [objects, setObjects] = useState<TrdpObject[]>(() => {
    try {
      const saved = localStorage.getItem(storageKey);
      return saved ? JSON.parse(saved) as TrdpObject[] : [];
    } catch { return []; }
  });
  const [xmlImport, setXmlImport] = useState<XmlImport | null>(null);
  const [decoded, setDecoded] = useState<DecodedDataset | null>(null);
  const [selectedPacket, setSelectedPacket] = useState<TrdpEvent | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    try { localStorage.setItem(storageKey, JSON.stringify(objects)); } catch { /* best effort */ }
  }, [objects, storageKey]);

  useEffect(() => {
    let disposed = false;
    const unlisten = listen<TrdpEvent>("trdp-event", ({ payload }) => {
      if (disposed || payload.session_id !== sessionId) return;
      if (payload.event === "packet") setEvents(prev => [...prev, payload].slice(-5000));
      if (payload.error) setError(payload.error);
    });
    return () => { disposed = true; void unlisten.then(fn => fn()); };
  }, [sessionId]);

  const datasetByComId = useMemo(() => {
    const result = new Map<number, number>();
    for (const telegram of xmlImport?.telegrams ?? []) result.set(telegram.com_id, telegram.dataset_id);
    return result;
  }, [xmlImport]);

  const flows = useMemo(() => {
    const map = new Map<string, { key: string; msg: string; comId: number; src: string; dst: string; count: number; lastSeq?: number; size?: number; link: string }>();
    for (const event of events) {
      if (event.com_id === undefined) continue;
      const key = `${event.link ?? ""}:${event.msg_type ?? event.kind}:${event.com_id}:${event.src_ip ?? ""}:${event.dest_ip ?? ""}`;
      const row = map.get(key) ?? { key, msg: event.msg_type ?? event.kind ?? "?", comId: event.com_id, src: event.src_ip ?? "—", dst: event.dest_ip ?? "—", count: 0, link: event.link ?? "—" };
      row.count += 1;
      row.lastSeq = event.seq_count;
      row.size = event.data_len;
      map.set(key, row);
    }
    return [...map.values()];
  }, [events]);

  async function command<T = unknown>(name: string, payload: Record<string, unknown> = {}): Promise<T> {
    setError(null);
    try {
      return await invoke<T>("trdp_command", { sessionId, command: { command: name, ...payload } });
    } catch (cause) {
      setError(String(cause));
      throw cause;
    }
  }

  function addObject(kind: TrdpObject["kind"]) {
    setObjects(prev => [...prev, createObject(kind, prev.filter(item => item.kind === kind).length + 1)]);
  }

  function patchObject(id: string, patch: Partial<TrdpObject>) {
    setObjects(prev => prev.map(item => item.id === id ? { ...item, ...patch } : item));
  }

  async function startObject(obj: TrdpObject) {
    await command("object_start", {
      object: {
        id: obj.id,
        kind: obj.kind,
        name: obj.name,
        com_id: obj.comId,
        link: obj.link,
        destination: obj.destination,
        source: obj.source,
        cycle_us: obj.cycleUs,
        payload_hex: obj.payloadHex,
        transport: obj.transport,
        etb_topo_count: obj.etbTopoCount,
        op_trn_topo_count: obj.opTrnTopoCount,
        red_id: obj.redId,
      },
    });
    // MD Request/Notify are one-shot operations and deliberately return to Stopped.
    if (obj.kind !== "md_request" && obj.kind !== "md_notify") patchObject(obj.id, { state: "running" });
  }

  async function stopObject(obj: TrdpObject) {
    await command("object_stop", { id: obj.id, kind: obj.kind });
    patchObject(obj.id, { state: "stopped" });
  }

  async function updatePayload(obj: TrdpObject) {
    await command("object_update", { id: obj.id, payload_hex: obj.payloadHex });
  }

  async function openCapture() {
    const path = await open({ multiple: false, filters: [{ name: "Packet Capture", extensions: ["pcap", "pcapng"] }] });
    if (typeof path !== "string") return;
    const packets = await invoke<TrdpEvent[]>("trdp_open_capture", { path });
    setEvents(prev => [...prev, ...packets].slice(-5000));
    setPage("traffic");
  }

  async function saveCapture() {
    const path = await save({ filters: [{ name: "PCAPNG", extensions: ["pcapng"] }] });
    if (!path) return;
    await invoke("trdp_save_capture", { path, packets: events.filter(event => event.raw_frame_hex) });
  }

  async function importXml() {
    const configured = typeof tab?.params?.xml_path === "string" ? tab.params.xml_path : "";
    let path = configured;
    if (!path) {
      const selected = await open({ multiple: false, filters: [{ name: "TRDP XML", extensions: ["xml"] }] });
      if (typeof selected !== "string") return;
      path = selected;
    }
    const imported = await command<XmlImport>("xml_import", { path });
    setXmlImport(imported);
    setDecoded(null);
  }

  async function inspectPacket(event: TrdpEvent) {
    setSelectedPacket(event);
    setDecoded(null);
    const datasetId = event.com_id === undefined ? undefined : datasetByComId.get(event.com_id);
    if (!xmlImport || !datasetId || !event.payload_hex) return;
    try {
      const result = await command<DecodedDataset>("dataset_decode", {
        path: xmlImport.path,
        dataset_id: datasetId,
        payload_hex: event.payload_hex,
      });
      setDecoded(result);
    } catch { /* error banner already populated */ }
  }

  function importTemplates() {
    if (!xmlImport) return;
    setObjects(prev => {
      const known = new Set(prev.map(item => `${item.comId}:${item.destination}`));
      const additions: TrdpObject[] = [];
      for (const telegram of xmlImport.telegrams) {
        for (const destination of telegram.destinations.length ? telegram.destinations : ["0.0.0.0"]) {
          if (known.has(`${telegram.com_id}:${destination}`)) continue;
          const item = createObject("pd_subscriber", additions.length + 1);
          item.name = `${telegram.name} (imported template)`;
          item.comId = telegram.com_id;
          item.destination = destination;
          item.source = telegram.sources[0] ?? "0.0.0.0";
          item.cycleUs = telegram.timeout_us ?? 300000;
          additions.push(item);
          known.add(`${telegram.com_id}:${destination}`);
        }
      }
      return [...prev, ...additions];
    });
  }

  const objectEditor = (obj: TrdpObject) => {
    const oneShot = obj.kind === "md_request" || obj.kind === "md_notify";
    return (
      <tr key={obj.id}>
        <td><input style={cellInputStyle} value={obj.name} onChange={event => patchObject(obj.id, { name: event.target.value })} /></td>
        <td><input style={cellInputStyle} type="number" min={1} value={obj.comId} onChange={event => patchObject(obj.id, { comId: Number(event.target.value) })} /></td>
        <td><select value={obj.link} onChange={event => patchObject(obj.id, { link: event.target.value as LinkChoice })}><option value="a">A</option><option value="b">B</option><option value="both">A+B</option></select></td>
        <td><input style={cellInputStyle} value={obj.destination} onChange={event => patchObject(obj.id, { destination: event.target.value })} /></td>
        <td>{obj.kind.startsWith("md_") ? <select value={obj.transport} onChange={event => patchObject(obj.id, { transport: event.target.value as "udp" | "tcp" })}><option value="udp">UDP</option><option value="tcp">TCP</option></select> : <input style={cellInputStyle} type="number" min={0} value={obj.cycleUs} onChange={event => patchObject(obj.id, { cycleUs: Number(event.target.value) })} />}</td>
        <td><input style={cellInputStyle} value={obj.payloadHex} onChange={event => patchObject(obj.id, { payloadHex: event.target.value.replace(/[^0-9a-f]/gi, "").toUpperCase() })} /></td>
        <td>
          <details>
            <summary>Advanced</summary>
            <label>ETB <input type="number" min={0} value={obj.etbTopoCount} onChange={event => patchObject(obj.id, { etbTopoCount: Number(event.target.value) })} /></label><br />
            <label>OpTrn <input type="number" min={0} value={obj.opTrnTopoCount} onChange={event => patchObject(obj.id, { opTrnTopoCount: Number(event.target.value) })} /></label><br />
            {obj.kind === "pd_publisher" && <label>Red ID <input type="number" min={0} value={obj.redId} onChange={event => patchObject(obj.id, { redId: Number(event.target.value) })} /></label>}
          </details>
        </td>
        <td style={{ whiteSpace: "nowrap" }}>
          {oneShot ? <button onClick={() => void startObject(obj)}>Send</button> : <button onClick={() => void (obj.state === "running" ? stopObject(obj) : startObject(obj))}>{obj.state === "running" ? "Stop" : "Start"}</button>}
          {obj.state === "running" && (obj.kind === "pd_publisher" || obj.kind === "md_listener") && <button onClick={() => void updatePayload(obj)}>Put</button>}
          <button onClick={() => setObjects(prev => prev.filter(item => item.id !== obj.id))} disabled={obj.state === "running"}>×</button>
        </td>
      </tr>
    );
  };

  return (
    <div style={{ height: "100%", display: "grid", gridTemplateRows: "auto 1fr", overflow: "hidden" }}>
      <div style={{ display: "flex", gap: 6, padding: 8, borderBottom: "1px solid var(--border-color)", overflowX: "auto" }}>
        {nav.filter(([key]) => mode === "monitor" ? ["overview", "traffic"].includes(key) : true).map(([key, label]) => (
          <button key={key} className={page === key ? "liquid-primary-button" : "liquid-glass-button"} onClick={() => setPage(key)}>{label}</button>
        ))}
      </div>
      <div style={{ padding: 12, overflow: "auto" }}>
        {error && <div style={{ padding: 10, marginBottom: 12, border: "1px solid var(--danger, #d44)", borderRadius: 8 }}>{error}</div>}

        {page === "overview" && (
          <div style={{ display: "grid", gap: 12 }}>
            <h2>TRDP {mode === "monitor" ? "Monitor" : "Node"}</h2>
            <div>PD: UDP/17224 · MD: UDP/TCP/17225 · SDTv2/SDTv4: detected only, validation out of scope</div>
            <div>Link A: {String(tab?.params?.link_a_ip ?? "—")} · Link B: {tab?.params?.link_b_enabled ? String(tab?.params?.link_b_ip ?? "—") : "Disabled"}</div>
            <div>发送策略 / TX policy: Publishers, PD Requests and MD Requests/Notify always require an explicit Start/Send action.</div>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button onClick={() => void importXml()}>导入 TRDP XML / Import XML</button>
              {mode === "monitor" && <><button onClick={() => void openCapture()}>打开 .pcap/.pcapng</button><button onClick={() => void saveCapture()} disabled={!events.some(event => event.raw_frame_hex)}>保存 .pcapng</button><button onClick={() => void command("capture_start", { interface: tab?.params?.capture_interface, filter: tab?.params?.capture_filter })}>Start Live Capture</button><button onClick={() => void command("capture_stop")}>Stop Capture</button></>}
            </div>
            {xmlImport && (
              <div className="liquid-glass-card" style={{ padding: 12 }}>
                <strong>Import Preview</strong>
                <div>{xmlImport.datasets.length} Datasets · {xmlImport.telegrams.length} Telegrams · SDT: {xmlImport.sdt_detected ? "Detected (not validated)" : "No configuration detected"}</div>
                {xmlImport.warnings.map(warning => <div key={warning} style={{ marginTop: 4 }}>⚠ {warning}</div>)}
                {mode === "node" && <button style={{ marginTop: 8 }} onClick={importTemplates}>将 Telegram 作为停止状态的订阅模板加入 Workspace</button>}
                <table style={{ ...tableStyle, marginTop: 8 }}><thead><tr><th>Telegram</th><th>ComID</th><th>Dataset</th><th>Cycle</th><th>Sources</th><th>Destinations</th></tr></thead><tbody>{xmlImport.telegrams.map(telegram => <tr key={`${telegram.com_id}-${telegram.name}`}><td>{telegram.name}</td><td>{telegram.com_id}</td><td>{telegram.dataset_id}</td><td>{telegram.cycle_us ?? "—"}</td><td>{telegram.sources.join(", ") || "—"}</td><td>{telegram.destinations.join(", ") || "—"}</td></tr>)}</tbody></table>
              </div>
            )}
          </div>
        )}

        {page === "publishers" && <section><div style={{ display: "flex", justifyContent: "space-between" }}><h2>发布 / Publishers · PD Publisher</h2><button onClick={() => addObject("pd_publisher")}>+ Publisher</button></div><table style={tableStyle}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination</th><th>Cycle µs</th><th>Payload HEX</th><th>Protocol</th><th>State</th></tr></thead><tbody>{objects.filter(object => object.kind === "pd_publisher").map(objectEditor)}</tbody></table></section>}
        {page === "subscribers" && <section><div style={{ display: "flex", gap: 8, alignItems: "center" }}><h2 style={{ marginRight: "auto" }}>订阅 / Subscribers · PD Subscriber / Request</h2><button onClick={() => addObject("pd_subscriber")}>+ Subscriber</button><button onClick={() => addObject("pd_request")}>+ PD Request</button></div><table style={tableStyle}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Multicast/Destination</th><th>Timeout µs</th><th>Payload HEX</th><th>Protocol</th><th>State</th></tr></thead><tbody>{objects.filter(object => object.kind === "pd_subscriber" || object.kind === "pd_request").map(objectEditor)}</tbody></table></section>}
        {page === "messages" && <section><div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}><h2 style={{ marginRight: "auto" }}>消息 / Messages · MD</h2><button onClick={() => addObject("md_request")}>+ Request</button><button onClick={() => addObject("md_listener")}>+ Listener/Replier</button><button onClick={() => addObject("md_notify")}>+ Notify</button></div><table style={tableStyle}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination/Filter</th><th>UDP/TCP</th><th>Payload HEX</th><th>Protocol</th><th>Action</th></tr></thead><tbody>{objects.filter(object => object.kind.startsWith("md_")).map(objectEditor)}</tbody></table></section>}

        {page === "traffic" && (
          <section>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}><h2>流量 / Traffic · TRDP Packet Inspector</h2><div style={{ display: "flex", gap: 8 }}><button onClick={() => void openCapture()}>Open capture</button><button onClick={() => void saveCapture()} disabled={!events.some(event => event.raw_frame_hex)}>Save pcapng</button><button onClick={() => { setEvents([]); setSelectedPacket(null); setDecoded(null); }}>Clear</button></div></div>
            <h3>Flows</h3>
            <table style={tableStyle}><thead><tr><th>Link</th><th>Type</th><th>ComID</th><th>Source</th><th>Destination</th><th>Packets</th><th>Seq</th><th>Size</th></tr></thead><tbody>{flows.map(flow => <tr key={flow.key}><td>{flow.link}</td><td>{flow.msg}</td><td>{flow.comId}</td><td>{flow.src}</td><td>{flow.dst}</td><td>{flow.count}</td><td>{flow.lastSeq ?? "—"}</td><td>{flow.size ?? "—"}</td></tr>)}</tbody></table>
            <h3>Packets</h3>
            <table style={{ ...tableStyle, fontFamily: "var(--font-mono)" }}><thead><tr><th>#</th><th>Link</th><th>Type</th><th>ComID</th><th>Source → Destination</th><th>Seq</th><th>Topo ETB/Op</th><th>Len</th><th>Payload</th></tr></thead><tbody>{events.slice().reverse().slice(0, 1000).map((event, index) => <tr key={`${event.timestamp_us ?? 0}-${index}`} onClick={() => void inspectPacket(event)} style={{ cursor: "pointer" }}><td>{events.length - index}</td><td>{event.link ?? "—"}</td><td>{event.msg_type ?? event.kind ?? "—"}</td><td>{event.com_id ?? "—"}</td><td>{event.src_ip ?? "—"} → {event.dest_ip ?? "—"}</td><td>{event.seq_count ?? "—"}</td><td>{event.etb_topo_count ?? "—"}/{event.op_trn_topo_count ?? "—"}</td><td>{event.data_len ?? "—"}</td><td title={event.payload_hex}>{hexPreview(event.payload_hex)}</td></tr>)}</tbody></table>
            {selectedPacket && <div className="liquid-glass-card" style={{ marginTop: 12, padding: 12 }}><h3>Packet Inspector</h3><div>Protocol {selectedPacket.protocol_version ?? "—"} · Result {selectedPacket.result_code ?? "—"} · Reply status {selectedPacket.reply_status ?? "—"} · User status {selectedPacket.user_status ?? "—"}</div><div>Raw payload: <code>{selectedPacket.payload_hex || "—"}</code></div>{decoded ? <><h4>{decoded.dataset_name} · Dataset {decoded.dataset_id}</h4><div>{decoded.consumed_bytes}/{decoded.payload_bytes} bytes decoded</div><table style={tableStyle}><thead><tr><th>Field</th><th>Type</th><th>Value</th><th>Unit</th></tr></thead><tbody>{decoded.fields.map((field, index) => <tr key={`${field.name}-${index}`}><td>{field.name}</td><td>{field.type}</td><td>{field.error ?? displayValue(field.value)}</td><td>{field.unit ?? "—"}</td></tr>)}</tbody></table></> : xmlImport && selectedPacket.com_id !== undefined ? <div>No dataset mapping/decodable payload for ComID {selectedPacket.com_id}.</div> : <div>Import a TRDP XML file to enable Dataset decoding.</div>}</div>}
          </section>
        )}
      </div>
    </div>
  );
}
