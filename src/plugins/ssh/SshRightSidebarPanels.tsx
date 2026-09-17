import { lazy, Suspense, useCallback } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../../components/RightSidebar/RightSidebarPanel";

const FileManagerPanel = lazy(() => import("./FileManager/FileManagerPanel"));
const JournaldViewerPanel = lazy(() => import("./JournaldViewer/JournaldViewerPanel"));

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
