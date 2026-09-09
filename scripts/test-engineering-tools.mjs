import assert from "node:assert/strict";
import {
  crc16,
  crc32,
  numberToHex,
  parseHexString,
} from "../src/utils/checksum.ts";
import {
  executeEncodingOp,
  swapEndian,
} from "../src/utils/encoding.ts";
import {
  bitwiseOp,
  parseIntegerInput,
  parseStructDefinition,
} from "../src/utils/bitops.ts";
import { parseProtocolInput } from "../src/utils/protocolParsing.ts";

const check = new TextEncoder().encode("123456789");
assert.equal(
  numberToHex(crc16(check, "CRC16-Modbus"), 16),
  "4B37",
  "CRC-16/MODBUS check vector must stay canonical",
);
assert.equal(
  numberToHex(crc32(check, "CRC32"), 32),
  "CBF43926",
  "CRC-32/ISO-HDLC must stay unsigned and format correctly",
);

assert.equal(
  executeEncodingOp("FFFFFFFFFFFFFFFF", "hex-to-dec"),
  "18446744073709551615",
  "integer conversion must not lose precision above Number.MAX_SAFE_INTEGER",
);
assert.match(
  executeEncodingOp("10102", "bin-to-dec"),
  /^\[Error:/,
  "binary conversion must reject invalid trailing digits instead of partially parsing",
);
assert.match(
  executeEncodingOp("0xAA0xBB", "hex-to-dec"),
  /^\[Error:/,
  "HEX conversion must reject repeated embedded prefixes",
);
assert.equal(executeEncodingOp("-0xFF", "hex-to-dec"), "-255");
assert.equal(executeEncodingOp("-11111111", "bin-to-hex"), "-FF");
assert.match(
  executeEncodingOp("41GG", "hex-to-string"),
  /^\[Error:/,
  "HEX-to-text conversion must reject malformed bytes instead of partially parsing",
);
assert.deepEqual(
  Array.from(parseHexString("0x01, 0x03, 00 00")),
  [0x01, 0x03, 0x00, 0x00],
);
assert.equal(
  parseHexString("01 03 0x00GG").length,
  0,
  "protocol/checksum HEX parsing must fail closed on malformed tokens",
);
assert.equal(swapEndian("01 02 03 04", 2), "0201 0403");
assert.match(
  swapEndian("01 02 03", 2),
  /^\[Error:/,
  "endian conversion must reject incomplete groups instead of silently dropping bytes",
);

assert.equal(parseIntegerInput("0xAA"), 170);
assert.equal(parseIntegerInput("0b1010"), 10);
assert.equal(parseIntegerInput("170"), 170);
assert.equal(parseIntegerInput("10oops"), null);
assert.equal(parseIntegerInput("0x100000000"), null);
assert.equal(bitwiseOp(0x80000000, 1, "URSHIFT").hex, "40000000");
assert.equal(
  parseStructDefinition("struct { uint8_t a; mystery_t hidden; uint16_t b; }"),
  null,
  "sizeof parser must fail closed when any member type is unsupported",
);

const rtu = parseProtocolInput("modbus-rtu", "01 03 00 00 00 01 84 0A");
assert.ok(rtu.result);
assert.equal(rtu.errorKey, undefined);
assert.equal(rtu.result.checksumValid, true);
assert.equal(rtu.result.fields[0].offset, 0);

const malformedRtu = parseProtocolInput("modbus-rtu", "01 03 00GG 00");
assert.equal(malformedRtu.result, null);
assert.equal(malformedRtu.errorKey, "tools.protocolParseErrorHex");

const ascii = parseProtocolInput("modbus-ascii", ":010300000001FB\r\n");
assert.ok(ascii.result);
assert.equal(ascii.result.checksumType, "lrc");
assert.equal(ascii.result.checksumValid, true);
assert.equal(ascii.result.checksumExpected, "0xFB");

const asciiWithoutTerminator = parseProtocolInput("modbus-ascii", ":010300000001FB");
assert.equal(asciiWithoutTerminator.result?.checksumValid, true);

const badAscii = parseProtocolInput("modbus-ascii", "010300000001FB");
assert.equal(badAscii.result, null);
assert.equal(badAscii.errorKey, "tools.protocolParseErrorASCII");

const exception = parseProtocolInput("modbus-rtu", "01 83 02 00 00");
assert.ok(exception.result);
assert.equal(
  exception.result.fields.some((field) => field.name === "tools.fieldExceptionCode"),
  true,
  "Modbus exception responses must expose their exception code",
);

const at = parseProtocolInput("at-response", "+CSQ: 20,99\r\nOK");
assert.ok(at.result);
assert.equal(at.result.checksumValid, null);
assert.equal(
  at.result.fields.some((field) => field.name === "+CSQ" && field.parsedValue === "20,99"),
  true,
);

console.log("engineering-tools: CRC, conversion, bitops and protocol parser contracts verified");
