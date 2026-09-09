import { bytesToHex, parseByteInput } from "./byteInput.ts";
import { toolErr, toolOk, type ToolResult } from "./toolResult.ts";

export interface DataInterpretation {
  id: string;
  label: string;
  value: string;
}

export interface DataInspection {
  bytes: Uint8Array;
  normalizedHex: string;
  interpretations: DataInterpretation[];
}

function asciiDisplay(bytes: Uint8Array): string {
  const control: Record<number, string> = {
    0x00: "\\0",
    0x08: "\\b",
    0x09: "\\t",
    0x0A: "\\n",
    0x0D: "\\r",
    0x1B: "\\e",
    0x7F: "\\x7F",
  };
  return Array.from(bytes, (byte) => {
    if (byte >= 0x20 && byte <= 0x7E) return String.fromCharCode(byte);
    return control[byte] ?? "\\x" + byte.toString(16).toUpperCase().padStart(2, "0");
  }).join("");
}

function decodeUtf8(bytes: Uint8Array): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return "[invalid UTF-8]";
  }
}

function groupValues(
  bytes: Uint8Array,
  width: 2 | 4 | 8,
  read: (view: DataView) => string,
): string | null {
  if (bytes.length === 0 || bytes.length % width !== 0) return null;
  const values: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += width) {
    const view = new DataView(
      bytes.buffer,
      bytes.byteOffset + offset,
      width,
    );
    values.push(read(view));
  }
  return values.join(", ");
}

function packedBcd(bytes: Uint8Array): string | null {
  let value = "";
  for (const byte of bytes) {
    const high = byte >>> 4;
    const low = byte & 0x0F;
    if (high > 9 || low > 9) return null;
    value += String(high) + String(low);
  }
  return value.replace(/^0+(?=\d)/, "");
}

export function inspectByteData(input: string): ToolResult<DataInspection> {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return toolErr(parsed.error.code, parsed.error.detail);
  const bytes = parsed.value.bytes;
  const interpretations: DataInterpretation[] = [
    { id: "length", label: "tools.dataBytes", value: String(bytes.length) },
    { id: "hex", label: "HEX", value: bytesToHex(bytes) },
    { id: "ascii", label: "ASCII", value: asciiDisplay(bytes) },
    { id: "utf8", label: "UTF-8", value: decodeUtf8(bytes) },
    {
      id: "u8",
      label: "UInt8[]",
      value: Array.from(bytes).join(", "),
    },
    {
      id: "i8",
      label: "Int8[]",
      value: Array.from(bytes, (byte) => (byte & 0x80 ? byte - 0x100 : byte)).join(", "),
    },
  ];

  const pushGrouped = (
    id: string,
    label: string,
    value: string | null,
  ) => {
    if (value !== null) interpretations.push({ id, label, value });
  };

  pushGrouped("u16be", "UInt16 BE", groupValues(bytes, 2, (view) => String(view.getUint16(0, false))));
  pushGrouped("u16le", "UInt16 LE", groupValues(bytes, 2, (view) => String(view.getUint16(0, true))));
  pushGrouped("i16be", "Int16 BE", groupValues(bytes, 2, (view) => String(view.getInt16(0, false))));
  pushGrouped("i16le", "Int16 LE", groupValues(bytes, 2, (view) => String(view.getInt16(0, true))));
  pushGrouped("u32be", "UInt32 BE", groupValues(bytes, 4, (view) => String(view.getUint32(0, false))));
  pushGrouped("u32le", "UInt32 LE", groupValues(bytes, 4, (view) => String(view.getUint32(0, true))));
  pushGrouped("i32be", "Int32 BE", groupValues(bytes, 4, (view) => String(view.getInt32(0, false))));
  pushGrouped("i32le", "Int32 LE", groupValues(bytes, 4, (view) => String(view.getInt32(0, true))));
  pushGrouped("f32be", "Float32 BE", groupValues(bytes, 4, (view) => String(view.getFloat32(0, false))));
  pushGrouped("f32le", "Float32 LE", groupValues(bytes, 4, (view) => String(view.getFloat32(0, true))));
  pushGrouped("u64be", "UInt64 BE", groupValues(bytes, 8, (view) => view.getBigUint64(0, false).toString(10)));
  pushGrouped("u64le", "UInt64 LE", groupValues(bytes, 8, (view) => view.getBigUint64(0, true).toString(10)));
  pushGrouped("i64be", "Int64 BE", groupValues(bytes, 8, (view) => view.getBigInt64(0, false).toString(10)));
  pushGrouped("i64le", "Int64 LE", groupValues(bytes, 8, (view) => view.getBigInt64(0, true).toString(10)));
  pushGrouped("f64be", "Float64 BE", groupValues(bytes, 8, (view) => String(view.getFloat64(0, false))));
  pushGrouped("f64le", "Float64 LE", groupValues(bytes, 8, (view) => String(view.getFloat64(0, true))));

  const bcd = packedBcd(bytes);
  if (bcd !== null) interpretations.push({ id: "bcd", label: "Packed BCD", value: bcd });

  return toolOk({
    bytes,
    normalizedHex: parsed.value.normalizedHex,
    interpretations,
  });
}
