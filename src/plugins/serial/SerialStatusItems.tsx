import { useTranslation } from "react-i18next";
import Icon from "../../components/common/Icon";
import {
  StatusBarAction,
  StatusBarBadge,
  StatusBarGroup,
  StatusBarText,
} from "../../components/Layout/StatusBarPrimitives";
import type { StatusBarContext } from "../../core/plugin-registry";
import { usePluginRuntime } from "../../core/usePluginRuntime";
import { useCom0comStatus } from "../../hooks/useCom0comStatus";
import { formatPortParams } from "../../utils/format";
import type { SerialRuntimeSnapshot } from "./runtime-store";

export function SerialLinkStatus({ activeTab }: StatusBarContext) {
  if (!activeTab?.params) return null;
  return <StatusBarText>{formatPortParams(activeTab.params)}</StatusBarText>;
}

export function SerialTypeStatus() {
  const { t } = useTranslation();
  return <StatusBarBadge>{t("statusBar.typeSerial")}</StatusBarBadge>;
}

export function SerialVirtualPortStatus({ sessionId, activeTab }: StatusBarContext) {
  const { t } = useTranslation();
  const runtime = usePluginRuntime<SerialRuntimeSnapshot>("serial", sessionId);
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
  const endpoints = runtime.endpoints ?? [];
  const error = connected ? runtime.error : undefined;
  const driverError = runtime.errorKind === "files_missing"
    || runtime.errorKind === "permission"
    || runtime.errorKind === "driver_missing";

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
            {runtime.errorKind === "files_missing"
              ? t("serial.virtualPort.filesMissing")
              : runtime.errorKind === "permission"
                ? t("serial.virtualPort.permissionRequired")
                : runtime.errorKind === "driver_missing"
                  ? t("serial.virtualPort.notInstalled")
                  : runtime.errorKind === "bridge_failed"
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
