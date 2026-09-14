import { useTranslation } from "react-i18next";
import type { StatusBarContext } from "../../core/plugin-registry";
import { useCom0comStatus } from "../../hooks/useCom0comStatus";
import { formatPortParams } from "../../utils/format";
import Icon from "../../components/common/Icon";
import styles from "./SerialStatusItems.module.css";

export function SerialLinkStatus({ activeTab }: StatusBarContext) {
  if (!activeTab?.params) return null;
  return <span className={styles.param}>{formatPortParams(activeTab.params)}</span>;
}

export function SerialTypeStatus() {
  const { t } = useTranslation();
  return <span className={styles.badge}>{t("statusBar.typeSerial")}</span>;
}

export function SerialVirtualPortStatus({ activeTab }: StatusBarContext) {
  const { t } = useTranslation();
  const {
    driverMissing,
    driverInstalling,
    cleaningPorts,
    orphanCount,
    handleRetryVPort,
    handleCleanupVPorts,
  } = useCom0comStatus();

  if (!activeTab) return null;
  const connected = activeTab.state === "connected" || activeTab.state === "transferring";
  const endpoints = activeTab.virtualVirtualEndpoints ?? [];
  const error = connected ? activeTab.virtualPortError : undefined;

  return (
    <span className={styles.group}>
      {connected && endpoints.length > 0 && (
        <span className={styles.param}>
          VPort: {endpoints.map(endpoint => endpoint.external_path).filter(Boolean).join(", ")}
        </span>
      )}

      {error && (
        <>
          <span className={`${styles.param} ${styles.warning}`} title={error}>
            <Icon name="warning" size="xs" />{" "}
            {activeTab.virtualPortErrorKind === "files_missing"
              ? t("serial.virtualPort.filesMissing")
              : activeTab.virtualPortErrorKind === "permission"
                ? t("serial.virtualPort.permissionRequired")
                : activeTab.virtualPortErrorKind === "driver_missing"
                  ? t("serial.virtualPort.notInstalled")
                  : t("serial.virtualPort.createFailed")}
          </span>
          <button
            type="button"
            className={styles.action}
            onClick={() => void handleRetryVPort()}
            disabled={driverInstalling}
            title={t("serial.virtualPort.retryHint")}
          >
            [{driverInstalling ? t("serial.virtualPort.installing") : t("serial.virtualPort.retry")}]
          </button>
        </>
      )}

      {!error && driverMissing && (
        <>
          <span className={`${styles.param} ${styles.warning}`}>
            <Icon name="warning" size="xs" /> {t("serial.virtualPort.notInstalled")}
          </span>
          <button
            type="button"
            className={styles.action}
            onClick={() => void handleRetryVPort()}
            disabled={driverInstalling}
            title={t("serial.virtualPort.retryHint")}
          >
            [{driverInstalling ? t("serial.virtualPort.installing") : t("serial.virtualPort.retry")}]
          </button>
        </>
      )}

      {orphanCount > 0 && (
        <>
          <span className={`${styles.param} ${styles.warning}`} title={t("serial.virtualPort.cleanupHint")}>
            <Icon name="warning" size="xs" /> VPort {orphanCount} {t("serial.virtualPort.orphansDetected")}
          </span>
          <button
            type="button"
            className={styles.action}
            onClick={() => void handleCleanupVPorts()}
            disabled={cleaningPorts}
            title={t("serial.virtualPort.cleanupHint")}
          >
            [{cleaningPorts ? (t("serial.virtualPort.cleaning") || "正在清理...") : (t("serial.virtualPort.cleanup") || "清理")}]
          </button>
        </>
      )}
    </span>
  );
}
