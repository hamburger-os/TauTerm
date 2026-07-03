// ── 校验和工具函数 ──────────────────────────────────────────────
// 纯 TypeScript 实现，无外部依赖。用于 ChecksumTool 计算。

// ══════════════════════════════════════════════════════════════════
// 类型定义
// ══════════════════════════════════════════════════════════════════

export type Crc8Preset = "CRC8-Basic" | "CRC8-MAXIM" | "CRC8-ITU" | "CRC8-ROHC";
export type Crc16Preset = "CRC16-Modbus" | "CRC16-CCITT" | "CRC16-XMODEM" | "CRC16-USB";
export type Crc32Preset = "CRC32" | "CRC32-MPEG2" | "CRC32-BZIP2" | "CRC32-CKSUM";

export interface CrcParams {
  poly: number;
  init: number;
  refIn: boolean;
  refOut: boolean;
  xorOut: number;
  width: number; // 8 | 16 | 32
}

// ══════════════════════════════════════════════════════════════════
// CRC 预设参数表
// ══════════════════════════════════════════════════════════════════

export const CRC8_PRESETS: Record<Crc8Preset, CrcParams> = {
  "CRC8-Basic": { poly: 0x07, init: 0x00, refIn: false, refOut: false, xorOut: 0x00, width: 8 },
  "CRC8-MAXIM": { poly: 0x31, init: 0x00, refIn: true,  refOut: true,  xorOut: 0x00, width: 8 },
  "CRC8-ITU":   { poly: 0x07, init: 0x00, refIn: false, refOut: false, xorOut: 0x55, width: 8 },
  "CRC8-ROHC":  { poly: 0x07, init: 0xFF, refIn: true,  refOut: true,  xorOut: 0x00, width: 8 },
};

export const CRC16_PRESETS: Record<Crc16Preset, CrcParams> = {
  "CRC16-Modbus":  { poly: 0x8005, init: 0xFFFF, refIn: true,  refOut: true,  xorOut: 0x0000, width: 16 },
  "CRC16-CCITT":   { poly: 0x1021, init: 0x0000, refIn: false, refOut: false, xorOut: 0x0000, width: 16 },
  "CRC16-XMODEM":  { poly: 0x1021, init: 0x0000, refIn: false, refOut: false, xorOut: 0x0000, width: 16 },
  "CRC16-USB":     { poly: 0x8005, init: 0xFFFF, refIn: true,  refOut: true,  xorOut: 0xFFFF, width: 16 },
};

export const CRC32_PRESETS: Record<Crc32Preset, CrcParams> = {
  "CRC32":        { poly: 0x04C11DB7, init: 0xFFFFFFFF, refIn: true,  refOut: true,  xorOut: 0xFFFFFFFF, width: 32 },
  "CRC32-MPEG2":  { poly: 0x04C11DB7, init: 0xFFFFFFFF, refIn: false, refOut: false, xorOut: 0x00000000, width: 32 },
  "CRC32-BZIP2":  { poly: 0x04C11DB7, init: 0xFFFFFFFF, refIn: false, refOut: false, xorOut: 0xFFFFFFFF, width: 32 },
  "CRC32-CKSUM":  { poly: 0x04C11DB7, init: 0x00000000, refIn: false, refOut: false, xorOut: 0xFFFFFFFF, width: 32 },
};

// ══════════════════════════════════════════════════════════════════
// CRC 通用计算
// ══════════════════════════════════════════════════════════════════

/** 单字节位反转 */
function reflect8(val: number): number {
  let r = 0;
  for (let i = 0; i < 8; i++) {
    r = (r << 1) | (val & 1);
    val >>= 1;
  }
  return r & 0xFF;
}

function reflect16(val: number): number {
  let r = 0;
  for (let i = 0; i < 16; i++) {
    r = (r << 1) | (val & 1);
    val >>= 1;
  }
  return r & 0xFFFF;
}

function reflect32(val: number): number {
  // JavaScript 位运算限制为 32 位有符号整数；用无符号右移保持正值
  let r = 0;
  for (let i = 0; i < 32; i++) {
    r = (r << 1) | (val & 1);
    val >>>= 1;
  }
  return r >>> 0;
}

/** 通用 CRC 计算（按字节处理） */
function crcCompute(data: Uint8Array, params: CrcParams): number {
  const { poly, init, refIn, refOut, xorOut, width } = params;
  const mask = width === 8 ? 0xFF : width === 16 ? 0xFFFF : 0xFFFFFFFF;

  let crc = init & mask;
  const polyMasked = poly & mask;

  for (let byte of data) {
    if (refIn) {
      byte = reflect8(byte);
    }

    if (width <= 8) {
      // CRC8: XOR-in then bit-by-bit
      crc ^= byte;
      for (let i = 0; i < 8; i++) {
        if (crc & 0x80) {
          crc = ((crc << 1) ^ polyMasked) & 0xFF;
        } else {
          crc = (crc << 1) & 0xFF;
        }
      }
    } else if (width <= 16) {
      // CRC16: XOR byte as top bits
      crc ^= (byte << 8);
      for (let i = 0; i < 8; i++) {
        if (crc & 0x8000) {
          crc = ((crc << 1) ^ polyMasked) & 0xFFFF;
        } else {
          crc = (crc << 1) & 0xFFFF;
        }
      }
    } else {
      // CRC32: XOR byte as top bits
      crc ^= (byte << 24);
      for (let i = 0; i < 8; i++) {
        if (crc & 0x80000000) {
          crc = ((crc << 1) ^ polyMasked) >>> 0;
        } else {
          crc = (crc << 1) >>> 0;
        }
      }
    }
  }

  if (refOut) {
    if (width === 8) crc = reflect8(crc);
    else if (width === 16) crc = reflect16(crc);
    else crc = reflect32(crc);
  }

  return (crc ^ xorOut) & mask;
}

// ══════════════════════════════════════════════════════════════════
// 公共 CRC API
// ══════════════════════════════════════════════════════════════════

/** CRC8 计算（可使用预设或自定义参数） */
export function crc8(data: Uint8Array, preset?: Crc8Preset, custom?: Partial<CrcParams>): number {
  const base = preset ? CRC8_PRESETS[preset] : CRC8_PRESETS["CRC8-Basic"];
  const params: CrcParams = { ...base, ...custom, width: 8 };
  return crcCompute(data, params);
}

/** CRC16 计算 */
export function crc16(data: Uint8Array, preset?: Crc16Preset, custom?: Partial<CrcParams>): number {
  const base = preset ? CRC16_PRESETS[preset] : CRC16_PRESETS["CRC16-Modbus"];
  const params: CrcParams = { ...base, ...custom, width: 16 };
  return crcCompute(data, params);
}

/** CRC32 计算 */
export function crc32(data: Uint8Array, preset?: Crc32Preset, custom?: Partial<CrcParams>): number {
  const base = preset ? CRC32_PRESETS[preset] : CRC32_PRESETS["CRC32"];
  const params: CrcParams = { ...base, ...custom, width: 32 };
  return crcCompute(data, params);
}

// ══════════════════════════════════════════════════════════════════
// 累加和 (CheckSum)
// ══════════════════════════════════════════════════════════════════

/** 8 位累加和（低8位） */
export function checksum8(data: Uint8Array): number {
  let sum = 0;
  for (const b of data) sum += b;
  return sum & 0xFF;
}

/** 16 位累加和 */
export function checksum16(data: Uint8Array): number {
  let sum = 0;
  for (const b of data) sum += b;
  return sum & 0xFFFF;
}

/** 32 位累加和 */
export function checksum32(data: Uint8Array): number {
  let sum = 0;
  for (const b of data) sum += b;
  return sum >>> 0;
}

// ══════════════════════════════════════════════════════════════════
// 异或校验
// ══════════════════════════════════════════════════════════════════

/** 异或校验（所有字节异或） */
export function xorChecksum(data: Uint8Array): number {
  let result = 0;
  for (const b of data) result ^= b;
  return result;
}

// ══════════════════════════════════════════════════════════════════
// 辅助：输入解析
// ══════════════════════════════════════════════════════════════════

/** 将字符串转为字节数组（使用 TextEncoder） */
export function stringToBytes(str: string): Uint8Array {
  return new TextEncoder().encode(str);
}

/**
 * 解析 HEX 字符串为字节数组。
 * 支持多种格式：
 *   - "AA BB CC" (空格分隔)
 *   - "0xAA,0xBB,0xCC" (逗号 + 0x 前缀)
 *   - "AABBCC" (无分隔，偶数长度)
 *   - "AA, BB, CC" (逗号 + 空格)
 */
export function parseHexString(hex: string): Uint8Array {
  // 统一清理：去空白、去 0x 前缀
  let cleaned = hex.replace(/\s+/g, "").replace(/0x/gi, "").replace(/,/g, "");

  // 如果长度为奇数，视为非法或在前补0
  if (cleaned.length % 2 !== 0) {
    return new Uint8Array(0);
  }

  const bytes: number[] = [];
  for (let i = 0; i < cleaned.length; i += 2) {
    const byte = parseInt(cleaned.substring(i, i + 2), 16);
    if (isNaN(byte)) return new Uint8Array(0);
    bytes.push(byte);
  }
  return new Uint8Array(bytes);
}

/** 将字节数组格式化为 HEX 字符串（大写，空格分隔） */
export function bytesToHex(bytes: Uint8Array, separator = " "): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).toUpperCase().padStart(2, "0"))
    .join(separator);
}

/** 将数字格式化为指定位宽的 HEX 字符串 */
export function numberToHex(value: number, width: number): string {
  const hexLen = Math.ceil(width / 4);
  // 使用 Math.pow 避免 JS 位运算限制：width=32 时 (1<<32) 溢出为 1，导致掩码为 0
  const v = value & (Math.pow(2, width) - 1);
  return v.toString(16).toUpperCase().padStart(hexLen, "0");
}
