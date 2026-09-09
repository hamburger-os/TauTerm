import { parseByteInput } from "./byteInput";
import { toolErr, toolOk, type ToolResult } from "./toolResult";

export interface SerialTimingResult {
  bitsPerFrame: number;
  microsecondsPerByte: number;
  millisecondsTotal: number;
}

export function serialTiming(
  baudRate: number,
  dataBits: 5 | 6 | 7 | 8,
  parity: "none" | "odd" | "even" | "mark" | "space",
  stopBits: 1 | 1.5 | 2,
  byteCount: number,
): ToolResult<SerialTimingResult> {
  if (!Number.isFinite(baudRate) || baudRate <= 0) return toolErr("invalidBaudRate");
  if (!Number.isFinite(byteCount) || byteCount < 0) return toolErr("invalidByteCount");
  const parityBits = parity === "none" ? 0 : 1;
  const bitsPerFrame = 1 + dataBits + parityBits + stopBits;
  const secondsPerByte = bitsPerFrame / baudRate;
  return toolOk({
    bitsPerFrame,
    microsecondsPerByte: secondsPerByte * 1_000_000,
    millisecondsTotal: secondsPerByte * byteCount * 1000,
  });
}

function parseIpv4(ip: string): number | null {
  const parts = ip.trim().split(".");
  if (parts.length !== 4) return null;
  let value = 0;
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part)) return null;
    const octet = Number(part);
    if (octet < 0 || octet > 255) return null;
    value = (value * 256 + octet) >>> 0;
  }
  return value >>> 0;
}

function formatIpv4(value: number): string {
  const unsigned = value >>> 0;
  return [
    (unsigned >>> 24) & 0xFF,
    (unsigned >>> 16) & 0xFF,
    (unsigned >>> 8) & 0xFF,
    unsigned & 0xFF,
  ].join(".");
}

export interface SubnetInfo {
  network: string;
  broadcast: string;
  firstHost: string;
  lastHost: string;
  mask: string;
  hostCount: string;
}

export function subnetInfo(
  ip: string,
  prefix: number,
): ToolResult<SubnetInfo> {
  const address = parseIpv4(ip);
  if (address === null) return toolErr("invalidIpv4");
  if (!Number.isInteger(prefix) || prefix < 0 || prefix > 32) {
    return toolErr("invalidPrefix");
  }

  const mask = prefix === 0 ? 0 : (0xFFFFFFFF << (32 - prefix)) >>> 0;
  const network = (address & mask) >>> 0;
  const broadcast = (network | (~mask >>> 0)) >>> 0;
  const total = 1n << BigInt(32 - prefix);
  const usable = prefix >= 31 ? total : total - 2n;
  const first = prefix >= 31 ? network : (network + 1) >>> 0;
  const last = prefix >= 31 ? broadcast : (broadcast - 1) >>> 0;

  return toolOk({
    network: formatIpv4(network),
    broadcast: formatIpv4(broadcast),
    firstHost: formatIpv4(first),
    lastHost: formatIpv4(last),
    mask: formatIpv4(mask),
    hostCount: usable.toString(10),
  });
}

export interface TimestampInfo {
  milliseconds: number;
  iso: string;
  utc: string;
  local: string;
}

export function timestampInfo(
  input: string,
  unit: "seconds" | "milliseconds" | "hex-seconds" | "hex-milliseconds",
): ToolResult<TimestampInfo> {
  const source = input.trim();
  if (!source) return toolErr("emptyInput");
  let raw: bigint;
  try {
    if (unit.startsWith("hex-")) {
      if (!/^(?:0[xX])?[0-9a-fA-F]+$/.test(source)) return toolErr("invalidTimestamp");
      raw = BigInt(source.toLowerCase().startsWith("0x") ? source : "0x" + source);
    } else {
      if (!/^[+-]?\d+$/.test(source)) return toolErr("invalidTimestamp");
      raw = BigInt(source);
    }
  } catch {
    return toolErr("invalidTimestamp");
  }

  const milliseconds = unit.endsWith("seconds") && !unit.endsWith("milliseconds")
    ? raw * 1000n
    : raw;
  if (
    milliseconds < BigInt(Number.MIN_SAFE_INTEGER)
    || milliseconds > BigInt(Number.MAX_SAFE_INTEGER)
  ) return toolErr("timestampOutOfRange");

  const numeric = Number(milliseconds);
  const date = new Date(numeric);
  if (Number.isNaN(date.getTime())) return toolErr("timestampOutOfRange");

  return toolOk({
    milliseconds: numeric,
    iso: date.toISOString(),
    utc: date.toUTCString(),
    local: date.toString(),
  });
}

export interface ByteDiffEntry {
  offset: number;
  left?: number;
  right?: number;
  xor?: number;
}

export interface ByteDiffResult {
  leftLength: number;
  rightLength: number;
  differences: ByteDiffEntry[];
}

export function diffBytes(
  left: string,
  right: string,
): ToolResult<ByteDiffResult> {
  const a = parseByteInput(left);
  if (!a.ok) return toolErr(a.error.code, "left: " + (a.error.detail ?? ""));
  const b = parseByteInput(right);
  if (!b.ok) return toolErr(b.error.code, "right: " + (b.error.detail ?? ""));

  const count = Math.max(a.value.bytes.length, b.value.bytes.length);
  const differences: ByteDiffEntry[] = [];
  for (let offset = 0; offset < count; offset += 1) {
    const leftByte = a.value.bytes[offset];
    const rightByte = b.value.bytes[offset];
    if (leftByte !== rightByte) {
      differences.push({
        offset,
        ...(leftByte !== undefined ? { left: leftByte } : {}),
        ...(rightByte !== undefined ? { right: rightByte } : {}),
        ...(leftByte !== undefined && rightByte !== undefined
          ? { xor: leftByte ^ rightByte }
          : {}),
      });
    }
  }

  return toolOk({
    leftLength: a.value.bytes.length,
    rightLength: b.value.bytes.length,
    differences,
  });
}
