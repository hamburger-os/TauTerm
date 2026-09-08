import { invoke } from "@tauri-apps/api/core";

const lastReported = new Map<string, number>();
const DEDUPE_WINDOW_MS = 5_000;
const MAX_DETAIL_CHARS = 4_000;

function normalizeError(error: unknown): string {
  if (error instanceof Error) {
    const stack = error.stack?.trim();
    return (stack || error.message || error.name).slice(0, MAX_DETAIL_CHARS);
  }
  return String(error).slice(0, MAX_DETAIL_CHARS);
}

export function reportFrontendError(
  source: string,
  error: unknown,
  context?: string
): void {
  const detail = normalizeError(error);
  const signature = source + "|" + detail.slice(0, 256);
  const now = Date.now();
  const previous = lastReported.get(signature) ?? 0;
  if (now - previous < DEDUPE_WINDOW_MS) return;
  lastReported.set(signature, now);

  const pieces = ["frontend_runtime_error source=" + source];
  if (context) pieces.push("context=" + context.slice(0, 512));
  pieces.push(detail);

  void invoke("log_event", {
    level: "ERROR",
    message: pieces.join(" | "),
  }).catch(() => {
    // Logging failures must not recursively create another frontend runtime error.
  });
}

export function installFrontendRuntimeDiagnostics(): () => void {
  const onError = (event: ErrorEvent) => {
    reportFrontendError(
      "window.error",
      event.error ?? event.message,
      event.filename || undefined
    );
  };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => {
    reportFrontendError("unhandledrejection", event.reason);
  };

  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onUnhandledRejection);
  return () => {
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onUnhandledRejection);
  };
}
