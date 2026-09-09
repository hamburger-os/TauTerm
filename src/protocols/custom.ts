import { bytesToHex, parseByteInput } from "../utils/byteInput.ts";
import {
  CRC_PRESETS,
  crcPreset,
  numberToHex,
  type CrcPreset,
} from "../utils/checksum.ts";
import type {
  ParsedField,
  ProtocolCheck,
  ProtocolIssue,
  ProtocolParseOutcome,
} from "./types.ts";

type CustomFieldType =
  | "u8"
  | "i8"
  | "u16"
  | "i16"
  | "u32"
  | "i32"
  | "u64"
  | "i64"
  | "float32"
  | "float64"
  | "bytes"
  | "ascii"
  | "utf8";

interface CustomFieldSchema {
  name: string;
  offset: number;
  type: CustomFieldType;
  endian?: "be" | "le";
  length?: number | "remaining";
  enum?: Record<string, string>;
}

interface CustomChecksumSchema {
  preset: CrcPreset;
  start?: number;
  end?: number;
  fieldOffset: number;
  byteOrder?: "be" | "le";
}

interface CustomSchema {
  name?: string;
  fields: CustomFieldSchema[];
  checksum?: CustomChecksumSchema;
}

function parseSchema(source: string): CustomSchema | null {
  try {
    const parsed = JSON.parse(source) as Partial<CustomSchema>;
    if (!parsed || !Array.isArray(parsed.fields)) return null;
    if (!parsed.fields.every((entry) => {
      if (!entry || typeof entry !== "object") return false;
      const field = entry as Partial<CustomFieldSchema>;
      return typeof field.name === "string"
        && Number.isInteger(field.offset)
        && Number(field.offset) >= 0
        && typeof field.type === "string";
    })) return null;
    return parsed as CustomSchema;
  } catch {
    return null;
  }
}

function fieldWidth(field: CustomFieldSchema, totalLength: number): number | null {
  switch (field.type) {
    case "u8":
    case "i8": return 1;
    case "u16":
    case "i16": return 2;
    case "u32":
    case "i32":
    case "float32": return 4;
    case "u64":
    case "i64":
    case "float64": return 8;
    case "bytes":
    case "ascii":
    case "utf8":
      if (field.length === "remaining") return Math.max(0, totalLength - field.offset);
      return Number.isInteger(field.length) && Number(field.length) >= 0
        ? Number(field.length)
        : null;
    default:
      return null;
  }
}

function enumValue(field: CustomFieldSchema, numeric: string): string | undefined {
  if (!field.enum) return undefined;
  if (field.enum[numeric] !== undefined) return field.enum[numeric];
  try {
    const hex = "0x" + BigInt(numeric).toString(16).toUpperCase();
    return field.enum[hex] ?? field.enum[hex.toLowerCase()];
  } catch {
    return undefined;
  }
}

function decodeField(
  bytes: Uint8Array,
  field: CustomFieldSchema,
  width: number,
): { value: string; issue?: ProtocolIssue } {
  const slice = bytes.slice(field.offset, field.offset + width);
  const view = new DataView(slice.buffer, slice.byteOffset, slice.byteLength);
  const little = field.endian === "le";
  let value: string;

  switch (field.type) {
    case "u8": value = String(view.getUint8(0)); break;
    case "i8": value = String(view.getInt8(0)); break;
    case "u16": value = String(view.getUint16(0, little)); break;
    case "i16": value = String(view.getInt16(0, little)); break;
    case "u32": value = String(view.getUint32(0, little)); break;
    case "i32": value = String(view.getInt32(0, little)); break;
    case "u64": value = view.getBigUint64(0, little).toString(10); break;
    case "i64": value = view.getBigInt64(0, little).toString(10); break;
    case "float32": {
      const number = view.getFloat32(0, little);
      value = Number.isNaN(number) ? "NaN" : String(number);
      break;
    }
    case "float64": {
      const number = view.getFloat64(0, little);
      value = Number.isNaN(number) ? "NaN" : String(number);
      break;
    }
    case "ascii":
      value = Array.from(slice, (byte) =>
        byte >= 0x20 && byte <= 0x7E ? String.fromCharCode(byte) : "."
      ).join("");
      break;
    case "utf8":
      try {
        value = new TextDecoder("utf-8", { fatal: true }).decode(slice);
      } catch {
        return {
          value: bytesToHex(slice),
          issue: {
            code: "customInvalidUtf8",
            severity: "warning",
            detail: field.name,
            range: { start: field.offset, length: width, unit: "byte" },
          },
        };
      }
      break;
    case "bytes":
    default:
      value = bytesToHex(slice);
      break;
  }

  const mapped = enumValue(field, value);
  return { value: mapped ? value + " (" + mapped + ")" : value };
}

function readChecksumValue(
  bytes: Uint8Array,
  offset: number,
  width: 8 | 16 | 32,
  byteOrder: "be" | "le",
): number {
  const count = width / 8;
  let value = 0;
  if (byteOrder === "be") {
    for (let index = 0; index < count; index += 1) {
      value = (value * 256 + bytes[offset + index]) >>> 0;
    }
  } else {
    for (let index = count - 1; index >= 0; index -= 1) {
      value = (value * 256 + bytes[offset + index]) >>> 0;
    }
  }
  return value >>> 0;
}

export const DEFAULT_CUSTOM_SCHEMA = JSON.stringify(
  {
    name: "Device Frame",
    fields: [
      { name: "Header", offset: 0, type: "u16", endian: "be", enum: { "0xAA55": "Magic" } },
      { name: "Command", offset: 2, type: "u8", enum: { "1": "Read", "2": "Write" } },
      { name: "Length", offset: 3, type: "u16", endian: "le" },
      { name: "Payload", offset: 5, type: "bytes", length: "remaining" },
    ],
  },
  null,
  2,
);

export function inspectCustomSchema(
  input: string,
  schemaSource: string,
): ProtocolParseOutcome {
  const parsedInput = parseByteInput(input);
  if (!parsedInput.ok) {
    return { result: null, errorCode: parsedInput.error.code };
  }
  const schema = parseSchema(schemaSource);
  if (!schema) return { result: null, errorCode: "customSchemaInvalid" };

  const bytes = parsedInput.value.bytes;
  const fields: ParsedField[] = [];
  const issues: ProtocolIssue[] = [];

  for (let index = 0; index < schema.fields.length; index += 1) {
    const definition = schema.fields[index];
    const width = fieldWidth(definition, bytes.length);
    if (width === null) {
      issues.push({
        code: "customFieldDefinitionInvalid",
        severity: "error",
        detail: definition.name,
      });
      continue;
    }
    if (definition.offset + width > bytes.length) {
      issues.push({
        code: "customFieldOutOfBounds",
        severity: "error",
        detail: definition.name,
        range: {
          start: Math.min(definition.offset, bytes.length),
          length: Math.max(0, bytes.length - definition.offset),
          unit: "byte",
        },
      });
      continue;
    }

    const decoded = decodeField(bytes, definition, width);
    if (decoded.issue) issues.push(decoded.issue);
    fields.push({
      id: "custom-" + index,
      name: definition.name,
      range: { start: definition.offset, length: width, unit: "byte" },
      rawValue: bytesToHex(bytes.slice(definition.offset, definition.offset + width)),
      parsedValue: decoded.value,
    });
  }

  const checks: ProtocolCheck[] = [
    {
      id: "schema",
      label: "tools.checkSchema",
      status: issues.some((issue) => issue.severity === "error") ? "fail" as const : "pass" as const,
    },
  ];

  if (schema.checksum) {
    const checksum = schema.checksum;
    const preset = CRC_PRESETS[checksum.preset];
    if (!preset) {
      issues.push({
        code: "customChecksumPresetInvalid",
        severity: "error",
        detail: String(checksum.preset),
      });
    } else {
      const width = preset.params.width;
      const checksumBytes = width / 8;
      const start = checksum.start ?? 0;
      const end = checksum.end ?? checksum.fieldOffset;
      if (
        start < 0
        || end < start
        || end > bytes.length
        || checksum.fieldOffset < 0
        || checksum.fieldOffset + checksumBytes > bytes.length
      ) {
        issues.push({
          code: "customChecksumRangeInvalid",
          severity: "error",
        });
      } else {
        const calculated = crcPreset(bytes.slice(start, end), checksum.preset);
        const actual = readChecksumValue(
          bytes,
          checksum.fieldOffset,
          width,
          checksum.byteOrder ?? "be",
        );
        const valid = actual === calculated;
        checks.push({
          id: "checksum",
          label: "tools.checkChecksum",
          status: valid ? "pass" as const : "fail" as const,
          detail:
            "received 0x" + numberToHex(actual, width)
            + ", calculated 0x" + numberToHex(calculated, width),
        });
        fields.push({
          id: "custom-checksum",
          name: "tools.fieldChecksum",
          range: {
            start: checksum.fieldOffset,
            length: checksumBytes,
            unit: "byte",
          },
          rawValue: bytesToHex(
            bytes.slice(checksum.fieldOffset, checksum.fieldOffset + checksumBytes),
          ),
          parsedValue: "0x" + numberToHex(actual, width),
        });
        if (!valid) {
          issues.push({
            code: "checksumMismatch",
            severity: "error",
            detail: "expected 0x" + numberToHex(calculated, width),
            range: {
              start: checksum.fieldOffset,
              length: checksumBytes,
              unit: "byte",
            },
          });
        }
      }
    }
  }

  return {
    result: {
      inspectorId: "custom-schema",
      fields,
      checks,
      issues,
      rawBytes: bytes,
      normalizedInput: parsedInput.value.normalizedHex,
    },
  };
}
