export type Page = "overview" | "pd" | "md" | "analysis";
export type LinkChoice = "a" | "b" | "both";
export type RedundancyState = "leader" | "follower";
export type CaptureInterface = { name: string; description: string };
export type ObjectKind =
  | "pd_publisher"
  | "pd_subscriber"
  | "pd_request"
  | "md_request"
  | "md_listener"
  | "md_notify";

export type TrdpEvent = {
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
  capture_id?: string;
  frame_count?: number;
  packet_count?: number;
  dropped_frames?: number;
  error?: string;
  state?: TrdpObject["state"];
};

export type TrdpObject = {
  id: string;
  kind: ObjectKind;
  name: string;
  comId: number;
  link: LinkChoice;
  state: "stopped" | "starting" | "running" | "stopping" | "sending" | "error";
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
  numReplies: number;
  replyTimeoutUs: number;
  responseMode: "reply" | "query";
  confirmTimeoutUs: number;
  replyComId: number;
  replyIp: string;
  sourceUri: string;
  destUri: string;
};

export type XmlElement = {
  name: string;
  data_type: string;
  type_id: number;
  array_size: number;
  dynamic: boolean;
  unit?: string;
  scale?: number;
  offset?: number;
};
export type XmlDataset = { id: number; name: string; elements: XmlElement[] };
export type XmlTelegram = {
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
export type XmlImport = {
  path: string;
  datasets: XmlDataset[];
  telegrams: XmlTelegram[];
  pd_port: number;
  md_udp_port: number;
  md_tcp_port: number;
  sdt_detected: boolean;
  warnings: string[];
};

export type DecodedField = {
  type: string;
  type_id?: number;
  unit?: string;
  raw?: unknown;
  value?: unknown;
  error?: string;
};
export type DecodedDataset = {
  dataset_id: number;
  dataset_name: string;
  consumed_bytes: number;
  payload_bytes: number;
  fields: Record<string, DecodedField>;
};
export type Workspace = {
  format: "tauterm-trdp-workspace/v2";
  name?: string;
  xml?: string;
  xml_path?: string;
  objects: Array<Record<string, unknown>>;
  redundancy_groups?: Record<string, RedundancyState>;
};
export type WorkspaceDraft = {
  format: "tauterm-trdp-draft/v2";
  objects: TrdpObject[];
  redundancyGroups: Record<string, RedundancyState>;
};


function workspaceWireObject(object: TrdpObject): Record<string, unknown> {
  return {
    id: object.id,
    kind: object.kind,
    name: object.name,
    com_id: object.comId,
    link: object.link,
    destination: object.destination,
    source: object.source,
    cycle_us: object.cycleUs,
    timeout_mode: object.timeoutMode,
    timeout_us: object.timeoutUs,
    timeout_behavior: object.timeoutBehavior,
    payload_hex: object.payloadHex,
    transport: object.transport,
    etb_topo_count: object.etbTopoCount,
    op_trn_topo_count: object.opTrnTopoCount,
    red_id: object.redId,
    num_replies: object.numReplies,
    reply_timeout_us: object.replyTimeoutUs,
    response_mode: object.responseMode,
    confirm_timeout_us: object.confirmTimeoutUs,
    reply_com_id: object.replyComId,
    reply_ip: object.replyIp,
    source_uri: object.sourceUri,
    dest_uri: object.destUri,
  };
}

export function workspaceDraftFromWorkspace(workspace: Workspace | null | undefined): WorkspaceDraft {
  if (!workspace || workspace.format !== "tauterm-trdp-workspace/v2" || !Array.isArray(workspace.objects)) {
    return { format: "tauterm-trdp-draft/v2", objects: [], redundancyGroups: {} };
  }
  return {
    format: "tauterm-trdp-draft/v2",
    objects: workspace.objects
      .map(workspaceObject)
      .filter((item): item is TrdpObject => item !== null)
      .map(item => ({ ...item, state: "stopped" })),
    redundancyGroups: workspace.redundancy_groups ?? {},
  };
}

export function workspaceFromDraft(
  draft: WorkspaceDraft,
  name?: string,
  xml?: string,
): Workspace {
  return {
    format: "tauterm-trdp-workspace/v2",
    ...(name ? { name } : {}),
    ...(xml ? { xml } : {}),
    objects: draft.objects.map(workspaceWireObject),
    redundancy_groups: { ...draft.redundancyGroups },
  };
}
export type EncodedDataset = {
  dataset_id: number;
  payload_bytes: number;
  payload_hex: string;
};
export type CaptureResult = {
  capture_id: string;
  frame_count: number;
  packet_count: number;
  dropped_frames: number;
  packets: TrdpEvent[];
};
export type RuntimeState = {
  objects: Record<string, TrdpObject["state"]>;
};
export type StructuredEditor = {
  objectId: string;
  datasetId: number;
  drafts: Record<string, string>;
};
export type FlowRow = {
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

const U32 = 0x1_0000_0000;
export const STANDARD_CAPTURE_FILTER = "udp port 17224 or udp port 17225 or tcp port 17225";
export const LIVE_CAPTURE_FRAME_LIMIT = 50_000;

export function captureFilterForPorts(pdPort: number, mdUdpPort: number, mdTcpPort: number) {
  return `udp port ${pdPort} or udp port ${mdUdpPort} or tcp port ${mdTcpPort}`;
}

export function paramNumber(
  params: Record<string, unknown> | undefined,
  key: string,
  fallback: number,
) {
  const value = params?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function isIpv4Text(value: string) {
  const parts = value.split(".");
  return parts.length === 4 && parts.every(part => {
    if (!/^\d{1,3}$/.test(part)) return false;
    const octet = Number(part);
    return octet >= 0 && octet <= 255;
  });
}

function isKind(value: unknown): value is ObjectKind {
  return typeof value === "string"
    && ["pd_publisher", "pd_subscriber", "pd_request", "md_request", "md_listener", "md_notify"].includes(value);
}

export function isOneShotKind(kind: ObjectKind) {
  return kind === "pd_request" || kind === "md_request" || kind === "md_notify";
}

export function createObject(kind: ObjectKind, index: number): TrdpObject {
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

export function workspaceObject(
  raw: Record<string, unknown>,
  index: number,
): TrdpObject | null {
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

export function displayValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (value === null || value === undefined) return "—";
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

export function missedBetween(previous: number, current: number) {
  const distance = (current - previous + U32) % U32;
  return distance > 1 && distance < 0x8000_0000 ? distance - 1 : 0;
}

export function defaultDatasetValues(
  imported: XmlImport,
  datasetId: number,
  visiting = new Set<number>(),
): Record<string, unknown> {
  if (visiting.has(datasetId)) return {};
  const dataset = imported.datasets.find(item => item.id === datasetId);
  if (!dataset) return {};
  const nextVisiting = new Set(visiting);
  nextVisiting.add(datasetId);
  const result: Record<string, unknown> = {};
  for (const element of dataset.elements) {
    const singleValue = () => {
      if (element.type_id >= 1000) {
        return defaultDatasetValues(imported, element.type_id, nextVisiting);
      }
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

export function draftsFromValues(values: Record<string, unknown>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(values).map(([name, value]) => [name, JSON.stringify(value)]),
  );
}

export function draftsFromDecoded(decoded: DecodedDataset): Record<string, string> {
  return Object.fromEntries(
    Object.entries(decoded.fields)
      .map(([name, field]) => [name, JSON.stringify(field.value ?? field.raw ?? null)]),
  );
}
