import type { ProtocolType } from "../../types/transfer";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "./RightSidebarPanel";
import TransmissionPanel from "../Transmission/TransmissionPanel";
import ProtocolTool from "../Tools/ProtocolTool";
import CalculatorTool from "../Tools/CalculatorTool";

export interface SessionRightSidebarProps {
  sessionId: string;
  isConnected: boolean;
  initialProtocol?: ProtocolType;
  showTransmission: boolean;
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
}: SessionRightSidebarProps) {
  const { t } = useTranslation();
  return (
    <>
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
