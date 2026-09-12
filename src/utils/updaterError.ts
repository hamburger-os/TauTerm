export type UpdaterFailureStage = "check" | "download-install" | "relaunch";

export type UpdaterErrorKind =
  | "timeout"
  | "dns"
  | "proxy"
  | "tls"
  | "http"
  | "metadata"
  | "signature"
  | "transport"
  | "install"
  | "relaunch"
  | "unknown";

export interface UpdaterErrorDetails {
  kind: UpdaterErrorKind;
  detail: string;
}

const MAX_ERROR_DETAIL_CHARS = 4_000;

function normalizeUpdaterError(error: unknown): string {
  if (error instanceof Error) {
    const stack = error.stack?.trim();
    return (stack || error.message || error.name).slice(0, MAX_ERROR_DETAIL_CHARS);
  }
  return String(error).slice(0, MAX_ERROR_DETAIL_CHARS);
}

function includesAny(value: string, needles: readonly string[]): boolean {
  return needles.some(needle => value.includes(needle));
}

/**
 * Classify updater failures without guessing beyond the evidence in the error.
 *
 * In particular, reqwest's generic `error sending request for url (...)` is a
 * transport failure, not proof of a TLS failure. The raw detail is retained for
 * the runtime diagnostic log while callers can present a stable, localized
 * category to the user.
 */
export function classifyUpdaterError(
  error: unknown,
  stage: UpdaterFailureStage,
): UpdaterErrorDetails {
  const detail = normalizeUpdaterError(error);
  const value = detail.toLowerCase();

  if (
    includesAny(value, [
      "minisign",
      "signature",
      "authentication failed",
      "signatureutf8",
    ])
  ) {
    return { kind: "signature", detail };
  }

  if (
    includesAny(value, [
      "target not found",
      "targets not found",
      "release not found",
      "invalid updater format",
      "deserialize",
      "serialization",
      "invalid json",
      "expected value",
      "eof while parsing",
    ])
  ) {
    return { kind: "metadata", detail };
  }

  if (
    includesAny(value, [
      "certificate",
      "cert error",
      "tls",
      "ssl",
      "handshake",
      "rustls",
      "schannel",
      "unknown issuer",
      "invalid peer certificate",
    ])
  ) {
    return { kind: "tls", detail };
  }

  if (
    includesAny(value, [
      "dns",
      "failed to lookup address",
      "name resolution",
      "could not resolve host",
      "no such host",
    ])
  ) {
    return { kind: "dns", detail };
  }

  if (includesAny(value, ["proxy", "tunnel", "status 407", "status code 407"])) {
    return { kind: "proxy", detail };
  }

  if (includesAny(value, ["timed out", "timeout", "deadline has elapsed"])) {
    return { kind: "timeout", detail };
  }

  if (
    includesAny(value, ["status code", "http status", "response status"]) ||
    /(?:^|\D)[45]\d{2}(?:\D|$)/.test(value)
  ) {
    return { kind: "http", detail };
  }

  if (
    includesAny(value, [
      "error sending request",
      "connection refused",
      "connection reset",
      "connection closed",
      "failed to connect",
      "network error",
      "download failed",
      "request failed",
    ])
  ) {
    return { kind: "transport", detail };
  }

  if (stage === "download-install") {
    return { kind: "install", detail };
  }
  if (stage === "relaunch") {
    return { kind: "relaunch", detail };
  }
  return { kind: "unknown", detail };
}

/** Only failures that are commonly transient are retried automatically. */
export function isRetryableUpdaterError(kind: UpdaterErrorKind): boolean {
  return kind === "timeout" || kind === "transport";
}
