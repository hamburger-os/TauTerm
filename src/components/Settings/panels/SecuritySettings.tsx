import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import Icon from "../../common/Icon";
import GlassButton from "../../common/GlassButton";
import settingsStyles from "../SettingsPage.module.css";
import styles from "./SecuritySettings.module.css";

interface SecurityStatus {
  backend: string;
  native_available: boolean;
  fallback_configured: boolean;
  fallback_unlocked: boolean;
}

type ViewState = "loading" | "error" | "ready" | "busy";
type FeedbackTone = "success" | "error";
type FeedbackKey =
  | "settings.securityVaultUnlocked"
  | "settings.securityVaultLocked"
  | "settings.securityOperationFailed"
  | "settings.securityStatusUnavailable";

interface Feedback {
  key: FeedbackKey;
  tone: FeedbackTone;
}

function getBackendLabel(backend: string, t: (key: string) => string): string {
  switch (backend) {
    case "native_keyring":
      return t("settings.securityBackendNativeKeyring");
    case "encrypted_vault":
      return t("settings.securityBackendEncryptedVault");
    default:
      return t("settings.securityBackendUnknown");
  }
}

export default function SecuritySettings() {
  const { t } = useTranslation();
  const [securityStatus, setSecurityStatus] = useState<SecurityStatus | null>(null);
  const [password, setPassword] = useState("");
  const [viewState, setViewState] = useState<ViewState>("loading");
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const operationLockRef = useRef(false);
  const requestSequenceRef = useRef(0);
  const mountedRef = useRef(true);

  const refresh = useCallback(async (keepBusy = false): Promise<boolean> => {
    if (operationLockRef.current && !keepBusy) return false;
    const requestSequence = ++requestSequenceRef.current;
    const isCurrentRequest = () =>
      mountedRef.current && requestSequenceRef.current === requestSequence;
    if (!keepBusy) {
      setFeedback(null);
      setViewState("loading");
    }

    try {
      const nextStatus = await invoke<SecurityStatus>("credential_storage_status");
      if (!isCurrentRequest()) return false;
      setSecurityStatus(nextStatus);
      setFeedback(null);
      if (!keepBusy) setViewState("ready");
      return true;
    } catch {
      if (!isCurrentRequest()) return false;
      setSecurityStatus(null);
      setFeedback({ key: "settings.securityStatusUnavailable", tone: "error" });
      console.error("Security settings status refresh failed");
      if (!keepBusy) setViewState("error");
      return false;
    } finally {
      if (!keepBusy && isCurrentRequest()) {
        setViewState((current) => (current === "loading" ? "error" : current));
      }
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => {
      mountedRef.current = false;
      ++requestSequenceRef.current;
      operationLockRef.current = false;
    };
  }, [refresh]);

  const unlock = async () => {
    if (
      operationLockRef.current ||
      !securityStatus ||
      securityStatus.native_available ||
      viewState === "busy"
    ) return;

    operationLockRef.current = true;
    ++requestSequenceRef.current;
    setViewState("busy");
    setFeedback(null);
    let refreshed = false;

    try {
      await invoke("unlock_credential_vault", { masterPassword: password });
      if (mountedRef.current) setPassword("");
      refreshed = await refresh(true);
      if (refreshed && mountedRef.current) {
        setFeedback({ key: "settings.securityVaultUnlocked", tone: "success" });
      }
    } catch {
      if (mountedRef.current) {
        setFeedback({ key: "settings.securityOperationFailed", tone: "error" });
      }
      console.error("Security settings vault unlock failed");
    } finally {
      if (mountedRef.current && operationLockRef.current) {
        setViewState(refreshed ? "ready" : "error");
        operationLockRef.current = false;
      }
    }
  };

  const lock = async () => {
    if (
      operationLockRef.current ||
      !securityStatus ||
      securityStatus.native_available ||
      !securityStatus.fallback_unlocked ||
      viewState === "busy"
    ) return;

    operationLockRef.current = true;
    ++requestSequenceRef.current;
    setViewState("busy");
    setFeedback(null);
    let refreshed = false;

    try {
      await invoke("lock_credential_vault");
      if (mountedRef.current) setPassword("");
      refreshed = await refresh(true);
      if (refreshed && mountedRef.current) {
        setFeedback({ key: "settings.securityVaultLocked", tone: "success" });
      }
    } catch {
      if (mountedRef.current) {
        setFeedback({ key: "settings.securityOperationFailed", tone: "error" });
      }
      console.error("Security settings vault lock failed");
    } finally {
      if (mountedRef.current && operationLockRef.current) {
        setViewState(refreshed ? "ready" : "error");
        operationLockRef.current = false;
      }
    }
  };

  const statusMessage = (() => {
    switch (viewState) {
      case "loading":
        return t("settings.securityLoading");
      case "busy":
        return t("settings.securityBusy");
      case "error":
        return t("settings.securityStatusUnavailable");
      case "ready":
        return t("settings.securityReady");
    }
  })();

  const feedbackMessage = feedback
    ? feedback.key === "settings.securityVaultUnlocked"
      ? t("settings.securityVaultUnlocked")
      : feedback.key === "settings.securityVaultLocked"
        ? t("settings.securityVaultLocked")
        : feedback.key === "settings.securityOperationFailed"
          ? t("settings.securityOperationFailed")
          : t("settings.securityStatusUnavailable")
    : null;
  const statusText = feedbackMessage ?? statusMessage;
  const statusRole = feedback?.tone === "error" || viewState === "error" ? "alert" : "status";
  const canEditVault = securityStatus !== null && !securityStatus.native_available && viewState === "ready";
  const isBusy = viewState === "busy";

  return (
    <div className={styles.panel} aria-busy={viewState === "busy"}>
      <h3 className={settingsStyles.panelTitle}>{t("settings.security")}</h3>

      <p
        className={settingsStyles.settingDesc}
        role={statusRole}
        aria-live={statusRole === "alert" ? "assertive" : "polite"}
      >
        {statusText}
      </p>

      <div className={styles.refreshRow}>
        <GlassButton
          variant="secondary"
          size="sm"
          type="button"
          disabled={viewState === "busy" || viewState === "loading"}
          onClick={() => void refresh()}
        >
          <Icon name="refresh" size="sm" />
          {t("settings.securityRefresh")}
        </GlassButton>
      </div>

      {viewState === "ready" && securityStatus && (
        <div className={styles.statusDetails}>
          <p className={settingsStyles.settingDesc}>
            {securityStatus.native_available
              ? t("settings.securityNative")
              : t("settings.securityFallback")}
          </p>
          <p className={settingsStyles.settingDesc}>
            <span className={settingsStyles.settingLabel}>{t("settings.securityBackend")}</span>
            {getBackendLabel(securityStatus.backend, t)}
          </p>
          {!securityStatus.native_available && (
            <p className={settingsStyles.settingDesc}>
              <span className={settingsStyles.settingLabel}>{t("settings.securityVaultStatus")}</span>
              {securityStatus.fallback_unlocked
                ? t("settings.securityVaultStateUnlocked")
                : securityStatus.fallback_configured
                  ? t("settings.securityVaultStateLocked")
                  : t("settings.securityVaultStateNotConfigured")}
            </p>
          )}
        </div>
      )}

      {canEditVault && (
        <div className={styles.vaultForm}>
          <label className={settingsStyles.settingLabel} htmlFor="security-master-password">
            {t("settings.securityMasterPassword")}
          </label>
          <input
            id="security-master-password"
            className={`liquid-glass-input ${styles.passwordInput}`}
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder={t("settings.securityMasterPasswordPlaceholder")}
            aria-label={t("settings.securityMasterPassword")}
            disabled={isBusy}
          />
          <div className={styles.vaultActions}>
            <GlassButton
              variant="secondary"
              size="sm"
              type="button"
              disabled={isBusy || password.length < 10}
              onClick={() => void unlock()}
            >
              <Icon name="lock" size="sm" />
              {t("settings.securityUnlock")}
            </GlassButton>
            <GlassButton
              variant="secondary"
              size="sm"
              type="button"
              disabled={isBusy || !securityStatus.fallback_unlocked}
              onClick={() => void lock()}
            >
              <Icon name="lock" size="sm" />
              {t("settings.securityLock")}
            </GlassButton>
          </div>
        </div>
      )}
    </div>
  );
}
