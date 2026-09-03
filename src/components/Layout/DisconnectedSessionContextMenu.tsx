import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSession, type TabInfo } from "../../context/SessionContext";
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
 * chrome. The Session remains a normal saved configuration: Connect / Configure / Delete work
 * exactly from the work area without requiring a trip back to the left Sidebar.
 */
export default function DisconnectedSessionContextMenu({
  state,
  onClose,
}: DisconnectedSessionContextMenuProps) {
  const { t } = useTranslation();
  const { connect, deleteSession } = useSession();
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

  const reconnect = useCallback(async (tab: TabInfo, initialElevated = false) => {
    if (tab.state !== "disconnected" || !tab.params) return;

    let params = tab.params as Record<string, unknown>;
    // Match Sidebar reconnect safety: a writable TFTP server exposed beyond loopback must be
    // explicitly confirmed again if old persisted data does not contain literal true.
    if (tab.pluginId === "tftp") {
      const bindIp = String(params.listen_ip ?? "").trim().toLowerCase();
      const loopback = bindIp === "127.0.0.1" || bindIp === "::1" || bindIp === "localhost";
      if (
        !loopback
        && params.write_enabled === true
        && params.overwrite === true
        && params.exposure_confirmed !== true
      ) {
        const ok = window.confirm(
          t("tftp.exposureWarning", {
            defaultValue:
              "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Continue only on a trusted network.",
          }),
        );
        if (!ok) return;
        params = { ...params, exposure_confirmed: true };
      }
    }

    await connect({
      endpoint: tab.endpoint,
      params,
      name: tab.name,
      pluginId: tab.pluginId,
      transferEnabled: initialElevated ? false : tab.transferEnabled,
      transferProtocol: tab.transferProtocol,
      sendBarEnabled: tab.sendBarEnabled,
      journaldEnabled: tab.journaldEnabled,
      sessionId: tab.id,
      initialElevated,
    });
  }, [connect, t]);

  const handleSelect = useCallback(async (itemId: string) => {
    const tab = state.session;
    if (!tab) return;

    switch (itemId) {
      case "connect":
        await reconnect(tab, false);
        break;
      case "connect_elevated":
        await reconnect(tab, true);
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
  }, [state.session, reconnect, deleteSession, t]);

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
