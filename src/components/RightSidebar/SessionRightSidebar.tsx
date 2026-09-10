import type { ProtocolType } from "../../types/transfer";
import { lazy, Suspense, useCallback } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "./RightSidebarPanel";
import TransmissionPanel from "../Transmission/TransmissionPanel";
import JournaldViewerPanel from "../JournaldViewer/JournaldViewerPanel";
const FileManagerPanel = lazy(() => import("../FileManager/FileManagerPanel"));
const ProtocolTool = lazy(() => import("../Tools/ProtocolTool"));
const CalculatorTool = lazy(() => import("../Tools/CalculatorTool"));

export interface SessionRightSidebarProps {
  sessionId: string;
  isConnected: boolean;
  initialProtocol?: ProtocolType;
  showTransmission: boolean;
  /** 是否显示文件管理器面板（SSH + fileServiceEnabled） */
  showFileManager: boolean;
  /** 是否显示 journald 日志查看器面板（SSH + journaldEnabled） */
  showJournald: boolean;
}

/**
 * 单个会话的右侧栏工具面板集合
 *
 * 每个会话标签页拥有独立的 SessionRightSidebar 实例，
 * 切换会话时各组件的 useState 状态自然保留，实现无缝的后台体验。
 */
export default function SessionRightSidebar({
  sessionId,
  isConnected,
  initialProtocol,
  showTransmission,
  showFileManager,
  showJournald,
}: SessionRightSidebarProps) {
  const { t } = useTranslation();

  // 文件管理器外层空白区域右键 → 触发空白区域菜单
  const handleFileManagerWrapperContext = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      window.dispatchEvent(
        new CustomEvent("tauterm:filemanager-blank-context", {
          detail: { clientX: e.clientX, clientY: e.clientY, sessionId },
        })
      );
    },
    [sessionId]
  );

  return (
    <>
      {/* 文件管理器（SSH 文件服务）—— onContextMenu 拦截面板内空白区域右键 */}
      {showFileManager && (
        <RightSidebarPanel
          title={t("fileManager.title")}
          defaultExpanded={true}
          onContextMenu={handleFileManagerWrapperContext}
        >
          <Suspense fallback={null}>
            <FileManagerPanel
              sessionId={sessionId}
              isConnected={isConnected}
            />
          </Suspense>
        </RightSidebarPanel>
      )}
      {/* 日志查看器（SSH journald）—— 组件自行管理 RightSidebarPanel */}
      {showJournald && (
        <JournaldViewerPanel
          sessionId={sessionId}
          isConnected={isConnected}
        />
      )}
      {/* 文件传输 */}
      {showTransmission && (
        <RightSidebarPanel title={t("transmission.title")}>
          <TransmissionPanel
            sessionId={sessionId}
            isConnected={isConnected}
            initialProtocol={initialProtocol}
          />
        </RightSidebarPanel>
      )}
      {/* 低频工程工具按需加载，避免把协议/数据分析代码压进 daily-driver ui-core。 */}
      <Suspense fallback={null}>
        <ProtocolTool sessionId={sessionId} />
        <CalculatorTool />
      </Suspense>
    </>
  );
}
