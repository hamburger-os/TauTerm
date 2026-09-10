import type { NewlineMode, SendMode } from "./types";

const NEWLINE_MAP: Record<NewlineMode, string> = {
  crlf: "\r\n",
  lf: "\n",
  cr: "\r",
  none: "",
};

export function normalizeHexInput(value: string): string {
  return value.replace(/\s/g, "");
}

export function isHexInputValid(value: string): boolean {
  const hex = normalizeHexInput(value);
  return hex.length > 0 && hex.length % 2 === 0 && /^[0-9a-fA-F]+$/.test(hex);
}

/**
 * Convert the current editor value into the exact bytes/string sent to the session router.
 * Returns null for an empty/invalid payload so every send path shares identical validation.
 */
export function buildSendPayload(
  input: string,
  sendMode: SendMode,
  newlineMode: NewlineMode,
): string | Uint8Array | null {
  if (sendMode === "text") {
    if (!input.trim()) return null;
    return input + NEWLINE_MAP[newlineMode];
  }

  const hex = normalizeHexInput(input);
  if (!isHexInputValid(hex)) return null;

  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}
