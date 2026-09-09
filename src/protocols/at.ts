import type {
  ParsedField,
  ProtocolParseOutcome,
  ProtocolRange,
} from "./types.ts";

function charRange(start: number, length: number): ProtocolRange {
  return { start, length, unit: "char" };
}

function csvTokens(source: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quoted = false;
  for (let index = 0; index < source.length; index += 1) {
    const char = source[index];
    if (char === "\"") {
      quoted = !quoted;
      current += char;
    } else if (char === "," && !quoted) {
      tokens.push(current.trim());
      current = "";
    } else {
      current += char;
    }
  }
  tokens.push(current.trim());
  return tokens;
}

function lineType(line: string): {
  name: string;
  parsed: string;
  severity?: "error";
} {
  if (/^AT(?:\+|$)/i.test(line)) {
    return { name: "tools.atCommandEcho", parsed: line };
  }
  if (/^\+(?:CME|CMS)\s+ERROR\s*:/i.test(line)) {
    return { name: "tools.atFinalResult", parsed: line, severity: "error" };
  }
  if (/^(?:OK|ERROR|NO CARRIER|BUSY|NO ANSWER|NO DIALTONE)$/i.test(line)) {
    return {
      name: "tools.atFinalResult",
      parsed: line,
      ...(line.toUpperCase() === "OK" ? {} : { severity: "error" as const }),
    };
  }
  if (/^CONNECT(?:\s|$)/i.test(line)) {
    return { name: "tools.atFinalResult", parsed: line };
  }
  if (line === ">") {
    return { name: "tools.atPrompt", parsed: line };
  }
  if (/^\+[A-Za-z0-9_-]+(?::|$)/.test(line)) {
    return { name: "tools.atInformationResponse", parsed: line };
  }
  return { name: "tools.atTextLine", parsed: line };
}

export function inspectAtResponse(input: string): ProtocolParseOutcome {
  const normalized = input.replace(/\r\n/g, "\n").replace(/\r/g, "\n").trim();
  if (!normalized) return { result: null, errorCode: "emptyInput" };

  const fields: ParsedField[] = [];
  const issues = [];
  let cursor = 0;

  for (const rawLine of normalized.split("\n")) {
    const leading = rawLine.length - rawLine.trimStart().length;
    const line = rawLine.trim();
    const lineStart = cursor + leading;
    cursor += rawLine.length + 1;
    if (!line) continue;

    const type = lineType(line);
    const children: ParsedField[] = [];
    const colon = line.match(/^\+([A-Za-z0-9_-]+):\s*(.*)$/);
    if (colon) {
      const valueStart = line.indexOf(":") + 1;
      const rawParams = colon[2];
      const tokens = csvTokens(rawParams);
      let searchOffset = valueStart;
      tokens.forEach((token, index) => {
        const tokenStart = line.indexOf(token, searchOffset);
        children.push({
          id: "param-" + index,
          name: "tools.atParameter",
          nameParams: { n: index + 1 },
          range: charRange(
            lineStart + Math.max(tokenStart, valueStart),
            token.length,
          ),
          rawValue: token,
          parsedValue: token.replace(/^"(.*)"$/, "$1"),
        });
        searchOffset = Math.max(tokenStart, valueStart) + token.length + 1;
      });
    }

    fields.push({
      id: "line-" + fields.length,
      name: type.name,
      range: charRange(lineStart, line.length),
      rawValue: line,
      parsedValue: type.parsed,
      ...(children.length > 0 ? { children } : {}),
    });

    if (type.severity === "error") {
      issues.push({
        code: "atErrorResult",
        severity: "warning" as const,
        detail: line,
        range: charRange(lineStart, line.length),
      });
    }
  }

  return {
    result: {
      inspectorId: "at-response",
      fields,
      checks: [
        {
          id: "structure",
          label: "tools.checkTextStructure",
          status: fields.length > 0 ? "pass" : "fail",
        },
        {
          id: "checksum",
          label: "tools.checkChecksum",
          status: "not-applicable",
        },
      ],
      issues,
      normalizedInput: normalized,
    },
  };
}
