import { bytesToHex, parseByteInput } from "./byteInput.ts";
import { toolErr, toolOk, type ToolResult } from "./toolResult.ts";

export interface EncodingOptions {
  base64IgnoreWhitespace?: boolean;
}

function strictUtf8Decode(bytes: Uint8Array): ToolResult<string> {
  try {
    return toolOk(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    return toolErr("invalidUtf8");
  }
}

export function base64Encode(input: string): ToolResult<string> {
  try {
    const bytes = new TextEncoder().encode(input);
    const binary = Array.from(bytes, (byte) => String.fromCharCode(byte)).join("");
    return toolOk(btoa(binary));
  } catch {
    return toolErr("base64EncodeFailed");
  }
}

export function base64Decode(input: string, ignoreWhitespace = false): ToolResult<string> {
  let source = input.trim();
  if (!source) return toolErr("emptyInput");
  if (ignoreWhitespace) source = source.replace(/\s+/g, "");
  else if (/\s/.test(source)) return toolErr("base64Whitespace");

  const canonical =
    /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
  if (!canonical.test(source)) return toolErr("invalidBase64");

  try {
    const bytes = Uint8Array.from(atob(source), (char) => char.charCodeAt(0));
    return strictUtf8Decode(bytes);
  } catch {
    return toolErr("invalidBase64");
  }
}

export function urlEncode(input: string): ToolResult<string> {
  return toolOk(encodeURIComponent(input));
}

export function urlDecode(input: string): ToolResult<string> {
  try {
    return toolOk(decodeURIComponent(input));
  } catch {
    return toolErr("invalidUrlEncoding");
  }
}

export function stringToHex(input: string): ToolResult<string> {
  return toolOk(bytesToHex(new TextEncoder().encode(input)));
}

export function hexToString(input: string): ToolResult<string> {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return parsed;
  return strictUtf8Decode(parsed.value.bytes);
}

export function hexToAscii(input: string): ToolResult<string> {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return parsed;
  const names: Record<number, string> = {
    0x00: "\\0",
    0x08: "\\b",
    0x09: "\\t",
    0x0A: "\\n",
    0x0D: "\\r",
    0x1B: "\\e",
    0x7F: "\\x7F",
  };
  const output = Array.from(parsed.value.bytes, (byte) => {
    if (byte >= 0x20 && byte <= 0x7E) return String.fromCharCode(byte);
    return names[byte] ?? "\\x" + byte.toString(16).toUpperCase().padStart(2, "0");
  }).join("");
  return toolOk(output);
}

function normalizePrefixedDigits(
  input: string,
  prefix: "0x" | "0b",
  digitPattern: RegExp,
): string | null {
  const source = input.trim();
  if (!source) return null;
  const tokens = source.split(/[\s,]+/).filter(Boolean);
  let cleaned = "";
  for (const token of tokens) {
    const normalizedPrefix = token.slice(0, 2).toLowerCase();
    const digits = normalizedPrefix === prefix ? token.slice(2) : token;
    if (!digits || !digitPattern.test(digits)) return null;
    cleaned += digits;
  }
  return cleaned || null;
}

function parseSignedRadix(
  input: string,
  prefix: "0x" | "0b",
  digitPattern: RegExp,
): { value: bigint; digits: string } | null {
  let source = input.trim();
  if (!source) return null;
  let sign = 1n;
  if (source.startsWith("+") || source.startsWith("-")) {
    sign = source[0] === "-" ? -1n : 1n;
    source = source.slice(1).trim();
  }
  source = source.replace(/_/g, "");
  const digits = normalizePrefixedDigits(source, prefix, digitPattern);
  if (!digits) return null;
  try {
    return { value: sign * BigInt(prefix + digits), digits };
  } catch {
    return null;
  }
}

function parseDecimalBigInt(input: string): bigint | null {
  const source = input.trim().replace(/_/g, "");
  if (!/^[+-]?\d+$/.test(source)) return null;
  try {
    return BigInt(source);
  } catch {
    return null;
  }
}

export function hexToDec(input: string): ToolResult<string> {
  const parsed = parseSignedRadix(input, "0x", /^[0-9a-fA-F]+$/);
  return parsed ? toolOk(parsed.value.toString(10)) : toolErr("invalidHexInteger");
}

export function decToHex(input: string, width?: number): ToolResult<string> {
  const value = parseDecimalBigInt(input);
  if (value === null) return toolErr("invalidDecimal");
  const negative = value < 0n;
  const magnitude = negative ? -value : value;
  let output = magnitude.toString(16).toUpperCase();
  if (width) output = output.padStart(Math.ceil(width / 4), "0");
  return toolOk((negative ? "-" : "") + output);
}

export function binToDec(input: string): ToolResult<string> {
  const parsed = parseSignedRadix(input, "0b", /^[01]+$/);
  return parsed ? toolOk(parsed.value.toString(10)) : toolErr("invalidBinary");
}

export function decToBin(input: string, width?: number): ToolResult<string> {
  const value = parseDecimalBigInt(input);
  if (value === null) return toolErr("invalidDecimal");
  const negative = value < 0n;
  const magnitude = negative ? -value : value;
  let output = magnitude.toString(2);
  if (width) output = output.padStart(width, "0");
  return toolOk((negative ? "-" : "") + output);
}

export function hexToBin(input: string): ToolResult<string> {
  const parsed = parseSignedRadix(input, "0x", /^[0-9a-fA-F]+$/);
  if (!parsed) return toolErr("invalidHexInteger");
  const negative = parsed.value < 0n;
  const magnitude = negative ? -parsed.value : parsed.value;
  const output = magnitude.toString(2).padStart(parsed.digits.length * 4, "0");
  return toolOk((negative ? "-" : "") + output);
}

export function binToHex(input: string): ToolResult<string> {
  const parsed = parseSignedRadix(input, "0b", /^[01]+$/);
  if (!parsed) return toolErr("invalidBinary");
  const negative = parsed.value < 0n;
  const magnitude = negative ? -parsed.value : parsed.value;
  const output = magnitude
    .toString(16)
    .toUpperCase()
    .padStart(Math.ceil(parsed.digits.length / 4), "0");
  return toolOk((negative ? "-" : "") + output);
}

export function swapEndian(
  input: string,
  byteSize: 1 | 2 | 4 | 8,
): ToolResult<string> {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return parsed;
  const bytes = parsed.value.bytes;
  if (bytes.length % byteSize !== 0) {
    return toolErr("incompleteEndianGroup", String(byteSize));
  }
  const output = new Uint8Array(bytes.length);
  for (let offset = 0; offset < bytes.length; offset += byteSize) {
    for (let index = 0; index < byteSize; index += 1) {
      output[offset + index] = bytes[offset + byteSize - 1 - index];
    }
  }
  return toolOk(bytesToHex(output));
}

function parseFloatInput(input: string): number | null {
  const source = input.trim();
  if (!source) return null;
  if (source === "NaN") return Number.NaN;
  if (source === "Infinity" || source === "+Infinity") return Number.POSITIVE_INFINITY;
  if (source === "-Infinity") return Number.NEGATIVE_INFINITY;
  if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(source)) return null;
  return Number(source);
}

function floatToHex(input: string, width: 32 | 64): ToolResult<string> {
  const value = parseFloatInput(input);
  if (value === null) return toolErr("invalidFloat");
  const buffer = new ArrayBuffer(width / 8);
  const view = new DataView(buffer);
  if (width === 32) view.setFloat32(0, value, false);
  else view.setFloat64(0, value, false);
  return toolOk(bytesToHex(new Uint8Array(buffer)));
}

function hexToFloat(input: string, width: 32 | 64): ToolResult<string> {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return parsed;
  if (parsed.value.bytes.length !== width / 8) {
    return toolErr("invalidFloatWidth", String(width));
  }
  const bytes = parsed.value.bytes;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const value = width === 32 ? view.getFloat32(0, false) : view.getFloat64(0, false);
  if (Number.isNaN(value)) return toolOk("NaN");
  if (value === Number.POSITIVE_INFINITY) return toolOk("Infinity");
  if (value === Number.NEGATIVE_INFINITY) return toolOk("-Infinity");
  if (Object.is(value, -0)) return toolOk("-0");
  return toolOk(String(value));
}

export function packedBcdToDecimal(input: string): ToolResult<string> {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return parsed;
  let output = "";
  for (const byte of parsed.value.bytes) {
    const high = byte >>> 4;
    const low = byte & 0x0F;
    if (high > 9 || low > 9) return toolErr("invalidBcd");
    output += String(high) + String(low);
  }
  return toolOk(output.replace(/^0+(?=\d)/, ""));
}

export function decimalToPackedBcd(input: string): ToolResult<string> {
  let source = input.trim();
  if (!/^\d+$/.test(source)) return toolErr("invalidBcdDecimal");
  if (source.length % 2 !== 0) source = "0" + source;
  const bytes = new Uint8Array(source.length / 2);
  for (let index = 0; index < source.length; index += 2) {
    bytes[index / 2] =
      (Number(source[index]) << 4) | Number(source[index + 1]);
  }
  return toolOk(bytesToHex(bytes));
}

export type EncodingOp =
  | "hex-to-string"
  | "hex-to-ascii"
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
  | "float32-to-hex"
  | "hex-to-float32"
  | "float64-to-hex"
  | "hex-to-float64"
  | "swap-endian-16"
  | "swap-endian-32"
  | "swap-endian-64"
  | "packed-bcd-to-dec"
  | "dec-to-packed-bcd";

export const ENCODING_OP_KEYS: EncodingOp[] = [
  "hex-to-string",
  "hex-to-ascii",
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
  "float32-to-hex",
  "hex-to-float32",
  "float64-to-hex",
  "hex-to-float64",
  "swap-endian-16",
  "swap-endian-32",
  "swap-endian-64",
  "packed-bcd-to-dec",
  "dec-to-packed-bcd",
];

export function executeEncodingOp(
  input: string,
  op: EncodingOp,
  options: EncodingOptions = {},
): ToolResult<string> {
  switch (op) {
    case "hex-to-string": return hexToString(input);
    case "hex-to-ascii": return hexToAscii(input);
    case "string-to-hex": return stringToHex(input);
    case "hex-to-dec": return hexToDec(input);
    case "dec-to-hex": return decToHex(input);
    case "hex-to-bin": return hexToBin(input);
    case "bin-to-hex": return binToHex(input);
    case "dec-to-bin": return decToBin(input);
    case "bin-to-dec": return binToDec(input);
    case "base64-encode": return base64Encode(input);
    case "base64-decode": return base64Decode(input, options.base64IgnoreWhitespace);
    case "url-encode": return urlEncode(input);
    case "url-decode": return urlDecode(input);
    case "float32-to-hex": return floatToHex(input, 32);
    case "hex-to-float32": return hexToFloat(input, 32);
    case "float64-to-hex": return floatToHex(input, 64);
    case "hex-to-float64": return hexToFloat(input, 64);
    case "swap-endian-16": return swapEndian(input, 2);
    case "swap-endian-32": return swapEndian(input, 4);
    case "swap-endian-64": return swapEndian(input, 8);
    case "packed-bcd-to-dec": return packedBcdToDecimal(input);
    case "dec-to-packed-bcd": return decimalToPackedBcd(input);
    default: return toolErr("unsupportedOperation");
  }
}
