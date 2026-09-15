import { useTranslation } from "react-i18next";
import Icon from "../../components/common/Icon";
import {
  StatusBarAction,
  StatusBarBadge,
  StatusBarGroup,
  StatusBarText,
} from "../../components/Layout/StatusBarPrimitives";
import type { StatusBarContext } from "../../core/plugin-registry";
import { useCom0comStatus } from "../../hooks/useCom0comStatus";
import { formatPortParams } from "../../utils/format";

export function SerialLinkStatus({ activeTab }: StatusBarContext) {
  if (!activeTab?.params) return null;
  return <StatusBarText>{formatPortParams(activeTab.params)}</StatusBarText>;
}

export function SerialTypeStatus() {
  const { t } = useTranslation();
  return <StatusBarBadge>{t("statusBar.typeSerial")}</StatusBarBadge>;
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
  const virtualPortEnabled = activeTab.params?.virtual_port_enabled === true;
  const endpoints = activeTab.virtualVirtualEndpoints ?? [];
  const error = connected ? activeTab.virtualPortError : undefined;
  const driverError = activeTab.virtualPortErrorKind === "files_missing"
    || activeTab.virtualPortErrorKind === "permission"
    || activeTab.virtualPortErrorKind === "driver_missing";

  return (
    <StatusBarGroup>
      {connected && endpoints.length > 0 ? (
        <StatusBarText>
          VPort: {endpoints.map(endpoint => endpoint.external_path).filter(Boolean).join(", ")}
        </StatusBarText>
      ) : null}

      {error ? (
        <>
          <StatusBarText tone="warning" title={error}>
            <Icon name="warning" size="xs" />{" "}
            {activeTab.virtualPortErrorKind === "files_missing"
              ? t("serial.virtualPort.filesMissing")
              : activeTab.virtualPortErrorKind === "permission"
                ? t("serial.virtualPort.permissionRequired")
                : activeTab.virtualPortErrorKind === "driver_missing"
                  ? t("serial.virtualPort.notInstalled")
                  : activeTab.virtualPortErrorKind === "bridge_failed"
                    ? t("serial.virtualPortBridgeFailed")
                    : t("serial.virtualPort.createFailed")}
          </StatusBarText>
          {driverError ? (
            <StatusBarAction
              onClick={() => void handleRetryVPort()}
              disabled={driverInstalling}
              title={t("serial.virtualPort.retryHint")}
            >
              {driverInstalling ? t("serial.virtualPort.installing") : t("serial.virtualPort.retry")}
            </StatusBarAction>
          ) : null}
        </>
      ) : null}

      {!error && virtualPortEnabled && driverMissing ? (
        <>
          <StatusBarText tone="warning">
            <Icon name="warning" size="xs" /> {t("serial.virtualPort.notInstalled")}
          </StatusBarText>
          <StatusBarAction
            onClick={() => void handleRetryVPort()}
            disabled={driverInstalling}
            title={t("serial.virtualPort.retryHint")}
          >
            {driverInstalling ? t("serial.virtualPort.installing") : t("serial.virtualPort.retry")}
          </StatusBarAction>
        </>
      ) : null}

      {orphanCount > 0 ? (
        <>
          <StatusBarText tone="warning" title={t("serial.virtualPort.cleanupHint")}>
            <Icon name="warning" size="xs" /> VPort {orphanCount} {t("serial.virtualPort.orphansDetected")}
          </StatusBarText>
          <StatusBarAction
            onClick={() => void handleCleanupVPorts()}
            disabled={cleaningPorts}
            title={t("serial.virtualPort.cleanupHint")}
          >
            {cleaningPorts ? t("serial.virtualPort.cleaning") : t("serial.virtualPort.cleanup")}
          </StatusBarAction>
        </>
      ) : null}
    </StatusBarGroup>
  );
}
