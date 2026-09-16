import { lazy, Suspense } from "react";
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
