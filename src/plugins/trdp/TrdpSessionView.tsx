import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useSession } from "../../context/SessionContext";

type Page = "overview" | "publishers" | "subscribers" | "messages" | "traffic";
type LinkChoice = "a" | "b" | "both";
type ObjectKind = "pd_publisher" | "pd_subscriber" | "pd_request" | "md_request" | "md_listener" | "md_notify";

type TrdpEvent = {
  session_id?: string;
  event?: string;
  command?: string;
  kind?: string;
  id?: string;
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
  link_type?: number;
  crc_valid?: boolean;
  protocol_valid?: boolean;
  timestamp_us?: number;
  latency_us?: number;
  result_code?: number;
  reply_status?: number;
  user_status?: number;
  num_replies?: number;
  num_expected_replies?: number;
  num_reply_queries?: number;
  num_confirm_sent?: number;
  num_confirm_timeout?: number;
  reply_timeout_us?: number;
  about_to_die?: boolean;
  src_uri?: string;
  dest_uri?: string;
  md_session_id?: string;
  error?: string;
};

type TrdpObject = {
  id: string;
  kind: ObjectKind;
  name: string;
  comId: number;
  link: LinkChoice;
  state: "stopped" | "running";
  destination: string;
  source: string;
  cycleUs: number;
  timeoutMode: "auto" | "custom";
  timeoutUs: number;
  timeoutBehavior: "keep" | "zero";
  payloadHex: string;
  transport: "udp" | "tcp";
  etbTopoCount: number;
  opTrnTopoCount: number;
  redId: number;
  redState: "leader" | "follower";
  numReplies: number;
  replyTimeoutUs: number;
  responseMode: "reply" | "query";
  confirmTimeoutUs: number;
  replyComId: number;
  replyIp: string;
  sourceUri: string;
  destUri: string;
};

type XmlElement = {
  name: string;
  data_type: string;
  type_id: number;
  array_size: number;
  dynamic: boolean;
  unit?: string;
  scale?: number;
  offset?: number;
};
type XmlDataset = { id: number; name: string; elements: XmlElement[] };
type XmlTelegram = {
  name: string;
  traffic_kind: "pd" | "md";
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
  pd_port: number;
  md_udp_port: number;
  md_tcp_port: number;
  sdt_detected: boolean;
  warnings: string[];
};
type DecodedField = { type: string; type_id?: number; unit?: string; raw?: unknown; value?: unknown; error?: string };
type DecodedDataset = {
  dataset_id: number;
  dataset_name: string;
  consumed_bytes: number;
  payload_bytes: number;
  fields: Record<string, DecodedField>;
};
type Workspace = {
  format: string;
  name?: string;
  xml?: string;
  objects?: Array<Record<string, unknown>>;
};
type EncodedDataset = {
  dataset_id: number;
  payload_bytes: number;
  payload_hex: string;
};
type StructuredEditor = {
  objectId: string;
  datasetId: number;
  drafts: Record<string, string>;
};

type FlowRow = {
  key: string;
  msg: string;
  comId: number;
  src: string;
  dst: string;
  count: number;
  lastSeq?: number;
  size?: number;
  link: string;
  missed: number;
  errors: number;
  minIntervalUs?: number;
  avgIntervalUs?: number;
  maxIntervalUs?: number;
  jitterUs?: number;
};

const nav: Array<[Page, string]> = [
  ["overview", "概览 / Overview · TRDP Application Session"],
  ["publishers", "发布 / Publishers · PD Publisher"],
  ["subscribers", "订阅 / Subscribers · PD Subscriber / Request"],
  ["messages", "消息 / Messages · MD Request / Reply / Notify"],
  ["traffic", "流量 / Traffic · Packet Inspector"],
];

const tableStyle = { width: "100%", borderCollapse: "collapse" } as const;
const cellInputStyle = { width: "100%", minWidth: 80 } as const;
const U32 = 0x1_0000_0000;
const STANDARD_CAPTURE_FILTER = "udp port 17224 or udp port 17225 or tcp port 17225";

function captureFilterForPorts(pdPort: number, mdUdpPort: number, mdTcpPort: number) {
  return `udp port ${pdPort} or udp port ${mdUdpPort} or tcp port ${mdTcpPort}`;
}

function paramNumber(params: Record<string, unknown> | undefined, key: string, fallback: number) {
  const value = params?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function hexPreview(value?: string) {
  if (!value) return "—";
  return value.length > 72 ? `${value.slice(0, 72)}…` : value;
}

function isKind(value: unknown): value is ObjectKind {
  return typeof value === "string" && ["pd_publisher", "pd_subscriber", "pd_request", "md_request", "md_listener", "md_notify"].includes(value);
}

function isOneShotKind(kind: ObjectKind) {
  return kind === "pd_request" || kind === "md_request" || kind === "md_notify";
}

function createObject(kind: ObjectKind, index: number): TrdpObject {
  const subscriber = kind === "pd_subscriber" || kind === "pd_request";
  return {
    id: crypto.randomUUID(),
    kind,
    name: `${kind} ${index}`,
    comId: 1000,
    link: "a",
    state: "stopped",
    destination: kind.startsWith("pd_") ? "239.255.1.1" : "10.0.0.2",
    source: "0.0.0.0",
    cycleUs: 100000,
    timeoutMode: "auto",
    timeoutUs: subscriber ? 300000 : 100000,
    timeoutBehavior: "keep",
    payloadHex: kind === "pd_subscriber" ? "" : "00000000",
    transport: "udp",
    etbTopoCount: 0,
    opTrnTopoCount: 0,
    redId: 0,
    redState: "leader",
    numReplies: 1,
    replyTimeoutUs: 5000000,
    responseMode: "reply",
    confirmTimeoutUs: 2000000,
    replyComId: 0,
    replyIp: "0.0.0.0",
    sourceUri: "",
    destUri: "",
  };
}

function workspaceObject(raw: Record<string, unknown>, index: number): TrdpObject | null {
  if (!isKind(raw.kind)) return null;
  const base = createObject(raw.kind, index + 1);
  const link = raw.link === "b" || raw.link === "both" ? raw.link : "a";
  const parsedTimeoutUs = Number(raw.timeout_us);
  const hasCustomTimeout = raw.timeout_us !== undefined
    && Number.isFinite(parsedTimeoutUs)
    && parsedTimeoutUs > 0;
  const timeoutMode = raw.timeout_mode === "custom"
    ? "custom"
    : raw.timeout_mode === "auto"
      ? "auto"
      : hasCustomTimeout
        ? "custom"
        : "auto";
  return {
    ...base,
    id: typeof raw.id === "string" && raw.id ? raw.id : crypto.randomUUID(),
    name: typeof raw.name === "string" ? raw.name : base.name,
    comId: Number(raw.com_id ?? base.comId),
    link,
    state: "stopped",
    destination: typeof raw.destination === "string" ? raw.destination : base.destination,
    source: typeof raw.source === "string" ? raw.source : base.source,
    cycleUs: Number(raw.cycle_us ?? base.cycleUs),
    timeoutMode,
    timeoutUs: hasCustomTimeout ? parsedTimeoutUs : base.timeoutUs,
    timeoutBehavior: raw.timeout_behavior === "zero" ? "zero" : "keep",
    payloadHex: typeof raw.payload_hex === "string" ? raw.payload_hex.toUpperCase() : base.payloadHex,
    transport: raw.transport === "tcp" ? "tcp" : "udp",
    etbTopoCount: Number(raw.etb_topo_count ?? 0),
    opTrnTopoCount: Number(raw.op_trn_topo_count ?? 0),
    redId: Number(raw.red_id ?? 0),
    redState: raw.red_state === "follower" ? "follower" : "leader",
    numReplies: Number(raw.num_replies ?? 1),
    replyTimeoutUs: Number(raw.reply_timeout_us ?? 5000000),
    responseMode: raw.response_mode === "query" ? "query" : "reply",
    confirmTimeoutUs: Number(raw.confirm_timeout_us ?? 2000000),
    replyComId: Number(raw.reply_com_id ?? 0),
    replyIp: typeof raw.reply_ip === "string" ? raw.reply_ip : "0.0.0.0",
    sourceUri: typeof raw.source_uri === "string" ? raw.source_uri : "",
    destUri: typeof raw.dest_uri === "string" ? raw.dest_uri : "",
  };
}

function displayValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (value === null || value === undefined) return "—";
  try { return JSON.stringify(value); } catch { return String(value); }
}

function missedBetween(previous: number, current: number) {
  const distance = (current - previous + U32) % U32;
  return distance > 1 && distance < 0x8000_0000 ? distance - 1 : 0;
}

function defaultDatasetValues(imported: XmlImport, datasetId: number, visiting = new Set<number>()): Record<string, unknown> {
  if (visiting.has(datasetId)) return {};
  const dataset = imported.datasets.find(item => item.id === datasetId);
  if (!dataset) return {};
  const nextVisiting = new Set(visiting);
  nextVisiting.add(datasetId);
  const result: Record<string, unknown> = {};
  for (const element of dataset.elements) {
    const singleValue = () => {
      if (element.type_id > 1000) return defaultDatasetValues(imported, element.type_id, nextVisiting);
      if (element.type_id === 15) return { seconds: 0, ticks: 0 };
      return 0;
    };
    if (element.dynamic) {
      result[element.name] = [];
    } else if (element.array_size > 1) {
      result[element.name] = Array.from({ length: element.array_size }, singleValue);
    } else {
      result[element.name] = singleValue();
    }
  }
  return result;
}

function draftsFromValues(values: Record<string, unknown>): Record<string, string> {
  return Object.fromEntries(Object.entries(values).map(([name, value]) => [name, JSON.stringify(value)]));
}

function draftsFromDecoded(decoded: DecodedDataset): Record<string, string> {
  return Object.fromEntries(
    Object.entries(decoded.fields).map(([name, field]) => [name, JSON.stringify(field.value ?? field.raw ?? null)]),
  );
}

export default function TrdpSessionView({ sessionId }: { sessionId: string }) {
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const params = tab?.params as Record<string, unknown> | undefined;
  const mode = (params?.mode as string | undefined) ?? "node";
  const storageKey = `tauterm:trdp:${sessionId}:objects`;
  const [page, setPage] = useState<Page>("overview");
  const [events, setEvents] = useState<TrdpEvent[]>([]);
  const [objects, setObjects] = useState<TrdpObject[]>(() => {
    try {
      const saved = localStorage.getItem(storageKey);
      if (!saved) return [];
      const parsed = JSON.parse(saved) as TrdpObject[];
      return Array.isArray(parsed)
        ? parsed.map(item => ({
            ...item,
            state: "stopped" as const,
            timeoutMode: item.timeoutMode
              ?? (item.kind === "pd_subscriber" || item.kind === "pd_request" ? "custom" : "auto"),
          }))
        : [];
    } catch { return []; }
  });
  const [xmlImport, setXmlImport] = useState<XmlImport | null>(null);
  const [workspaceName, setWorkspaceName] = useState<string | null>(null);
  const [decoded, setDecoded] = useState<DecodedDataset | null>(null);
  const [selectedPacket, setSelectedPacket] = useState<TrdpEvent | null>(null);
  const mdRequestStartedUs = useRef(new Map<string, number>());
  const [structuredEditor, setStructuredEditor] = useState<StructuredEditor | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    try { localStorage.setItem(storageKey, JSON.stringify(objects)); } catch { /* best effort */ }
  }, [objects, storageKey]);

  useEffect(() => {
    let disposed = false;
    const unlisten = listen<TrdpEvent>("trdp-event", ({ payload }) => {
      if (disposed || payload.session_id !== sessionId) return;
      if (payload.event === "md_session" && payload.md_session_id && payload.timestamp_us !== undefined) {
        const starts = mdRequestStartedUs.current;
        starts.set(payload.md_session_id, payload.timestamp_us);
        if (starts.size > 1024) {
          const oldest = starts.keys().next().value;
          if (typeof oldest === "string") starts.delete(oldest);
        }
      }
      if (payload.event === "packet") {
        let packet = payload;
        if (
          payload.md_session_id
          && payload.timestamp_us !== undefined
          && ["Mp", "Mq", "Me"].includes(payload.msg_type ?? "")
        ) {
          const started = mdRequestStartedUs.current.get(payload.md_session_id);
          if (started !== undefined && payload.timestamp_us >= started) {
            packet = { ...payload, latency_us: payload.timestamp_us - started };
          }
        }
        setEvents(prev => [...prev, packet].slice(-5000));
        if (payload.about_to_die && payload.md_session_id) {
          mdRequestStartedUs.current.delete(payload.md_session_id);
        }
      }
      if (payload.event === "ack" && payload.id) {
        setObjects(prev => prev.map(item => {
          if (item.id !== payload.id) return item;
          if (payload.command === "object_stop") return { ...item, state: "stopped" };
          if (payload.command === "object_start") {
            return { ...item, state: isOneShotKind(item.kind) ? "stopped" : "running" };
          }
          return item;
        }));
      }
      if (payload.error) setError(payload.error);
    });
    return () => { disposed = true; void unlisten.then(fn => fn()); };
  }, [sessionId]);

  const datasetByComId = useMemo(() => {
    const result = new Map<number, number>();
    for (const telegram of xmlImport?.telegrams ?? []) result.set(telegram.com_id, telegram.dataset_id);
    return result;
  }, [xmlImport]);

  const expectedCycleByComId = useMemo(() => {
    const result = new Map<number, number>();
    for (const object of objects) {
      if (object.kind === "pd_publisher") result.set(object.comId, object.cycleUs);
    }
    for (const telegram of xmlImport?.telegrams ?? []) {
      if (telegram.cycle_us && !result.has(telegram.com_id)) result.set(telegram.com_id, telegram.cycle_us);
    }
    return result;
  }, [objects, xmlImport]);

  const flows = useMemo(() => {
    type MutableFlow = FlowRow & { intervals: number[]; previousSeq?: number; previousTimestamp?: number };
    const map = new Map<string, MutableFlow>();
    for (const event of events) {
      if (event.com_id === undefined) continue;
      const key = `${event.link ?? ""}:${event.msg_type ?? event.kind}:${event.com_id}:${event.src_ip ?? ""}:${event.dest_ip ?? ""}`;
      const row = map.get(key) ?? {
        key,
        msg: event.msg_type ?? event.kind ?? "?",
        comId: event.com_id,
        src: event.src_ip ?? "—",
        dst: event.dest_ip ?? "—",
        count: 0,
        link: event.link ?? "—",
        missed: 0,
        errors: 0,
        intervals: [],
      };
      row.count += 1;
      if (
        (event.result_code !== undefined && event.result_code !== 0)
        || event.crc_valid === false
        || event.protocol_valid === false
      ) {
        row.errors += 1;
      }
      if (event.seq_count !== undefined) {
        if (row.previousSeq !== undefined) row.missed += missedBetween(row.previousSeq, event.seq_count);
        row.previousSeq = event.seq_count;
        row.lastSeq = event.seq_count;
      }
      if (event.timestamp_us !== undefined) {
        if (row.previousTimestamp !== undefined && event.timestamp_us >= row.previousTimestamp) {
          row.intervals.push(event.timestamp_us - row.previousTimestamp);
        }
        row.previousTimestamp = event.timestamp_us;
      }
      row.size = event.data_len;
      map.set(key, row);
    }
    return [...map.values()].map(row => {
      const expected = expectedCycleByComId.get(row.comId);
      if (row.intervals.length) {
        row.minIntervalUs = Math.min(...row.intervals);
        row.maxIntervalUs = Math.max(...row.intervals);
        row.avgIntervalUs = row.intervals.reduce((sum, value) => sum + value, 0) / row.intervals.length;
        if (expected !== undefined) row.jitterUs = row.intervals.reduce((sum, value) => sum + Math.abs(value - expected), 0) / row.intervals.length;
      }
      const { intervals: _intervals, previousSeq: _previousSeq, previousTimestamp: _previousTimestamp, ...result } = row;
      return result;
    });
  }, [events, expectedCycleByComId]);

  async function command<T = unknown>(name: string, payload: Record<string, unknown> = {}): Promise<T> {
    setError(null);
    try {
      return await invoke<T>("trdp_command", { sessionId, command: { command: name, ...payload } });
    } catch (cause) {
      setError(String(cause));
      throw cause;
    }
  }

  function addObject(kind: ObjectKind) {
    setObjects(prev => [...prev, createObject(kind, prev.filter(item => item.kind === kind).length + 1)]);
  }

  function patchObject(id: string, patch: Partial<TrdpObject>) {
    setObjects(prev => prev.map(item => item.id === id ? { ...item, ...patch } : item));
  }

  async function startObject(obj: TrdpObject) {
    if (obj.kind === "pd_request") {
      // PD Request is a Send action in the UI. The native side may retain a
      // subscriber handle for the reply window, so replace any previous handle
      // before issuing the next request with the same object id.
      await command("object_stop", { id: obj.id, kind: obj.kind });
    }
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
        timeout_us: obj.timeoutMode === "custom" ? obj.timeoutUs : 0,
        timeout_behavior: obj.timeoutBehavior,
        payload_hex: obj.payloadHex,
        transport: obj.transport,
        etb_topo_count: obj.etbTopoCount,
        op_trn_topo_count: obj.opTrnTopoCount,
        red_id: obj.redId,
        red_state: obj.redState,
        num_replies: obj.numReplies,
        reply_timeout_us: obj.replyTimeoutUs,
        response_mode: obj.responseMode,
        confirm_timeout_us: obj.confirmTimeoutUs,
        reply_com_id: obj.replyComId,
        reply_ip: obj.replyIp,
        source_uri: obj.sourceUri,
        dest_uri: obj.destUri,
      },
    });
  }

  async function stopObject(obj: TrdpObject) {
    await command("object_stop", { id: obj.id, kind: obj.kind });
  }

  async function removeObject(obj: TrdpObject) {
    if (obj.state === "running") return;
    if (obj.kind === "pd_request") {
      await command("object_stop", { id: obj.id, kind: obj.kind });
    }
    setObjects(prev => prev.filter(item => item.id !== obj.id));
  }

  async function updatePayload(obj: TrdpObject) {
    await command("object_update", { id: obj.id, payload_hex: obj.payloadHex });
  }

  async function openCapture() {
    const path = await open({ multiple: false, filters: [{ name: "Packet Capture", extensions: ["pcap", "pcapng"] }] });
    if (typeof path !== "string") return;
    const pdPort = paramNumber(params, "pd_port", 17224);
    const mdPorts = [...new Set([paramNumber(params, "md_udp_port", 17225), paramNumber(params, "md_tcp_port", 17225)])];
    const packets = await invoke<TrdpEvent[]>("trdp_open_capture", { path, pdPorts: [pdPort], mdPorts });
    setEvents(prev => [...prev, ...packets].slice(-5000));
    setPage("traffic");
  }

  async function saveCapture() {
    const path = await save({ filters: [{ name: "PCAPNG", extensions: ["pcapng"] }] });
    if (!path) return;
    await invoke("trdp_save_capture", { path, packets: events.filter(event => event.raw_frame_hex) });
  }

  async function importXml() {
    const configured = typeof params?.xml_path === "string" ? params.xml_path : "";
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

  async function importWorkspace() {
    const selected = await open({ multiple: false, filters: [{ name: "TauTerm TRDP Workspace", extensions: ["json"] }] });
    if (typeof selected !== "string") return;
    const workspace = await command<Workspace>("workspace_import", { path: selected });
    const imported = (workspace.objects ?? []).map(workspaceObject).filter((item): item is TrdpObject => item !== null);
    setObjects(imported.map(item => ({ ...item, state: "stopped" })));
    setWorkspaceName(workspace.name ?? selected);
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

  async function openStructuredEditor(obj: TrdpObject) {
    if (!xmlImport) {
      setError("先导入 TRDP XML 才能使用 Dataset structured editor。");
      return;
    }
    const datasetId = datasetByComId.get(obj.comId);
    if (!datasetId) {
      setError(`ComID ${obj.comId} 没有 XML Dataset 映射。`);
      return;
    }
    if (obj.payloadHex) {
      try {
        const result = await command<DecodedDataset>("dataset_decode", {
          path: xmlImport.path,
          dataset_id: datasetId,
          payload_hex: obj.payloadHex,
        });
        setStructuredEditor({ objectId: obj.id, datasetId, drafts: draftsFromDecoded(result) });
        return;
      } catch {
        return;
      }
    }
    setStructuredEditor({
      objectId: obj.id,
      datasetId,
      drafts: draftsFromValues(defaultDatasetValues(xmlImport, datasetId)),
    });
  }

  async function decodeStructuredFromHex() {
    if (!structuredEditor || !xmlImport) return;
    const obj = objects.find(item => item.id === structuredEditor.objectId);
    if (!obj || !obj.payloadHex) {
      setError("当前对象没有可解码的 Payload HEX。");
      return;
    }
    try {
      const result = await command<DecodedDataset>("dataset_decode", {
        path: xmlImport.path,
        dataset_id: structuredEditor.datasetId,
        payload_hex: obj.payloadHex,
      });
      setStructuredEditor(prev => prev ? { ...prev, drafts: draftsFromDecoded(result) } : prev);
    } catch {
      // command() already populated the error banner.
    }
  }

  async function applyStructuredToHex() {
    if (!structuredEditor || !xmlImport) return;
    const obj = objects.find(item => item.id === structuredEditor.objectId);
    if (!obj) return;
    const values: Record<string, unknown> = {};
    try {
      for (const [name, draft] of Object.entries(structuredEditor.drafts)) {
        values[name] = JSON.parse(draft);
      }
    } catch (cause) {
      setError(`Structured field 必须是合法 JSON 值: ${String(cause)}`);
      return;
    }
    try {
      const encoded = await command<EncodedDataset>("dataset_encode", {
        path: xmlImport.path,
        dataset_id: structuredEditor.datasetId,
        values,
      });
      patchObject(obj.id, { payloadHex: encoded.payload_hex });
      if (obj.state === "running" && (obj.kind === "pd_publisher" || obj.kind === "md_listener")) {
        await command("object_update", { id: obj.id, payload_hex: encoded.payload_hex });
      }
    } catch {
      // command() already populated the error banner.
    }
  }

  function importTemplates() {
    if (!xmlImport) return;
    setObjects(prev => {
      const known = new Set(prev.map(item => `${item.comId}:${item.destination}`));
      const additions: TrdpObject[] = [];
      for (const telegram of xmlImport.telegrams) {
        if (telegram.traffic_kind !== "pd") continue;
        for (const destination of telegram.destinations.length ? telegram.destinations : ["0.0.0.0"]) {
          if (known.has(`${telegram.com_id}:${destination}`)) continue;
          const item = createObject("pd_subscriber", additions.length + 1);
          item.name = `${telegram.name} (imported template)`;
          item.comId = telegram.com_id;
          item.destination = destination;
          item.source = telegram.sources[0] ?? "0.0.0.0";
          if (telegram.timeout_us !== undefined) {
            item.timeoutMode = "custom";
            item.timeoutUs = telegram.timeout_us;
          } else {
            item.timeoutMode = "auto";
          }
          additions.push(item);
          known.add(`${telegram.com_id}:${destination}`);
        }
      }
      return [...prev, ...additions];
    });
  }

  async function startLiveCapture() {
    const interfaceA = typeof params?.capture_interface === "string" ? params.capture_interface : "";
    const interfaceB = params?.capture_interface_b_enabled && typeof params?.capture_interface_b === "string" ? params.capture_interface_b : "";
    const configuredFilter = typeof params?.capture_filter === "string" ? params.capture_filter : STANDARD_CAPTURE_FILTER;
    const filterAuto = typeof params?.capture_filter_auto === "boolean"
      ? params.capture_filter_auto
      : configuredFilter === STANDARD_CAPTURE_FILTER;
    const filter = filterAuto
      ? captureFilterForPorts(
          paramNumber(params, "pd_port", 17224),
          paramNumber(params, "md_udp_port", 17225),
          paramNumber(params, "md_tcp_port", 17225),
        )
      : configuredFilter;
    await command("capture_start", {
      interface: interfaceA,
      interface_b: interfaceB,
      filter,
    });
  }

  async function confirmMessage(event: TrdpEvent) {
    if (!event.md_session_id) return;
    await command("md_confirm", { md_session_id: event.md_session_id, link: event.link ?? "a", user_status: 0 });
  }

  function mdLatencyUs(event: TrdpEvent) {
    if (event.latency_us !== undefined) return event.latency_us;
    if (!event.md_session_id || event.timestamp_us === undefined || !["Mp", "Mq", "Me"].includes(event.msg_type ?? "")) {
      return undefined;
    }
    const capturedRequests = events.filter(candidate =>
      candidate.md_session_id === event.md_session_id
      && candidate.msg_type === "Mr"
      && candidate.timestamp_us !== undefined
      && candidate.timestamp_us <= event.timestamp_us!,
    );
    const capturedRequest = capturedRequests.length > 0
      ? capturedRequests[capturedRequests.length - 1]
      : undefined;
    const started = capturedRequest?.timestamp_us ?? mdRequestStartedUs.current.get(event.md_session_id);
    return started === undefined || event.timestamp_us < started ? undefined : event.timestamp_us - started;
  }

  function observedMdReplies(event: TrdpEvent) {
    if (!event.md_session_id) return undefined;
    return events.filter(candidate =>
      candidate.md_session_id === event.md_session_id
      && ["Mp", "Mq"].includes(candidate.msg_type ?? ""),
    ).length;
  }

  const structuredObject = structuredEditor ? objects.find(item => item.id === structuredEditor.objectId) : undefined;
  const structuredDataset = structuredEditor && xmlImport
    ? xmlImport.datasets.find(item => item.id === structuredEditor.datasetId)
    : undefined;

  const objectEditor = (obj: TrdpObject) => {
    const oneShot = isOneShotKind(obj.kind);
    const subscriber = obj.kind === "pd_subscriber" || obj.kind === "pd_request";
    return (
      <tr key={obj.id}>
        <td><input style={cellInputStyle} value={obj.name} onChange={event => patchObject(obj.id, { name: event.target.value })} /></td>
        <td><input style={cellInputStyle} type="number" min={1} value={obj.comId} onChange={event => patchObject(obj.id, { comId: Number(event.target.value) })} /></td>
        <td><select value={obj.link} onChange={event => patchObject(obj.id, { link: event.target.value as LinkChoice })}><option value="a">A</option><option value="b">B</option><option value="both">A+B</option></select></td>
        <td><input style={cellInputStyle} value={obj.destination} onChange={event => patchObject(obj.id, { destination: event.target.value })} /></td>
        <td>{obj.kind.startsWith("md_")
          ? <select value={obj.transport} onChange={event => patchObject(obj.id, { transport: event.target.value as "udp" | "tcp" })}><option value="udp">UDP</option><option value="tcp">TCP</option></select>
          : subscriber
            ? <span style={{ display: "inline-flex", gap: 4, alignItems: "center" }}>
                <select value={obj.timeoutMode} onChange={event => patchObject(obj.id, { timeoutMode: event.target.value as "auto" | "custom" })}><option value="auto">Auto</option><option value="custom">Custom</option></select>
                {obj.timeoutMode === "custom" && <input style={cellInputStyle} type="number" min={1} value={obj.timeoutUs} onChange={event => patchObject(obj.id, { timeoutUs: Number(event.target.value) })} />}
              </span>
            : <input style={cellInputStyle} type="number" min={1} value={obj.cycleUs} onChange={event => patchObject(obj.id, { cycleUs: Number(event.target.value) })} />}</td>
        <td><input style={cellInputStyle} value={obj.payloadHex} onChange={event => patchObject(obj.id, { payloadHex: event.target.value.replace(/[^0-9a-f]/gi, "").toUpperCase() })} /></td>
        <td>
          <details>
            <summary>Advanced</summary>
            <label>Source <input value={obj.source} onChange={event => patchObject(obj.id, { source: event.target.value })} /></label><br />
            <label>ETB <input type="number" min={0} value={obj.etbTopoCount} onChange={event => patchObject(obj.id, { etbTopoCount: Number(event.target.value) })} /></label><br />
            <label>OpTrn <input type="number" min={0} value={obj.opTrnTopoCount} onChange={event => patchObject(obj.id, { opTrnTopoCount: Number(event.target.value) })} /></label><br />
            {subscriber && <label>Timeout behavior <select value={obj.timeoutBehavior} onChange={event => patchObject(obj.id, { timeoutBehavior: event.target.value as "keep" | "zero" })}><option value="keep">Keep last value</option><option value="zero">Set to zero</option></select></label>}
            {obj.kind === "pd_request" && <><br /><label>Reply ComID <input type="number" min={0} value={obj.replyComId} onChange={event => patchObject(obj.id, { replyComId: Number(event.target.value) })} /></label><br /><small>0 = same as request ComID</small><br /><label>Reply IP <input value={obj.replyIp} onChange={event => patchObject(obj.id, { replyIp: event.target.value })} /></label><br /><small>0.0.0.0 = Link local IP</small></>}
            {obj.kind === "pd_publisher" && <><label>Red ID <input type="number" min={0} value={obj.redId} onChange={event => patchObject(obj.id, { redId: Number(event.target.value) })} /></label><br /><label>Red state <select value={obj.redState} onChange={event => patchObject(obj.id, { redState: event.target.value as "leader" | "follower" })}><option value="leader">Leader</option><option value="follower">Follower</option></select></label></>}
            {obj.kind.startsWith("md_") && <><label>Source URI <input value={obj.sourceUri} onChange={event => patchObject(obj.id, { sourceUri: event.target.value })} /></label><br /><label>Destination URI <input value={obj.destUri} onChange={event => patchObject(obj.id, { destUri: event.target.value })} /></label><br /></>}
            {obj.kind === "md_request" && <><label>Replies <input type="number" min={1} value={obj.numReplies} onChange={event => patchObject(obj.id, { numReplies: Number(event.target.value) })} /></label><br /><label>Reply timeout µs <input type="number" min={1} value={obj.replyTimeoutUs} onChange={event => patchObject(obj.id, { replyTimeoutUs: Number(event.target.value) })} /></label></>}
            {obj.kind === "md_listener" && <><label>Response <select value={obj.responseMode} onChange={event => patchObject(obj.id, { responseMode: event.target.value as "reply" | "query" })}><option value="reply">Reply (Mp)</option><option value="query">ReplyQuery (Mq)</option></select></label>{obj.responseMode === "query" && <><br /><label>Confirm timeout µs <input type="number" min={1} value={obj.confirmTimeoutUs} onChange={event => patchObject(obj.id, { confirmTimeoutUs: Number(event.target.value) })} /></label></>}</>}
          </details>
        </td>
        <td style={{ whiteSpace: "nowrap" }}>
          {oneShot ? <button onClick={() => void startObject(obj)}>Send</button> : <button onClick={() => void (obj.state === "running" ? stopObject(obj) : startObject(obj))}>{obj.state === "running" ? "Stop" : "Start"}</button>}
          {obj.state === "running" && (obj.kind === "pd_publisher" || obj.kind === "md_listener") && <button onClick={() => void updatePayload(obj)}>Put</button>}
          {obj.kind !== "pd_subscriber" && datasetByComId.has(obj.comId) && <button onClick={() => void openStructuredEditor(obj)}>Dataset</button>}
          <button onClick={() => void removeObject(obj)} disabled={obj.state === "running"}>×</button>
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

        {structuredEditor && structuredObject && structuredDataset && (
          <div className="liquid-glass-card" style={{ padding: 12, marginBottom: 12 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <strong>Dataset Structured Editor · {structuredDataset.name} ({structuredDataset.id})</strong>
              <span>Object: {structuredObject.name} · ComID {structuredObject.comId}</span>
              <button style={{ marginLeft: "auto" }} onClick={() => void decodeStructuredFromHex()}>HEX → Fields</button>
              <button onClick={() => void applyStructuredToHex()}>Fields → HEX</button>
              <button onClick={() => setStructuredEditor(null)}>Close</button>
            </div>
            <div style={{ marginTop: 6, opacity: 0.8 }}>
              每个字段输入合法 JSON 值；数组/嵌套 Dataset 使用 JSON array/object。Fields → HEX 会按 XML 类型、网络字节序及 scale/offset 编码，HEX 仍是最终 wire truth source。
            </div>
            <table style={{ ...tableStyle, marginTop: 8 }}>
              <thead><tr><th>Field</th><th>Type</th><th>Array</th><th>Value (JSON)</th><th>Unit</th></tr></thead>
              <tbody>
                {structuredDataset.elements.map(element => (
                  <tr key={element.name}>
                    <td>{element.name}</td>
                    <td>{element.data_type}{element.scale !== undefined ? ` ×${element.scale}` : ""}{element.offset !== undefined ? ` +${element.offset}` : ""}</td>
                    <td>{element.dynamic ? "dynamic" : element.array_size}</td>
                    <td>
                      <textarea
                        rows={1}
                        style={{ width: "100%", minWidth: 220, fontFamily: "var(--font-mono)" }}
                        value={structuredEditor.drafts[element.name] ?? "null"}
                        onChange={event => setStructuredEditor(prev => prev ? {
                          ...prev,
                          drafts: { ...prev.drafts, [element.name]: event.target.value },
                        } : prev)}
                      />
                    </td>
                    <td>{element.unit ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div style={{ marginTop: 6 }}>Payload HEX: <code>{structuredObject.payloadHex || "—"}</code></div>
          </div>
        )}

        {page === "overview" && (
          <div style={{ display: "grid", gap: 12 }}>
            <h2>TRDP {mode === "monitor" ? "Monitor" : "Node"}</h2>
            <div>PD: UDP/{paramNumber(params, "pd_port", 17224)} · MD: UDP/{paramNumber(params, "md_udp_port", 17225)} TCP/{paramNumber(params, "md_tcp_port", 17225)} · SDTv2/SDTv4: detected only, validation out of scope</div>
            {mode === "node" ? <div>Link A: {String(params?.link_a_ip ?? "—")} · Link B: {params?.link_b_enabled ? String(params?.link_b_ip ?? "—") : "Disabled"}</div> : <div>Capture A: {String(params?.capture_interface ?? "—")} · Capture B: {params?.capture_interface_b_enabled ? String(params?.capture_interface_b ?? "—") : "Disabled"}</div>}
            <div>发送策略 / TX policy: Publishers, PD Requests and MD Requests/Notify always require an explicit Start/Send action.</div>
            <div>Safety: TauTerm TRDP is a diagnostic/development tool and does not perform SDT safety validation or claim safety certification.</div>
            {workspaceName && <div>Workspace: {workspaceName} · imported objects forced to Stopped.</div>}
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button onClick={() => void importXml()}>导入 TRDP XML / Import XML</button>
              {mode === "node" && <button onClick={() => void importWorkspace()}>导入 Workspace JSON</button>}
              {mode === "monitor" && <><button onClick={() => void openCapture()}>打开 .pcap/.pcapng</button><button onClick={() => void saveCapture()} disabled={!events.some(event => event.raw_frame_hex)}>保存 .pcapng</button><button onClick={() => void startLiveCapture()}>Start Live Capture</button><button onClick={() => void command("capture_stop")}>Stop Capture</button></>}
            </div>
            {xmlImport && (
              <div className="liquid-glass-card" style={{ padding: 12 }}>
                <strong>Import Preview</strong>
                <div>{xmlImport.datasets.length} Datasets · {xmlImport.telegrams.length} Telegrams · ports {xmlImport.pd_port}/{xmlImport.md_udp_port}/{xmlImport.md_tcp_port} · SDT: {xmlImport.sdt_detected ? "Detected (not validated)" : "No configuration detected"}</div>
                {xmlImport.warnings.map(warning => <div key={warning} style={{ marginTop: 4 }}>⚠ {warning}</div>)}
                {mode === "node" && <button style={{ marginTop: 8 }} onClick={importTemplates}>将 PD Telegram 作为停止状态的 Subscriber 模板加入 Workspace</button>}
                <table style={{ ...tableStyle, marginTop: 8 }}><thead><tr><th>Type</th><th>Telegram</th><th>ComID</th><th>Dataset</th><th>Cycle</th><th>Sources</th><th>Destinations</th></tr></thead><tbody>{xmlImport.telegrams.map(telegram => <tr key={`${telegram.com_id}-${telegram.name}`}><td>{telegram.traffic_kind.toUpperCase()}</td><td>{telegram.name}</td><td>{telegram.com_id}</td><td>{telegram.dataset_id}</td><td>{telegram.cycle_us ?? "—"}</td><td>{telegram.sources.join(", ") || "—"}</td><td>{telegram.destinations.join(", ") || "—"}</td></tr>)}</tbody></table>
              </div>
            )}
          </div>
        )}

        {page === "publishers" && <section><div style={{ display: "flex", justifyContent: "space-between" }}><h2>发布 / Publishers · PD Publisher</h2><button onClick={() => addObject("pd_publisher")}>+ Publisher</button></div><table style={tableStyle}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination</th><th>Cycle µs</th><th>Payload HEX</th><th>Protocol</th><th>State</th></tr></thead><tbody>{objects.filter(object => object.kind === "pd_publisher").map(objectEditor)}</tbody></table></section>}
        {page === "subscribers" && <section><div style={{ display: "flex", gap: 8, alignItems: "center" }}><h2 style={{ marginRight: "auto" }}>订阅 / Subscribers · PD Subscriber / Request</h2><button onClick={() => addObject("pd_subscriber")}>+ Subscriber</button><button onClick={() => addObject("pd_request")}>+ PD Request</button></div><table style={tableStyle}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Multicast/Destination</th><th>Timeout µs</th><th>Payload HEX</th><th>Protocol</th><th>State</th></tr></thead><tbody>{objects.filter(object => object.kind === "pd_subscriber" || object.kind === "pd_request").map(objectEditor)}</tbody></table><h3>Subscriber diagnostics</h3><table style={tableStyle}><thead><tr><th>Link</th><th>ComID</th><th>Packets</th><th>Missed seq</th><th>Last seq</th><th>Interval min/avg/max µs</th><th>Avg jitter µs</th><th>Errors</th></tr></thead><tbody>{flows.filter(flow => flow.msg.startsWith("P")).map(flow => <tr key={`diag-${flow.key}`}><td>{flow.link}</td><td>{flow.comId}</td><td>{flow.count}</td><td>{flow.missed}</td><td>{flow.lastSeq ?? "—"}</td><td>{flow.minIntervalUs === undefined ? "—" : `${Math.round(flow.minIntervalUs)}/${Math.round(flow.avgIntervalUs ?? 0)}/${Math.round(flow.maxIntervalUs ?? 0)}`}</td><td>{flow.jitterUs === undefined ? "—" : Math.round(flow.jitterUs)}</td><td>{flow.errors}</td></tr>)}</tbody></table></section>}
        {page === "messages" && <section><div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}><h2 style={{ marginRight: "auto" }}>消息 / Messages · MD</h2><button onClick={() => addObject("md_request")}>+ Request</button><button onClick={() => addObject("md_listener")}>+ Listener/Replier</button><button onClick={() => addObject("md_notify")}>+ Notify</button></div><table style={tableStyle}><thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination/Filter</th><th>UDP/TCP</th><th>Payload HEX</th><th>Protocol</th><th>Action</th></tr></thead><tbody>{objects.filter(object => object.kind.startsWith("md_")).map(objectEditor)}</tbody></table></section>}

        {page === "traffic" && (
          <section>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}><h2>流量 / Traffic · TRDP Packet Inspector</h2><div style={{ display: "flex", gap: 8 }}><button onClick={() => void openCapture()}>Open capture</button><button onClick={() => void saveCapture()} disabled={!events.some(event => event.raw_frame_hex)}>Save pcapng</button><button onClick={() => { setEvents([]); setSelectedPacket(null); setDecoded(null); }}>Clear</button></div></div>
            <h3>Flows</h3>
            <table style={tableStyle}><thead><tr><th>Link</th><th>Type</th><th>ComID</th><th>Source</th><th>Destination</th><th>Packets</th><th>Missed</th><th>Seq</th><th>Rate/Interval µs</th><th>Size</th><th>Errors</th></tr></thead><tbody>{flows.map(flow => <tr key={flow.key}><td>{flow.link}</td><td>{flow.msg}</td><td>{flow.comId}</td><td>{flow.src}</td><td>{flow.dst}</td><td>{flow.count}</td><td>{flow.missed}</td><td>{flow.lastSeq ?? "—"}</td><td>{flow.avgIntervalUs === undefined ? "—" : Math.round(flow.avgIntervalUs)}</td><td>{flow.size ?? "—"}</td><td>{flow.errors}</td></tr>)}</tbody></table>
            <h3>Packets</h3>
            <table style={{ ...tableStyle, fontFamily: "var(--font-mono)" }}><thead><tr><th>#</th><th>Link</th><th>Type</th><th>ComID</th><th>Source → Destination</th><th>Seq</th><th>Topo ETB/Op</th><th>Len</th><th>Payload</th></tr></thead><tbody>{events.slice().reverse().slice(0, 1000).map((event, index) => <tr key={`${event.timestamp_us ?? 0}-${index}`} onClick={() => void inspectPacket(event)} style={{ cursor: "pointer" }}><td>{events.length - index}</td><td>{event.link ?? "—"}</td><td>{event.msg_type ?? event.kind ?? "—"}</td><td>{event.com_id ?? "—"}</td><td>{event.src_ip ?? "—"} → {event.dest_ip ?? "—"}</td><td>{event.seq_count ?? "—"}</td><td>{event.etb_topo_count ?? "—"}/{event.op_trn_topo_count ?? "—"}</td><td>{event.data_len ?? "—"}</td><td title={event.payload_hex}>{hexPreview(event.payload_hex)}</td></tr>)}</tbody></table>
            {selectedPacket && <div className="liquid-glass-card" style={{ marginTop: 12, padding: 12 }}><h3>Packet Inspector</h3><div>Protocol {selectedPacket.protocol_version ?? "—"} ({selectedPacket.protocol_valid === undefined ? "not checked" : selectedPacket.protocol_valid ? "valid" : "invalid"}) · CRC {selectedPacket.crc_valid === undefined ? "not checked" : selectedPacket.crc_valid ? "valid" : "invalid"} · Result {selectedPacket.result_code ?? "—"} · Reply status {selectedPacket.reply_status ?? "—"} · User status {selectedPacket.user_status ?? "—"} · Replies {selectedPacket.num_replies ?? observedMdReplies(selectedPacket) ?? "—"}/{selectedPacket.num_expected_replies ?? "—"}</div>{selectedPacket.md_session_id && <div>MD Session UUID: <code>{selectedPacket.md_session_id}</code> · Request/Reply latency {mdLatencyUs(selectedPacket) ?? "—"} µs{selectedPacket.msg_type === "Mq" && <button style={{ marginLeft: 8 }} onClick={() => void confirmMessage(selectedPacket)}>Confirm (Mc)</button>}</div>}{selectedPacket.md_session_id && <div>ReplyQuery {selectedPacket.num_reply_queries ?? "—"} · Confirms {selectedPacket.num_confirm_sent ?? "—"} · Confirm timeouts {selectedPacket.num_confirm_timeout ?? "—"} · Reply timeout {selectedPacket.reply_timeout_us ?? "—"} µs</div>}{(selectedPacket.src_uri || selectedPacket.dest_uri) && <div>URI: <code>{selectedPacket.src_uri || "—"}</code> → <code>{selectedPacket.dest_uri || "—"}</code></div>}<div>Raw payload: <code>{selectedPacket.payload_hex || "—"}</code></div>{decoded ? <><h4>{decoded.dataset_name} · Dataset {decoded.dataset_id}</h4><div>{decoded.consumed_bytes}/{decoded.payload_bytes} bytes decoded</div><table style={tableStyle}><thead><tr><th>Field</th><th>Type</th><th>Value</th><th>Unit</th></tr></thead><tbody>{Object.entries(decoded.fields).map(([name, field]) => <tr key={name}><td>{name}</td><td>{field.type}</td><td>{field.error ?? displayValue(field.value)}</td><td>{field.unit ?? "—"}</td></tr>)}</tbody></table></> : xmlImport && selectedPacket.com_id !== undefined ? <div>No dataset mapping/decodable payload for ComID {selectedPacket.com_id}.</div> : <div>Import a TRDP XML file to enable Dataset decoding.</div>}</div>}
          </section>
        )}
      </div>
    </div>
  );
}
