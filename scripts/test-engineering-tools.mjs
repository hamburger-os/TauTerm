import assert from "node:assert/strict";
import {
  CRC_PRESETS,
  crc16,
  crcPreset,
  numberToHex,
} from "../src/utils/checksum.ts";
import { parseByteInput } from "../src/utils/byteInput.ts";
import {
  executeEncodingOp,
  swapEndian,
} from "../src/utils/encoding.ts";
import {
  bitwiseOp,
  extractBitRange,
  parseIntegerInput,
  parseStructDefinition,
} from "../src/utils/bitops.ts";
import { inspectByteData } from "../src/utils/dataInspector.ts";
import {
  diffBytes,
  serialTiming,
  subnetInfo,
  timestampInfo,
} from "../src/utils/engineering.ts";
import {
  DEFAULT_CUSTOM_SCHEMA,
  parseProtocolInput,
} from "../src/utils/protocolParsing.ts";

function ok(outcome, message = "expected successful tool result") {
  assert.equal(outcome.ok, true, message);
  return outcome.value;
}

function protocol(template, input, options = {}) {
  const outcome = parseProtocolInput(template, input, options);
  assert.ok(outcome.result, template + " should parse");
  return outcome.result;
}

function modbusRtu(payload) {
  const body = Uint8Array.from(payload);
  const crc = crc16(body, "CRC-16/MODBUS");
  return [
    ...payload,
    crc & 0xFF,
    (crc >>> 8) & 0xFF,
  ].map((value) => value.toString(16).toUpperCase().padStart(2, "0")).join(" ");
}

const check = new TextEncoder().encode("123456789");
for (const [name, definition] of Object.entries(CRC_PRESETS)) {
  const calculated = crcPreset(check, name);
  assert.equal(
    numberToHex(calculated, definition.params.width),
    numberToHex(definition.check, definition.params.width),
    name + " canonical check vector must stay stable",
  );
}

for (const sample of [
  "AA BB CC",
  "AABBCC",
  "0xAA,0xBB,0xCC",
  "\\xAA\\xBB\\xCC",
  "AA:BB:CC",
  "AA-BB-CC",
  "uint8_t data[] = { 0xAA, 0xBB, 0xCC };",
]) {
  assert.deepEqual(
    Array.from(ok(parseByteInput(sample)).bytes),
    [0xAA, 0xBB, 0xCC],
    "shared byte parser must normalize " + sample,
  );
}
assert.equal(parseByteInput("AA 0xGG").ok, false);
assert.equal(parseByteInput("0xAA0xBB").ok, false);

assert.equal(
  ok(executeEncodingOp("FFFFFFFFFFFFFFFF", "hex-to-dec")),
  "18446744073709551615",
);
assert.equal(executeEncodingOp("10102", "bin-to-dec").ok, false);
assert.equal(executeEncodingOp("41GG", "hex-to-string").ok, false);
assert.equal(
  ok(executeEncodingOp("E4 B8 AD E6 96 87", "hex-to-string")),
  "中文",
);
assert.equal(
  executeEncodingOp("FF", "hex-to-string").ok,
  false,
  "HEX-to-text must reject invalid UTF-8 instead of replacement decoding",
);
assert.equal(
  executeEncodingOp("SGVs\nbG8=", "base64-decode").ok,
  false,
  "strict Base64 must reject whitespace",
);
assert.equal(
  ok(executeEncodingOp("SGVs\nbG8=", "base64-decode", { base64IgnoreWhitespace: true })),
  "Hello",
);
assert.equal(
  ok(executeEncodingOp("01 02 03 04 05 06 07 08", "swap-endian-64")),
  "08 07 06 05 04 03 02 01",
);
assert.equal(ok(executeEncodingOp("12 34 56", "packed-bcd-to-dec")), "123456");
assert.equal(ok(executeEncodingOp("123456", "dec-to-packed-bcd")), "12 34 56");
assert.equal(
  ok(swapEndian("01 02 03 04", 2)),
  "02 01 04 03",
);
assert.equal(swapEndian("01 02 03", 2).ok, false);

assert.equal(parseIntegerInput("0xAA", 8), 170n);
assert.equal(parseIntegerInput("-1", 64), -1n);
assert.equal(parseIntegerInput("0xFFFFFFFFFFFFFFFF", 64), 0xFFFFFFFFFFFFFFFFn);
assert.equal(parseIntegerInput("0x100", 8), null);
const shifted = bitwiseOp(0x80000000n, 1n, "URSHIFT", 32);
assert.equal(shifted.hex, "40000000");
assert.equal(shifted.signed, "1073741824");
const not64 = bitwiseOp(0n, 0n, "NOT", 64);
assert.equal(not64.hex, "FFFFFFFFFFFFFFFF");
assert.equal(not64.signed, "-1");
assert.deepEqual(
  extractBitRange(0x1234n, 11, 8, 16),
  { maskHex: "0F00", valueHex: "2", unsigned: "2" },
);

const ilp32 = parseStructDefinition(
  "struct { char a; int b; void *p; }",
  "ILP32",
  0,
);
assert.ok(ilp32);
assert.equal(ilp32.members[1].paddingBefore, 3);
assert.equal(ilp32.members[2].size, 4);
const lp64 = parseStructDefinition(
  "struct { long a; void *p; }",
  "LP64",
  0,
);
assert.ok(lp64);
assert.equal(lp64.members[0].size, 8);
assert.equal(lp64.members[1].size, 8);
const llp64 = parseStructDefinition(
  "struct { long a; void *p; }",
  "LLP64",
  0,
);
assert.ok(llp64);
assert.equal(llp64.members[0].size, 4);
assert.equal(llp64.members[1].size, 8);
const packed = parseStructDefinition(
  "struct { char a; int b; }",
  "ILP32",
  1,
);
assert.ok(packed);
assert.equal(packed.totalSize, 5);
assert.equal(
  parseStructDefinition("struct { uint8_t a; mystery_t hidden; uint16_t b; }"),
  null,
);

const request = protocol(
  "modbus-rtu",
  "01 03 00 00 00 01 84 0A",
);
assert.equal(request.effectiveDirection, "request");
assert.equal(request.checks.find((item) => item.id === "crc")?.status, "pass");
assert.ok(request.fields.some((field) => field.id === "start-address"));
assert.ok(request.fields.some((field) => field.id === "quantity"));

const response = protocol(
  "modbus-rtu",
  "01 03 02 00 2A 39 9B",
);
assert.equal(response.effectiveDirection, "response");
const registerData = response.fields.find((field) => field.id === "read-data");
assert.equal(registerData?.children?.[0]?.parsedValue, "42 (0x002A)");

const semanticBad = protocol(
  "modbus-rtu",
  modbusRtu([0x01, 0x03, 0x00, 0x00, 0x00, 0x00]),
);
assert.equal(semanticBad.checks.find((item) => item.id === "crc")?.status, "pass");
assert.equal(
  semanticBad.checks.find((item) => item.id === "semantics")?.status,
  "fail",
  "valid CRC must not hide invalid Modbus quantity",
);

const crcBad = protocol(
  "modbus-rtu",
  "01 03 00 00 00 01 00 00",
);
assert.equal(crcBad.checks.find((item) => item.id === "crc")?.status, "fail");
assert.equal(
  crcBad.checks.find((item) => item.id === "structure")?.status,
  "pass",
  "checksum failure must stay separate from frame structure",
);

const ascii = protocol("modbus-ascii", ":010300000001FB\r\n");
assert.equal(ascii.checks.find((item) => item.id === "lrc")?.status, "pass");
assert.equal(ascii.fields[1].range.unit, "char");

const tcp = protocol(
  "modbus-tcp",
  "00 01 00 00 00 06 01 03 00 00 00 01",
);
assert.equal(tcp.checks.find((item) => item.id === "mbap")?.status, "pass");
assert.equal(tcp.effectiveDirection, "request");

const exception = protocol(
  "modbus-rtu",
  modbusRtu([0x01, 0x83, 0x02]),
);
assert.equal(exception.effectiveDirection, "response");
assert.ok(exception.fields.some((field) => field.id === "exception-code"));

const at = protocol(
  "at-response",
  "AT+CSQ\r\n+CSQ: 20,99\r\nOK",
);
assert.equal(at.fields[0].name, "tools.atCommandEcho");
assert.equal(at.fields[1].children?.length, 2);
assert.equal(at.fields[2].name, "tools.atFinalResult");

const nmea = protocol(
  "nmea-0183",
  "$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47",
);
assert.equal(nmea.checks.find((item) => item.id === "checksum")?.status, "pass");
assert.equal(nmea.fields[0].children?.find((item) => item.id === "param-1")?.parsedValue, "48.1173000°");

const custom = protocol(
  "custom-schema",
  "AA 55 01 04 00 DE AD BE EF",
  { customSchema: DEFAULT_CUSTOM_SCHEMA },
);
assert.equal(custom.checks.find((item) => item.id === "schema")?.status, "pass");
assert.equal(custom.fields[0].parsedValue, "43605 (Magic)");

const detectedRtu = parseProtocolInput(
  "auto",
  "01 03 00 00 00 01 84 0A",
);
assert.equal(detectedRtu.detectedTemplate, "modbus-rtu");
const detectedNmea = parseProtocolInput(
  "auto",
  "$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47",
);
assert.equal(detectedNmea.detectedTemplate, "nmea-0183");

const inspected = ok(inspectByteData("FF FF FF FF"));
assert.equal(
  inspected.interpretations.find((item) => item.id === "u32be")?.value,
  "4294967295",
);
assert.equal(
  inspected.interpretations.find((item) => item.id === "i32be")?.value,
  "-1",
);

const timing = ok(serialTiming(115200, 8, "none", 1, 100));
assert.equal(timing.bitsPerFrame, 10);
assert.ok(Math.abs(timing.millisecondsTotal - 8.6805555556) < 0.0001);

const subnet = ok(subnetInfo("192.168.1.100", 24));
assert.equal(subnet.network, "192.168.1.0");
assert.equal(subnet.broadcast, "192.168.1.255");
assert.equal(subnet.hostCount, "254");

const epoch = ok(timestampInfo("0", "seconds"));
assert.equal(epoch.iso, "1970-01-01T00:00:00.000Z");

const diff = ok(diffBytes("AA BB CC", "AA BC CC DD"));
assert.deepEqual(
  diff.differences.map((entry) => entry.offset),
  [1, 3],
);

console.log(
  "engineering-tools: byte input, CRC catalogue, encoding, bit/layout, protocol inspectors, data inspector and engineering conversions verified",
);
