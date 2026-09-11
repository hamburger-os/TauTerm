import { invoke } from "@tauri-apps/api/core";
import type {
  JournaldErrorPayload,
  JournaldFilter,
  JournaldQueryResponse,
} from "../types";

function escapePcreLiteral(value: string): string {
  return value.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
}

function keywordForBackend(filter: JournaldFilter): string | null {
  const keyword = filter.keyword?.trim();
  if (!keyword) return null;
  return (filter.searchMode ?? "literal") === "regex"
    ? keyword
    : escapePcreLiteral(keyword);
}

function errorCodeFromMessage(message: string): string {
  const lower = message.toLowerCase();
  if (lower.includes("already running") || lower.includes("operation_already_running")) {
    return "already_running";
  }
  if (
    lower.includes("command not found") ||
    lower.includes("journalctl: not found") ||
    lower.includes("no such file") ||
    lower.includes("不可用")
  ) {
    return "command_unavailable";
  }
  if (lower.includes("permission denied") || lower.includes("not permitted")) {
    return "permission_denied";
  }
  if (
    lower.includes("invalid regular expression") ||
    lower.includes("pcre2") ||
    lower.includes("invalid argument")
  ) {
    return "invalid_filter";
  }
  if (lower.includes("timed out") || lower.includes("timeout")) return "timeout";
  if (lower.includes("cancel")) return "cancelled";
  if (lower.includes("ssh") || lower.includes("channel") || lower.includes("connection")) {
    return "connection_error";
  }
  return "unknown";
}

export function normalizeJournaldError(error: unknown): JournaldErrorPayload {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<JournaldErrorPayload>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return { code: candidate.code, message: candidate.message };
    }
  }
  if (typeof error === "string") {
    try {
      const parsed = JSON.parse(error) as Partial<JournaldErrorPayload>;
      if (typeof parsed.code === "string" && typeof parsed.message === "string") {
        return { code: parsed.code, message: parsed.message };
      }
    } catch {
      // Tauri command errors are normalized from their plain message here.
    }
    return { code: errorCodeFromMessage(error), message: error };
  }
  const message = String(error);
  return { code: errorCodeFromMessage(message), message };
}

export async function startJournalStream(
  sessionId: string,
  filter: JournaldFilter,
): Promise<void> {
  await invoke<void>("start_journald_stream", {
    sessionId,
    level: filter.level ?? null,
    keyword: keywordForBackend(filter),
    unit: filter.unit?.trim() || null,
    kernelOnly: filter.kernelOnly ?? false,
  });
}

export async function stopJournalStream(sessionId: string): Promise<void> {
  await invoke<void>("stop_journald_stream", { sessionId });
}

export async function queryJournalHistory(
  sessionId: string,
  filter: JournaldFilter,
  cursor: string | null,
  limit = 100,
): Promise<JournaldQueryResponse> {
  const response = await invoke<JournaldQueryResponse>("journald_query_cmd", {
    request: {
      sessionId,
      level: filter.level ?? null,
      keyword: keywordForBackend(filter),
      unit: filter.unit?.trim() || null,
      kernelOnly: filter.kernelOnly ?? false,
      since: filter.since ?? null,
      until: filter.until ?? null,
      cursor,
      limit,
    },
  });
  // The data source emits next_cursor only when a look-ahead record proves
  // another page exists. This is stricter than a len>=limit heuristic.
  return { ...response, has_more: response.next_cursor !== null };
}

export async function startJournalExport(
  sessionId: string,
  filePath: string,
  filter: JournaldFilter,
): Promise<void> {
  await invoke<void>("start_journald_export", {
    request: {
      sessionId,
      filePath,
      level: filter.level ?? null,
      keyword: keywordForBackend(filter),
      unit: filter.unit?.trim() || null,
      kernelOnly: filter.kernelOnly ?? false,
      since: filter.since ?? null,
      until: filter.until ?? null,
    },
  });
}

export async function cancelJournalExport(sessionId: string): Promise<void> {
  await invoke<void>("stop_journald_export", { sessionId });
}
