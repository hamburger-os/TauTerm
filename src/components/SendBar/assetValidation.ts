import type {
  AutoReplyConfig,
  AutoReplyRule,
  CommandConfig,
  CommandItem,
  MatchCondition,
  ReplyAction,
} from "./types";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isFiniteNonNegative(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function isCommandItem(value: unknown): value is CommandItem {
  return isRecord(value)
    && typeof value.id === "string"
    && typeof value.command === "string"
    && typeof value.note === "string"
    && isFiniteNonNegative(value.delay);
}

export function parseCommandConfig(value: unknown): CommandConfig {
  if (!isRecord(value)
    || value.version !== 1
    || typeof value.name !== "string"
    || !value.name.trim()
    || !isFiniteNonNegative(value.defaultDelay)
    || !Array.isArray(value.commands)
    || !value.commands.every(isCommandItem)) {
    throw new Error("Invalid command-set format");
  }
  return value as unknown as CommandConfig;
}

const MATCH_MODES = new Set(["contains", "equals", "starts_with", "regex", "lua_pattern"]);
const MATCH_FORMATS = new Set(["text", "hex"]);

function isMatchCondition(value: unknown): value is MatchCondition {
  return isRecord(value)
    && typeof value.pattern === "string"
    && typeof value.mode === "string"
    && MATCH_MODES.has(value.mode)
    && typeof value.caseSensitive === "boolean"
    && typeof value.negate === "boolean"
    && (value.matchFormat === undefined
      || (typeof value.matchFormat === "string" && MATCH_FORMATS.has(value.matchFormat)));
}

function isReplyAction(value: unknown): value is ReplyAction {
  return isRecord(value)
    && isFiniteNonNegative(value.delayMs)
    && typeof value.data === "string"
    && (value.format === "text" || value.format === "hex");
}

function isAutoReplyRule(value: unknown): value is AutoReplyRule {
  if (!isRecord(value)
    || typeof value.id !== "string"
    || (value.label !== undefined && typeof value.label !== "string")
    || (value.triggerType !== "data" && value.triggerType !== "timer")
    || !isFiniteNonNegative(value.timerIntervalMs)
    || !Array.isArray(value.conditions)
    || !value.conditions.every(isMatchCondition)
    || (value.conditionLogic !== "and" && value.conditionLogic !== "or")
    || !Array.isArray(value.actions)
    || !value.actions.every(isReplyAction)
    || typeof value.enabled !== "boolean"
    || !isFiniteNonNegative(value.cooldownMs)) {
    return false;
  }

  return value.triggerType === "timer" || value.conditions.length > 0;
}

export function parseAutoReplyConfig(value: unknown): AutoReplyConfig {
  if (!isRecord(value)
    || typeof value.name !== "string"
    || !value.name.trim()
    || (value.matchStrategy !== "first" && value.matchStrategy !== "all")
    || !Array.isArray(value.rules)
    || !value.rules.every(isAutoReplyRule)) {
    throw new Error("Invalid auto-reply format");
  }
  return value as unknown as AutoReplyConfig;
}

export interface ScriptImport {
  name: string;
  code: string;
}

export function parseScriptImport(value: unknown): ScriptImport {
  if (!isRecord(value)
    || typeof value.name !== "string"
    || !value.name.trim()
    || typeof value.code !== "string") {
    throw new Error("Invalid script format");
  }
  return { name: value.name.trim(), code: value.code };
}

export function uniqueAssetName(
  requestedName: string,
  existingNames: Iterable<string>,
  suffix: string,
): string {
  const base = requestedName.trim() || "Untitled";
  const existing = new Set(existingNames);
  if (!existing.has(base)) return base;

  const first = `${base} (${suffix})`;
  if (!existing.has(first)) return first;

  let index = 2;
  while (existing.has(`${base} (${suffix} ${index})`)) index += 1;
  return `${base} (${suffix} ${index})`;
}
