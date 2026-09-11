import type { ProtocolType } from "../../types/transfer";
import { lazy, Suspense, useCallback } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "./RightSidebarPanel";
import TransmissionPanel from "../Transmission/TransmissionPanel";

const FileManagerPanel = lazy(() => import("../FileManager/FileManagerPanel"));
const JournaldViewerPanel = lazy(() => import("../JournaldViewer/JournaldViewerPanel"));
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
 * 单个会话的右侧栏工具面板集合。
 *
 * 每个会话标签页拥有独立实例；高成本、低频面板按需加载，避免把 SSH
 * 文件/日志工具压入默认 UI bundle。
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

  const handleFileManagerWrapperContext = useCallback(
    (event: React.MouseEvent) => {
      event.preventDefault();
      window.dispatchEvent(
        new CustomEvent("tauterm:filemanager-blank-context", {
          detail: {
            clientX: event.clientX,
            clientY: event.clientY,
            sessionId,
          },
        }),
      );
    },
    [sessionId],
  );

  return (
    <>
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
      {showJournald && (
        <Suspense fallback={null}>
          <JournaldViewerPanel
            sessionId={sessionId}
            isConnected={isConnected}
          />
        </Suspense>
      )}
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
