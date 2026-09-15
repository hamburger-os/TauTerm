from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 match, got {count}")
    return text.replace(old, new, 1)


# PluginRegistry: app-overlay and right-sidebar extension points.
path = Path("src/core/plugin-registry.ts")
text = path.read_text(encoding="utf-8")
anchor = '''export interface PluginSessionTreeContribution {
  groupKey?: (params: Record<string, unknown>, fallback: string) => string;
  children: (
    sessionId: string,
    params: Record<string, unknown>,
    runtimeSnapshot: unknown,
  ) => PluginSessionTreeChild[];
  onParentSelect?: (
    sessionId: string,
    params: Record<string, unknown>,
    runtimeSnapshot: unknown,
  ) => void;
}

'''
addition = anchor + '''export interface PluginRightSidebarPanel {
  id: string;
  when?: (params: Record<string, unknown>) => boolean;
  component: ComponentType<{ sessionId: string; isConnected: boolean }>;
}

export interface PluginRightSidebarContribution {
  /** Override the content-type default: terminal=true, custom=false. */
  available?: (params: Record<string, unknown>) => boolean;
  panels?: PluginRightSidebarPanel[];
}

'''
text = replace_once(text, anchor, addition, "plugin right sidebar contracts")
text = replace_once(
    text,
    '''  terminalLocalEcho?: (runtimeSnapshot: unknown) => boolean;
  toolbarItems?: ToolbarItem[];''',
    '''  terminalLocalEcho?: (runtimeSnapshot: unknown) => boolean;
  /** 插件级应用覆盖层，例如连接安全确认；App Shell 只负责挂载。 */
  appOverlay?: ComponentType;
  /** 插件专属右侧栏能力；应用壳不识别具体协议 ID。 */
  rightSidebar?: PluginRightSidebarContribution;
  toolbarItems?: ToolbarItem[];''',
    "plugin app shell contributions",
)
path.write_text(text, encoding="utf-8")

# SSH host-key confirmation owns its own event lifecycle.
Path("src/plugins/ssh/SshHostKeyGate.tsx").write_text('''import { useCallback, useEffect, useRef, useState } from "react";
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
        ? `${t("ssh.hostKeyHost", { defaultValue: "Host" })}: ${pending.host}:${pending.port}\n${t("ssh.hostKeyFingerprint")}: ${pending.fingerprint}\n\n${t("ssh.hostKeyPrompt")}`
        : undefined}
      onConfirm={() => void settle(true)}
      onCancel={() => void settle(false)}
    />
  );
}
''', encoding="utf-8")

# SSH right-sidebar composition lives with the plugin.
Path("src/plugins/ssh/SshRightSidebarPanels.tsx").write_text('''import { lazy, Suspense, useCallback } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../../components/RightSidebar/RightSidebarPanel";

const FileManagerPanel = lazy(() => import("../../components/FileManager/FileManagerPanel"));
const JournaldViewerPanel = lazy(() => import("../../components/JournaldViewer/JournaldViewerPanel"));

export function SshFileManagerSidebarPanel({
  sessionId,
  isConnected,
}: {
  sessionId: string;
  isConnected: boolean;
}) {
  const { t } = useTranslation();
  const onContextMenu = useCallback((event: React.MouseEvent) => {
    event.preventDefault();
    window.dispatchEvent(new CustomEvent("tauterm:filemanager-blank-context", {
      detail: { clientX: event.clientX, clientY: event.clientY, sessionId },
    }));
  }, [sessionId]);

  return (
    <RightSidebarPanel
      title={t("fileManager.title")}
      defaultExpanded={true}
      onContextMenu={onContextMenu}
    >
      <Suspense fallback={null}>
        <FileManagerPanel sessionId={sessionId} isConnected={isConnected} />
      </Suspense>
    </RightSidebarPanel>
  );
}

export function SshJournaldSidebarPanel({
  sessionId,
  isConnected,
}: {
  sessionId: string;
  isConnected: boolean;
}) {
  return (
    <Suspense fallback={null}>
      <JournaldViewerPanel sessionId={sessionId} isConnected={isConnected} />
    </Suspense>
  );
}
''', encoding="utf-8")

# Generic SessionRightSidebar only renders registered panels plus common tools.
Path("src/components/RightSidebar/SessionRightSidebar.tsx").write_text('''import { lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { pluginRegistry } from "../../core/plugin-registry";
import type { ProtocolType } from "../../types/transfer";
import TransmissionPanel from "../Transmission/TransmissionPanel";
import RightSidebarPanel from "./RightSidebarPanel";

const ProtocolTool = lazy(() => import("../Tools/ProtocolTool"));
const CalculatorTool = lazy(() => import("../Tools/CalculatorTool"));

export interface SessionRightSidebarProps {
  sessionId: string;
  pluginId: string;
  params: Record<string, unknown>;
  isConnected: boolean;
  initialProtocol?: ProtocolType;
  showTransmission: boolean;
}

/** Per-session tool host. Protocol-specific panels come from PluginRegistry. */
export default function SessionRightSidebar({
  sessionId,
  pluginId,
  params,
  isConnected,
  initialProtocol,
  showTransmission,
}: SessionRightSidebarProps) {
  const { t } = useTranslation();
  const pluginPanels = (pluginRegistry.get(pluginId)?.rightSidebar?.panels ?? [])
    .filter(panel => panel.when?.(params) ?? true);

  return (
    <>
      {pluginPanels.map(panel => {
        const Panel = panel.component;
        return <Panel key={panel.id} sessionId={sessionId} isConnected={isConnected} />;
      })}
      {showTransmission && (
        <RightSidebarPanel title={t("transmission.title")}>
          <TransmissionPanel
            sessionId={sessionId}
            isConnected={isConnected}
            initialProtocol={initialProtocol}
          />
        </RightSidebarPanel>
      )}
      <Suspense fallback={null}>
        <ProtocolTool sessionId={sessionId} />
        <CalculatorTool />
      </Suspense>
    </>
  );
}
''', encoding="utf-8")

# SSH registration owns overlays and right-sidebar panels.
path = Path("src/plugins/ssh/index.ts")
text = path.read_text(encoding="utf-8")
text = replace_once(
    text,
    'import SshStatusItems from "./SshStatusItems";\n',
    'import SshStatusItems from "./SshStatusItems";\nimport SshHostKeyGate from "./SshHostKeyGate";\nimport { SshFileManagerSidebarPanel, SshJournaldSidebarPanel } from "./SshRightSidebarPanels";\n',
    "SSH app-shell imports",
)
text = replace_once(
    text,
    '''  statusBarItems: [
    {
      id: "ssh-runtime",
      priority: 860,
      when: connected,
      render: context => createElement(SshStatusItems, context),
    },
  ],''',
    '''  appOverlay: SshHostKeyGate,
  rightSidebar: {
    panels: [
      {
        id: "ssh-file-manager",
        when: params => params.file_service_enabled === true,
        component: SshFileManagerSidebarPanel,
      },
      {
        id: "ssh-journald",
        when: params => params.journald_enabled === true,
        component: SshJournaldSidebarPanel,
      },
    ],
  },
  statusBarItems: [
    {
      id: "ssh-runtime",
      priority: 860,
      when: connected,
      render: context => createElement(SshStatusItems, context),
    },
  ],''',
    "SSH app shell registration",
)
path.write_text(text, encoding="utf-8")

# Custom Network explicitly opts into the generic right sidebar; Local Shell opts out.
path = Path("src/plugins/network/index.tsx")
text = path.read_text(encoding="utf-8")
text = replace_once(text, '  customView: NetworkDebugSessionView,\n', '  customView: NetworkDebugSessionView,\n  rightSidebar: { available: () => true },\n', "Network right sidebar")
path.write_text(text, encoding="utf-8")

path = Path("src/plugins/local-shell/index.ts")
text = path.read_text(encoding="utf-8")
text = replace_once(text, '  toolbarItems: [],\n', '  toolbarItems: [],\n  rightSidebar: { available: () => false },\n', "Local Shell right sidebar")
path.write_text(text, encoding="utf-8")

# App Shell no longer knows SSH, Network or Local Shell business rules.
path = Path("src/App.tsx")
text = path.read_text(encoding="utf-8")
text = replace_once(text, 'import { invoke } from "@tauri-apps/api/core";\n', '', "App invoke import")
text = replace_once(text, 'import { listen } from "@tauri-apps/api/event";\n', '', "App listen import")
text = replace_once(text, 'import ConfirmDialog from "./components/common/ConfirmDialog";\n', '', "App ConfirmDialog import")
text = replace_once(text, '''interface PendingHostKeyVerification {
  requestId: string;
  host: string;
  port: number;
  fingerprint: string;
}

''', '', "App SSH host key type")
text = replace_once(text, '  const [pendingHostKey, setPendingHostKey] = useState<PendingHostKeyVerification | null>(null);\n  const queuedHostKeysRef = useRef<PendingHostKeyVerification[]>([]);\n', '', "App SSH state")
start = text.find('  const enqueueHostKey = useCallback(')
end = text.find('  // 窗口最大化/还原状态追踪', start)
if start < 0 or end < 0:
    raise SystemExit("App SSH host key lifecycle block not found")
text = text[:start] + text[end:]
old = '''                const isCustomContent = activePlugin?.manifest.content_type === "custom";
                // 网络调试会话（custom）保留右侧栏：提供校验和/编码/协议解析等开发工具
                const isNetworkDebug = activePlugin?.manifest.id === "network";
                const isLocalShell = activePlugin?.manifest.id === "local-shell";
                const hasAnyPanel = activeTab
                  ? ((activePlugin?.manifest.transfer_protocols?.length ?? 0) > 0 && activeTab.transferEnabled !== false)
                    || (activePlugin?.manifest.id === "ssh" && activeTab.params?.file_service_enabled === true)
                    || (activePlugin?.manifest.id === "ssh" && activeTab.params?.journald_enabled === true)
                  : false;

                // custom 内容类型无侧栏面板时完全隐藏右侧栏（网络调试除外）
                if (isLocalShell || (isCustomContent && !hasAnyPanel && !isNetworkDebug)) return null;
'''
new = '''                const params = activeTab?.params ?? {};
                const defaultAvailable = activePlugin?.manifest.content_type !== "custom";
                const sidebarAvailable = activePlugin?.rightSidebar?.available?.(params) ?? defaultAvailable;
                if (!activeTab || !activePlugin || !sidebarAvailable) return null;
'''
text = replace_once(text, old, new, "App right sidebar availability")
text = replace_once(text, '''                        const showFileManager = tabPlugin?.manifest.id === "ssh" && tab.params?.file_service_enabled === true;
                        const showJournald = tabPlugin?.manifest.id === "ssh" && tab.params?.journald_enabled === true;
''', '', "App SSH panel flags")
text = replace_once(text, '''                              sessionId={tab.id}
                              isConnected={tab.state === "connected" || tab.state === "transferring"}
                              initialProtocol={tab.transferProtocol as ProtocolType | undefined}
                              showTransmission={showTransmission}
                              showFileManager={showFileManager}
                              showJournald={showJournald}
''', '''                              sessionId={tab.id}
                              pluginId={tab.pluginId}
                              params={tab.params ?? {}}
                              isConnected={tab.state === "connected" || tab.state === "transferring"}
                              initialProtocol={tab.transferProtocol as ProtocolType | undefined}
                              showTransmission={showTransmission}
''', "App generic SessionRightSidebar props")
confirm_start = text.find('      <ConfirmDialog\n        open={pendingHostKey !== null}')
if confirm_start < 0:
    raise SystemExit("App host key dialog start not found")
confirm_end = text.find('\n      />', confirm_start)
if confirm_end < 0:
    raise SystemExit("App host key dialog end not found")
confirm_end += len('\n      />')
overlays = '''      {pluginRegistry.getAll().map(plugin => {
        const Overlay = plugin.appOverlay;
        return Overlay ? <Overlay key={plugin.manifest.id} /> : null;
      })}'''
text = text[:confirm_start] + overlays + text[confirm_end:]
path.write_text(text, encoding="utf-8")

# Regression contract: the app shell must not regress to built-in protocol branches.
path = Path("src-tauri/src/architecture_contract.rs")
text = path.read_text(encoding="utf-8")
addition = '''
#[test]
fn frontend_app_shell_is_plugin_driven() {
    let app = read_workspace_source("src/App.tsx");
    for plugin_id in ["ssh", "network", "local-shell", "serial", "telnet", "trdp", "modbus"] {
        assert!(
            !app.contains(&format!(r#"manifest.id === "{plugin_id}""#))
                && !app.contains(&format!(r#"pluginId === "{plugin_id}""#)),
            "App Shell must not branch on built-in plugin '{plugin_id}'"
        );
    }
    assert!(!app.contains("ssh-host-key-verify"));
    assert!(!app.contains("file_service_enabled"));
    assert!(!app.contains("journald_enabled"));
}
'''
if "fn frontend_app_shell_is_plugin_driven()" not in text:
    text = text.rstrip() + "\n" + addition
path.write_text(text, encoding="utf-8")

# UI owner doc records the contribution boundary while retaining the StatusBar v2 details.
path = Path("docs/modules/UI_FOUNDATION.md")
text = path.read_text(encoding="utf-8")
text = replace_once(
    text,
    "- Session 列表与 Pane Header 采用统一的双层身份模型：第一行 `Session.name` 是稳定、可显式重命名的会话身份，默认名称仅在创建时计算一次；第二行是当前配置摘要，允许随 host/port/串口/角色等参数变化。协议默认名与摘要优先由 `PluginRegistration.sessionPresentation` 声明，应用壳不重复维护协议格式；\n",
    "- Session 列表与 Pane Header 采用统一的双层身份模型：第一行 `Session.name` 是稳定、可显式重命名的会话身份，默认名称仅在创建时计算一次；第二行是当前配置摘要，允许随 host/port/串口/角色等参数变化。协议默认名与摘要由 `PluginRegistration.sessionPresentation` 声明；需要宿主能力计算默认名时使用 `resolveDefaultSessionName`，应用壳不维护 built-in 协议格式；\n- 前端只有一个运行时 `PluginRegistry`。协议的 presentation、运行态、发送目标、应用覆盖层和右侧栏面板都由 registration contribution 声明；`App.tsx` / `SessionRightSidebar` 只挂载这些 contribution，不识别 SSH、Network、Local Shell 等具体插件 ID；\n",
    "UI plugin registry ownership",
)
path.write_text(text, encoding="utf-8")

print("app-shell plugin boundary cleanup applied")
