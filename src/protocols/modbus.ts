import { bytesToHex, parseByteInput } from "../utils/byteInput.ts";
import { crc16, numberToHex } from "../utils/checksum.ts";
import type {
  ParseResult,
  ParsedField,
  ProtocolCheck,
  ProtocolDirection,
  ProtocolInspectorOptions,
  ProtocolIssue,
  ProtocolParseOutcome,
  ProtocolRange,
} from "./types.ts";

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

type RangeMapper = (start: number, length: number) => ProtocolRange;

interface PduParse {
  fields: ParsedField[];
  issues: ProtocolIssue[];
  direction: Exclude<ProtocolDirection, "auto"> | "ambiguous";
}

function byteToHex(value: number): string {
  return value.toString(16).toUpperCase().padStart(2, "0");
}

function wordAt(bytes: Uint8Array, offset: number): number {
  return (bytes[offset] << 8) | bytes[offset + 1];
}

function functionParsedValue(functionCode: number): string {
  return MODBUS_FUNCTION_KEYS[functionCode & 0x7F]
    ?? "0x" + byteToHex(functionCode);
}

function exceptionParsedValue(exceptionCode: number): string {
  return MODBUS_EXCEPTION_KEYS[exceptionCode]
    ?? "0x" + byteToHex(exceptionCode);
}

function field(
  id: string,
  name: string,
  range: ProtocolRange,
  rawValue: string,
  parsedValue?: string,
  children?: ParsedField[],
  nameParams?: Record<string, unknown>,
): ParsedField {
  return {
    id,
    name,
    range,
    rawValue,
    ...(parsedValue !== undefined ? { parsedValue } : {}),
    ...(children && children.length > 0 ? { children } : {}),
    ...(nameParams ? { nameParams } : {}),
  };
}

function pushIssue(
  issues: ProtocolIssue[],
  code: string,
  severity: ProtocolIssue["severity"],
  detail?: string,
  range?: ProtocolRange,
): void {
  issues.push({
    code,
    severity,
    ...(detail ? { detail } : {}),
    ...(range ? { range } : {}),
  });
}

function inferDirection(
  functionCode: number,
  data: Uint8Array,
): Exclude<ProtocolDirection, "auto"> | "ambiguous" {
  if ((functionCode & 0x80) !== 0) return "response";
  const base = functionCode & 0x7F;
  if (base >= 0x01 && base <= 0x04) {
    if (data.length === 4) return "request";
    if (data.length >= 1 && data.length === data[0] + 1) return "response";
    return "ambiguous";
  }
  if (base === 0x05 || base === 0x06) {
    return data.length === 4 ? "ambiguous" : "ambiguous";
  }
  if (base === 0x0F || base === 0x10) {
    if (data.length === 4) return "response";
    if (data.length >= 5) return "request";
    return "ambiguous";
  }
  return "ambiguous";
}

function resolveDirection(
  requested: ProtocolDirection | undefined,
  functionCode: number,
  data: Uint8Array,
): Exclude<ProtocolDirection, "auto"> | "ambiguous" {
  if (requested && requested !== "auto") return requested;
  return inferDirection(functionCode, data);
}

function quantityRangeFor(functionCode: number): [number, number] | null {
  switch (functionCode) {
    case 0x01:
    case 0x02:
      return [1, 2000];
    case 0x03:
    case 0x04:
      return [1, 125];
    case 0x0F:
      return [1, 1968];
    case 0x10:
      return [1, 123];
    default:
      return null;
  }
}

function addAddressQuantityFields(
  fields: ParsedField[],
  issues: ProtocolIssue[],
  data: Uint8Array,
  globalDataOffset: number,
  mapper: RangeMapper,
  functionCode: number,
): void {
  if (data.length !== 4) {
    pushIssue(
      issues,
      "modbusLengthMismatch",
      "error",
      "expected 4 data bytes, got " + data.length,
      mapper(globalDataOffset, data.length),
    );
    return;
  }

  const address = wordAt(data, 0);
  const quantity = wordAt(data, 2);
  fields.push(
    field(
      "start-address",
      "tools.fieldStartAddress",
      mapper(globalDataOffset, 2),
      bytesToHex(data.slice(0, 2)),
      address + " (0x" + address.toString(16).toUpperCase().padStart(4, "0") + ")",
    ),
  );
  fields.push(
    field(
      "quantity",
      "tools.fieldQuantity",
      mapper(globalDataOffset + 2, 2),
      bytesToHex(data.slice(2, 4)),
      String(quantity),
    ),
  );

  const allowed = quantityRangeFor(functionCode);
  if (allowed && (quantity < allowed[0] || quantity > allowed[1])) {
    pushIssue(
      issues,
      "modbusQuantityOutOfRange",
      "error",
      "quantity " + quantity + " outside " + allowed[0] + ".." + allowed[1],
      mapper(globalDataOffset + 2, 2),
    );
  }
}

function addReadResponseFields(
  fields: ParsedField[],
  issues: ProtocolIssue[],
  data: Uint8Array,
  globalDataOffset: number,
  mapper: RangeMapper,
  functionCode: number,
): void {
  if (data.length < 1) {
    pushIssue(issues, "modbusMissingByteCount", "error");
    return;
  }
  const byteCount = data[0];
  fields.push(
    field(
      "byte-count",
      "tools.fieldByteCount",
      mapper(globalDataOffset, 1),
      byteToHex(byteCount),
      String(byteCount),
    ),
  );

  const payload = data.slice(1);
  if (payload.length !== byteCount) {
    pushIssue(
      issues,
      "modbusByteCountMismatch",
      "error",
      "declared " + byteCount + ", actual " + payload.length,
      mapper(globalDataOffset, data.length),
    );
  }

  const children: ParsedField[] = [];
  if (functionCode === 0x03 || functionCode === 0x04) {
    if (byteCount % 2 !== 0) {
      pushIssue(
        issues,
        "modbusRegisterByteCountOdd",
        "error",
        String(byteCount),
        mapper(globalDataOffset, 1),
      );
    }
    for (let offset = 0; offset + 1 < payload.length; offset += 2) {
      const value = wordAt(payload, offset);
      children.push(
        field(
          "register-" + offset / 2,
          "tools.fieldRegisterN",
          mapper(globalDataOffset + 1 + offset, 2),
          bytesToHex(payload.slice(offset, offset + 2)),
          value + " (0x" + value.toString(16).toUpperCase().padStart(4, "0") + ")",
          undefined,
          { n: offset / 2 },
        ),
      );
    }
  }

  fields.push(
    field(
      "read-data",
      functionCode === 0x01 || functionCode === 0x02
        ? "tools.fieldBitData"
        : "tools.fieldRegisterData",
      mapper(globalDataOffset + 1, payload.length),
      bytesToHex(payload),
      "(" + payload.length + " bytes)",
      children,
    ),
  );
}

function addSingleWriteFields(
  fields: ParsedField[],
  issues: ProtocolIssue[],
  data: Uint8Array,
  globalDataOffset: number,
  mapper: RangeMapper,
  functionCode: number,
): void {
  if (data.length !== 4) {
    pushIssue(
      issues,
      "modbusLengthMismatch",
      "error",
      "expected 4 data bytes, got " + data.length,
      mapper(globalDataOffset, data.length),
    );
    return;
  }

  const address = wordAt(data, 0);
  const value = wordAt(data, 2);
  fields.push(
    field(
      "write-address",
      functionCode === 0x05 ? "tools.fieldCoilAddress" : "tools.fieldRegisterAddress",
      mapper(globalDataOffset, 2),
      bytesToHex(data.slice(0, 2)),
      address + " (0x" + address.toString(16).toUpperCase().padStart(4, "0") + ")",
    ),
  );
  fields.push(
    field(
      "write-value",
      functionCode === 0x05 ? "tools.fieldCoilValue" : "tools.fieldRegisterValue",
      mapper(globalDataOffset + 2, 2),
      bytesToHex(data.slice(2, 4)),
      functionCode === 0x05
        ? value === 0xFF00
          ? "ON"
          : value === 0x0000
            ? "OFF"
            : "0x" + value.toString(16).toUpperCase().padStart(4, "0")
        : value + " (0x" + value.toString(16).toUpperCase().padStart(4, "0") + ")",
    ),
  );

  if (functionCode === 0x05 && value !== 0x0000 && value !== 0xFF00) {
    pushIssue(
      issues,
      "modbusInvalidCoilValue",
      "error",
      "0x" + value.toString(16).toUpperCase().padStart(4, "0"),
      mapper(globalDataOffset + 2, 2),
    );
  }
}

function addMultiWriteRequestFields(
  fields: ParsedField[],
  issues: ProtocolIssue[],
  data: Uint8Array,
  globalDataOffset: number,
  mapper: RangeMapper,
  functionCode: number,
): void {
  if (data.length < 5) {
    pushIssue(
      issues,
      "modbusLengthMismatch",
      "error",
      "expected at least 5 data bytes, got " + data.length,
      mapper(globalDataOffset, data.length),
    );
    return;
  }

  const address = wordAt(data, 0);
  const quantity = wordAt(data, 2);
  const byteCount = data[4];
  const payload = data.slice(5);

  fields.push(
    field(
      "start-address",
      "tools.fieldStartAddress",
      mapper(globalDataOffset, 2),
      bytesToHex(data.slice(0, 2)),
      address + " (0x" + address.toString(16).toUpperCase().padStart(4, "0") + ")",
    ),
    field(
      "quantity",
      "tools.fieldQuantity",
      mapper(globalDataOffset + 2, 2),
      bytesToHex(data.slice(2, 4)),
      String(quantity),
    ),
    field(
      "byte-count",
      "tools.fieldByteCount",
      mapper(globalDataOffset + 4, 1),
      byteToHex(byteCount),
      String(byteCount),
    ),
    field(
      "write-data",
      "tools.fieldWriteData",
      mapper(globalDataOffset + 5, payload.length),
      bytesToHex(payload),
      "(" + payload.length + " bytes)",
    ),
  );

  const allowed = quantityRangeFor(functionCode);
  if (allowed && (quantity < allowed[0] || quantity > allowed[1])) {
    pushIssue(
      issues,
      "modbusQuantityOutOfRange",
      "error",
      "quantity " + quantity + " outside " + allowed[0] + ".." + allowed[1],
      mapper(globalDataOffset + 2, 2),
    );
  }
  if (payload.length !== byteCount) {
    pushIssue(
      issues,
      "modbusByteCountMismatch",
      "error",
      "declared " + byteCount + ", actual " + payload.length,
      mapper(globalDataOffset + 4, 1 + payload.length),
    );
  }

  const expectedByteCount =
    functionCode === 0x0F ? Math.ceil(quantity / 8) : quantity * 2;
  if (byteCount !== expectedByteCount) {
    pushIssue(
      issues,
      "modbusQuantityByteCountMismatch",
      "error",
      "quantity implies " + expectedByteCount + " bytes, declared " + byteCount,
      mapper(globalDataOffset + 2, 3),
    );
  }
}

function parsePdu(
  pdu: Uint8Array,
  globalPduOffset: number,
  mapper: RangeMapper,
  requestedDirection: ProtocolDirection | undefined,
): PduParse {
  const fields: ParsedField[] = [];
  const issues: ProtocolIssue[] = [];

  if (pdu.length < 1) {
    pushIssue(issues, "modbusMissingFunction", "error");
    return { fields, issues, direction: "ambiguous" };
  }

  const functionCode = pdu[0];
  const data = pdu.slice(1);
  const baseFunction = functionCode & 0x7F;
  const direction = resolveDirection(requestedDirection, functionCode, data);

  fields.push(
    field(
      "function",
      "tools.fieldFuncCode",
      mapper(globalPduOffset, 1),
      byteToHex(functionCode),
      functionParsedValue(functionCode),
    ),
  );

  if ((functionCode & 0x80) !== 0) {
    if (data.length !== 1) {
      pushIssue(
        issues,
        "modbusExceptionLength",
        "error",
        "expected 1 exception byte, got " + data.length,
        mapper(globalPduOffset + 1, data.length),
      );
    }
    if (data.length >= 1) {
      fields.push(
        field(
          "exception-code",
          "tools.fieldExceptionCode",
          mapper(globalPduOffset + 1, 1),
          byteToHex(data[0]),
          exceptionParsedValue(data[0]),
        ),
      );
    }
    return { fields, issues, direction: "response" };
  }

  if (!MODBUS_FUNCTION_KEYS[baseFunction]) {
    pushIssue(
      issues,
      "modbusUnsupportedFunction",
      "warning",
      "0x" + byteToHex(baseFunction),
      mapper(globalPduOffset, 1),
    );
    if (data.length > 0) {
      fields.push(
        field(
          "data",
          "tools.fieldData",
          mapper(globalPduOffset + 1, data.length),
          bytesToHex(data),
          "(" + data.length + " bytes)",
        ),
      );
    }
    return { fields, issues, direction };
  }

  const globalDataOffset = globalPduOffset + 1;
  if (baseFunction >= 0x01 && baseFunction <= 0x04) {
    if (direction === "request") {
      addAddressQuantityFields(
        fields,
        issues,
        data,
        globalDataOffset,
        mapper,
        baseFunction,
      );
    } else if (direction === "response") {
      addReadResponseFields(
        fields,
        issues,
        data,
        globalDataOffset,
        mapper,
        baseFunction,
      );
    } else {
      pushIssue(issues, "modbusDirectionAmbiguous", "warning");
      fields.push(
        field(
          "data",
          "tools.fieldData",
          mapper(globalDataOffset, data.length),
          bytesToHex(data),
          "(" + data.length + " bytes)",
        ),
      );
    }
  } else if (baseFunction === 0x05 || baseFunction === 0x06) {
    addSingleWriteFields(
      fields,
      issues,
      data,
      globalDataOffset,
      mapper,
      baseFunction,
    );
  } else if (baseFunction === 0x0F || baseFunction === 0x10) {
    if (direction === "request") {
      addMultiWriteRequestFields(
        fields,
        issues,
        data,
        globalDataOffset,
        mapper,
        baseFunction,
      );
    } else if (direction === "response") {
      addAddressQuantityFields(
        fields,
        issues,
        data,
        globalDataOffset,
        mapper,
        baseFunction,
      );
    } else {
      pushIssue(issues, "modbusDirectionAmbiguous", "warning");
      fields.push(
        field(
          "data",
          "tools.fieldData",
          mapper(globalDataOffset, data.length),
          bytesToHex(data),
          "(" + data.length + " bytes)",
        ),
      );
    }
  }

  return { fields, issues, direction };
}

function semanticChecks(issues: ProtocolIssue[]): ProtocolCheck[] {
  const structuralCodes = new Set([
    "modbusLengthMismatch",
    "modbusMissingByteCount",
    "modbusByteCountMismatch",
    "modbusRegisterByteCountOdd",
    "modbusMissingFunction",
    "modbusExceptionLength",
    "modbusTcpProtocolId",
    "modbusTcpLengthMismatch",
  ]);
  const relevant = issues.filter((issue) => issue.code !== "checksumMismatch");
  const structuralError = relevant.some(
    (issue) => issue.severity === "error" && structuralCodes.has(issue.code),
  );
  const semanticError = relevant.some((issue) => issue.severity === "error");
  const semanticWarning = relevant.some((issue) => issue.severity === "warning");
  return [
    {
      id: "structure",
      label: "tools.checkFrameStructure",
      status: structuralError ? "fail" : "pass",
    },
    {
      id: "semantics",
      label: "tools.checkProtocolSemantics",
      status: semanticError ? "fail" : semanticWarning ? "warning" : "pass",
    },
  ];
}

function validateSerialAddress(
  address: number,
  direction: PduParse["direction"],
  mapper: RangeMapper,
): ProtocolIssue[] {
  const issues: ProtocolIssue[] = [];
  if (address === 0) {
    pushIssue(
      issues,
      "modbusBroadcastAddress",
      direction === "response" ? "error" : "info",
      undefined,
      mapper(0, 1),
    );
  } else if (address > 247) {
    pushIssue(
      issues,
      "modbusSerialAddressRange",
      "warning",
      String(address),
      mapper(0, 1),
    );
  }
  return issues;
}

export function isValidModbusRtu(bytes: Uint8Array): boolean {
  if (bytes.length < 4) return false;
  const offset = bytes.length - 2;
  const actual = bytes[offset] | (bytes[offset + 1] << 8);
  return crc16(bytes.slice(0, offset), "CRC-16/MODBUS") === actual;
}

export function inspectModbusRtu(
  input: string,
  options: ProtocolInspectorOptions = {},
): ProtocolParseOutcome {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return { result: null, errorCode: parsed.error.code };
  const bytes = parsed.value.bytes;
  if (bytes.length < 4) return { result: null, errorCode: "protocolFrameTooShort" };

  const crcOffset = bytes.length - 2;
  const actualCrc = bytes[crcOffset] | (bytes[crcOffset + 1] << 8);
  const expectedCrc = crc16(bytes.slice(0, crcOffset), "CRC-16/MODBUS");
  const mapper: RangeMapper = (start, length) => ({ start, length, unit: "byte" });

  const address = bytes[0];
  const pdu = parsePdu(
    bytes.slice(1, crcOffset),
    1,
    mapper,
    options.direction,
  );
  const issues = [
    ...validateSerialAddress(address, pdu.direction, mapper),
    ...pdu.issues,
  ];
  if (actualCrc !== expectedCrc) {
    pushIssue(
      issues,
      "checksumMismatch",
      "error",
      "expected 0x" + numberToHex(expectedCrc, 16),
      mapper(crcOffset, 2),
    );
  }

  const fields: ParsedField[] = [
    field(
      "address",
      "tools.fieldDevAddr",
      mapper(0, 1),
      byteToHex(address),
      String(address),
    ),
    ...pdu.fields,
    field(
      "crc",
      "tools.fieldCRC16",
      mapper(crcOffset, 2),
      bytesToHex(bytes.slice(crcOffset)),
      "0x" + numberToHex(actualCrc, 16),
    ),
  ];

  const checks: ProtocolCheck[] = [
    {
      id: "crc",
      label: "tools.checkCrc",
      status: actualCrc === expectedCrc ? "pass" : "fail",
      detail:
        "received 0x" + numberToHex(actualCrc, 16)
        + ", calculated 0x" + numberToHex(expectedCrc, 16),
    },
    ...semanticChecks(issues),
  ];

  return {
    result: {
      inspectorId: "modbus-rtu",
      fields,
      checks,
      issues,
      rawBytes: bytes,
      normalizedInput: parsed.value.normalizedHex,
      effectiveDirection: pdu.direction,
    },
  };
}

function lrc(bytes: Uint8Array): number {
  let sum = 0;
  for (const byte of bytes) sum = (sum + byte) & 0xFF;
  return (-sum) & 0xFF;
}

export function inspectModbusAscii(
  input: string,
  options: ProtocolInspectorOptions = {},
): ProtocolParseOutcome {
  const frame = input.trim();
  if (!/^:[0-9A-Fa-f]+$/.test(frame)) {
    return { result: null, errorCode: "protocolParseErrorASCII" };
  }
  const payload = frame.slice(1);
  if (payload.length < 6 || payload.length % 2 !== 0) {
    return { result: null, errorCode: "protocolParseErrorASCII" };
  }

  const parsed = parseByteInput(payload);
  if (!parsed.ok || parsed.value.bytes.length < 3) {
    return { result: null, errorCode: "protocolParseErrorASCII" };
  }
  const bytes = parsed.value.bytes;
  const mapper: RangeMapper = (start, length) => ({
    start: 1 + start * 2,
    length: length * 2,
    unit: "char",
  });

  const lrcOffset = bytes.length - 1;
  const actualLrc = bytes[lrcOffset];
  const expectedLrc = lrc(bytes.slice(0, lrcOffset));
  const address = bytes[0];
  const pdu = parsePdu(
    bytes.slice(1, lrcOffset),
    1,
    mapper,
    options.direction,
  );
  const issues = [
    ...validateSerialAddress(address, pdu.direction, mapper),
    ...pdu.issues,
  ];
  if (actualLrc !== expectedLrc) {
    pushIssue(
      issues,
      "checksumMismatch",
      "error",
      "expected 0x" + byteToHex(expectedLrc),
      mapper(lrcOffset, 1),
    );
  }

  const fields: ParsedField[] = [
    field(
      "start",
      "tools.fieldStartDelim",
      { start: 0, length: 1, unit: "char" },
      ":",
      ":",
    ),
    field(
      "address",
      "tools.fieldAddress",
      mapper(0, 1),
      byteToHex(address),
      String(address),
    ),
    ...pdu.fields,
    field(
      "lrc",
      "tools.fieldLRC",
      mapper(lrcOffset, 1),
      byteToHex(actualLrc),
      "0x" + byteToHex(actualLrc),
    ),
  ];

  return {
    result: {
      inspectorId: "modbus-ascii",
      fields,
      checks: [
        {
          id: "lrc",
          label: "tools.checkLrc",
          status: actualLrc === expectedLrc ? "pass" : "fail",
          detail:
            "received 0x" + byteToHex(actualLrc)
            + ", calculated 0x" + byteToHex(expectedLrc),
        },
        ...semanticChecks(issues),
      ],
      issues,
      normalizedInput: frame,
      effectiveDirection: pdu.direction,
    },
  };
}

export function isLikelyModbusTcp(bytes: Uint8Array): boolean {
  if (bytes.length < 8) return false;
  const protocolId = wordAt(bytes, 2);
  const length = wordAt(bytes, 4);
  return protocolId === 0 && length === bytes.length - 6;
}

export function inspectModbusTcp(
  input: string,
  options: ProtocolInspectorOptions = {},
): ProtocolParseOutcome {
  const parsed = parseByteInput(input);
  if (!parsed.ok) return { result: null, errorCode: parsed.error.code };
  const bytes = parsed.value.bytes;
  if (bytes.length < 8) return { result: null, errorCode: "protocolFrameTooShort" };
  const mapper: RangeMapper = (start, length) => ({ start, length, unit: "byte" });

  const transactionId = wordAt(bytes, 0);
  const protocolId = wordAt(bytes, 2);
  const declaredLength = wordAt(bytes, 4);
  const actualLength = bytes.length - 6;
  const unitId = bytes[6];
  const pdu = parsePdu(bytes.slice(7), 7, mapper, options.direction);
  const issues = [...pdu.issues];

  if (protocolId !== 0) {
    pushIssue(
      issues,
      "modbusTcpProtocolId",
      "error",
      String(protocolId),
      mapper(2, 2),
    );
  }
  if (declaredLength !== actualLength) {
    pushIssue(
      issues,
      "modbusTcpLengthMismatch",
      "error",
      "declared " + declaredLength + ", actual " + actualLength,
      mapper(4, 2),
    );
  }

  const fields: ParsedField[] = [
    field(
      "transaction-id",
      "tools.fieldTransactionId",
      mapper(0, 2),
      bytesToHex(bytes.slice(0, 2)),
      String(transactionId),
    ),
    field(
      "protocol-id",
      "tools.fieldProtocolId",
      mapper(2, 2),
      bytesToHex(bytes.slice(2, 4)),
      String(protocolId),
    ),
    field(
      "length",
      "tools.fieldLength",
      mapper(4, 2),
      bytesToHex(bytes.slice(4, 6)),
      String(declaredLength),
    ),
    field(
      "unit-id",
      "tools.fieldUnitId",
      mapper(6, 1),
      byteToHex(unitId),
      String(unitId),
    ),
    ...pdu.fields,
  ];

  return {
    result: {
      inspectorId: "modbus-tcp",
      fields,
      checks: [
        {
          id: "mbap",
          label: "tools.checkMbap",
          status:
            protocolId === 0 && declaredLength === actualLength
              ? "pass"
              : "fail",
        },
        ...semanticChecks(issues),
      ],
      issues,
      rawBytes: bytes,
      normalizedInput: parsed.value.normalizedHex,
      effectiveDirection: pdu.direction,
    },
  };
}
