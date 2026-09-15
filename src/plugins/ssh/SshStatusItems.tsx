import { useTranslation } from "react-i18next";
import {
  StatusBarBadge,
  StatusBarGroup,
  StatusBarText,
} from "../../components/Layout/StatusBarPrimitives";
import type { StatusBarContext } from "../../core/plugin-registry";

export default function SshStatusItems({ activeTab }: StatusBarContext) {
  const { t } = useTranslation();
  if (!activeTab?.params) return null;

  const username = typeof activeTab.params.username === "string"
    ? activeTab.params.username.trim()
    : "";
  const authMethod = activeTab.params.auth_method === "key" ? "key" : "password";
  const fileServiceEnabled = activeTab.params.file_service_enabled === true;
  const fileServiceProtocol = typeof activeTab.params.file_service_protocol === "string"
    ? activeTab.params.file_service_protocol.toUpperCase()
    : "SFTP";

  return (
    <StatusBarGroup>
      <StatusBarBadge>{t("statusBar.typeSsh")}</StatusBarBadge>
      {username ? <StatusBarText>{username}</StatusBarText> : null}
      <StatusBarBadge tone="muted">
        {authMethod === "key" ? t("statusBar.authKey") : t("statusBar.authPassword")}
      </StatusBarBadge>
      {fileServiceEnabled ? (
        <StatusBarBadge tone="success">{fileServiceProtocol}</StatusBarBadge>
      ) : null}
    </StatusBarGroup>
  );
}
