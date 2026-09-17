import { useEffect, useMemo, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import type { StatusBarTab } from "../../core/plugin-registry";
import { useSessionTrafficRate } from "../../hooks/useSessionTrafficRate";
import type { UpdatePhase } from "../../types/updater";
import { charsetLabel } from "../../utils/charsets";
import { formatBytes, formatRate, formatUptime } from "../../utils/format";
import Icon from "../common/Icon";
import {
  StatusBarAction,
  StatusBarBadge,
  StatusBarGroup,
  StatusBarIndicator,
  StatusBarText,
} from "./StatusBarPrimitives";

function isConnected(tab: StatusBarTab | null | undefined): boolean {
  return tab?.state === "connected" || tab?.state === "transferring";
}

export function SessionConnectionStatus({ tab }: { tab: StatusBarTab | null }) {
  const { t } = useTranslation();
  if (!tab) return null;

  const connected = isConnected(tab);
  const connecting = tab.state === "connecting";
  const tone = connected ? "success" : connecting ? "warning" : "muted";

  return (
    <StatusBarGroup>
      <StatusBarIndicator tone={tone} pulse={connecting} />
      <StatusBarText mono={false} title={connected || connecting ? tab.endpoint : undefined}>
        {connected || connecting ? tab.endpoint : t("statusBar.disconnected")}
      </StatusBarText>
    </StatusBarGroup>
  );
}

/**
 * Activity 是连接健康之外的正交提示。当前 SessionStore 仍把文件传输编码在
 * transferring state 中，这里先确保视觉语义不再把“正在传输”误画成连接异常。
 */
export function SessionActivityStatus({ tab }: { tab: StatusBarTab | null }) {
  const { t } = useTranslation();
  if (tab?.state !== "transferring") return null;
  return <StatusBarBadge tone="warning">{t("session.transferring")}</StatusBarBadge>;
}

export function SessionUptimeStatus({ tab }: { tab: StatusBarTab | null }) {
  const connected = isConnected(tab);
  const connectedAt = connected ? tab?.connectedAt : undefined;
  const [, setTick] = useState(0);

  useEffect(() => {
    if (!tab?.id || !connectedAt) return;

    const tick = () => setTick(value => value + 1);
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, [tab?.id, connectedAt]);

  const uptime = tab?.id && connectedAt
    ? Math.max(0, Math.floor((Date.now() - connectedAt) / 1000))
    : 0;

  if (uptime <= 0) return null;
  return (
    <StatusBarText>
      <Icon name="stopwatch" size="sm" /> {formatUptime(uptime)}
    </StatusBarText>
  );
}

export function StreamModeStatus({ tab }: { tab: StatusBarTab | null }) {
  if (!tab || !isConnected(tab)) return null;

  const mode = tab.params?.data_mode;
  const label = mode === "hex"
    ? "HEX"
    : mode === "dual"
      ? "DUAL"
      : mode === "text"
        ? "TEXT"
        : null;

  return label ? <StatusBarBadge>{label}</StatusBarBadge> : null;
}

export function StreamEncodingStatus({ tab }: { tab: StatusBarTab | null }) {
  if (!tab || !isConnected(tab)) return null;
  const encoding = typeof tab.params?.encoding === "string" ? charsetLabel(tab.params.encoding) : null;
  return encoding ? <StatusBarText>{encoding}</StatusBarText> : null;
}

export function SessionTrafficStatus({ tab }: { tab: StatusBarTab | null }) {
  const connected = isConnected(tab);
  const txBytes = tab?.stats?.txBytes ?? 0;
  const rxBytes = tab?.stats?.rxBytes ?? 0;
  const rate = useSessionTrafficRate(tab?.id, connected, txBytes, rxBytes);

  if (!tab || !connected) return null;
  return (
    <StatusBarGroup>
      <StatusBarText title="TX">
        <Icon name="arrow-up" size="xs" /> {formatBytes(txBytes)} · {formatRate(rate.tx)}
      </StatusBarText>
      <StatusBarText title="RX">
        <Icon name="arrow-down" size="xs" /> {formatBytes(rxBytes)} · {formatRate(rate.rx)}
      </StatusBarText>
    </StatusBarGroup>
  );
}

interface LoggingStatusProps {
  activeSessionId: string | null;
  loggingSessions: Set<string>;
  logStatuses: Map<string, { fileName: string; bytesWritten: number }>;
}

export function LoggingStatus({
  activeSessionId,
  loggingSessions,
  logStatuses,
}: LoggingStatusProps) {
  if (loggingSessions.size === 0) return null;

  const activeStatus = activeSessionId ? logStatuses.get(activeSessionId) : undefined;
  const activeLogging = activeSessionId ? loggingSessions.has(activeSessionId) : false;
  const backgroundCount = Math.max(0, loggingSessions.size - (activeLogging ? 1 : 0));

  return (
    <StatusBarGroup>
      <StatusBarIndicator tone="danger" pulse />
      {activeStatus ? (
        <StatusBarText title={activeStatus.fileName}>
          REC {activeStatus.fileName} ({formatBytes(activeStatus.bytesWritten)})
        </StatusBarText>
      ) : (
        <StatusBarText>REC {loggingSessions.size}</StatusBarText>
      )}
      {activeStatus && backgroundCount > 0 ? (
        <StatusBarBadge tone="muted">+{backgroundCount}</StatusBarBadge>
      ) : null}
    </StatusBarGroup>
  );
}

interface AppVersionStatusProps {
  updatePhase: UpdatePhase;
  latestVersion?: string;
  downloadedBytes?: number;
  totalBytes?: number;
  onVersionClick: () => void;
}

export function AppVersionStatus({
  updatePhase,
  latestVersion,
  downloadedBytes,
  totalBytes,
  onVersionClick,
}: AppVersionStatusProps) {
  const { t } = useTranslation();
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    getVersion().then(version => setAppVersion(`v${version}`)).catch(() => setAppVersion(""));
  }, []);

  const downloadPercent = useMemo(() => {
    if (updatePhase !== "downloading" || !totalBytes) return null;
    return Math.min(Math.round(((downloadedBytes ?? 0) / totalBytes) * 100), 99);
  }, [updatePhase, downloadedBytes, totalBytes]);

  if (!appVersion) return null;

  const actionable = updatePhase === "available" || updatePhase === "ready";
  const tone = actionable ? "success" : updatePhase === "checking" ? "muted" : "neutral";
  const title = updatePhase === "available"
    ? t("statusBar.updateAvailable", { version: latestVersion ?? "" })
    : updatePhase === "ready"
      ? t("updater.installAndRelaunch")
      : updatePhase === "downloading"
        ? t("updater.downloading")
        : t("settings.about");

  return (
    <StatusBarGroup>
      {downloadPercent !== null ? (
        <StatusBarText tone="accent">
          <Icon name="arrow-down" size="xs" /> {downloadPercent}%
        </StatusBarText>
      ) : null}
      <StatusBarAction
        tone={tone}
        onClick={onVersionClick}
        title={title}
        aria-label={title}
      >
        {appVersion}
        {actionable ? <StatusBarIndicator tone="success" pulse={updatePhase === "available"} /> : null}
      </StatusBarAction>
    </StatusBarGroup>
  );
}
