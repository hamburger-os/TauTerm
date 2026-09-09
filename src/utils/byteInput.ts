import { toolErr, toolOk, type ToolResult } from "./toolResult.ts";

export type ByteInputFormat =
  | "continuous-hex"
  | "tokenized-hex"
  | "escaped-hex"
  | "c-array";

export interface ParsedByteInput {
  bytes: Uint8Array;
  normalizedHex: string;
  format: ByteInputFormat;
}

function toNormalizedHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((byte) => byte.toString(16).toUpperCase().padStart(2, "0"))
    .join(" ");
}

function parseCleanHex(cleaned: string, format: ByteInputFormat): ToolResult<ParsedByteInput> {
  if (!cleaned) return toolErr("emptyInput");
  if (!/^[0-9a-fA-F]+$/.test(cleaned)) return toolErr("invalidHex");
  if (cleaned.length % 2 !== 0) return toolErr("oddHexLength");

  const bytes = new Uint8Array(cleaned.length / 2);
  for (let i = 0; i < cleaned.length; i += 2) {
    bytes[i / 2] = Number.parseInt(cleaned.slice(i, i + 2), 16);
  }
  return toolOk({ bytes, normalizedHex: toNormalizedHex(bytes), format });
}

/**
 * Strict shared byte-input parser used by protocol inspection and engineering tools.
 *
 * Accepted forms:
 * - AA BB CC / AA,BB,CC / AA:BB:CC / AA-BB-CC
 * - AABBCC
 * - 0xAA, 0xBB, 0xCC
 * - \xAA\xBB\xCC
 * - { 0xAA, 0xBB, 0xCC }
 * - uint8_t data[] = { 0xAA, 0xBB, 0xCC };
 *
 * Parsing fails closed: malformed or partially valid tokens are rejected.
 */
export function parseByteInput(input: string): ToolResult<ParsedByteInput> {
  const trimmed = input.trim();
  if (!trimmed) return toolErr("emptyInput");

  if (trimmed.includes("\\x") || trimmed.includes("\\X")) {
    if (!/^(?:\\[xX][0-9a-fA-F]{2}\s*)+$/.test(trimmed)) {
      return toolErr("invalidEscapedHex");
    }
    const cleaned = Array.from(trimmed.matchAll(/\\[xX]([0-9a-fA-F]{2})/g))
      .map((match) => match[1])
      .join("");
    return parseCleanHex(cleaned, "escaped-hex");
  }

  let body = trimmed;
  let format: ByteInputFormat = "tokenized-hex";
  const openBrace = body.indexOf("{");
  const closeBrace = body.lastIndexOf("}");
  if (openBrace >= 0 || closeBrace >= 0) {
    if (openBrace < 0 || closeBrace <= openBrace) return toolErr("invalidHex");
    const trailing = body.slice(closeBrace + 1).trim();
    if (trailing && trailing !== ";") return toolErr("invalidHex");
    body = body.slice(openBrace + 1, closeBrace).trim();
    format = "c-array";
  }

  const hasSeparator = /[\s,;:\-]/.test(body);
  if (!hasSeparator && !/^0[xX]/.test(body)) {
    return parseCleanHex(body, "continuous-hex");
  }

  const tokens = body.split(/[\s,;:\-]+/).filter(Boolean);
  if (tokens.length === 0) return toolErr("emptyInput");

  let cleaned = "";
  for (const token of tokens) {
    const match = token.match(/^(?:0[xX])?([0-9a-fA-F]+)$/);
    if (!match) return toolErr("invalidHexToken", token);
    if (match[1].length % 2 !== 0) return toolErr("oddHexLength", token);
    cleaned += match[1];
  }

  return parseCleanHex(cleaned, format);
}

export function bytesToHex(bytes: Uint8Array, separator = " "): string {
  return Array.from(bytes)
    .map((byte) => byte.toString(16).toUpperCase().padStart(2, "0"))
    .join(separator);
}
