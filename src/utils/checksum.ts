import { parseByteInput, bytesToHex } from "./byteInput.ts";

export type Crc8Preset =
  | "CRC-8"
  | "CRC-8/MAXIM-DOW"
  | "CRC-8/I-432-1"
  | "CRC-8/ROHC";

export type Crc16Preset =
  | "CRC-16/MODBUS"
  | "CRC-16/XMODEM"
  | "CRC-16/IBM-3740"
  | "CRC-16/USB";

export type Crc32Preset =
  | "CRC-32/ISO-HDLC"
  | "CRC-32/MPEG-2"
  | "CRC-32/BZIP2"
  | "CRC-32/CKSUM";

export type CrcPreset = Crc8Preset | Crc16Preset | Crc32Preset;
export type CrcWidth = 8 | 16 | 32;

export interface CrcParams {
  poly: number;
  init: number;
  refIn: boolean;
  refOut: boolean;
  xorOut: number;
  width: CrcWidth;
}

export interface CrcPresetDefinition {
  params: CrcParams;
  check: number;
}

export const CRC_PRESETS: Record<CrcPreset, CrcPresetDefinition> = {
  "CRC-8": {
    params: { poly: 0x07, init: 0x00, refIn: false, refOut: false, xorOut: 0x00, width: 8 },
    check: 0xF4,
  },
  "CRC-8/MAXIM-DOW": {
    params: { poly: 0x31, init: 0x00, refIn: true, refOut: true, xorOut: 0x00, width: 8 },
    check: 0xA1,
  },
  "CRC-8/I-432-1": {
    params: { poly: 0x07, init: 0x00, refIn: false, refOut: false, xorOut: 0x55, width: 8 },
    check: 0xA1,
  },
  "CRC-8/ROHC": {
    params: { poly: 0x07, init: 0xFF, refIn: true, refOut: true, xorOut: 0x00, width: 8 },
    check: 0xD0,
  },
  "CRC-16/MODBUS": {
    params: { poly: 0x8005, init: 0xFFFF, refIn: true, refOut: true, xorOut: 0x0000, width: 16 },
    check: 0x4B37,
  },
  "CRC-16/XMODEM": {
    params: { poly: 0x1021, init: 0x0000, refIn: false, refOut: false, xorOut: 0x0000, width: 16 },
    check: 0x31C3,
  },
  "CRC-16/IBM-3740": {
    params: { poly: 0x1021, init: 0xFFFF, refIn: false, refOut: false, xorOut: 0x0000, width: 16 },
    check: 0x29B1,
  },
  "CRC-16/USB": {
    params: { poly: 0x8005, init: 0xFFFF, refIn: true, refOut: true, xorOut: 0xFFFF, width: 16 },
    check: 0xB4C8,
  },
  "CRC-32/ISO-HDLC": {
    params: { poly: 0x04C11DB7, init: 0xFFFFFFFF, refIn: true, refOut: true, xorOut: 0xFFFFFFFF, width: 32 },
    check: 0xCBF43926,
  },
  "CRC-32/MPEG-2": {
    params: { poly: 0x04C11DB7, init: 0xFFFFFFFF, refIn: false, refOut: false, xorOut: 0x00000000, width: 32 },
    check: 0x0376E6E7,
  },
  "CRC-32/BZIP2": {
    params: { poly: 0x04C11DB7, init: 0xFFFFFFFF, refIn: false, refOut: false, xorOut: 0xFFFFFFFF, width: 32 },
    check: 0xFC891918,
  },
  "CRC-32/CKSUM": {
    params: { poly: 0x04C11DB7, init: 0x00000000, refIn: false, refOut: false, xorOut: 0xFFFFFFFF, width: 32 },
    check: 0x765E7680,
  },
};

export const CRC8_PRESETS = Object.fromEntries(
  (Object.entries(CRC_PRESETS) as [CrcPreset, CrcPresetDefinition][])
    .filter(([, definition]) => definition.params.width === 8)
    .map(([name, definition]) => [name, definition.params]),
) as Record<Crc8Preset, CrcParams>;

export const CRC16_PRESETS = Object.fromEntries(
  (Object.entries(CRC_PRESETS) as [CrcPreset, CrcPresetDefinition][])
    .filter(([, definition]) => definition.params.width === 16)
    .map(([name, definition]) => [name, definition.params]),
) as Record<Crc16Preset, CrcParams>;

export const CRC32_PRESETS = Object.fromEntries(
  (Object.entries(CRC_PRESETS) as [CrcPreset, CrcPresetDefinition][])
    .filter(([, definition]) => definition.params.width === 32)
    .map(([name, definition]) => [name, definition.params]),
) as Record<Crc32Preset, CrcParams>;

function reflect(value: number, width: CrcWidth): number {
  let source = value >>> 0;
  let result = 0;
  for (let i = 0; i < width; i += 1) {
    result = (result << 1) | (source & 1);
    source >>>= 1;
  }
  return width === 32 ? result >>> 0 : result & ((1 << width) - 1);
}

export function crcWithParams(data: Uint8Array, params: CrcParams): number {
  const { poly, init, refIn, refOut, xorOut, width } = params;
  const mask = width === 8 ? 0xFF : width === 16 ? 0xFFFF : 0xFFFFFFFF;
  const topBit = width === 8 ? 0x80 : width === 16 ? 0x8000 : 0x80000000;

  let crc = width === 32 ? init >>> 0 : init & mask;
  const polynomial = width === 32 ? poly >>> 0 : poly & mask;

  for (let byte of data) {
    if (refIn) byte = reflect(byte, 8);
    if (width === 8) crc ^= byte;
    else if (width === 16) crc ^= byte << 8;
    else crc = (crc ^ (byte << 24)) >>> 0;

    for (let bit = 0; bit < 8; bit += 1) {
      const hasTopBit = (crc & topBit) !== 0;
      if (width === 32) {
        crc = ((crc << 1) >>> 0);
        if (hasTopBit) crc = (crc ^ polynomial) >>> 0;
      } else {
        crc = (crc << 1) & mask;
        if (hasTopBit) crc = (crc ^ polynomial) & mask;
      }
    }
  }

  if (refOut) crc = reflect(crc, width);
  const finalValue = crc ^ xorOut;
  return width === 32 ? finalValue >>> 0 : finalValue & mask;
}

export function crcPreset(data: Uint8Array, preset: CrcPreset): number {
  return crcWithParams(data, CRC_PRESETS[preset].params);
}

export function crc8(
  data: Uint8Array,
  preset: Crc8Preset = "CRC-8",
  custom?: Partial<CrcParams>,
): number {
  return crcWithParams(data, { ...CRC8_PRESETS[preset], ...custom, width: 8 });
}

export function crc16(
  data: Uint8Array,
  preset: Crc16Preset = "CRC-16/MODBUS",
  custom?: Partial<CrcParams>,
): number {
  return crcWithParams(data, { ...CRC16_PRESETS[preset], ...custom, width: 16 });
}

export function crc32(
  data: Uint8Array,
  preset: Crc32Preset = "CRC-32/ISO-HDLC",
  custom?: Partial<CrcParams>,
): number {
  return crcWithParams(data, { ...CRC32_PRESETS[preset], ...custom, width: 32 });
}

export function checksum8(data: Uint8Array): number {
  let sum = 0;
  for (const byte of data) sum += byte;
  return sum & 0xFF;
}

export function checksum16(data: Uint8Array): number {
  let sum = 0;
  for (const byte of data) sum += byte;
  return sum & 0xFFFF;
}

export function checksum32(data: Uint8Array): number {
  let sum = 0;
  for (const byte of data) sum = (sum + byte) >>> 0;
  return sum >>> 0;
}

export function xorChecksum(data: Uint8Array): number {
  let result = 0;
  for (const byte of data) result ^= byte;
  return result & 0xFF;
}

export function stringToBytes(value: string): Uint8Array {
  return new TextEncoder().encode(value);
}

/** Compatibility facade for older callers. New code should consume parseByteInput directly. */
export function parseHexString(input: string): Uint8Array {
  const parsed = parseByteInput(input);
  return parsed.ok ? parsed.value.bytes : new Uint8Array(0);
}

export { bytesToHex };

export function numberToHex(value: number, width: CrcWidth): string {
  const digits = width / 4;
  const normalized = width === 32 ? value >>> 0 : value & (2 ** width - 1);
  return normalized.toString(16).toUpperCase().padStart(digits, "0");
}

export function crcValueBytes(value: number, width: CrcWidth, byteOrder: "be" | "le"): Uint8Array {
  const byteCount = width / 8;
  const bytes = new Uint8Array(byteCount);
  for (let index = 0; index < byteCount; index += 1) {
    const shift = (byteOrder === "be" ? byteCount - 1 - index : index) * 8;
    bytes[index] = (value >>> shift) & 0xFF;
  }
  return bytes;
}
