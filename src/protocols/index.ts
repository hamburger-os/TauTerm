import { bytesToHex, parseByteInput } from "../utils/byteInput.ts";
import { inspectAtResponse } from "./at.ts";
import { DEFAULT_CUSTOM_SCHEMA, inspectCustomSchema } from "./custom.ts";
import {
  inspectModbusAscii,
  inspectModbusRtu,
  inspectModbusTcp,
  isLikelyModbusTcp,
  isValidModbusRtu,
} from "./modbus.ts";
import { inspectNmea0183 } from "./nmea.ts";
import type {
  ProtocolInputKind,
  ProtocolInspectorOptions,
  ProtocolParseOutcome,
  ProtocolTemplate,
} from "./types.ts";

export * from "./types.ts";
export { DEFAULT_CUSTOM_SCHEMA } from "./custom.ts";

export const PROTOCOL_TEMPLATE_NAMES: Record<ProtocolTemplate, string> = {
  auto: "tools.protocolTemplateAuto",
  "modbus-rtu": "tools.protocolTemplateModbusRTU",
  "modbus-ascii": "tools.protocolTemplateModbusASCII",
  "modbus-tcp": "tools.protocolTemplateModbusTCP",
  "at-response": "tools.protocolTemplateAT",
  "nmea-0183": "tools.protocolTemplateNmea",
  raw: "tools.protocolTemplateRaw",
  "custom-schema": "tools.protocolTemplateCustomSchema",
};

export const PROTOCOL_TEMPLATE_INPUT_KIND: Record<ProtocolTemplate, ProtocolInputKind> = {
  auto: "mixed",
  "modbus-rtu": "hex",
  "modbus-ascii": "text",
  "modbus-tcp": "hex",
  "at-response": "text",
  "nmea-0183": "text",
  raw: "hex",
  "custom-schema": "hex",
};

function inspectRaw(input: string): ProtocolParseOutcome {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return { result: null, errorCode: parsed.error.code };
  const bytes = parsed.value.bytes;
  return {
    result: {
      inspectorId: "raw",
      fields: [
        {
          id: "raw",
          name: "tools.fieldRawData",
          range: { start: 0, length: bytes.length, unit: "byte" },
          rawValue: bytesToHex(bytes),
          parsedValue: "(" + bytes.length + " bytes)",
        },
      ],
      checks: [
        {
          id: "checksum",
          label: "tools.checkChecksum",
          status: "not-applicable",
        },
      ],
      issues: [],
      rawBytes: bytes,
      normalizedInput: parsed.value.normalizedHex,
    },
  };
}

function withDetection(
  outcome: ProtocolParseOutcome,
  template: Exclude<ProtocolTemplate, "auto">,
  confidence: "high" | "medium" | "low",
): ProtocolParseOutcome {
  if (outcome.result) outcome.result.detectedConfidence = confidence;
  return { ...outcome, detectedTemplate: template };
}

function autoInspect(
  input: string,
  options: ProtocolInspectorOptions,
): ProtocolParseOutcome {
  const trimmed = input.trim();
  if (!trimmed) return { result: null };

  if (trimmed.startsWith(":")) {
    const outcome = inspectModbusAscii(input, options);
    if (outcome.result) return withDetection(outcome, "modbus-ascii", "high");
  }

  if (/^[!$][A-Za-z0-9]{3,}/.test(trimmed)) {
    const outcome = inspectNmea0183(input);
    if (outcome.result) return withDetection(outcome, "nmea-0183", "high");
  }

  const byteInput = parseByteInput(input);
  if (byteInput.ok) {
    const bytes = byteInput.value.bytes;
    if (isValidModbusRtu(bytes)) {
      return withDetection(inspectModbusRtu(input, options), "modbus-rtu", "high");
    }
    if (isLikelyModbusTcp(bytes)) {
      return withDetection(inspectModbusTcp(input, options), "modbus-tcp", "high");
    }
    return withDetection(inspectRaw(input), "raw", "low");
  }

  if (
    /(?:^|\n)\s*AT(?:\+|$)/i.test(trimmed)
    || /(?:^|\n)\s*(?:\+[A-Za-z0-9_-]+:|OK$|ERROR$|NO CARRIER$|BUSY$|>)/im.test(trimmed)
  ) {
    return withDetection(inspectAtResponse(input), "at-response", "high");
  }

  return withDetection(inspectAtResponse(input), "at-response", "low");
}

export function parseProtocolInput(
  template: ProtocolTemplate,
  input: string,
  options: ProtocolInspectorOptions = {},
): ProtocolParseOutcome {
  if (!input.trim()) return { result: null };

  switch (template) {
    case "auto":
      return autoInspect(input, options);
    case "modbus-rtu":
      return inspectModbusRtu(input, options);
    case "modbus-ascii":
      return inspectModbusAscii(input, options);
    case "modbus-tcp":
      return inspectModbusTcp(input, options);
    case "at-response":
      return inspectAtResponse(input);
    case "nmea-0183":
      return inspectNmea0183(input);
    case "raw":
      return inspectRaw(input);
    case "custom-schema":
      return inspectCustomSchema(input, options.customSchema ?? DEFAULT_CUSTOM_SCHEMA);
    default:
      return { result: null, errorCode: "unsupportedProtocolInspector" };
  }
}
