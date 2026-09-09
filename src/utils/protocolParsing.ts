import {
  crc16,
  numberToHex,
  parseHexString,
} from "./checksum.ts";

export type ProtocolTemplate =
  | "at-response"
  | "modbus-rtu"
  | "modbus-ascii"
  | "custom";

export type ProtocolInputKind = "hex" | "text";

export interface ParsedField {
  name: string;
  offset: number;
  length: number;
  hexValue: string;
  parsedValue: string;
  nameParams?: Record<string, unknown>;
}

export interface ParseResult {
  fields: ParsedField[];
  checksumValid: boolean | null;
  checksumInfo: string;
  checksumType?: "crc" | "lrc";
  checksumExpected?: string;
}

export interface ProtocolParseOutcome {
  result: ParseResult | null;
  errorKey?: string;
}

export const PROTOCOL_TEMPLATE_NAMES: Record<ProtocolTemplate, string> = {
  "at-response": "tools.protocolTemplateAT",
  "modbus-rtu": "tools.protocolTemplateModbusRTU",
  "modbus-ascii": "tools.protocolTemplateModbusASCII",
  "custom": "tools.protocolTemplateCustom",
};

export const PROTOCOL_TEMPLATE_INPUT_KIND: Record<ProtocolTemplate, ProtocolInputKind> = {
  "at-response": "text",
  "modbus-rtu": "hex",
  "modbus-ascii": "text",
  "custom": "hex",
};

const MODBUS_FUNCTION_KEYS: Record<number, string> = {
  0x01: "tools.funcReadCoils",
  0x02: "tools.funcReadDiscreteInputs",
  0x03: "tools.funcReadHoldingRegs",
  0x04: "tools.funcReadInputRegs",
  0x05: "tools.funcWriteSingleCoil",
  0x06: "tools.funcWriteSingleReg",
  0x0F: "tools.funcWriteMultiCoils",
  0x10: "tools.funcWriteMultiRegs",
};

const MODBUS_EXCEPTION_KEYS: Record<number, string> = {
  0x01: "tools.modbusExceptions.illegalFunction",
  0x02: "tools.modbusExceptions.illegalDataAddress",
  0x03: "tools.modbusExceptions.illegalDataValue",
  0x04: "tools.modbusExceptions.serverDeviceFailure",
  0x05: "tools.modbusExceptions.acknowledge",
  0x06: "tools.modbusExceptions.serverDeviceBusy",
  0x08: "tools.modbusExceptions.memoryParityError",
  0x0A: "tools.modbusExceptions.gatewayPathUnavailable",
  0x0B: "tools.modbusExceptions.gatewayTargetNoResponse",
};

function byteToHex(value: number): string {
  return value.toString(16).toUpperCase().padStart(2, "0");
}

function bytesToSpacedHex(bytes: Uint8Array): string {
  return Array.from(bytes, byteToHex).join(" ");
}

function functionParsedValue(functionCode: number): string {
  const baseCode = functionCode & 0x7F;
  return MODBUS_FUNCTION_KEYS[baseCode]
    ?? `0x${byteToHex(functionCode)}`;
}

function exceptionParsedValue(exceptionCode: number): string {
  return MODBUS_EXCEPTION_KEYS[exceptionCode]
    ?? `0x${byteToHex(exceptionCode)}`;
}

function parseModbusRTU(bytes: Uint8Array): ParseResult {
  const fields: ParsedField[] = [];
  const functionCode = bytes[1];
  const isException = (functionCode & 0x80) !== 0;

  fields.push({
    name: "tools.fieldDevAddr",
    offset: 0,
    length: 1,
    hexValue: byteToHex(bytes[0]),
    parsedValue: String(bytes[0]),
  });
  fields.push({
    name: "tools.fieldFuncCode",
    offset: 1,
    length: 1,
    hexValue: byteToHex(functionCode),
    parsedValue: functionParsedValue(functionCode),
  });

  const dataLength = bytes.length - 4;
  if (dataLength > 0) {
    const dataBytes = bytes.slice(2, 2 + dataLength);
    if (isException && dataBytes.length >= 1) {
      fields.push({
        name: "tools.fieldExceptionCode",
        offset: 2,
        length: 1,
        hexValue: byteToHex(dataBytes[0]),
        parsedValue: exceptionParsedValue(dataBytes[0]),
      });
      if (dataBytes.length > 1) {
        fields.push({
          name: "tools.fieldData",
          offset: 3,
          length: dataBytes.length - 1,
          hexValue: bytesToSpacedHex(dataBytes.slice(1)),
          parsedValue: `(${dataBytes.length - 1} bytes)`,
        });
      }
    } else {
      fields.push({
        name: "tools.fieldData",
        offset: 2,
        length: dataLength,
        hexValue: bytesToSpacedHex(dataBytes),
        parsedValue: dataLength > 1 ? `(${dataLength} bytes)` : String(dataBytes[0]),
      });
    }
  }

  const crcOffset = bytes.length - 2;
  const crcBytes = bytes.slice(crcOffset);
  const actualCrc = (crcBytes[1] << 8) | crcBytes[0];
  const expectedCrc = crc16(bytes.slice(0, crcOffset), "CRC16-Modbus");

  fields.push({
    name: "tools.fieldCRC16",
    offset: crcOffset,
    length: 2,
    hexValue: bytesToSpacedHex(crcBytes),
    parsedValue: `0x${numberToHex(actualCrc, 16)}`,
  });

  return {
    fields,
    checksumValid: actualCrc === expectedCrc,
    checksumInfo: "",
    checksumType: "crc",
    checksumExpected: `0x${numberToHex(expectedCrc, 16)}`,
  };
}

function normalizeModbusASCIIInput(input: string): string {
  const trimmed = input.trim();
  return trimmed.endsWith("\r\n") ? trimmed.slice(0, -2) : trimmed;
}

function parseModbusASCII(input: string): ParseResult | null {
  const frame = normalizeModbusASCIIInput(input);
  if (!/^:[0-9A-Fa-f]+$/.test(frame)) return null;

  const payloadHex = frame.slice(1);
  // Address + Function + LRC 至少 3 bytes；每个 byte 由 2 个 ASCII HEX 字符表示。
  if (payloadHex.length < 6 || payloadHex.length % 2 !== 0) return null;

  const bytes = parseHexString(payloadHex);
  if (bytes.length < 3) return null;

  const address = bytes[0];
  const functionCode = bytes[1];
  const isException = (functionCode & 0x80) !== 0;
  const actualLrc = bytes[bytes.length - 1];
  const dataBytes = bytes.slice(2, bytes.length - 1);

  let sum = 0;
  for (const byte of bytes.slice(0, bytes.length - 1)) {
    sum = (sum + byte) & 0xFF;
  }
  const expectedLrc = (-sum) & 0xFF;

  const fields: ParsedField[] = [
    {
      name: "tools.fieldStartDelim",
      offset: 0,
      length: 1,
      hexValue: "3A",
      parsedValue: ":",
    },
    {
      name: "tools.fieldAddress",
      offset: 1,
      length: 2,
      hexValue: byteToHex(address),
      parsedValue: String(address),
    },
    {
      name: "tools.fieldFuncCode",
      offset: 3,
      length: 2,
      hexValue: byteToHex(functionCode),
      parsedValue: functionParsedValue(functionCode),
    },
  ];

  if (dataBytes.length > 0) {
    if (isException) {
      fields.push({
        name: "tools.fieldExceptionCode",
        offset: 5,
        length: 2,
        hexValue: byteToHex(dataBytes[0]),
        parsedValue: exceptionParsedValue(dataBytes[0]),
      });
      if (dataBytes.length > 1) {
        fields.push({
          name: "tools.fieldData",
          offset: 7,
          length: (dataBytes.length - 1) * 2,
          hexValue: Array.from(dataBytes.slice(1), byteToHex).join(""),
          parsedValue: `(${dataBytes.length - 1} bytes)`,
        });
      }
    } else {
      fields.push({
        name: "tools.fieldData",
        offset: 5,
        length: dataBytes.length * 2,
        hexValue: Array.from(dataBytes, byteToHex).join(""),
        parsedValue: `(${dataBytes.length} bytes)`,
      });
    }
  }

  fields.push({
    name: "tools.fieldLRC",
    offset: frame.length - 2,
    length: 2,
    hexValue: byteToHex(actualLrc),
    parsedValue: `0x${byteToHex(actualLrc)}`,
  });

  return {
    fields,
    checksumValid: actualLrc === expectedLrc,
    checksumInfo: "",
    checksumType: "lrc",
    checksumExpected: `0x${byteToHex(expectedLrc)}`,
  };
}

function parseATResponse(input: string): ParseResult {
  const normalized = input.replace(/\r\n/g, "\n").replace(/\r/g, "\n").trim();
  const lines = normalized
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const fields: ParsedField[] = [];

  if (lines.length > 0) {
    const firstLine = lines[0];
    if (
      firstLine.startsWith("+")
      || firstLine === "OK"
      || firstLine.startsWith("ERROR")
    ) {
      fields.push({
        name: "tools.fieldRespType",
        offset: 0,
        length: firstLine.length,
        hexValue: bytesToSpacedHex(new TextEncoder().encode(firstLine)),
        parsedValue: firstLine,
      });
    } else {
      fields.push({
        name: "tools.fieldLine",
        offset: 0,
        length: firstLine.length,
        hexValue: "",
        parsedValue: firstLine,
        nameParams: { n: 1 },
      });
    }

    const colonMatch = firstLine.match(/^\+(\w+):\s*(.+)/);
    if (colonMatch) {
      fields.push({
        name: `+${colonMatch[1]}`,
        offset: 0,
        length: firstLine.length,
        hexValue: "",
        parsedValue: colonMatch[2],
      });
    }
  }

  for (let i = 1; i < lines.length; i++) {
    fields.push({
      name: "tools.fieldLine",
      offset: 0,
      length: lines[i].length,
      hexValue: "",
      parsedValue: lines[i],
      nameParams: { n: i + 1 },
    });
  }

  return {
    fields,
    checksumValid: null,
    checksumInfo: "tools.noChecksum",
  };
}

function parseCustom(bytes: Uint8Array): ParseResult {
  return {
    fields: [{
      name: "tools.fieldRawData",
      offset: 0,
      length: bytes.length,
      hexValue: bytesToSpacedHex(bytes),
      parsedValue: `(${bytes.length} bytes)`,
    }],
    checksumValid: null,
    checksumInfo: "tools.rawDataOnly",
  };
}

export function parseProtocolInput(
  template: ProtocolTemplate,
  input: string,
): ProtocolParseOutcome {
  if (!input.trim()) return { result: null };

  if (template === "modbus-ascii") {
    const result = parseModbusASCII(input);
    return result
      ? { result }
      : { result: null, errorKey: "tools.protocolParseErrorASCII" };
  }

  if (template === "at-response") {
    return { result: parseATResponse(input) };
  }

  const bytes = parseHexString(input);
  if (bytes.length === 0) {
    return { result: null, errorKey: "tools.protocolParseErrorHex" };
  }

  if (template === "modbus-rtu") {
    if (bytes.length < 4) {
      return { result: null, errorKey: "tools.protocolFrameTooShort" };
    }
    return { result: parseModbusRTU(bytes) };
  }

  return { result: parseCustom(bytes) };
}
