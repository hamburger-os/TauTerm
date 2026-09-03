import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useSession } from "../../context/SessionContext";

type Page = "overview" | "publishers" | "subscribers" | "messages" | "traffic";

type TrdpEvent = {
  session_id?: string;
  event?: string;
  kind?: string;
  link?: string;
  com_id?: number;
  src_ip?: string;
  dest_ip?: string;
  msg_type?: string;
  seq_count?: number;
  data_len?: number;
  payload_hex?: string;
  timestamp_us?: number;
  error?: string;
};

type TrdpObject = {
  id: string;
  kind: string;
  name: string;
  comId: number;
  link: "a" | "b" | "both";
  state: "stopped" | "running";
  destination?: string;
  source?: string;
  cycleUs?: number;
  payloadHex?: string;
  transport?: "udp" | "tcp";
};

const nav: Array<[Page, string]> = [
  ["overview", "概览 / Overview · TRDP Application Session"],
  ["publishers", "发布 / Publishers · PD Publisher"],
  ["subscribers", "订阅 / Subscribers · PD Subscriber"],
  ["messages", "消息 / Messages · MD Request/Reply/Notify"],
  ["traffic", "流量 / Traffic · Packet Inspector"],
];

function hexPreview(value?: string) {
  if (!value) return "—";
  return value.length > 72 ? `${value.slice(0, 72)}…` : value;
}

export default function TrdpSessionView({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const mode = (tab?.params?.mode as string | undefined) ?? "node";
  const [page, setPage] = useState<Page>("overview");
  const [events, setEvents] = useState<TrdpEvent[]>([]);
  const [objects, setObjects] = useState<TrdpObject[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlisten = listen<TrdpEvent>("trdp-event", ({ payload }) => {
      if (disposed || payload.session_id !== sessionId) return;
      setEvents(prev => [...prev, payload].slice(-5000));
      if (payload.error) setError(payload.error);
    });
    return () => { disposed = true; void unlisten.then(fn => fn()); };
  }, [sessionId]);

  const flows = useMemo(() => {
    const map = new Map<string, { key: string; msg: string; comId: number; src: string; dst: string; count: number; lastSeq?: number; size?: number }>();
    for (const event of events) {
      if (!event.com_id) continue;
      const key = `${event.msg_type ?? event.kind}:${event.com_id}:${event.src_ip ?? ""}:${event.dest_ip ?? ""}`;
      const row = map.get(key) ?? { key, msg: event.msg_type ?? event.kind ?? "?", comId: event.com_id, src: event.src_ip ?? "—", dst: event.dest_ip ?? "—", count: 0 };
      row.count += 1;
      row.lastSeq = event.seq_count;
      row.size = event.data_len;
      map.set(key, row);
    }
    return [...map.values()];
  }, [events]);

  async function command(command: string, payload: Record<string, unknown> = {}) {
    setError(null);
    try {
      await invoke("trdp_command", { sessionId, command: { command, ...payload } });
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  function addObject(kind: TrdpObject["kind"]) {
    const id = crypto.randomUUID();
    setObjects(prev => [...prev, {
      id,
      kind,
      name: `${kind} ${prev.filter(item => item.kind === kind).length + 1}`,
      comId: 1000,
      link: "a",
      state: "stopped",
      destination: kind.includes("subscriber") ? "239.255.1.1" : "239.255.1.1",
      cycleUs: 100000,
      payloadHex: "00000000",
      transport: "udp",
    }]);
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
      },
    });
    setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, state: "running" } : item));
  }

  async function stopObject(obj: TrdpObject) {
    await command("object_stop", { id: obj.id, kind: obj.kind });
    setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, state: "stopped" } : item));
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
    await invoke("trdp_save_capture", { path, packets: events });
  }

  const objectEditor = (obj: TrdpObject) => (
    <tr key={obj.id}>
      <td><input value={obj.name} onChange={e => setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, name: e.target.value } : item))} /></td>
      <td><input type="number" value={obj.comId} onChange={e => setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, comId: Number(e.target.value) } : item))} /></td>
      <td><select value={obj.link} onChange={e => setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, link: e.target.value as TrdpObject["link"] } : item))}><option value="a">A</option><option value="b">B</option><option value="both">A+B</option></select></td>
      <td><input value={obj.destination ?? ""} onChange={e => setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, destination: e.target.value } : item))} /></td>
      <td>{obj.kind.startsWith("pd_") ? <input type="number" value={obj.cycleUs ?? 100000} onChange={e => setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, cycleUs: Number(e.target.value) } : item))} /> : <select value={obj.transport} onChange={e => setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, transport: e.target.value as "udp" | "tcp" } : item))}><option value="udp">UDP</option><option value="tcp">TCP</option></select>}</td>
      <td><input value={obj.payloadHex ?? ""} onChange={e => setObjects(prev => prev.map(item => item.id === obj.id ? { ...item, payloadHex: e.target.value.replace(/[^0-9a-f]/gi, "").toUpperCase() } : item))} /></td>
      <td><button onClick={() => obj.state === "running" ? stopObject(obj) : startObject(obj)}>{obj.state === "running" ? "Stop" : "Start"}</button></td>
    </tr>
  );

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
            <div>发送策略 / TX policy: Publishers, PD Requests and MD Requests/Notify are always Stopped after creation/import.</div>
            {mode === "monitor" && <div style={{ display: "flex", gap: 8 }}><button onClick={openCapture}>打开 .pcap/.pcapng</button><button onClick={saveCapture} disabled={events.length === 0}>保存 .pcapng</button><button onClick={() => command("capture_start", { interface: tab?.params?.capture_interface, filter: tab?.params?.capture_filter })}>Start Live Capture</button><button onClick={() => command("capture_stop")}>Stop Capture</button></div>}
          </div>
        )}

        {page === "publishers" && <section><div style={{ display: "flex", justifyContent: "space-between" }}><h2>发布 / Publishers · PD Publisher</h2><button onClick={() => addObject("pd_publisher")}>+ Publisher</button></div><table style={{ width: "100%" }}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination</th><th>Cycle µs</th><th>Payload HEX</th><th>State</th></tr></thead><tbody>{objects.filter(o => o.kind === "pd_publisher").map(objectEditor)}</tbody></table></section>}
        {page === "subscribers" && <section><div style={{ display: "flex", justifyContent: "space-between" }}><h2>订阅 / Subscribers · PD Subscriber</h2><button onClick={() => addObject("pd_subscriber")}>+ Subscriber</button></div><table style={{ width: "100%" }}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Multicast/Destination</th><th>Timeout µs</th><th>Initial HEX</th><th>State</th></tr></thead><tbody>{objects.filter(o => o.kind === "pd_subscriber").map(objectEditor)}</tbody></table></section>}
        {page === "messages" && <section><div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}><h2 style={{ marginRight: "auto" }}>消息 / Messages · MD</h2><button onClick={() => addObject("md_request")}>+ Request</button><button onClick={() => addObject("md_listener")}>+ Listener/Replier</button><button onClick={() => addObject("md_notify")}>+ Notify</button></div><table style={{ width: "100%" }}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination/Filter</th><th>UDP/TCP</th><th>Payload HEX</th><th>State</th></tr></thead><tbody>{objects.filter(o => o.kind.startsWith("md_")).map(objectEditor)}</tbody></table></section>}

        {page === "traffic" && (
          <section>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}><h2>流量 / Traffic · TRDP Packet Inspector</h2><div style={{ display: "flex", gap: 8 }}><button onClick={openCapture}>Open capture</button><button onClick={saveCapture} disabled={events.length === 0}>Save pcapng</button><button onClick={() => setEvents([])}>Clear</button></div></div>
            <h3>Flows</h3>
            <table style={{ width: "100%" }}><thead><tr><th>Type</th><th>ComID</th><th>Source</th><th>Destination</th><th>Packets</th><th>Seq</th><th>Size</th></tr></thead><tbody>{flows.map(flow => <tr key={flow.key}><td>{flow.msg}</td><td>{flow.comId}</td><td>{flow.src}</td><td>{flow.dst}</td><td>{flow.count}</td><td>{flow.lastSeq ?? "—"}</td><td>{flow.size ?? "—"}</td></tr>)}</tbody></table>
            <h3>Packets</h3>
            <table style={{ width: "100%", fontFamily: "var(--font-mono)" }}><thead><tr><th>#</th><th>Link</th><th>Type</th><th>ComID</th><th>Source → Destination</th><th>Seq</th><th>Len</th><th>Payload</th></tr></thead><tbody>{events.slice().reverse().slice(0, 1000).map((event, index) => <tr key={`${event.timestamp_us ?? 0}-${index}`}><td>{events.length - index}</td><td>{event.link ?? "—"}</td><td>{event.msg_type ?? event.kind ?? "—"}</td><td>{event.com_id ?? "—"}</td><td>{event.src_ip ?? "—"} → {event.dest_ip ?? "—"}</td><td>{event.seq_count ?? "—"}</td><td>{event.data_len ?? "—"}</td><td title={event.payload_hex}>{hexPreview(event.payload_hex)}</td></tr>)}</tbody></table>
          </section>
        )}
      </div>
    </div>
  );
}
