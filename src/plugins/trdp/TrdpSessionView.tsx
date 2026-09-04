import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import styles from "./TrdpSessionView.module.css";

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
  timeoutMode: "auto" | "custom" | "disabled";
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
  traffic_kind: "pd" | "md" | "unknown" | "ambiguous";
  com_id: number;
  dataset_id: number;
  cycle_us?: number;
  timeout_us?: number;
  timeout_behavior?: "zero" | "keep";
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
  ["overview", "trdp.nav.overview"],
  ["publishers", "trdp.nav.publishers"],
  ["subscribers", "trdp.nav.subscribers"],
  ["messages", "trdp.nav.messages"],
  ["traffic", "trdp.nav.traffic"],
];

const U32 = 0x1_0000_0000;
const STANDARD_CAPTURE_FILTER = "udp port 17224 or udp port 17225 or tcp port 17225";
const LIVE_CAPTURE_FRAME_LIMIT = 50_000;

function captureFilterForPorts(pdPort: number, mdUdpPort: number, mdTcpPort: number) {
  return `udp port ${pdPort} or udp port ${mdUdpPort} or tcp port ${mdTcpPort}`;
}

function paramNumber(params: Record<string, unknown> | undefined, key: string, fallback: number) {
  const value = params?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function isIpv4Text(value: string) {
  const parts = value.split(".");
  return parts.length === 4 && parts.every(part => {
    if (!/^\d{1,3}$/.test(part)) return false;
    const octet = Number(part);
    return octet >= 0 && octet <= 255;
  });
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
    : raw.timeout_mode === "disabled"
      ? "disabled"
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
      if (element.type_id >= 1000) return defaultDatasetValues(imported, element.type_id, nextVisiting);
      if (element.type_id === 15) return { seconds: 0, ticks: 0 };
      if (element.type_id === 16) return { seconds: 0, microseconds: 0 };
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
  const { t } = useTranslation();
  const { state } = useSession();
  const tab = state.tabs.find(item => item.id === sessionId);
  const params = tab?.params as Record<string, unknown> | undefined;
  const mode = (params?.mode as string | undefined) ?? "node";
  const storageKey = `tauterm:trdp:${sessionId}:objects`;
  const [page, setPage] = useState<Page>("overview");
  const [events, setEvents] = useState<TrdpEvent[]>([]);
  const [captureFrames, setCaptureFrames] = useState<TrdpEvent[]>([]);
  const [captureSource, setCaptureSource] = useState<"offline" | "live" | null>(null);
  const [captureRunning, setCaptureRunning] = useState(false);
  const [captureDroppedFrames, setCaptureDroppedFrames] = useState(0);
  const captureStartPending = useRef<{
    source: "offline" | "live" | null;
    frames: TrdpEvent[];
    droppedFrames: number;
  } | null>(null);
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
      if (payload.event === "capture_frame") {
        setCaptureFrames(prev => {
          if (prev.length < LIVE_CAPTURE_FRAME_LIMIT) return [...prev, payload];
          setCaptureDroppedFrames(count => count + 1);
          return [...prev.slice(1), payload];
        });
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
      if (payload.event === "ack" && payload.command === "capture_start") {
        captureStartPending.current = null;
        setCaptureSource("live");
        setCaptureRunning(true);
      }
      if (payload.event === "ack" && payload.command === "capture_stop") {
        setCaptureRunning(false);
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
      if (payload.error) {
        const pendingCapture = captureStartPending.current;
        if (pendingCapture) {
          captureStartPending.current = null;
          setCaptureFrames(pendingCapture.frames);
          setCaptureSource(pendingCapture.source);
          setCaptureDroppedFrames(pendingCapture.droppedFrames);
          // Native capture_start stops the previous capture before attempting
          // the replacement. A failed restart therefore never restores a
          // "running" state, even though the previous capture buffer is kept.
          setCaptureRunning(false);
        }
        setError(payload.error);
      }
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
        timeout_us: obj.timeoutMode === "disabled" ? 0xffff_ffff : obj.timeoutMode === "custom" ? obj.timeoutUs : 0,
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
    // PD Request is one-shot in the UI but TCNOpen retains a subscriber handle
    // for its reply window. Clean that native handle only while this Node
    // session is actually connected. Once disconnected, the side-channel and
    // all native handles are already gone; sending object_stop would fail and
    // incorrectly block deletion of the local object.
    if (obj.kind === "pd_request" && tab?.state === "connected") {
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
    // The current capture is a separate data source from the rolling event log.
    // Replacing it here prevents Save from accidentally mixing multiple files
    // or stale live-capture traffic.
    setCaptureFrames(packets);
    setCaptureSource("offline");
    setCaptureRunning(false);
    setCaptureDroppedFrames(0);
    setEvents(packets.slice(-5000));
    setPage("traffic");
  }

  async function saveCapture() {
    const path = await save({ filters: [{ name: "PCAPNG", extensions: ["pcapng"] }] });
    if (!path) return;
    await invoke("trdp_save_capture", { path, packets: captureFrames });
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
        const ipv4Destinations = telegram.destinations.filter(isIpv4Text);
        const templateDestinations = ipv4Destinations.length > 0 ? ipv4Destinations : ["0.0.0.0"];
        const source = telegram.sources.find(isIpv4Text) ?? "0.0.0.0";
        for (const destination of templateDestinations) {
          if (known.has(`${telegram.com_id}:${destination}`)) continue;
          const item = createObject("pd_subscriber", additions.length + 1);
          item.name = `${telegram.name} (imported template)`;
          item.comId = telegram.com_id;
          item.destination = destination;
          item.source = source;
          if (telegram.timeout_us === undefined || telegram.timeout_us === 0) {
            item.timeoutMode = "disabled";
          } else {
            item.timeoutMode = "custom";
            item.timeoutUs = telegram.timeout_us;
          }
          item.timeoutBehavior = telegram.timeout_behavior === "keep" ? "keep" : "zero";
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
    const previousCapture = {
      source: captureSource,
      frames: captureFrames,
      droppedFrames: captureDroppedFrames,
    };
    captureStartPending.current = previousCapture;
    setCaptureFrames([]);
    setCaptureDroppedFrames(0);
    setCaptureRunning(false);
    try {
      await command("capture_start", {
        interface: interfaceA,
        interface_b: interfaceB,
        filter,
      });
    } catch {
      if (captureStartPending.current === previousCapture) {
        captureStartPending.current = null;
        setCaptureFrames(previousCapture.frames);
        setCaptureSource(previousCapture.source);
        setCaptureDroppedFrames(previousCapture.droppedFrames);
        setCaptureRunning(false);
      }
    }
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
    const inputClass = styles.cellInput + " liquid-glass-input";
    const numberClass = styles.cellInput + " " + styles.cellNumber + " liquid-glass-input";
    const selectClass = styles.cellSelect + " liquid-glass-input liquid-glass-select";
    const compactClass = styles.compactButton + " liquid-glass-button";

    return (
      <tr key={obj.id}>
        <td><input className={inputClass} value={obj.name} onChange={event => patchObject(obj.id, { name: event.target.value })} /></td>
        <td><input className={numberClass} type="number" min={1} value={obj.comId} onChange={event => patchObject(obj.id, { comId: Number(event.target.value) })} /></td>
        <td>
          <select className={selectClass} value={obj.link} onChange={event => patchObject(obj.id, { link: event.target.value as LinkChoice })}>
            <option value="a">A</option><option value="b">B</option><option value="both">A+B</option>
          </select>
        </td>
        <td><input className={inputClass} value={obj.destination} onChange={event => patchObject(obj.id, { destination: event.target.value })} /></td>
        <td>
          {obj.kind.startsWith("md_")
            ? (
              <select className={selectClass} value={obj.transport} onChange={event => patchObject(obj.id, { transport: event.target.value as "udp" | "tcp" })}>
                <option value="udp">UDP</option><option value="tcp">TCP</option>
              </select>
            )
            : subscriber
              ? (
                <span className={styles.inlineControls}>
                  <select className={selectClass} value={obj.timeoutMode} onChange={event => patchObject(obj.id, { timeoutMode: event.target.value as "auto" | "custom" | "disabled" })}>
                    <option value="auto">{t("trdp.status.auto")}</option><option value="custom">{t("trdp.status.custom")}</option><option value="disabled">{t("trdp.status.disabled")}</option>
                  </select>
                  {obj.timeoutMode === "custom" && <input className={numberClass} type="number" min={1} value={obj.timeoutUs} onChange={event => patchObject(obj.id, { timeoutUs: Number(event.target.value) })} />}
                </span>
              )
              : <input className={numberClass} type="number" min={1} value={obj.cycleUs} onChange={event => patchObject(obj.id, { cycleUs: Number(event.target.value) })} />}
        </td>
        <td><input className={inputClass} value={obj.payloadHex} onChange={event => patchObject(obj.id, { payloadHex: event.target.value.replace(/[^0-9a-f]/gi, "").toUpperCase() })} /></td>
        <td>
          <details className={styles.advancedDetails}>
            <summary>{t("trdp.actions.advanced")}</summary>
            <label>{t("trdp.advanced.source")} <input className="liquid-glass-input" value={obj.source} onChange={event => patchObject(obj.id, { source: event.target.value })} /></label><br />
            <label>ETB <input className="liquid-glass-input" type="number" min={0} value={obj.etbTopoCount} onChange={event => patchObject(obj.id, { etbTopoCount: Number(event.target.value) })} /></label><br />
            <label>OpTrn <input className="liquid-glass-input" type="number" min={0} value={obj.opTrnTopoCount} onChange={event => patchObject(obj.id, { opTrnTopoCount: Number(event.target.value) })} /></label><br />
            {subscriber && <label>{t("trdp.advanced.timeoutBehavior")} <select className="liquid-glass-input liquid-glass-select" value={obj.timeoutBehavior} onChange={event => patchObject(obj.id, { timeoutBehavior: event.target.value as "keep" | "zero" })}><option value="keep">{t("trdp.advanced.keepLast")}</option><option value="zero">{t("trdp.advanced.setZero")}</option></select></label>}
            {obj.kind === "pd_request" && <><br /><label>{t("trdp.advanced.replyComId")} <input className="liquid-glass-input" type="number" min={0} value={obj.replyComId} onChange={event => patchObject(obj.id, { replyComId: Number(event.target.value) })} /></label><br /><small>{t("trdp.advanced.sameAsRequest")}</small><br /><label>{t("trdp.advanced.replyIp")} <input className="liquid-glass-input" value={obj.replyIp} onChange={event => patchObject(obj.id, { replyIp: event.target.value })} /></label><br /><small>{t("trdp.advanced.linkLocalIp")}</small></>}
            {obj.kind === "pd_publisher" && <><label>{t("trdp.advanced.redId")} <input className="liquid-glass-input" type="number" min={0} value={obj.redId} onChange={event => patchObject(obj.id, { redId: Number(event.target.value) })} /></label><br /><label>{t("trdp.advanced.redState")} <select className="liquid-glass-input liquid-glass-select" value={obj.redState} onChange={event => patchObject(obj.id, { redState: event.target.value as "leader" | "follower" })}><option value="leader">{t("trdp.advanced.leader")}</option><option value="follower">{t("trdp.advanced.follower")}</option></select></label></>}
            {obj.kind.startsWith("md_") && <><label>{t("trdp.advanced.sourceUri")} <input className="liquid-glass-input" value={obj.sourceUri} onChange={event => patchObject(obj.id, { sourceUri: event.target.value })} /></label><br /><label>{t("trdp.advanced.destinationUri")} <input className="liquid-glass-input" value={obj.destUri} onChange={event => patchObject(obj.id, { destUri: event.target.value })} /></label><br /></>}
            {obj.kind === "md_request" && <><label>{t("trdp.advanced.replies")} <input className="liquid-glass-input" type="number" min={1} value={obj.numReplies} onChange={event => patchObject(obj.id, { numReplies: Number(event.target.value) })} /></label><br /><label>{t("trdp.advanced.replyTimeout")} <input className="liquid-glass-input" type="number" min={1} value={obj.replyTimeoutUs} onChange={event => patchObject(obj.id, { replyTimeoutUs: Number(event.target.value) })} /></label></>}
            {obj.kind === "md_listener" && <><label>{t("trdp.advanced.response")} <select className="liquid-glass-input liquid-glass-select" value={obj.responseMode} onChange={event => patchObject(obj.id, { responseMode: event.target.value as "reply" | "query" })}><option value="reply">Reply (Mp)</option><option value="query">ReplyQuery (Mq)</option></select></label>{obj.responseMode === "query" && <><br /><label>{t("trdp.advanced.confirmTimeout")} <input className="liquid-glass-input" type="number" min={1} value={obj.confirmTimeoutUs} onChange={event => patchObject(obj.id, { confirmTimeoutUs: Number(event.target.value) })} /></label></>}</>}
          </details>
        </td>
        <td>
          <span className={styles.rowActions}>
            {oneShot
              ? <button className={styles.compactButton + " liquid-primary-button"} onClick={() => void startObject(obj)}>{t("trdp.actions.send")}</button>
              : <button className={styles.compactButton + " " + (obj.state === "running" ? "liquid-glass-button" : "liquid-primary-button")} onClick={() => void (obj.state === "running" ? stopObject(obj) : startObject(obj))}>{obj.state === "running" ? t("trdp.actions.stop") : t("trdp.actions.start")}</button>}
            {obj.state === "running" && (obj.kind === "pd_publisher" || obj.kind === "md_listener") && <button className={compactClass} onClick={() => void updatePayload(obj)}>{t("trdp.actions.update")}</button>}
            {obj.kind !== "pd_subscriber" && datasetByComId.has(obj.comId) && <button className={compactClass} onClick={() => void openStructuredEditor(obj)}>{t("trdp.actions.dataset")}</button>}
            <button className={compactClass} onClick={() => void removeObject(obj)} disabled={obj.state === "running"} title={t("trdp.actions.remove")}>×</button>
          </span>
        </td>
      </tr>
    );
  };

  const visibleNav = nav.filter(([key]) => mode === "monitor" ? ["overview", "traffic"].includes(key) : true);
  const publisherObjects = objects.filter(object => object.kind === "pd_publisher");
  const subscriberObjects = objects.filter(object => object.kind === "pd_subscriber" || object.kind === "pd_request");
  const messageObjects = objects.filter(object => object.kind.startsWith("md_"));
  const subscriberFlows = flows.filter(flow => flow.msg.startsWith("P"));
  const packetRows = events.slice().reverse().slice(0, 1000);

  return (
    <div className={styles.root}>
      <div className={styles.navBar}>
        {visibleNav.map(([key, label]) => (
          <button
            key={key}
            className={styles.navButton + " " + (page === key ? "liquid-primary-button" : "liquid-glass-button")}
            onClick={() => setPage(key)}
          >
            {t(label)}
          </button>
        ))}
      </div>

      <div className={styles.content}>
        {error && <div className={styles.error}>{error}</div>}

        {structuredEditor && structuredObject && structuredDataset && (
          <div className={styles.structuredEditor + " liquid-glass-card"}>
            <div className={styles.structuredHeader}>
              <strong>Dataset Structured Editor · {structuredDataset.name} ({structuredDataset.id})</strong>
              <span>Object: {structuredObject.name} · ComID {structuredObject.comId}</span>
              <div className={styles.structuredActions}>
                <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void decodeStructuredFromHex()}>{t("trdp.actions.hexToFields")}</button>
                <button className={styles.actionButton + " liquid-primary-button"} onClick={() => void applyStructuredToHex()}>{t("trdp.actions.fieldsToHex")}</button>
                <button className={styles.actionButton + " liquid-glass-button"} onClick={() => setStructuredEditor(null)}>{t("trdp.actions.close")}</button>
              </div>
            </div>
            <div className={styles.structuredHint}>
              {t("trdp.structured.hint")}
            </div>
            <table className={styles.table} style={{ marginTop: 8 }}>
              <thead><tr><th>{t("trdp.table.field")}</th><th>{t("trdp.table.type")}</th><th>{t("trdp.table.array")}</th><th>{t("trdp.table.valueJson")}</th><th>{t("trdp.table.unit")}</th></tr></thead>
              <tbody>
                {structuredDataset.elements.map(element => (
                  <tr key={element.name}>
                    <td>{element.name}</td>
                    <td>{element.data_type}{element.scale !== undefined ? " ×" + element.scale : ""}{element.offset !== undefined ? " +" + element.offset : ""}</td>
                    <td>{element.dynamic ? "dynamic" : element.array_size}</td>
                    <td>
                      <textarea
                        rows={1}
                        className={styles.textarea + " liquid-glass-input liquid-glass-textarea"}
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
            <div className={styles.payload}>{t("trdp.structured.payloadHex")}: <code>{structuredObject.payloadHex || "—"}</code></div>
          </div>
        )}

        {page === "overview" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>TRDP {mode === "monitor" ? "Monitor" : "Node"}</h2>
            </div>

            <div className={styles.overviewInfo}>
              <div className={styles.infoCard + " liquid-glass-card"}>
                <strong>Protocol</strong><br />
                PD: UDP/{paramNumber(params, "pd_port", 17224)} · MD: UDP/{paramNumber(params, "md_udp_port", 17225)} TCP/{paramNumber(params, "md_tcp_port", 17225)} · SDTv2/SDTv4: detected only, validation out of scope
              </div>
              <div className={styles.infoCard + " liquid-glass-card"}>
                <strong>{mode === "node" ? "Links" : "Capture interfaces"}</strong><br />
                {mode === "node"
                  ? <>Link A: {String(params?.link_a_ip ?? "—")} · Link B: {params?.link_b_enabled ? String(params?.link_b_ip ?? "—") : "Disabled"}</>
                  : <>Capture A: {String(params?.capture_interface ?? "—")} · Capture B: {params?.capture_interface_b_enabled ? String(params?.capture_interface_b ?? "—") : "Disabled"}</>}
              </div>
              <div className={styles.infoCard + " liquid-glass-card"}>
                <strong>发送策略 / TX policy</strong><br />
                Publishers, PD Requests and MD Requests/Notify always require an explicit Start/Send action.
              </div>
              <div className={styles.infoCard + " liquid-glass-card"}>
                <strong>Safety</strong><br />
                TauTerm TRDP is a diagnostic/development tool and does not perform SDT safety validation or claim safety certification.
              </div>
              {workspaceName && <div className={styles.infoCard + " liquid-glass-card"}><strong>Workspace</strong><br />{workspaceName} · imported objects forced to Stopped.</div>}
            </div>

            <div className={styles.toolbar}>
              <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void importXml()}>{t("trdp.actions.importXml")}</button>
              {mode === "node" && <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void importWorkspace()}>{t("trdp.actions.importWorkspace")}</button>}
              {mode === "monitor" && (
                <>
                  <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void openCapture()}>{t("trdp.actions.openCapture")}</button>
                  <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void saveCapture()} disabled={captureFrames.length === 0}>{t("trdp.actions.saveCapture")}</button>
                  <button className={styles.actionButton + " liquid-primary-button"} onClick={() => void startLiveCapture()} disabled={captureRunning}>{t("trdp.actions.startCapture")}</button>
                  <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void command("capture_stop")} disabled={!captureRunning}>{t("trdp.actions.stopCapture")}</button>
                </>
              )}
            </div>

            {mode === "monitor" && (
              <div className={styles.infoCard + " liquid-glass-card"}>
                <strong>Capture</strong><br />
                {captureRunning ? "Running" : captureSource ? "Stopped" : "Not started"}
                {captureSource ? <> · source {captureSource}</> : null}
                {" · buffered frames "}{captureFrames.length}
                {captureDroppedFrames > 0 ? <> · ⚠ {captureDroppedFrames} older live frames dropped after reaching the {LIVE_CAPTURE_FRAME_LIMIT.toLocaleString()}-frame in-memory limit</> : null}
              </div>
            )}

            {xmlImport && (
              <div className={styles.infoCard + " liquid-glass-card"}>
                <strong>Import Preview</strong>
                <div>{xmlImport.datasets.length} Datasets · {xmlImport.telegrams.length} Telegrams · ports {xmlImport.pd_port}/{xmlImport.md_udp_port}/{xmlImport.md_tcp_port} · SDT: {xmlImport.sdt_detected ? "Detected (not validated)" : "No configuration detected"}</div>
                {xmlImport.warnings.map(warning => <div key={warning} style={{ marginTop: 4 }}>⚠ {warning}</div>)}
                {mode === "node" && <button className={styles.actionButton + " liquid-glass-button"} style={{ marginTop: 8 }} onClick={importTemplates}>{t("trdp.actions.importTemplates")}</button>}
                <table className={styles.table} style={{ marginTop: 8 }}>
                  <thead><tr><th>Type</th><th>Telegram</th><th>ComID</th><th>Dataset</th><th>Cycle</th><th>Timeout</th><th>Sources</th><th>Destinations</th></tr></thead>
                  <tbody>
                    {xmlImport.telegrams.length === 0
                      ? <tr><td colSpan={8} className={styles.emptyState}>XML 中没有可显示的 Telegram。</td></tr>
                      : xmlImport.telegrams.map(telegram => (
                        <tr key={telegram.com_id + "-" + telegram.name}>
                          <td>{telegram.traffic_kind.toUpperCase()}</td><td>{telegram.name}</td><td>{telegram.com_id}</td><td>{telegram.dataset_id}</td><td>{telegram.cycle_us ?? "—"}</td>
                          <td>{telegram.traffic_kind === "pd" ? (telegram.timeout_us && telegram.timeout_us > 0 ? telegram.timeout_us + " µs / " + (telegram.timeout_behavior ?? "zero") : "disabled / " + (telegram.timeout_behavior ?? "zero")) : "—"}</td>
                          <td>{telegram.sources.join(", ") || "—"}</td><td>{telegram.destinations.join(", ") || "—"}</td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        )}

        {page === "publishers" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>发布 / Publishers · PD Publisher</h2>
              <button className={styles.actionButton + " liquid-primary-button"} onClick={() => addObject("pd_publisher")}>{t("trdp.actions.addPublisher")}</button>
            </div>
            <table className={styles.table}>
              <thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination</th><th>Cycle µs</th><th>Payload HEX</th><th>Protocol</th><th>State</th></tr></thead>
              <tbody>{publisherObjects.length === 0 ? <tr><td colSpan={8} className={styles.emptyState}>暂无 Publisher，请点击右上角添加。</td></tr> : publisherObjects.map(objectEditor)}</tbody>
            </table>
          </section>
        )}

        {page === "subscribers" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>订阅 / Subscribers · PD Subscriber / Request</h2>
              <button className={styles.actionButton + " liquid-primary-button"} onClick={() => addObject("pd_subscriber")}>{t("trdp.actions.addSubscriber")}</button>
              <button className={styles.actionButton + " liquid-primary-button"} onClick={() => addObject("pd_request")}>{t("trdp.actions.addPdRequest")}</button>
            </div>
            <table className={styles.table}>
              <thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Multicast/Destination</th><th>Timeout</th><th>Payload HEX</th><th>Protocol</th><th>State</th></tr></thead>
              <tbody>{subscriberObjects.length === 0 ? <tr><td colSpan={8} className={styles.emptyState}>暂无 Subscriber / PD Request，请点击右上角添加。</td></tr> : subscriberObjects.map(objectEditor)}</tbody>
            </table>
            <h3 className={styles.subheading}>Subscriber diagnostics</h3>
            <table className={styles.table}>
              <thead><tr><th>Link</th><th>ComID</th><th>Packets</th><th>Missed seq</th><th>Last seq</th><th>Interval min/avg/max µs</th><th>Avg jitter µs</th><th>Errors</th></tr></thead>
              <tbody>
                {subscriberFlows.length === 0
                  ? <tr><td colSpan={8} className={styles.emptyState}>尚未收到 PD 流量。</td></tr>
                  : subscriberFlows.map(flow => (
                    <tr key={"diag-" + flow.key}><td>{flow.link}</td><td>{flow.comId}</td><td>{flow.count}</td><td>{flow.missed}</td><td>{flow.lastSeq ?? "—"}</td>
                      <td>{flow.minIntervalUs === undefined ? "—" : Math.round(flow.minIntervalUs) + "/" + Math.round(flow.avgIntervalUs ?? 0) + "/" + Math.round(flow.maxIntervalUs ?? 0)}</td>
                      <td>{flow.jitterUs === undefined ? "—" : Math.round(flow.jitterUs)}</td><td>{flow.errors}</td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </section>
        )}

        {page === "messages" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>消息 / Messages · MD</h2>
              <button className={styles.actionButton + " liquid-primary-button"} onClick={() => addObject("md_request")}>{t("trdp.actions.addRequest")}</button>
              <button className={styles.actionButton + " liquid-primary-button"} onClick={() => addObject("md_listener")}>{t("trdp.actions.addListener")}</button>
              <button className={styles.actionButton + " liquid-primary-button"} onClick={() => addObject("md_notify")}>{t("trdp.actions.addNotify")}</button>
            </div>
            <table className={styles.table}>
              <thead><tr><th>Name</th><th>ComID</th><th>Link</th><th>Destination/Filter</th><th>UDP/TCP</th><th>Payload HEX</th><th>Protocol</th><th>Action</th></tr></thead>
              <tbody>{messageObjects.length === 0 ? <tr><td colSpan={8} className={styles.emptyState}>暂无 MD 对象，请点击右上角添加。</td></tr> : messageObjects.map(objectEditor)}</tbody>
            </table>
          </section>
        )}

        {page === "traffic" && (
          <section className={styles.section}>
            <div className={styles.sectionHeader}>
              <h2 className={styles.sectionTitle}>流量 / Traffic · TRDP Packet Inspector</h2>
              <div className={styles.toolbar}>
                <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void openCapture()}>{t("trdp.actions.openCapture")}</button>
                <button className={styles.actionButton + " liquid-glass-button"} onClick={() => void saveCapture()} disabled={captureFrames.length === 0}>{t("trdp.actions.saveCapture")}</button>
                <button className={styles.actionButton + " liquid-glass-button"} onClick={() => { setEvents([]); setSelectedPacket(null); setDecoded(null); }}>{t("trdp.actions.clear")}</button>
              </div>
            </div>

            <h3 className={styles.subheading}>Flows</h3>
            <table className={styles.table}>
              <thead><tr><th>Link</th><th>Type</th><th>ComID</th><th>Source</th><th>Destination</th><th>Packets</th><th>Missed</th><th>Seq</th><th>Rate/Interval µs</th><th>Size</th><th>Errors</th></tr></thead>
              <tbody>
                {flows.length === 0
                  ? <tr><td colSpan={11} className={styles.emptyState}>暂无流量。请打开抓包文件，或在 Monitor 模式启动实时抓包。</td></tr>
                  : flows.map(flow => <tr key={flow.key}><td>{flow.link}</td><td>{flow.msg}</td><td>{flow.comId}</td><td>{flow.src}</td><td>{flow.dst}</td><td>{flow.count}</td><td>{flow.missed}</td><td>{flow.lastSeq ?? "—"}</td><td>{flow.avgIntervalUs === undefined ? "—" : Math.round(flow.avgIntervalUs)}</td><td>{flow.size ?? "—"}</td><td>{flow.errors}</td></tr>)}
              </tbody>
            </table>

            <h3 className={styles.subheading}>Packets</h3>
            <table className={styles.table + " " + styles.monoTable}>
              <thead><tr><th>#</th><th>Link</th><th>Type</th><th>ComID</th><th>Source → Destination</th><th>Seq</th><th>Topo ETB/Op</th><th>Len</th><th>Payload</th></tr></thead>
              <tbody>
                {packetRows.length === 0
                  ? <tr><td colSpan={9} className={styles.emptyState}>暂无可检查的数据包。</td></tr>
                  : packetRows.map((event, index) => (
                    <tr key={String(event.timestamp_us ?? 0) + "-" + index} onClick={() => void inspectPacket(event)} style={{ cursor: "pointer" }}>
                      <td>{events.length - index}</td><td>{event.link ?? "—"}</td><td>{event.msg_type ?? event.kind ?? "—"}</td><td>{event.com_id ?? "—"}</td><td>{event.src_ip ?? "—"} → {event.dest_ip ?? "—"}</td><td>{event.seq_count ?? "—"}</td><td>{event.etb_topo_count ?? "—"}/{event.op_trn_topo_count ?? "—"}</td><td>{event.data_len ?? "—"}</td><td title={event.payload_hex}>{hexPreview(event.payload_hex)}</td>
                    </tr>
                  ))}
              </tbody>
            </table>

            {selectedPacket && (
              <div className={styles.packetCard + " liquid-glass-card"}>
                <h3>Packet Inspector</h3>
                <div>Protocol {selectedPacket.protocol_version ?? "—"} ({selectedPacket.protocol_valid === undefined ? "not checked" : selectedPacket.protocol_valid ? "valid" : "invalid"}) · CRC {selectedPacket.crc_valid === undefined ? "not checked" : selectedPacket.crc_valid ? "valid" : "invalid"} · Result {selectedPacket.result_code ?? "—"} · Reply status {selectedPacket.reply_status ?? "—"} · User status {selectedPacket.user_status ?? "—"} · Replies {selectedPacket.num_replies ?? observedMdReplies(selectedPacket) ?? "—"}/{selectedPacket.num_expected_replies ?? "—"}</div>
                {selectedPacket.md_session_id && <div>MD Session UUID: <code>{selectedPacket.md_session_id}</code> · Request/Reply latency {mdLatencyUs(selectedPacket) ?? "—"} µs{selectedPacket.msg_type === "Mq" && <button className={styles.compactButton + " liquid-glass-button"} style={{ marginLeft: 8 }} onClick={() => void confirmMessage(selectedPacket)}>{t("trdp.actions.confirm")} (Mc)</button>}</div>}
                {selectedPacket.md_session_id && <div>ReplyQuery {selectedPacket.num_reply_queries ?? "—"} · Confirms {selectedPacket.num_confirm_sent ?? "—"} · Confirm timeouts {selectedPacket.num_confirm_timeout ?? "—"} · Reply timeout {selectedPacket.reply_timeout_us ?? "—"} µs</div>}
                {(selectedPacket.src_uri || selectedPacket.dest_uri) && <div>URI: <code>{selectedPacket.src_uri || "—"}</code> → <code>{selectedPacket.dest_uri || "—"}</code></div>}
                <div className={styles.payload}>Raw payload: <code>{selectedPacket.payload_hex || "—"}</code></div>
                {decoded ? (
                  <>
                    <h4>{decoded.dataset_name} · Dataset {decoded.dataset_id}</h4>
                    <div>{decoded.consumed_bytes}/{decoded.payload_bytes} bytes decoded</div>
                    <table className={styles.table}>
                      <thead><tr><th>Field</th><th>Type</th><th>Value</th><th>Unit</th></tr></thead>
                      <tbody>{Object.entries(decoded.fields).map(([name, field]) => <tr key={name}><td>{name}</td><td>{field.type}</td><td>{field.error ?? displayValue(field.value)}</td><td>{field.unit ?? "—"}</td></tr>)}</tbody>
                    </table>
                  </>
                ) : xmlImport && selectedPacket.com_id !== undefined
                  ? <div>No dataset mapping/decodable payload for ComID {selectedPacket.com_id}.</div>
                  : <div>Import a TRDP XML file to enable Dataset decoding.</div>}
              </div>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
