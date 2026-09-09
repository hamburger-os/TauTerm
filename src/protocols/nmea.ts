import type {
  ParsedField,
  ProtocolCheckStatus,
  ProtocolIssue,
  ProtocolParseOutcome,
  ProtocolRange,
} from "./types.ts";

function charRange(start: number, length: number): ProtocolRange {
  return { start, length, unit: "char" };
}

function xorChecksum(source: string): number {
  let checksum = 0;
  for (let index = 0; index < source.length; index += 1) {
    checksum ^= source.charCodeAt(index);
  }
  return checksum & 0xFF;
}

function hexByte(value: number): string {
  return value.toString(16).toUpperCase().padStart(2, "0");
}

function coordinate(value: string, hemisphere: string): string | undefined {
  if (!/^\d+(?:\.\d+)?$/.test(value)) return undefined;
  const dot = value.indexOf(".");
  const degreeDigits = (dot >= 0 ? dot : value.length) > 4 ? 3 : 2;
  const degrees = Number(value.slice(0, degreeDigits));
  const minutes = Number(value.slice(degreeDigits));
  if (!Number.isFinite(degrees) || !Number.isFinite(minutes)) return undefined;
  let decimal = degrees + minutes / 60;
  if (hemisphere === "S" || hemisphere === "W") decimal = -decimal;
  return decimal.toFixed(7) + "°";
}

const GGA_NAMES = [
  "tools.nmeaUtc",
  "tools.nmeaLatitude",
  "tools.nmeaNorthSouth",
  "tools.nmeaLongitude",
  "tools.nmeaEastWest",
  "tools.nmeaFixQuality",
  "tools.nmeaSatellites",
  "tools.nmeaHdop",
  "tools.nmeaAltitude",
  "tools.nmeaAltitudeUnit",
  "tools.nmeaGeoidSeparation",
  "tools.nmeaGeoidUnit",
  "tools.nmeaDgpsAge",
  "tools.nmeaDgpsStation",
];

const RMC_NAMES = [
  "tools.nmeaUtc",
  "tools.nmeaStatus",
  "tools.nmeaLatitude",
  "tools.nmeaNorthSouth",
  "tools.nmeaLongitude",
  "tools.nmeaEastWest",
  "tools.nmeaSpeedKnots",
  "tools.nmeaCourse",
  "tools.nmeaDate",
  "tools.nmeaMagneticVariation",
  "tools.nmeaMagneticDirection",
  "tools.nmeaMode",
];

function fieldNames(formatter: string): string[] {
  if (formatter === "GGA") return GGA_NAMES;
  if (formatter === "RMC") return RMC_NAMES;
  return [];
}

function parameterOffsets(body: string, sentenceStart: number): number[] {
  const offsets: number[] = [];
  let offset = body.indexOf(",");
  while (offset >= 0) {
    offsets.push(sentenceStart + 1 + offset + 1);
    offset = body.indexOf(",", offset + 1);
  }
  return offsets;
}

export function inspectNmea0183(input: string): ProtocolParseOutcome {
  const normalized = input.replace(/\r\n/g, "\n").replace(/\r/g, "\n").trim();
  if (!normalized) return { result: null, errorCode: "emptyInput" };

  const fields: ParsedField[] = [];
  const issues: ProtocolIssue[] = [];
  const checksumStatuses: ProtocolCheckStatus[] = [];
  let cursor = 0;

  for (const rawLine of normalized.split("\n")) {
    const leading = rawLine.length - rawLine.trimStart().length;
    const line = rawLine.trim();
    const lineStart = cursor + leading;
    cursor += rawLine.length + 1;
    if (!line) continue;

    if (!/^[!$]/.test(line)) {
      issues.push({
        code: "nmeaStartDelimiter",
        severity: "error",
        range: charRange(lineStart, line.length),
      });
      continue;
    }

    const star = line.lastIndexOf("*");
    const hasChecksum = star >= 0;
    const body = line.slice(1, hasChecksum ? star : undefined);
    const checksumText = hasChecksum ? line.slice(star + 1) : "";
    const checksumValidSyntax = !hasChecksum || /^[0-9A-Fa-f]{2}$/.test(checksumText);
    const calculated = xorChecksum(body);
    const received = checksumValidSyntax && hasChecksum
      ? Number.parseInt(checksumText, 16)
      : undefined;

    let checksumStatus: ProtocolCheckStatus = "warning";
    if (hasChecksum && checksumValidSyntax) {
      checksumStatus = received === calculated ? "pass" : "fail";
      if (received !== calculated) {
        issues.push({
          code: "checksumMismatch",
          severity: "error",
          detail: "expected 0x" + hexByte(calculated),
          range: charRange(lineStart + star + 1, 2),
        });
      }
    } else if (hasChecksum) {
      checksumStatus = "fail";
      issues.push({
        code: "nmeaChecksumFormat",
        severity: "error",
        range: charRange(lineStart + star + 1, checksumText.length),
      });
    } else {
      issues.push({
        code: "nmeaChecksumMissing",
        severity: "warning",
        range: charRange(lineStart, line.length),
      });
    }
    checksumStatuses.push(checksumStatus);

    const parts = body.split(",");
    const sentenceId = parts[0];
    const talker = sentenceId.length >= 5 ? sentenceId.slice(0, 2) : "";
    const formatter = sentenceId.length >= 5 ? sentenceId.slice(2, 5) : sentenceId;
    const params = parts.slice(1);
    const names = fieldNames(formatter);
    const offsets = parameterOffsets(body, lineStart);
    const children: ParsedField[] = [
      {
        id: "talker",
        name: "tools.nmeaTalker",
        range: charRange(lineStart + 1, Math.min(2, sentenceId.length)),
        rawValue: talker,
        parsedValue: talker,
      },
      {
        id: "formatter",
        name: "tools.nmeaSentenceType",
        range: charRange(lineStart + 3, Math.max(0, sentenceId.length - 2)),
        rawValue: formatter,
        parsedValue: formatter,
      },
    ];

    params.forEach((value, index) => {
      let parsedValue = value;
      if (formatter === "GGA" && index === 1) {
        parsedValue = coordinate(value, params[2]) ?? value;
      } else if (formatter === "GGA" && index === 3) {
        parsedValue = coordinate(value, params[4]) ?? value;
      } else if (formatter === "RMC" && index === 2) {
        parsedValue = coordinate(value, params[3]) ?? value;
      } else if (formatter === "RMC" && index === 4) {
        parsedValue = coordinate(value, params[5]) ?? value;
      }

      children.push({
        id: "param-" + index,
        name: names[index] ?? "tools.nmeaField",
        nameParams: names[index] ? undefined : { n: index + 1 },
        range: charRange(offsets[index] ?? lineStart, value.length),
        rawValue: value,
        parsedValue,
      });
    });

    if (hasChecksum) {
      children.push({
        id: "checksum",
        name: "tools.fieldChecksum",
        range: charRange(lineStart + star + 1, checksumText.length),
        rawValue: checksumText,
        parsedValue:
          "0x" + checksumText.toUpperCase()
          + " / calc 0x" + hexByte(calculated),
      });
    }

    fields.push({
      id: "sentence-" + fields.length,
      name: "tools.nmeaSentence",
      range: charRange(lineStart, line.length),
      rawValue: line,
      parsedValue: sentenceId,
      children,
    });
  }

  if (fields.length === 0) {
    return { result: null, errorCode: "nmeaParseError" };
  }

  const checksumStatus = checksumStatuses.includes("fail")
    ? "fail"
    : checksumStatuses.includes("warning")
      ? "warning"
      : "pass";

  return {
    result: {
      inspectorId: "nmea-0183",
      fields,
      checks: [
        {
          id: "structure",
          label: "tools.checkFrameStructure",
          status: issues.some((issue) => issue.code === "nmeaStartDelimiter")
            ? "fail"
            : "pass",
        },
        {
          id: "checksum",
          label: "tools.checkChecksum",
          status: checksumStatus,
        },
      ],
      issues,
      normalizedInput: normalized,
    },
  };
}
