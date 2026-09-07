import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import ContextMenu, { type ContextMenuItem } from "../common/ContextMenu";
import ConnectDialog from "./ConnectDialog";

interface DisconnectedSessionContextMenuProps {
  state: ContextMenuState;
  onClose: () => void;
}

/**
 * Session-card-equivalent menu for a disconnected Session shown inside a Pane.
 *
 * Keeping this menu next to SplitView avoids treating the disconnected placeholder as browser
 * surface. The Session remains a normal saved configuration: Connect / Configure / Delete work
 * exactly from the work area without requiring a trip back to the left Sidebar.
 */
export default function DisconnectedSessionContextMenu({
  state,
  onClose,
}: DisconnectedSessionContextMenuProps) {
  const { t } = useTranslation();
  const { reconnectSession, deleteSession } = useSession();
  const [editSessionId, setEditSessionId] = useState<string | null>(null);

  const menuItems = useMemo<ContextMenuItem[]>(() => {
    const tab = state.session;
    if (!tab) return [];

    const capabilities = pluginRegistry.get(tab.pluginId)?.manifest.capabilities ?? [];
    const supportsElevation = capabilities.includes("elevated_session")
      && tab.params?.shell_kind !== "wsl";

    const items: ContextMenuItem[] = [
      { id: "connect", label: t("contextMenu.connect") || "Connect", icon: "play" },
      { id: "configure", label: t("contextMenu.configure") || "Configure", icon: "settings" },
    ];
    if (supportsElevation) {
      items.splice(1, 0, {
        id: "connect_elevated",
        label: t("contextMenu.connectAsAdministrator"),
        icon: "shield",
      });
    }
    items.push({ id: "delete", label: t("contextMenu.delete") || "Delete", icon: "trash", danger: true });
    return items;
  }, [state.session, t]);

  const handleSelect = useCallback(async (itemId: string) => {
    const tab = state.session;
    if (!tab) return;

    switch (itemId) {
      case "connect":
        await reconnectSession(tab.id);
        break;
      case "connect_elevated":
        await reconnectSession(tab.id, true);
        break;
      case "configure":
        setEditSessionId(tab.id);
        break;
      case "delete":
        if (window.confirm(t("session.deleteConfirm") || "Delete this session?")) {
          await deleteSession(tab.id);
        }
        break;
    }
  }, [state.session, reconnectSession, deleteSession, t]);

  return (
    <>
      <ContextMenu
        state={state}
        items={menuItems}
        onSelect={(itemId) => { void handleSelect(itemId); }}
        onClose={onClose}
      />
      <ConnectDialog
        isOpen={editSessionId !== null}
        onClose={() => setEditSessionId(null)}
        editSessionId={editSessionId}
      />
    </>
  );
}
