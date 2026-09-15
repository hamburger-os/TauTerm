import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import ConfirmDialog from "../../components/common/ConfirmDialog";
import { useToast } from "../../context/ToastContext";

interface PendingHostKeyVerification {
  requestId: string;
  host: string;
  port: number;
  fingerprint: string;
}

export default function SshHostKeyGate() {
  const { t } = useTranslation();
  const { showToast } = useToast();
  const [pending, setPending] = useState<PendingHostKeyVerification | null>(null);
  const queueRef = useRef<PendingHostKeyVerification[]>([]);

  const enqueue = useCallback((request: PendingHostKeyVerification) => {
    setPending(current => {
      if (!current) return request;
      queueRef.current.push(request);
      return current;
    });
  }, []);

  const settle = useCallback(async (accepted: boolean) => {
    const current = pending;
    if (!current) return;
    setPending(queueRef.current.shift() ?? null);
    try {
      await invoke("confirm_host_key", { requestId: current.requestId, accepted });
    } catch (error) {
      const message = String(error);
      if (message.includes("未找到或已过期") || message.includes("not found") || message.includes("expired")) return;
      showToast("error", t("ssh.hostKeyError", { error: message }));
    }
  }, [pending, showToast, t]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<{
      request_id: string;
      host: string;
      port: number;
      fingerprint: string;
    }>("ssh-host-key-verify", event => {
      if (cancelled) return;
      const { request_id: requestId, host, port, fingerprint } = event.payload;
      enqueue({ requestId, host, port, fingerprint });
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
      host: string;
      port: number;
      expected_fingerprint: string;
      actual_fingerprint: string;
    }>("ssh-host-key-changed", event => {
      if (cancelled) return;
      showToast("error", t("ssh.hostKeyChanged", {
        defaultValue: "SSH host key changed for {{host}}:{{port}}. Connection was refused. Expected {{expected}}, received {{actual}}.",
        host: event.payload.host,
        port: event.payload.port,
        expected: event.payload.expected_fingerprint,
        actual: event.payload.actual_fingerprint,
      }));
    }).then(fn => {
      if (cancelled) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [showToast, t]);

  return (
    <ConfirmDialog
      open={pending !== null}
      title={t("ssh.hostKeyTitle")}
      message={pending
        ? `${t("ssh.hostKeyHost", { defaultValue: "Host" })}: ${pending.host}:${pending.port}
${t("ssh.hostKeyFingerprint")}: ${pending.fingerprint}

${t("ssh.hostKeyPrompt")}`
        : undefined}
      onConfirm={() => void settle(true)}
      onCancel={() => void settle(false)}
    />
  );
}
