import type { ReactNode } from "react";
import { useSession } from "../../context/SessionContext";
import { useTransfer } from "../../context/TransferContext";
import {
  pluginRegistry,
  type StatusBarContext,
  type StatusBarItem,
  type StatusBarOverflow,
} from "../../core/plugin-registry";
import type { UpdateInfo } from "../../types/updater";
import {
  AppVersionStatus,
  LoggingStatus,
  SessionActivityStatus,
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
  activity: 760,
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
  updateInfo: UpdateInfo;
  onVersionClick: () => void;
}

interface PluginStatusItemHostProps {
  item: StatusBarItem;
  context: StatusBarContext;
}

/**
 * 每个插件状态项拥有独立 React 边界。
 *
 * StatusBarItem.render 是插件扩展点；把它直接作为普通函数在 StatusBar 主组件中调用，
 * 会让插件内部 Hook 共享宿主 Hook 顺序。独立 Host 让每个 contribution 的 Hook 生命周期
 * 随自己的 key 挂载/卸载，不受其它状态项显隐、排序或插件切换影响。
 */
function PluginStatusItemHost({ item, context }: PluginStatusItemHostProps) {
  return <>{item.render(context)}</>;
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
  updateInfo,
  onVersionClick,
}: StatusBarProps) {
  const { state, loggingSessions, logStatuses } = useSession();
  const { getActiveTaskForSession } = useTransfer();
  const activeTab = state.tabs.find(tab => tab.id === state.activeTabId) ?? null;
  const activePlugin = activeTab ? pluginRegistry.get(activeTab.pluginId) : undefined;
  const connected = activeTab?.state === "connected";
  const activeTransfer = activeTab ? getActiveTaskForSession(activeTab.id) : undefined;
  const transferring = connected && activeTransfer?.runtimeId === activeTab?.runtimeId;
  const supportsStreamStatus = activePlugin?.manifest.content_type === "terminal"
    || activePlugin?.manifest.send_bar === true;
  const dataMode = activeTab?.params?.data_mode;
  const hasDataMode = dataMode === "text" || dataMode === "hex" || dataMode === "dual";
  const hasEncoding = typeof activeTab?.params?.encoding === "string"
    && activeTab.params.encoding.trim().length > 0;

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
      node: <PluginStatusItemHost item={item} context={statusBarContext} />,
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
    transferring
      ? {
          key: "core:activity",
          priority: PRI.activity,
          node: <SessionActivityStatus active />,
        }
      : null,
    connected
      ? {
          key: "core:uptime",
          priority: PRI.uptime,
          node: <SessionUptimeStatus tab={activeTab} />,
        }
      : null,
    connected && supportsStreamStatus && hasDataMode
      ? {
          key: "core:data-mode",
          priority: PRI.dataMode,
          node: <StreamModeStatus tab={activeTab} />,
        }
      : null,
    connected && supportsStreamStatus && hasEncoding
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
          updateInfo={updateInfo}
          onVersionClick={onVersionClick}
        />
      </div>
    </div>
  );
}
