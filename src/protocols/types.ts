export type ProtocolTemplate =
  | "auto"
  | "modbus-rtu"
  | "modbus-ascii"
  | "modbus-tcp"
  | "at-response"
  | "nmea-0183"
  | "raw"
  | "custom-schema";

export type ProtocolInputKind = "hex" | "text" | "mixed";
export type ProtocolDirection = "auto" | "request" | "response";
export type ProtocolRangeUnit = "byte" | "char";
export type ProtocolSeverity = "info" | "warning" | "error";
export type ProtocolCheckStatus = "pass" | "fail" | "warning" | "not-applicable";

export interface ProtocolRange {
  start: number;
  length: number;
  unit: ProtocolRangeUnit;
}

export interface ParsedField {
  id: string;
  name: string;
  range: ProtocolRange;
  rawValue: string;
  parsedValue?: string;
  nameParams?: Record<string, unknown>;
  children?: ParsedField[];
}

export interface ProtocolCheck {
  id: string;
  label: string;
  status: ProtocolCheckStatus;
  detail?: string;
}

export interface ProtocolIssue {
  code: string;
  severity: ProtocolSeverity;
  detail?: string;
  range?: ProtocolRange;
}

export interface ParseResult {
  inspectorId: Exclude<ProtocolTemplate, "auto">;
  fields: ParsedField[];
  checks: ProtocolCheck[];
  issues: ProtocolIssue[];
  rawBytes?: Uint8Array;
  normalizedInput: string;
  detectedConfidence?: "high" | "medium" | "low";
  effectiveDirection?: Exclude<ProtocolDirection, "auto"> | "ambiguous";
}

export interface ProtocolParseOutcome {
  result: ParseResult | null;
  errorCode?: string;
  detectedTemplate?: Exclude<ProtocolTemplate, "auto">;
}

export interface ProtocolInspectorOptions {
  direction?: ProtocolDirection;
  customSchema?: string;
}
