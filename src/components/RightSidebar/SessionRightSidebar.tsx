import type { ProtocolType } from "../../types/transfer";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "./RightSidebarPanel";
import TransmissionPanel from "../Transmission/TransmissionPanel";
import FileManagerPanel from "../FileManager/FileManagerPanel";
import ProtocolTool from "../Tools/ProtocolTool";
import CalculatorTool from "../Tools/CalculatorTool";

export interface SessionRightSidebarProps {
  sessionId: string;
  isConnected: boolean;
  initialProtocol?: ProtocolType;
  showTransmission: boolean;
  /** 是否显示文件管理器面板（SSH + fileServiceEnabled） */
  showFileManager: boolean;
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
}: SessionRightSidebarProps) {
  const { t } = useTranslation();

  // 文件管理器外层空白区域右键 → 触发空白区域菜单
  const handleFileManagerWrapperContext = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      window.dispatchEvent(
        new CustomEvent("tauterm:filemanager-blank-context", {
          detail: { clientX: e.clientX, clientY: e.clientY },
        })
      );
    },
    []
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
          <FileManagerPanel
            sessionId={sessionId}
            isConnected={isConnected}
          />
        </RightSidebarPanel>
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
      {/* 面板2: 协议帧解析 */}
      <ProtocolTool />
      {/* 面板3: 快捷工具 (校验和 + 编码 + 位操作) */}
      <CalculatorTool />
    </>
  );
}
