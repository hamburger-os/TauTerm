import { bytesToHex, parseByteInput } from "../utils/byteInput.ts";
import {
  CRC_PRESETS,
  crcPreset,
  numberToHex,
  type CrcPreset,
  type CrcPresetDefinition,
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
  | "utf8"
  | "reserved"
  | "bitfield";

interface CustomBitSchema {
  name: string;
  high: number;
  low?: number;
  enum?: Record<string, string>;
}

interface CustomFieldSchema {
  name: string;
  offset: number;
  type: CustomFieldType;
  endian?: "be" | "le";
  length?: number | "remaining";
  lengthFrom?: string;
  enum?: Record<string, string>;
  expected?: string | number;
  bits?: CustomBitSchema[];
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

interface DecodedField {
  value: string;
  numeric?: bigint;
  children?: ParsedField[];
  issue?: ProtocolIssue;
}

const SCHEMA_DEFINITION_ISSUES = new Set([
  "customFieldDefinitionInvalid",
  "customLengthSourceInvalid",
  "customBitDefinitionInvalid",
  "customChecksumPresetInvalid",
  "customChecksumRangeInvalid",
]);

function parseSchema(source: string): CustomSchema | null {
  try {
    const parsed = JSON.parse(source) as Partial<CustomSchema>;
    if (!parsed || !Array.isArray(parsed.fields)) return null;
    if (!parsed.fields.every((entry) => {
      if (!entry || typeof entry !== "object") return false;
      const field = entry as Partial<CustomFieldSchema>;
      return typeof field.name === "string"
        && field.name.trim().length > 0
        && Number.isInteger(field.offset)
        && Number(field.offset) >= 0
        && typeof field.type === "string";
    })) return null;
    return parsed as CustomSchema;
  } catch {
    return null;
  }
}

function fixedScalarWidth(type: CustomFieldType): number | null {
  switch (type) {
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
    default: return null;
  }
}

function variableFieldWidth(
  field: CustomFieldSchema,
  totalLength: number,
  numericFields: Map<string, bigint>,
): { width: number | null; issueCode?: string } {
  const fixed = fixedScalarWidth(field.type);
  if (fixed !== null) return { width: fixed };

  if (field.type === "bitfield") {
    const width = field.length ?? 1;
    if (
      typeof width !== "number"
      || !Number.isInteger(width)
      || ![1, 2, 4, 8].includes(width)
    ) {
      return { width: null, issueCode: "customFieldDefinitionInvalid" };
    }
    return { width };
  }

  if (!["bytes", "ascii", "utf8", "reserved"].includes(field.type)) {
    return { width: null, issueCode: "customFieldDefinitionInvalid" };
  }

  if (field.lengthFrom) {
    const referenced = numericFields.get(field.lengthFrom);
    if (referenced === undefined || referenced < 0n || referenced > BigInt(Number.MAX_SAFE_INTEGER)) {
      return { width: null, issueCode: "customLengthSourceInvalid" };
    }
    return { width: Number(referenced) };
  }

  if (field.length === "remaining") {
    return { width: Math.max(0, totalLength - field.offset) };
  }

  return Number.isInteger(field.length) && Number(field.length) >= 0
    ? { width: Number(field.length) }
    : { width: null, issueCode: "customFieldDefinitionInvalid" };
}

function enumLookup(
  mapping: Record<string, string> | undefined,
  numeric: bigint,
): string | undefined {
  if (!mapping) return undefined;
  const decimal = numeric.toString(10);
  if (mapping[decimal] !== undefined) return mapping[decimal];
  const hex = "0x" + numeric.toString(16).toUpperCase();
  return mapping[hex] ?? mapping[hex.toLowerCase()];
}

function readUnsignedBigInt(
  bytes: Uint8Array,
  little: boolean,
): bigint {
  let value = 0n;
  if (little) {
    for (let index = bytes.length - 1; index >= 0; index -= 1) {
      value = (value << 8n) | BigInt(bytes[index]);
    }
  } else {
    for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  }
  return value;
}

function validateBitDefinitions(
  field: CustomFieldSchema,
  widthBytes: number,
): boolean {
  if (!field.bits || field.bits.length === 0) return false;
  const maxBit = widthBytes * 8 - 1;
  return field.bits.every((bit) => {
    const low = bit.low ?? bit.high;
    return typeof bit.name === "string"
      && bit.name.length > 0
      && Number.isInteger(bit.high)
      && Number.isInteger(low)
      && low >= 0
      && bit.high >= low
      && bit.high <= maxBit;
  });
}

function decodeBitfield(
  bytes: Uint8Array,
  field: CustomFieldSchema,
  width: number,
): DecodedField {
  if (!validateBitDefinitions(field, width)) {
    return {
      value: bytesToHex(bytes.slice(field.offset, field.offset + width)),
      issue: {
        code: "customBitDefinitionInvalid",
        severity: "error",
        detail: field.name,
        range: { start: field.offset, length: width, unit: "byte" },
      },
    };
  }

  const slice = bytes.slice(field.offset, field.offset + width);
  const numeric = readUnsignedBigInt(slice, field.endian === "le");
  const children: ParsedField[] = field.bits!.map((bit, index) => {
    const low = bit.low ?? bit.high;
    const count = bit.high - low + 1;
    const mask = (1n << BigInt(count)) - 1n;
    const extracted = (numeric >> BigInt(low)) & mask;
    const mapped = enumLookup(bit.enum, extracted);
    return {
      id: "bit-" + index,
      name: bit.name,
      range: { start: field.offset, length: width, unit: "byte" },
      rawValue: bit.high === low ? "bit " + bit.high : "bits " + bit.high + ":" + low,
      parsedValue: mapped
        ? extracted.toString(10) + " (" + mapped + ")"
        : extracted.toString(10),
    };
  });

  return {
    value: "0x" + numeric.toString(16).toUpperCase().padStart(width * 2, "0"),
    numeric,
    children,
  };
}

function decodeField(
  bytes: Uint8Array,
  field: CustomFieldSchema,
  width: number,
): DecodedField {
  if (field.type === "bitfield") return decodeBitfield(bytes, field, width);

  const slice = bytes.slice(field.offset, field.offset + width);
  const view = new DataView(slice.buffer, slice.byteOffset, slice.byteLength);
  const little = field.endian === "le";
  let value: string;
  let numeric: bigint | undefined;

  switch (field.type) {
    case "u8":
      numeric = BigInt(view.getUint8(0));
      value = numeric.toString(10);
      break;
    case "i8":
      numeric = BigInt(view.getInt8(0));
      value = numeric.toString(10);
      break;
    case "u16":
      numeric = BigInt(view.getUint16(0, little));
      value = numeric.toString(10);
      break;
    case "i16":
      numeric = BigInt(view.getInt16(0, little));
      value = numeric.toString(10);
      break;
    case "u32":
      numeric = BigInt(view.getUint32(0, little));
      value = numeric.toString(10);
      break;
    case "i32":
      numeric = BigInt(view.getInt32(0, little));
      value = numeric.toString(10);
      break;
    case "u64":
      numeric = view.getBigUint64(0, little);
      value = numeric.toString(10);
      break;
    case "i64":
      numeric = view.getBigInt64(0, little);
      value = numeric.toString(10);
      break;
    case "float32": {
      const number = view.getFloat32(0, little);
      value = Number.isNaN(number) ? "NaN" : Object.is(number, -0) ? "-0" : String(number);
      break;
    }
    case "float64": {
      const number = view.getFloat64(0, little);
      value = Number.isNaN(number) ? "NaN" : Object.is(number, -0) ? "-0" : String(number);
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
    case "reserved":
      value = bytesToHex(slice) + " (reserved)";
      break;
    case "bytes":
    default:
      value = bytesToHex(slice);
      break;
  }

  if (numeric !== undefined) {
    const mapped = enumLookup(field.enum, numeric);
    if (mapped) value += " (" + mapped + ")";
  }

  return { value, ...(numeric !== undefined ? { numeric } : {}) };
}

function expectedMatches(
  field: CustomFieldSchema,
  decoded: DecodedField,
  raw: Uint8Array,
): boolean {
  if (field.expected === undefined) return true;

  if (decoded.numeric !== undefined) {
    try {
      const expected = typeof field.expected === "number"
        ? BigInt(field.expected)
        : BigInt(field.expected);
      return decoded.numeric === expected;
    } catch {
      return false;
    }
  }

  if (field.type === "bytes" || field.type === "reserved") {
    if (typeof field.expected !== "string") return false;
    const parsed = parseByteInput(field.expected);
    return parsed.ok
      && parsed.value.bytes.length === raw.length
      && parsed.value.bytes.every((byte, index) => byte === raw[index]);
  }

  return String(field.expected) === decoded.value;
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

function lookupCrcPreset(name: string): CrcPresetDefinition | undefined {
  return (CRC_PRESETS as Partial<Record<string, CrcPresetDefinition>>)[name];
}

export const DEFAULT_CUSTOM_SCHEMA = JSON.stringify(
  {
    name: "Device Frame",
    fields: [
      { name: "Header", offset: 0, type: "u16", endian: "be", expected: "0xAA55", enum: { "0xAA55": "Magic" } },
      { name: "Command", offset: 2, type: "u8", enum: { "1": "Read", "2": "Write" } },
      { name: "Length", offset: 3, type: "u16", endian: "le" },
      { name: "Payload", offset: 5, type: "bytes", lengthFrom: "Length" },
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
  const numericFields = new Map<string, bigint>();
  const checks: ProtocolCheck[] = [];

  for (let index = 0; index < schema.fields.length; index += 1) {
    const definition = schema.fields[index];
    const resolvedWidth = variableFieldWidth(definition, bytes.length, numericFields);
    if (resolvedWidth.width === null) {
      issues.push({
        code: resolvedWidth.issueCode ?? "customFieldDefinitionInvalid",
        severity: "error",
        detail: definition.lengthFrom
          ? definition.name + " <- " + definition.lengthFrom
          : definition.name,
      });
      continue;
    }
    const width = resolvedWidth.width;

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
    if (decoded.numeric !== undefined) {
      numericFields.set(definition.name, decoded.numeric);
    }

    const raw = bytes.slice(definition.offset, definition.offset + width);
    if (!expectedMatches(definition, decoded, raw)) {
      issues.push({
        code: "customExpectedMismatch",
        severity: "error",
        detail: definition.name + " expected " + String(definition.expected),
        range: { start: definition.offset, length: width, unit: "byte" },
      });
    }

    fields.push({
      id: "custom-" + index,
      name: definition.name,
      range: { start: definition.offset, length: width, unit: "byte" },
      rawValue: bytesToHex(raw),
      parsedValue: decoded.value,
      ...(decoded.children ? { children: decoded.children } : {}),
    });
  }

  if (schema.checksum) {
    const checksum = schema.checksum;
    const preset = lookupCrcPreset(String(checksum.preset));
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
        !Number.isInteger(start)
        || !Number.isInteger(end)
        || !Number.isInteger(checksum.fieldOffset)
        || start < 0
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
          status: valid ? "pass" : "fail",
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

  const schemaDefinitionFailed = issues.some(
    (issue) => SCHEMA_DEFINITION_ISSUES.has(issue.code),
  );
  const constraintFailed = issues.some(
    (issue) =>
      issue.severity === "error"
      && !SCHEMA_DEFINITION_ISSUES.has(issue.code)
      && issue.code !== "checksumMismatch",
  );
  const constraintWarning = issues.some(
    (issue) => issue.severity === "warning",
  );

  checks.unshift(
    {
      id: "schema",
      label: "tools.checkSchema",
      status: schemaDefinitionFailed ? "fail" : "pass",
    },
    {
      id: "constraints",
      label: "tools.checkProtocolSemantics",
      status: constraintFailed ? "fail" : constraintWarning ? "warning" : "pass",
    },
  );

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
