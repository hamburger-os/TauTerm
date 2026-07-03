// ── 编码与数值转换工具函数 ──────────────────────────────────────

// ══════════════════════════════════════════════════════════════════
// Base64 编解码
// ══════════════════════════════════════════════════════════════════

export function base64Encode(input: string): string {
  if (!input) return "";
  try {
    const bytes = new TextEncoder().encode(input);
    const binary = Array.from(bytes, (b) => String.fromCharCode(b)).join("");
    return btoa(binary);
  } catch {
    try {
      return btoa(input);
    } catch {
      return `[Error: Base64 encode failed - input contains unsupported characters]`;
    }
  }
}

export function base64Decode(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return "";

  // 格式校验：合法 base64 仅包含 A-Za-z0-9+/=
  if (!/^[A-Za-z0-9+/=]+$/.test(trimmed)) {
    return `[Error: Invalid base64 input - contains illegal characters]`;
  }

  try {
    const bytes = Uint8Array.from(atob(trimmed), (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    try {
      return atob(trimmed);
    } catch {
      return `[Error: Base64 decode failed]`;
    }
  }
}

// ══════════════════════════════════════════════════════════════════
// URL 编解码
// ══════════════════════════════════════════════════════════════════

export function urlEncode(input: string): string {
  return encodeURIComponent(input);
}

export function urlDecode(input: string): string {
  try {
    return decodeURIComponent(input);
  } catch {
    return `[Error: Invalid URL-encoded input]`;
  }
}

// ══════════════════════════════════════════════════════════════════
// HEX ↔ 字符串
// ══════════════════════════════════════════════════════════════════

/** 字符串 → HEX（每字节两位大写HEX） */
export function stringToHex(str: string): string {
  const bytes = new TextEncoder().encode(str);
  return Array.from(bytes)
    .map((b) => b.toString(16).toUpperCase().padStart(2, "0"))
    .join(" ");
}

/** HEX → 字符串（支持空格/逗号/0x分隔） */
export function hexToString(hex: string): string {
  const cleaned = hex.replace(/\s+/g, "").replace(/0x/gi, "").replace(/,/g, "");
  if (cleaned.length % 2 !== 0) return "[Error: Invalid HEX input — odd number of nibbles]";
  const bytes: number[] = [];
  for (let i = 0; i < cleaned.length; i += 2) {
    const b = parseInt(cleaned.substring(i, i + 2), 16);
    if (isNaN(b)) return "[Error: Invalid HEX input — contains non-HEX characters]";
    bytes.push(b);
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

// ══════════════════════════════════════════════════════════════════
// 进制转换
// ══════════════════════════════════════════════════════════════════

export function hexToDec(hex: string): string {
  const v = parseInt(hex.replace(/\s|0x/gi, ""), 16);
  if (isNaN(v)) return "[Error: Invalid HEX input]";
  return v.toString(10);
}

export function decToHex(dec: string, width?: number): string {
  const v = parseInt(dec, 10);
  if (isNaN(v)) return "[Error: Invalid decimal input]";
  const hex = v.toString(16).toUpperCase();
  if (width) return hex.padStart(Math.ceil(width / 4), "0");
  return hex;
}

export function binToDec(bin: string): string {
  const v = parseInt(bin.replace(/\s/g, ""), 2);
  if (isNaN(v)) return "[Error: Invalid binary input]";
  return v.toString(10);
}

export function decToBin(dec: string, width?: number): string {
  const v = parseInt(dec, 10);
  if (isNaN(v)) return "[Error: Invalid decimal input]";
  const bin = v.toString(2);
  return width ? bin.padStart(width, "0") : bin;
}

export function hexToBin(hex: string): string {
  const v = parseInt(hex.replace(/\s|0x/gi, ""), 16);
  if (isNaN(v)) return "[Error: Invalid HEX input]";
  return v.toString(2);
}

export function binToHex(bin: string): string {
  const v = parseInt(bin.replace(/\s/g, ""), 2);
  if (isNaN(v)) return "[Error: Invalid binary input]";
  return v.toString(16).toUpperCase();
}

// ══════════════════════════════════════════════════════════════════
// 大小端切换
// ══════════════════════════════════════════════════════════════════

/**
 * 根据字节宽度反转字节序。
 * @param hex 输入的 HEX 字符串（如 "01020304"）
 * @param byteSize 每组的字节宽度（1/2/4/8）
 */
export function swapEndian(hex: string, byteSize: 1 | 2 | 4 | 8): string {
  const cleaned = hex.replace(/\s+/g, "").replace(/0x/gi, "").replace(/,/g, "");
  if (cleaned.length % 2 !== 0) return "[Error: Invalid HEX input — odd number of nibbles]";

  // 按 byteSize 分组
  const groups: string[] = [];
  const groupHexLen = byteSize * 2;
  for (let i = 0; i < cleaned.length; i += groupHexLen) {
    const group = cleaned.substring(i, i + groupHexLen);
    if (group.length < groupHexLen) break; // 不完整分组忽略
    // 反转该组内的字节序
    const reversed =
      group.length === 2 ? group
        : group.match(/.{2}/g)?.reverse().join("") ?? group;
    groups.push(reversed);
  }
  return groups.join(" ");
}

// ══════════════════════════════════════════════════════════════════
// IEEE754 单精度浮点 ↔ HEX
// ══════════════════════════════════════════════════════════════════

/** 32位单精度浮点数 → HEX（大端序） */
export function floatToHex(value: number): string {
  const buf = new ArrayBuffer(4);
  new DataView(buf).setFloat32(0, value, false); // big-endian
  const bytes = new Uint8Array(buf);
  return Array.from(bytes)
    .map((b) => b.toString(16).toUpperCase().padStart(2, "0"))
    .join(" ");
}

/** HEX → 32位单精度浮点数（大端序） */
export function hexToFloat(hex: string): number | null {
  const cleaned = hex.replace(/\s+/g, "").replace(/0x/gi, "").replace(/,/g, "");
  if (cleaned.length !== 8) return null;
  const bytes = new Uint8Array(4);
  for (let i = 0; i < 4; i++) {
    const b = parseInt(cleaned.substring(i * 2, i * 2 + 2), 16);
    if (isNaN(b)) return null;
    bytes[i] = b;
  }
  return new DataView(bytes.buffer).getFloat32(0, false);
}

// ══════════════════════════════════════════════════════════════════
// 转换操作枚举
// ══════════════════════════════════════════════════════════════════

export type EncodingOp =
  | "hex-to-string"
  | "string-to-hex"
  | "hex-to-dec"
  | "dec-to-hex"
  | "hex-to-bin"
  | "bin-to-hex"
  | "dec-to-bin"
  | "bin-to-dec"
  | "base64-encode"
  | "base64-decode"
  | "url-encode"
  | "url-decode"
  | "float-to-hex"
  | "hex-to-float"
  | "swap-endian-16"
  | "swap-endian-32";

export const ENCODING_OP_KEYS: EncodingOp[] = [
  "hex-to-string",
  "string-to-hex",
  "hex-to-dec",
  "dec-to-hex",
  "hex-to-bin",
  "bin-to-hex",
  "dec-to-bin",
  "bin-to-dec",
  "base64-encode",
  "base64-decode",
  "url-encode",
  "url-decode",
  "float-to-hex",
  "hex-to-float",
  "swap-endian-16",
  "swap-endian-32",
];

/** 执行编码转换 */
export function executeEncodingOp(input: string, op: EncodingOp): string {
  switch (op) {
    case "hex-to-string": return hexToString(input);
    case "string-to-hex": return stringToHex(input);
    case "hex-to-dec": return hexToDec(input);
    case "dec-to-hex": return decToHex(input);
    case "hex-to-bin": return hexToBin(input);
    case "bin-to-hex": return binToHex(input);
    case "dec-to-bin": return decToBin(input);
    case "bin-to-dec": return binToDec(input);
    case "base64-encode": return base64Encode(input);
    case "base64-decode": return base64Decode(input);
    case "url-encode": return urlEncode(input);
    case "url-decode": return urlDecode(input);
    case "float-to-hex": {
      const f = parseFloat(input);
      if (isNaN(f)) return `[Error: Invalid float value]`;
      return floatToHex(f);
    }
    case "hex-to-float": {
      const f = hexToFloat(input);
      return f !== null ? f.toString() : "[Error: Invalid HEX float input]";
    }
    case "swap-endian-16": return swapEndian(input, 2);
    case "swap-endian-32": return swapEndian(input, 4);
    default: return `[Error: Unknown encoding operation]`;
  }
}
