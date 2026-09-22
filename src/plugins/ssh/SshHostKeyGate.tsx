import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import ConfirmDialog from "../../components/common/ConfirmDialog";
import { useToast } from "../../context/ToastContext";

type HostTrustReason = "first_seen" | "additional_key";

interface PendingHostKeyVerification {
  kind: "verify";
  requestId: string;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  reason: HostTrustReason;
  knownAlgorithms: string[];
}

interface PendingHostKeyChange {
  kind: "changed";
  requestId: string;
  host: string;
  port: number;
  algorithm: string;
  expectedFingerprints: string[];
  actualFingerprint: string;
}

interface PendingTrustStoreReset {
  kind: "storeUnavailable";
  reason: string;
}

type PendingHostTrustAction =
  | PendingHostKeyVerification
  | PendingHostKeyChange
  | PendingTrustStoreReset;

function endpoint(host: string, port: number): string {
  const normalized = host.includes(":") && !host.startsWith("[") ? `[${host}]` : host;
  return `${normalized}:${port}`;
}

export default function SshHostKeyGate() {
  const { t } = useTranslation();
  const { showToast } = useToast();
  const [pending, setPending] = useState<PendingHostTrustAction | null>(null);
  const [busy, setBusy] = useState(false);
  const queueRef = useRef<PendingHostTrustAction[]>([]);

  const enqueue = useCallback((request: PendingHostTrustAction) => {
    setPending(current => {
      if (!current) return request;
      queueRef.current.push(request);
      return current;
    });
  }, []);

  const advance = useCallback(() => {
    setPending(queueRef.current.shift() ?? null);
  }, []);

  const settle = useCallback(async (accepted: boolean) => {
    const current = pending;
    if (!current || busy) return;

    setBusy(true);
    try {
      if (current.kind === "verify") {
        await invoke("confirm_host_key", {
          requestId: current.requestId,
          accepted,
        });
      } else if (current.kind === "changed") {
        await invoke("confirm_host_key_change", {
          requestId: current.requestId,
          accepted,
        });
        if (accepted) {
          showToast("success", t("ssh.hostKeyTrustUpdated"));
        }
      } else if (accepted) {
        await invoke("reset_ssh_known_hosts");
        showToast("success", t("ssh.hostTrustStoreResetDone"));
      }
    } catch (error) {
      const message = String(error);
      const stale = message.includes("未找到或已过期")
        || message.includes("not found")
        || message.includes("expired");
      if (!stale) {
        showToast("error", t("ssh.hostKeyError", { error: message }));
      }
    } finally {
      setBusy(false);
      advance();
    }
  }, [advance, busy, pending, showToast, t]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<{
      request_id: string;
      host: string;
      port: number;
      algorithm: string;
      fingerprint: string;
      reason: HostTrustReason;
      known_algorithms: string[];
    }>("ssh-host-key-verify", event => {
      if (cancelled) return;
      const {
        request_id: requestId,
        host,
        port,
        algorithm,
        fingerprint,
        reason,
        known_algorithms: knownAlgorithms,
      } = event.payload;
      enqueue({
        kind: "verify",
        requestId,
        host,
        port,
        algorithm,
        fingerprint,
        reason,
        knownAlgorithms,
      });
    }).then(fn => {
      if (cancelled) fn();
      else unlisten = fn;
    }).catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enqueue]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<{
      request_id: string;
      host: string;
      port: number;
      algorithm: string;
      expected_fingerprint?: string;
      expected_fingerprints: string[];
      actual_fingerprint: string;
    }>("ssh-host-key-changed", event => {
      if (cancelled) return;
      const {
        request_id: requestId,
        host,
        port,
        algorithm,
        expected_fingerprint: expectedFingerprint,
        expected_fingerprints: expectedFingerprints,
        actual_fingerprint: actualFingerprint,
      } = event.payload;
      enqueue({
        kind: "changed",
        requestId,
        host,
        port,
        algorithm,
        expectedFingerprints: expectedFingerprints.length > 0
          ? expectedFingerprints
          : expectedFingerprint ? [expectedFingerprint] : [],
        actualFingerprint,
      });
    }).then(fn => {
      if (cancelled) fn();
      else unlisten = fn;
    }).catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enqueue]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<{ reason: string }>("ssh-host-trust-store-unavailable", event => {
      if (cancelled) return;
      enqueue({
        kind: "storeUnavailable",
        reason: event.payload.reason,
      });
    }).then(fn => {
      if (cancelled) fn();
      else unlisten = fn;
    }).catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enqueue]);

  const message = pending
    ? pending.kind === "storeUnavailable"
      ? t("ssh.hostTrustStoreResetPrompt", { reason: pending.reason })
      : pending.kind === "changed"
        ? t("ssh.hostKeyChangedPrompt", {
            host: endpoint(pending.host, pending.port),
            algorithm: pending.algorithm,
            expected: pending.expectedFingerprints.join("\n"),
            actual: pending.actualFingerprint,
          })
        : t(
            pending.reason === "additional_key"
              ? "ssh.hostKeyAdditionalPrompt"
              : "ssh.hostKeyFirstSeenPrompt",
            {
              host: endpoint(pending.host, pending.port),
              algorithm: pending.algorithm,
              fingerprint: pending.fingerprint,
              knownAlgorithms: pending.knownAlgorithms.join(", "),
            },
          )
    : undefined;

  return (
    <ConfirmDialog
      open={pending !== null}
      title={pending?.kind === "storeUnavailable"
        ? t("ssh.hostTrustStoreBlockedTitle")
        : pending?.kind === "changed"
          ? t("ssh.hostKeyChangedTitle")
          : t("ssh.hostKeyTitle")}
      message={message}
      intent={pending?.kind === "changed" || pending?.kind === "storeUnavailable"
        ? "danger"
        : "primary"}
      busy={busy}
      onConfirm={() => void settle(true)}
      onCancel={() => void settle(false)}
    />
  );
}
