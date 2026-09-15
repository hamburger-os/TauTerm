import type { ReactNode } from "react";
import { useSession } from "../../context/SessionContext";
import {
  pluginRegistry,
  type StatusBarContext,
  type StatusBarItem,
  type StatusBarOverflow,
} from "../../core/plugin-registry";
import type { UpdatePhase } from "../../types/updater";
import {
  AppVersionStatus,
  LoggingStatus,
  SessionConnectionStatus,
  SessionTrafficStatus,
  SessionUptimeStatus,
  StreamEncodingStatus,
  StreamModeStatus,
} from "./StatusBarItems";
import styles from "./StatusBar.module.css";

/** 左区段优先级。插件与核心贡献共享同一条排序轴。 */
const PRI = {
  connection: 1000,
  uptime: 700,
  dataMode: 600,
  encoding: 500,
  traffic: 400,
  log: 200,
} as const;

interface StatusSegment {
  key: string;
  priority: number;
  overflow?: StatusBarOverflow;
  node: ReactNode;
}

interface StatusBarProps {
  updatePhase: UpdatePhase;
  latestVersion?: string;
  downloadedBytes?: number;
  totalBytes?: number;
  onVersionClick: () => void;
}

function priorityClass(priority: number): string {
  if (priority >= 800) return styles.priorityHigh;
  if (priority >= 500) return styles.priorityMedium;
  return styles.priorityLow;
}

function overflowClass(overflow: StatusBarOverflow | undefined): string {
  if (overflow === "preserve") return styles.overflowPreserve;
  if (overflow === "early") return styles.overflowEarly;
  return styles.overflowAuto;
}

function segmentClass(segment: StatusSegment): string {
  return [
    styles.segment,
    priorityClass(segment.priority),
    overflowClass(segment.overflow),
  ].join(" ");
}

export default function StatusBar({
  updatePhase,
  latestVersion,
  downloadedBytes,
  totalBytes,
  onVersionClick,
}: StatusBarProps) {
  const { state, loggingSessions, logStatuses } = useSession();
  const activeTab = state.tabs.find(tab => tab.id === state.activeTabId) ?? null;
  const activePlugin = activeTab ? pluginRegistry.get(activeTab.pluginId) : undefined;
  const connected = activeTab?.state === "connected" || activeTab?.state === "transferring";
  const supportsStreamStatus = activePlugin?.manifest.content_type === "terminal"
    || activePlugin?.manifest.send_bar === true;

  const statusBarContext: StatusBarContext = {
    sessionId: activeTab?.id ?? "",
    activeTab,
  };

  const pluginSegments: StatusSegment[] = (activePlugin?.statusBarItems ?? [])
    .filter((item: StatusBarItem) => item.when ? item.when(statusBarContext) : true)
    .map(item => ({
      key: `plugin:${activeTab?.pluginId ?? "none"}:${item.id}`,
      priority: item.priority,
      overflow: item.overflow,
      node: item.render(statusBarContext),
    }));

  const coreSegments: Array<StatusSegment | null> = [
    activeTab
      ? {
          key: "core:connection",
          priority: PRI.connection,
          overflow: "preserve",
          node: <SessionConnectionStatus tab={activeTab} />,
        }
      : null,
    connected
      ? {
          key: "core:uptime",
          priority: PRI.uptime,
          node: <SessionUptimeStatus tab={activeTab} />,
        }
      : null,
    connected && supportsStreamStatus
      ? {
          key: "core:data-mode",
          priority: PRI.dataMode,
          node: <StreamModeStatus tab={activeTab} />,
        }
      : null,
    connected && supportsStreamStatus
      ? {
          key: "core:encoding",
          priority: PRI.encoding,
          overflow: "early",
          node: <StreamEncodingStatus tab={activeTab} />,
        }
      : null,
    connected && supportsStreamStatus
      ? {
          key: "core:traffic",
          priority: PRI.traffic,
          node: <SessionTrafficStatus tab={activeTab} />,
        }
      : null,
    loggingSessions.size > 0
      ? {
          key: "core:logging",
          priority: PRI.log,
          overflow: "early",
          node: (
            <LoggingStatus
              activeSessionId={activeTab?.id ?? null}
              loggingSessions={loggingSessions}
              logStatuses={logStatuses}
            />
          ),
        }
      : null,
  ];

  const segments = [...coreSegments, ...pluginSegments]
    .filter((segment): segment is StatusSegment => segment !== null)
    .sort((a, b) => b.priority - a.priority);

  return (
    <div className={`${styles.bar} liquid-glass`}>
      <div className={styles.left}>
        {segments.map(segment => (
          <div key={segment.key} className={segmentClass(segment)}>
            {segment.node}
          </div>
        ))}
      </div>

      <div className={styles.right}>
        <AppVersionStatus
          updatePhase={updatePhase}
          latestVersion={latestVersion}
          downloadedBytes={downloadedBytes}
          totalBytes={totalBytes}
          onVersionClick={onVersionClick}
        />
      </div>
    </div>
  );
}
