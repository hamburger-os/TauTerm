import type { DualLine } from "../../components/Terminal/DualPane";
import { dataToDualLine, normalizeDecodedText } from "../../utils/streamDisplay";

export interface PacketRow {
  id: number;
  time: string;
  direction: "RX" | "TX";
  /** 来源地址（RX）或目标地址（TX） */
  peerLabel: string;
  length: number;
  hex: string;
  text: string;
}

export interface NetworkPresentation {
  transport: "tcp" | "udp";
  role: "client" | "server";
  /** 会话字符编码（文本栏解码；连接时确定） */
  encoding: string;
  /** TCP 对端帧（DualLine 列表，按对端隔离） */
  frames: Record<string, DualLine[]>;
  /** UDP 逐数据报网格（会话级时间线） */
  packets: PacketRow[];
  /** UDP 会话级累计 RX 字节（无对端模型，直接驱动状态栏） */
  rxBytes: number;
  /** UDP 会话级累计 TX 字节（无对端模型，直接驱动状态栏） */
  txBytes: number;
  /** UDP 会话级累计 RX 报文数（驱动状态栏报文计数） */
  rxPackets: number;
  /** UDP 会话级累计 TX 报文数 */
  txPackets: number;
}


const FRAME_BUFFER_LINES = 2000;
const PACKET_BUFFER_ROWS = 2000;

export function nowTime(): string {
  const d = new Date();
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

export function decodeText(bytes: Uint8Array, encoding: string, stream = false): string {
  try {
    return new TextDecoder(encoding).decode(bytes, stream ? { stream: true } : undefined);
  } catch {
    return new TextDecoder("utf-8").decode(bytes);
  }
}

/** 追加一帧到对端帧列表（读取最新 state 计算 id，避免同快照连续调用产生重复 key） */
export function appendFrame(
  fresh: NetworkPresentation,
  peerId: string,
  direction: "RX" | "TX",
  data: Uint8Array,
  encoding: string,
  decodedOverride?: string,
) {
  if (data.length === 0) return {};
  const decoded = decodedOverride !== undefined
    ? decodedOverride
    : decodeText(data, encoding, direction === "RX");
  const base = { ...dataToDualLine(data, direction), text: normalizeDecodedText(decoded) };
  if (base.text.length === 0 && base.hex.length === 0) return {};
  const prev = fresh.frames[peerId] ?? [];
  const id = prev.length > 0 ? prev[prev.length - 1].id + 1 : 1;
  const list = [...prev, { ...base, id }];
  if (list.length > FRAME_BUFFER_LINES) list.splice(0, list.length - FRAME_BUFFER_LINES);
  return { frames: { ...fresh.frames, [peerId]: list } };
}

/** 追加一个 UDP 数据报行（读取最新 state 计算 id，保证会话级时间线 id 单调唯一） */
export function appendPacket(
  fresh: NetworkPresentation,
  row: Omit<PacketRow, "id">,
) {
  const prev = fresh.packets;
  const id = prev.length > 0 ? prev[prev.length - 1].id + 1 : 1;
  const list = [...prev, { ...row, id }];
  if (list.length > PACKET_BUFFER_ROWS) list.splice(0, list.length - PACKET_BUFFER_ROWS);
  return {
    packets: list,
    rxBytes: fresh.rxBytes + (row.direction === "RX" ? row.length : 0),
    txBytes: fresh.txBytes + (row.direction === "TX" ? row.length : 0),
    rxPackets: fresh.rxPackets + (row.direction === "RX" ? 1 : 0),
    txPackets: fresh.txPackets + (row.direction === "TX" ? 1 : 0),
  };
}

export function createNetworkPresentation(
  transport: "tcp" | "udp" = "tcp",
  role: "client" | "server" = "client",
  encoding = "utf-8",
): NetworkPresentation {
  return {
    transport,
    role,
    encoding,
    frames: {},
    packets: [],
    rxBytes: 0,
    txBytes: 0,
    rxPackets: 0,
    txPackets: 0,
  };
}


export function bytesToHex(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i++) {
    if (i > 0) parts.push(" ");
    if (i > 0 && i % 8 === 0) parts.push(" ");
    parts.push(bytes[i].toString(16).padStart(2, "0"));
  }
  return parts.join("");
}
